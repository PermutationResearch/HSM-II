//! Run manifests, JSONL metrics, and append-only run index for outer-loop workflows.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::metrics::TurnMetrics;

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Try `git rev-parse HEAD` for manifest provenance (best-effort).
pub fn try_git_head() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactPaths {
    pub manifest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns_baseline_jsonl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns_hsm_jsonl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hsm_trace_jsonl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_suite_json: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunManifest {
    pub run_id: String,
    pub created_unix: u64,
    pub git_commit: Option<String>,
    /// e.g. hsm-eval, hsm_meta_harness, hsm_outer_loop
    pub harness: String,
    pub suites: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suite_weights: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks_filter: Option<String>,
    pub task_count: usize,
    pub turn_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    pub artifact_paths: ArtifactPaths,
}

impl RunManifest {
    pub fn new(
        harness: &str,
        run_dir: &Path,
        suites: Vec<String>,
        suite_weights: Option<Vec<f64>>,
        tasks_filter: Option<String>,
        task_count: usize,
        turn_count: usize,
        parent_run_id: Option<String>,
        rel_paths: ArtifactPaths,
    ) -> Self {
        let run_id = run_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("run")
            .to_string();
        Self {
            run_id,
            created_unix: unix_now(),
            git_commit: try_git_head(),
            harness: harness.to_string(),
            suites,
            suite_weights,
            tasks_filter,
            task_count,
            turn_count,
            parent_run_id,
            artifact_paths: rel_paths,
        }
    }
}

/// Write one JSON object per line (turn metrics).
pub fn write_turn_metrics_jsonl(path: &Path, turns: &[TurnMetrics]) -> io::Result<()> {
    let mut f = File::create(path)?;
    for t in turns {
        serde_json::to_writer(&mut f, t)?;
        f.write_all(b"\n")?;
    }
    Ok(())
}

/// Generic JSONL (one serialized row per item).
pub fn write_jsonl<T: Serialize>(path: &Path, rows: &[T]) -> io::Result<()> {
    let mut f = File::create(path)?;
    for row in rows {
        serde_json::to_writer(&mut f, row)?;
        f.write_all(b"\n")?;
    }
    Ok(())
}

pub fn write_manifest(run_dir: &Path, manifest: &RunManifest) -> io::Result<()> {
    let p = run_dir.join(&manifest.artifact_paths.manifest);
    let j = serde_json::to_string_pretty(manifest)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(p, j)
}

/// Append a compact JSON line for cross-run search (default: `runs/runs_index.jsonl`).
pub fn append_runs_index(index_path: &Path, summary: &serde_json::Value) -> io::Result<()> {
    if let Some(parent) = index_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(index_path)?;
    serde_json::to_writer(&mut f, summary)?;
    f.write_all(b"\n")?;
    Ok(())
}

/// Default index path next to a `runs/run_*` directory.
pub fn default_runs_index(run_dir: &Path) -> PathBuf {
    run_dir.parent().unwrap_or(run_dir).join("runs_index.jsonl")
}
