//! Hermes-style GEPA workflow: **collect** failure traces locally → cluster by “why” → **optimize** DSPy
//! signatures with mutation order driven by clusters (not flat brute-force trials).
//!
//! - [`collect_bundle`]: pull low-scoring traces from RooDB, redact, aggregate signals, write JSON.
//! - [`mutation_style_names_from_bundle`]: map failure clusters to DSPy mutation kinds (strings).
//!
//! Config: [`GepaConfig`] YAML (`serde_yaml`) for thresholds, redaction, and caps.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::database::{DspyTraceRow, RooDb};

/// Top-level YAML config for local GEPA runs (no network; paths/caps only).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GepaConfig {
    /// Traces with `score <= this` are treated as failures for collect (default 0.55).
    #[serde(default = "default_failure_max_score")]
    pub failure_max_score: f64,
    #[serde(default = "default_max_traces")]
    pub max_failure_traces: usize,
    #[serde(default = "default_max_input_chars")]
    pub max_input_chars: usize,
    #[serde(default = "default_max_output_chars")]
    pub max_output_chars: usize,
    /// Substrings to replace in question/output when writing bundle (privacy).
    #[serde(default)]
    pub redact_substrings: Vec<String>,
    /// Minimum traces sharing a failure code to form a cluster (default 1).
    #[serde(default = "default_min_cluster_size")]
    pub min_cluster_size: usize,
}

fn default_failure_max_score() -> f64 {
    0.55
}
fn default_max_traces() -> usize {
    200
}
fn default_max_input_chars() -> usize {
    500
}
fn default_max_output_chars() -> usize {
    800
}
fn default_min_cluster_size() -> usize {
    1
}

impl GepaConfig {
    pub fn load_path(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read GEPA config {}", path.display()))?;
        let c: GepaConfig = serde_yaml::from_str(&raw)
            .with_context(|| format!("parse YAML GEPA config {}", path.display()))?;
        Ok(c)
    }
}

/// Serializable collected bundle (write with [`save_bundle`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GepaCollectedBundle {
    pub signature_name: String,
    pub created_at_unix: u64,
    pub config_snapshot: GepaConfig,
    pub failure_traces: Vec<GepaTraceSummary>,
    pub clusters: Vec<FailureCluster>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GepaTraceSummary {
    pub trace_id: i64,
    pub score: f64,
    pub semantic_ok: bool,
    pub repair_count: i32,
    pub failure_code: String,
    pub failure_detail: String,
    pub signals_json: String,
    pub input_redacted: String,
    pub output_redacted: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureCluster {
    pub primary_code: String,
    pub count: usize,
    pub sample_details: Vec<String>,
}

pub fn redact_string(s: &str, patterns: &[String], max_len: usize) -> String {
    let mut out = s.to_string();
    for p in patterns {
        if !p.is_empty() {
            out = out.replace(p, "[REDACTED]");
        }
    }
    if out.len() > max_len {
        out.truncate(max_len);
        out.push_str("…");
    }
    out
}

/// Ensure `failure_code` / `failure_detail` are populated using the same heuristics as DSPy persist.
pub fn ensure_failure_fields(row: &mut DspyTraceRow) {
    if !row.failure_code.is_empty() {
        return;
    }
    let (code, detail, sig) = crate::dspy::infer_failure_metadata(
        row.score,
        row.semantic_ok,
        row.repair_count,
        &row.output,
    );
    row.failure_code = code;
    row.failure_detail = detail;
    row.signals_json = sig;
}

/// Pull failure-band traces, redact, cluster; does not call any LLM API.
pub async fn collect_bundle(
    db: &RooDb,
    signature_name: &str,
    cfg: &GepaConfig,
) -> anyhow::Result<GepaCollectedBundle> {
    let mut rows = db
        .fetch_dspy_traces_low_scoring(
            signature_name,
            cfg.failure_max_score,
            cfg.max_failure_traces,
        )
        .await?;

    for r in rows.iter_mut() {
        ensure_failure_fields(r);
    }

    let failure_traces: Vec<GepaTraceSummary> = rows
        .iter()
        .map(|r| GepaTraceSummary {
            trace_id: r.id,
            score: r.score,
            semantic_ok: r.semantic_ok,
            repair_count: r.repair_count,
            failure_code: r.failure_code.clone(),
            failure_detail: r.failure_detail.clone(),
            signals_json: r.signals_json.clone(),
            input_redacted: redact_string(
                &r.input_question,
                &cfg.redact_substrings,
                cfg.max_input_chars,
            ),
            output_redacted: redact_string(&r.output, &cfg.redact_substrings, cfg.max_output_chars),
        })
        .collect();

    let clusters = cluster_failures(&rows, cfg.min_cluster_size);

    Ok(GepaCollectedBundle {
        signature_name: signature_name.to_string(),
        created_at_unix: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        config_snapshot: cfg.clone(),
        failure_traces,
        clusters,
    })
}

pub fn save_bundle(path: &Path, bundle: &GepaCollectedBundle) -> anyhow::Result<()> {
    let j = serde_json::to_string_pretty(bundle)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, j).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn load_bundle(path: &Path) -> anyhow::Result<GepaCollectedBundle> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse bundle {}", path.display()))
}

