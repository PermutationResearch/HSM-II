use std::collections::BTreeMap;

use serde::Serialize;

use super::hsm_native_tasks::{HsmNativeGold, HsmNativeTask};

const SMB_NAME: &str = "Stigmergic Memory Benchmark";

#[derive(Clone, Debug, Serialize)]
pub struct HsmNativeTaskResult {
    pub benchmark: String,
    pub suite: String,
    pub variant: String,
    pub task_id: String,
    pub hypothesis: String,
    pub answer_accuracy: f64,
    pub required_fact_recall: f64,
    pub stale_fact_suppression: f64,
    pub handoff_success: f64,
    pub policy_consistency: f64,
    pub explanation_grounding: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct HsmNativeSuiteSummary {
    pub benchmark: String,
    pub suite: String,
    pub variant: String,
    pub n_tasks: usize,
    pub answer_accuracy: f64,
    pub required_fact_recall: f64,
    pub stale_fact_suppression: f64,
    pub handoff_success: f64,
    pub policy_consistency: f64,
    pub explanation_grounding: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct HsmNativeReport {
    pub benchmark: String,
    pub variant: String,
    pub n_tasks: usize,
    pub answer_accuracy: f64,
    pub required_fact_recall: f64,
    pub stale_fact_suppression: f64,
    pub handoff_success: f64,
    pub policy_consistency: f64,
    pub explanation_grounding: f64,
    pub suites: Vec<HsmNativeSuiteSummary>,
}

fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

fn contains_fact(text: &str, fact: &str) -> bool {
    let text = normalize(text);
    let fact = normalize(fact);
    if fact.is_empty() {
        return false;
    }
    if text.contains(&fact) {
        return true;
    }
    let text_tokens = text.split_whitespace().collect::<Vec<_>>();
    let fact_tokens = fact.split_whitespace().collect::<Vec<_>>();
    if fact_tokens.is_empty() {
        return false;
    }
    fact_tokens.iter().all(|ft| {
        text_tokens.iter().any(|tt| {
            tt == ft
                || tt.starts_with(ft)
                || ft.starts_with(tt)
                || (tt.len() >= 4 && ft.len() >= 4 && tt[..4] == ft[..4])
        })
    })
}

fn correction_context_present(hypothesis: &str, fact: &str) -> bool {
    let hyp = normalize(hypothesis);
    let fact = normalize(fact);
    if fact.is_empty() || !hyp.contains(&fact) {
        return false;
    }
    let correction_markers = [
        "updated",
        "update",
        "corrected",
        "correction",
        "revised",
        "revision",
        "changed",
        "moved",
        "instead",
        "rather than",
        "no longer",
        "later session",
        "newer session",
        "final decision",
        "final",
    ];
    correction_markers.iter().any(|marker| hyp.contains(marker))
}

fn answer_accuracy(gold: &HsmNativeGold, hypothesis: &str) -> f64 {
    let gold_norm = normalize(&gold.answer);
    let hyp_norm = normalize(hypothesis);
    if gold_norm.is_empty() || hyp_norm.is_empty() {
        return 0.0;
    }
    if hyp_norm.contains(&gold_norm) || gold_norm.contains(&hyp_norm) {
        return 1.0;
    }
    if !gold.required_facts.is_empty()
        && gold
            .required_facts
            .iter()
            .all(|fact| contains_fact(hypothesis, fact))
    {
        return 1.0;
    }
    0.0
}

fn required_fact_recall(gold: &HsmNativeGold, hypothesis: &str) -> f64 {
    if gold.required_facts.is_empty() {
        return 1.0;
    }
    let hits = gold
        .required_facts
        .iter()
        .filter(|fact| contains_fact(hypothesis, fact))
        .count();
    hits as f64 / gold.required_facts.len() as f64
}

fn stale_fact_suppression(gold: &HsmNativeGold, hypothesis: &str) -> f64 {
    if gold.forbidden_stale_facts.is_empty() {
        return 1.0;
    }
    let leaks = gold
        .forbidden_stale_facts
        .iter()
        .filter(|fact| {
            contains_fact(hypothesis, fact) && !correction_context_present(hypothesis, fact)
        })
        .count();
    if leaks == 0 {
        1.0
    } else {
        0.0
    }
}

pub fn score_task(task: &HsmNativeTask, variant: &str, hypothesis: &str) -> HsmNativeTaskResult {
    let answer_accuracy = answer_accuracy(&task.gold, hypothesis);
    let required_fact_recall = required_fact_recall(&task.gold, hypothesis);
    let stale_fact_suppression = stale_fact_suppression(&task.gold, hypothesis);
    let handoff_success =
        if task.suite == "agent_handoff" && answer_accuracy == 1.0 && required_fact_recall >= 1.0 {
            1.0
        } else if task.suite == "agent_handoff" {
            0.0
        } else {
            1.0
        };
    let policy_consistency = if task.suite == "policy_persistence" {
        (required_fact_recall + stale_fact_suppression) / 2.0
    } else {
        1.0
    };
    let explanation_grounding = if task.gold.required_facts.is_empty() {
        answer_accuracy
    } else {
        required_fact_recall
    };
    HsmNativeTaskResult {
        benchmark: SMB_NAME.into(),
        suite: task.suite.clone(),
        variant: variant.to_string(),
        task_id: task.id.clone(),
        hypothesis: hypothesis.to_string(),
        answer_accuracy,
        required_fact_recall,
        stale_fact_suppression,
        handoff_success,
        policy_consistency,
        explanation_grounding,
    }
}

fn average_by<F>(rows: &[HsmNativeTaskResult], f: F) -> f64
where
    F: Fn(&HsmNativeTaskResult) -> f64,
{
    if rows.is_empty() {
        return 0.0;
    }
    rows.iter().map(f).sum::<f64>() / rows.len() as f64
}

pub fn summarize_results(variant: &str, rows: &[HsmNativeTaskResult]) -> HsmNativeReport {
    let mut by_suite: BTreeMap<String, Vec<HsmNativeTaskResult>> = BTreeMap::new();
    for row in rows {
        by_suite
            .entry(row.suite.clone())
            .or_default()
            .push(row.clone());
    }
    let suites = by_suite
        .into_iter()
        .map(|(suite, rows)| HsmNativeSuiteSummary {
            benchmark: SMB_NAME.into(),
            suite,
            variant: variant.to_string(),
            n_tasks: rows.len(),
            answer_accuracy: average_by(&rows, |r| r.answer_accuracy),
            required_fact_recall: average_by(&rows, |r| r.required_fact_recall),
            stale_fact_suppression: average_by(&rows, |r| r.stale_fact_suppression),
            handoff_success: average_by(&rows, |r| r.handoff_success),
            policy_consistency: average_by(&rows, |r| r.policy_consistency),
            explanation_grounding: average_by(&rows, |r| r.explanation_grounding),
        })
        .collect::<Vec<_>>();
    HsmNativeReport {
        benchmark: SMB_NAME.into(),
        variant: variant.to_string(),
        n_tasks: rows.len(),
        answer_accuracy: average_by(rows, |r| r.answer_accuracy),
        required_fact_recall: average_by(rows, |r| r.required_fact_recall),
        stale_fact_suppression: average_by(rows, |r| r.stale_fact_suppression),
        handoff_success: average_by(rows, |r| r.handoff_success),
        policy_consistency: average_by(rows, |r| r.policy_consistency),
        explanation_grounding: average_by(rows, |r| r.explanation_grounding),
        suites,
    }
}
