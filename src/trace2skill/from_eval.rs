//! Normalize **`hsm-eval` / meta-harness artifacts** into [`TrajectoryRecord`] JSONL for Trace2Skill.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use anyhow::Context;

use crate::eval::artifacts::RunManifest;
use crate::eval::judges::parse_tool_json;
use crate::eval::metrics::TurnMetrics;
use crate::eval::suites::eval_tasks_for_suite;
use crate::eval::tasks::{EvalTask, Turn};
use crate::eval::trace::HsmTurnTrace;
use crate::eval::{
    load_eval_suite, suite_council_vs_single, suite_memory_retrieval, suite_tool_routing,
};

use super::redact_params;
use super::{truncate, ToolStepRecord, TrajectoryOutcome, TrajectoryRecord};

pub fn read_run_manifest(artifacts_dir: &Path) -> anyhow::Result<Option<RunManifest>> {
    let p = artifacts_dir.join("manifest.json");
    if !p.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p).with_context(|| p.display().to_string())?;
    Ok(Some(
        serde_json::from_str(&text).with_context(|| "parse manifest.json")?,
    ))
}

pub fn default_eval_task_union() -> HashMap<String, EvalTask> {
    let mut m = HashMap::new();
    for t in load_eval_suite() {
        m.entry(t.id.clone()).or_insert(t);
    }
    for t in suite_memory_retrieval() {
        m.entry(t.id.clone()).or_insert(t);
    }
    for t in suite_tool_routing() {
        m.entry(t.id.clone()).or_insert(t);
    }
    for t in suite_council_vs_single() {
        m.entry(t.id.clone()).or_insert(t);
    }
    m
}

pub fn task_map_for_artifacts(artifacts_dir: &Path) -> anyhow::Result<HashMap<String, EvalTask>> {
    if let Some(manifest) = read_run_manifest(artifacts_dir)? {
        let mut tasks = HashMap::new();
        for s in &manifest.suites {
            for t in eval_tasks_for_suite(s).map_err(anyhow::Error::msg)? {
                tasks.entry(t.id.clone()).or_insert(t);
            }
        }
        if !tasks.is_empty() {
            return Ok(tasks);
        }
    }
    Ok(default_eval_task_union())
}

fn lookup_turn<'a>(
    tasks: &'a HashMap<String, EvalTask>,
    task_id: &str,
    turn_index: usize,
) -> Option<&'a Turn> {
    tasks.get(task_id)?.turns.get(turn_index)
}

pub fn read_turn_metrics_jsonl(path: &Path) -> anyhow::Result<Vec<TurnMetrics>> {
    let f = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        out.push(serde_json::from_str(t).context("parse TurnMetrics JSONL line")?);
    }
    Ok(out)
}

pub fn load_hsm_trace_map(path: &Path) -> anyhow::Result<HashMap<(String, usize), HsmTurnTrace>> {
    let f = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    let mut m = HashMap::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let tr: HsmTurnTrace = serde_json::from_str(t).context("parse HsmTurnTrace JSONL")?;
        m.insert((tr.task_id.clone(), tr.turn_index), tr);
    }
    Ok(m)
}

fn tool_steps_from_eval(response: &str, turn: Option<&Turn>) -> Vec<ToolStepRecord> {
    if let Some((name, params)) = parse_tool_json(response) {
        let args_redacted =
            serde_json::to_string(&redact_params(&params)).unwrap_or_else(|_| "{}".into());
        let ok = match turn.and_then(|t| t.expected_tool.as_ref()) {
            Some(exp) => exp == &name,
            None => true,
        };
        let result_summary = if ok {
            "tool json parsed".into()
        } else {
            "tool name mismatch vs expected".into()
        };
        return vec![ToolStepRecord {
            name,
            args_redacted,
            ok,
            result_summary,
        }];
    }
    if let Some(t) = turn {
        if let Some(ref exp) = t.expected_tool {
            return vec![ToolStepRecord {
                name: exp.clone(),
                args_redacted: "(missing)".into(),
                ok: false,
                result_summary: "expected tool JSON in response".into(),
            }];
        }
    }
    Vec::new()
}