fn cluster_failures(rows: &[DspyTraceRow], min_size: usize) -> Vec<FailureCluster> {
    let mut by_code: HashMap<String, Vec<&DspyTraceRow>> = HashMap::new();
    for r in rows {
        let code = if r.failure_code.is_empty() {
            "unknown"
        } else {
            r.failure_code.as_str()
        };
        by_code.entry(code.to_string()).or_default().push(r);
    }

    let mut clusters: Vec<FailureCluster> = by_code
        .into_iter()
        .filter(|(_, v)| v.len() >= min_size)
        .map(|(code, traces)| {
            let mut details: Vec<String> = traces
                .iter()
                .filter_map(|t| {
                    let d = t.failure_detail.trim();
                    if d.is_empty() {
                        None
                    } else {
                        Some(d.to_string())
                    }
                })
                .take(5)
                .collect();
            if details.is_empty() {
                details.push(format!(
                    "score={}, semantic_ok={}",
                    traces[0].score, traces[0].semantic_ok
                ));
            }
            FailureCluster {
                primary_code: code,
                count: traces.len(),
                sample_details: details,
            }
        })
        .collect();

    clusters.sort_by(|a, b| b.count.cmp(&a.count));
    clusters
}

/// Map failure codes to DSPy mutation style names (see `dspy::mutation_style_from_name`).
pub fn styles_for_failure_code(code: &str) -> Vec<&'static str> {
    match code {
        "empty_output" | "truncation" => vec!["DemoSubset", "SystemRephrase", "DemoReorder"],
        "repair_loop" => vec!["SystemRephrase", "NotebookFirst", "DemoSubset"],
        "format" | "claim_evidence" => vec!["XmlConverged", "DemoReorder", "LateInteraction"],
        "semantic_fail" | "low_score" | "unknown" => {
            vec!["NotebookFirst", "LateInteraction", "SystemRephrase"]
        }
        "ok" | _ => vec![],
    }
}

/// Ordered mutation names: clusters by frequency first, then default sweep.
pub fn mutation_style_names_from_bundle(bundle: &GepaCollectedBundle) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for c in &bundle.clusters {
        for s in styles_for_failure_code(&c.primary_code) {
            names.push(s.to_string());
        }
    }
    // De-dup preserving order
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.clone()));

    for d in default_style_round_robin() {
        if seen.insert(d.to_string()) {
            names.push(d.to_string());
        }
    }
    names
}

fn default_style_round_robin() -> [&'static str; 6] {
    [
        "DemoSubset",
        "DemoReorder",
        "SystemRephrase",
        "NotebookFirst",
        "XmlConverged",
        "LateInteraction",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::DspyTraceRow;

    #[test]
    fn cluster_orders_by_count() {
        let rows = vec![
            DspyTraceRow {
                id: 1,
                signature_name: "t".into(),
                input_question: "q".into(),
                input_context_hash: "".into(),
                output: "a".into(),
                score: 0.2,
                semantic_ok: false,
                repair_count: 0,
                model: "".into(),
                latency_ms: 0,
                created_at: 0,
                failure_code: "format".into(),
                failure_detail: "missing evidence".into(),
                signals_json: "{}".into(),
            },
            DspyTraceRow {
                id: 2,
                signature_name: "t".into(),
                input_question: "q2".into(),
                input_context_hash: "".into(),
                output: "b".into(),
                score: 0.3,
                semantic_ok: false,
                repair_count: 0,
                model: "".into(),
                latency_ms: 0,
                created_at: 0,
                failure_code: "format".into(),
                failure_detail: "".into(),
                signals_json: "{}".into(),
            },
            DspyTraceRow {
                id: 3,
                signature_name: "t".into(),
                input_question: "q3".into(),
                input_context_hash: "".into(),
                output: "".into(),
                score: 0.1,
                semantic_ok: false,
                repair_count: 0,
                model: "".into(),
                latency_ms: 0,
                created_at: 0,
                failure_code: "empty_output".into(),
                failure_detail: "".into(),
                signals_json: "{}".into(),
            },
        ];
        let c = cluster_failures(&rows, 1);
        assert_eq!(c[0].primary_code, "format");
        assert_eq!(c[0].count, 2);
    }
}
