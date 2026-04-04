//! Execution trajectories as offline skill proposals (Trace2Skill).
//!
//! Records tool-using turns, exports JSONL when `HSM_TRACE2SKILL_JSONL` is set, and
//! provides merge + `SkillBank` ingestion for Hermes-style “history → skill” loops.
//!
//! - **[`from_eval`]** maps `hsm-eval` / meta-harness `turns_hsm.jsonl` (+ optional `hsm_trace.jsonl`)
//!   into the same [`TrajectoryRecord`] format for `merge`.
//! - **[`analyst`]** builds sectioned, deduped `principle` text; set **`HSM_TRACE2SKILL_LLM=1`**
//!   for an optional LLM lesson per trajectory (sequential; needs LLM env like eval).
//! - After **`merge`**, run **`apply --merged …`** to ingest into the embedded world; set **`HSM_SKILL_BANK_RELOAD_SECS`**
//!   on `personal_agent` to pick up changes without restart.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod analyst;
pub mod from_eval;

pub use analyst::{
    heuristic_lesson, lesson_for_record, merge_sectioned_principle, parallel_lessons,
};
pub use from_eval::{
    default_eval_task_union, import_eval_artifacts_to_jsonl, read_run_manifest,
    read_turn_metrics_jsonl, task_map_for_artifacts, trajectory_from_eval_turn,
};

/// One executed tool step in a turn (args redacted for logs).
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ToolStepRecord {
    pub name: String,
    pub args_redacted: String,
    pub ok: bool,
    pub result_summary: String,
}

/// Canonical serialized trajectory for one agent turn.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrajectoryRecord {
    pub id: String,
    pub ts_unix: u64,
    pub user_task: String,
    pub outcome: TrajectoryOutcome,
    pub confidence: f64,
    pub turn_route: String,
    pub council_used: bool,
    pub primary_agent: u64,
    pub skills_accessed: Vec<String>,
    pub skills_used_ids: Vec<String>,
    pub tool_steps: Vec<ToolStepRecord>,
    pub response_preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrajectoryOutcome {
    Success,
    Failure,
    Partial,
}

/// Merged analyst output ready for `SkillBank::ingest_trace2skill_proposal`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MergedTraceSkill {
    pub title: String,
    pub principle: String,
    pub trajectory_ids: Vec<String>,
}

const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "secret",
    "token",
    "api_key",
    "apikey",
    "authorization",
    "bearer",
    "credential",
];

/// Redact sensitive fields in a JSON value (shallow + recursive on objects).
pub fn redact_params(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, val) in map {
                let kl = k.to_lowercase();
                if SENSITIVE_KEYS.iter().any(|s| kl.contains(s)) {
                    out.insert(k.clone(), Value::String("[REDACTED]".into()));
                } else {
                    out.insert(k.clone(), redact_params(val));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(redact_params).collect()),
        _ => v.clone(),
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

pub fn summarize_tool_output(success: bool, result: &str, err: Option<&str>) -> String {
    if success {
        truncate(result, 400)
    } else {
        truncate(err.unwrap_or("tool failed"), 400)
    }
}

/// Guess route label from message prefix and response metadata.
pub fn infer_turn_route(msg: &str, response_council: bool, skills_used: &[String]) -> String {
    if response_council {
        return "council".into();
    }
    if msg.starts_with("/ralph") || skills_used.iter().any(|s| s.contains("ralph")) {
        return "ralph".into();
    }
    if msg.starts_with("/rlm") || skills_used.iter().any(|s| s.contains("rlm")) {
        return "rlm".into();
    }
    if msg.starts_with("/tool") {
        return "tool_cmd".into();
    }
    "skills".into()
}

pub fn outcome_from_turn(confidence: f64, tool_steps: &[ToolStepRecord]) -> TrajectoryOutcome {
    if tool_steps.iter().any(|t| !t.ok) {
        return TrajectoryOutcome::Failure;
    }
    if confidence >= 0.55 {
        TrajectoryOutcome::Success
    } else if confidence >= 0.35 {
        TrajectoryOutcome::Partial
    } else {
        TrajectoryOutcome::Failure
    }
}

impl TrajectoryRecord {
    pub fn from_turn(
        msg_content: &str,
        council_used: bool,
        primary_agent: u64,
        skills_accessed: &[String],
        skills_used_ids: &[String],
        tool_steps: &[ToolStepRecord],
        confidence: f64,
        response_content: &str,
    ) -> Self {
        let ts_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let tool_steps = tool_steps.to_vec();
        let outcome = outcome_from_turn(confidence, &tool_steps);
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            ts_unix,
            user_task: truncate(msg_content, 800),
            outcome,
            confidence,
            turn_route: infer_turn_route(msg_content, council_used, skills_used_ids),
            council_used,
            primary_agent,
            skills_accessed: skills_accessed.to_vec(),
            skills_used_ids: skills_used_ids.to_vec(),
            tool_steps,
            response_preview: truncate(response_content, 600),
        }
    }
}