fn eval_outcome(tm: &TurnMetrics, tool_steps: &[ToolStepRecord]) -> (TrajectoryOutcome, f64) {
    let c = tm.rubric_composite.max(tm.keyword_score);
    if tm.error.is_some() {
        return (TrajectoryOutcome::Failure, c);
    }
    if tool_steps.iter().any(|t| !t.ok) {
        return (TrajectoryOutcome::Failure, c);
    }
    if tm.rubric_pass {
        return (TrajectoryOutcome::Success, c);
    }
    if tm.rubric_composite >= 0.45 || tm.keyword_score >= 0.45 {
        return (TrajectoryOutcome::Partial, c);
    }
    (TrajectoryOutcome::Failure, c)
}

/// Build a [`TrajectoryRecord`] from one eval turn + optional HSM trace row.
pub fn trajectory_from_eval_turn(
    tm: &TurnMetrics,
    turn: Option<&Turn>,
    trace: Option<&HsmTurnTrace>,
    suite_label: &str,
    ts_unix: u64,
) -> TrajectoryRecord {
    let user_task = turn
        .map(|t| t.user.clone())
        .unwrap_or_else(|| format!("(task {} turn {})", tm.task_id, tm.turn_index));
    let tool_steps = tool_steps_from_eval(&tm.response, turn);
    let (outcome, confidence) = eval_outcome(tm, &tool_steps);
    let mut skills_used = Vec::new();
    let mut skills_accessed = Vec::new();
    if let Some(tr) = trace {
        if let Some(ref id) = tr.selected_skill_id {
            skills_used.push(id.clone());
            skills_accessed.push(id.clone());
        }
    }
    TrajectoryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        ts_unix,
        user_task: truncate(&user_task, 800),
        outcome,
        confidence,
        turn_route: format!("eval:{suite_label}"),
        council_used: false,
        primary_agent: 0,
        skills_accessed,
        skills_used_ids: skills_used,
        tool_steps,
        response_preview: truncate(&tm.response, 600),
    }
}

fn turns_hsm_jobs(
    artifacts: &Path,
    manifest: Option<&RunManifest>,
) -> Vec<(std::path::PathBuf, String)> {
    let mut out = Vec::new();
    let root = artifacts.join("turns_hsm.jsonl");
    if root.is_file() {
        out.push((root, "artifact_root".into()));
    }
    if let Some(m) = manifest {
        for suite in &m.suites {
            let p = artifacts.join(suite).join("turns_hsm.jsonl");
            if p.is_file() {
                out.push((p, suite.clone()));
            }
        }
    }
    if out.is_empty() {
        if let Ok(rd) = std::fs::read_dir(artifacts) {
            for ent in rd.flatten() {
                let p = ent.path();
                if !p.is_dir() {
                    continue;
                }
                let t = p.join("turns_hsm.jsonl");
                if t.is_file() {
                    let label = p
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "unknown".into());
                    out.push((t, label));
                }
            }
        }
    }
    out
}

/// Read `**/turns_hsm.jsonl` (and optional `hsm_trace.jsonl`) and append [`TrajectoryRecord`] lines to `out`.
pub fn import_eval_artifacts_to_jsonl(artifacts: &Path, out: &Path) -> anyhow::Result<usize> {
    let task_map = task_map_for_artifacts(artifacts)?;
    let manifest = read_run_manifest(artifacts)?;
    let base_ts = manifest.as_ref().map(|m| m.created_unix).unwrap_or(0);
    let jobs = turns_hsm_jobs(artifacts, manifest.as_ref());
    if jobs.is_empty() {
        anyhow::bail!(
            "no turns_hsm.jsonl under {} (expected suite subdirs or artifact_root)",
            artifacts.display()
        );
    }
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
        .with_context(|| out.display().to_string())?;
    let mut n = 0usize;
    let mut seq = 0u64;
    for (turns_path, suite_label) in jobs {
        let suite_dir = turns_path.parent().unwrap_or(artifacts);
        let trace_path = suite_dir.join("hsm_trace.jsonl");
        let traces = if trace_path.is_file() {
            load_hsm_trace_map(&trace_path)?
        } else {
            HashMap::new()
        };
        for tm in read_turn_metrics_jsonl(&turns_path)? {
            let turn = lookup_turn(&task_map, &tm.task_id, tm.turn_index);
            let trace = traces.get(&(tm.task_id.clone(), tm.turn_index));
            let ts = base_ts.saturating_add(seq);
            seq += 1;
            let rec = trajectory_from_eval_turn(&tm, turn, trace, &suite_label, ts);
            writeln!(f, "{}", serde_json::to_string(&rec)?)?;
            n += 1;
        }
    }
    f.flush()?;
    Ok(n)
}