/// Append one JSON line to a JSONL file (creates parent dirs best-effort).
pub fn append_jsonl(path: &Path, record: &TrajectoryRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open trace2skill JSONL {}", path.display()))?;
    let line = serde_json::to_string(record)?;
    writeln!(f, "{}", line)?;
    f.flush()?;
    Ok(())
}

/// Read all trajectories from JSONL (skips blank lines).
pub fn read_jsonl(path: &Path) -> Result<Vec<TrajectoryRecord>> {
    let f = File::open(path).with_context(|| format!("read {}", path.display()))?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        out.push(serde_json::from_str(t).with_context(|| "parse trajectory line")?);
    }
    Ok(out)
}

fn derive_merge_title(records: &[TrajectoryRecord]) -> String {
    if records.is_empty() {
        return "Trace2Skill empty pool".into();
    }
    let mut routes: Vec<String> = records.iter().map(|r| r.turn_route.clone()).collect();
    routes.sort();
    routes.dedup();
    let route_summary = routes.join("+");
    let seed = truncate(
        records
            .iter()
            .max_by_key(|r| r.ts_unix)
            .map(|r| r.user_task.as_str())
            .unwrap_or("merged"),
        48,
    );
    format!("Trace2Skill: {} [{}]", seed, route_summary)
}

/// Merge many trajectories into one proposed skill (sectioned + deduped lessons).
pub fn merge_pool(records: &[TrajectoryRecord]) -> MergedTraceSkill {
    let principle = analyst::merge_sectioned_principle(records);
    let title = derive_merge_title(records);
    let trajectory_ids = records.iter().map(|r| r.id.clone()).collect();
    MergedTraceSkill {
        title,
        principle,
        trajectory_ids,
    }
}

pub fn save_merged(path: &Path, merged: &MergedTraceSkill) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(merged)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_merged(path: &Path) -> Result<MergedTraceSkill> {
    let s = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_nested() {
        let v = serde_json::json!({
            "query": "x",
            "api_key": "secret123",
            "nested": { "password": "p" }
        });
        let r = redact_params(&v);
        assert_eq!(r["api_key"], "[REDACTED]");
        assert_eq!(r["nested"]["password"], "[REDACTED]");
        assert_eq!(r["query"], "x");
    }

    #[test]
    fn merge_pool_orders_ids() {
        let a = TrajectoryRecord {
            id: "a".into(),
            ts_unix: 1,
            user_task: "hello".into(),
            outcome: TrajectoryOutcome::Success,
            confidence: 0.9,
            turn_route: "skills".into(),
            council_used: false,
            primary_agent: 0,
            skills_accessed: vec![],
            skills_used_ids: vec![],
            tool_steps: vec![],
            response_preview: "".into(),
        };
        let m = merge_pool(&[a.clone()]);
        assert!(m.title.contains("hello") || m.title.contains("Trace2Skill"));
        assert!(!m.principle.is_empty());
        assert!(
            m.principle.contains("## Success") || m.principle.contains("success"),
            "principle={:?}",
            m.principle
        );
        assert_eq!(m.trajectory_ids, vec!["a".to_string()]);
    }
}
