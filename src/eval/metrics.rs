//! Evaluation metrics — scoring, comparison, and reporting.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::judges::RubricExtras;

/// Metrics collected for a single turn
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TurnMetrics {
    pub task_id: String,
    pub turn_index: usize,
    pub session: u32,
    pub requires_recall: bool,
    /// LLM response text
    pub response: String,
    /// Wall-clock time for this turn (ms)
    pub latency_ms: u64,
    /// Prompt tokens consumed
    pub prompt_tokens: usize,
    /// Completion tokens generated
    pub completion_tokens: usize,
    /// Keyword hit rate (0.0-1.0) — what fraction of expected keywords appeared
    pub keyword_score: f64,
    /// Number of LLM calls made for this turn
    pub llm_calls: u32,
    /// Whether an error occurred
    pub error: Option<String>,
    /// Deterministic pass from keyword rubric
    #[serde(default)]
    pub deterministic_pass: bool,
    /// Combined rubric pass (keywords + grounding + tool + optional LLM judge)
    #[serde(default)]
    pub rubric_pass: bool,
    /// Weighted composite quality score in \[0,1\]
    #[serde(default)]
    pub rubric_composite: f64,
    #[serde(default)]
    pub grounding_applicable: bool,
    #[serde(default)]
    pub grounding_score: f64,
    #[serde(default)]
    pub grounding_pass: bool,
    #[serde(default)]
    pub tool_check_applicable: bool,
    #[serde(default)]
    pub tool_pass: Option<bool>,
    #[serde(default)]
    pub llm_judge_pass: Option<bool>,
    #[serde(default)]
    pub llm_judge_notes: Option<String>,
    /// Same as latency_ms; explicit for cost reports when tokens are missing
    #[serde(default)]
    pub wall_clock_ms: u64,
    /// Outbound LLM HTTP calls for this turn (main + judge)
    #[serde(default)]
    pub llm_http_requests: u32,
}

/// Aggregate metrics for one runner across all tasks
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunnerMetrics {
    pub runner_name: String,
    pub turns: Vec<TurnMetrics>,
    pub total_duration_ms: u64,
}

impl RunnerMetrics {
    pub fn new(name: &str) -> Self {
        Self {
            runner_name: name.to_string(),
            turns: Vec::new(),
            total_duration_ms: 0,
        }
    }

    /// Average keyword score across all turns
    pub fn avg_keyword_score(&self) -> f64 {
        if self.turns.is_empty() { return 0.0; }
        let sum: f64 = self.turns.iter().map(|t| t.keyword_score).sum();
        sum / self.turns.len() as f64
    }

    /// Average keyword score only for turns that require recall
    pub fn avg_recall_score(&self) -> f64 {
        let recall_turns: Vec<&TurnMetrics> = self.turns.iter().filter(|t| t.requires_recall).collect();
        if recall_turns.is_empty() { return 0.0; }
        let sum: f64 = recall_turns.iter().map(|t| t.keyword_score).sum();
        sum / recall_turns.len() as f64
    }

    /// Average keyword score for first-turn (no recall needed)
    pub fn avg_cold_score(&self) -> f64 {
        let cold_turns: Vec<&TurnMetrics> = self.turns.iter().filter(|t| !t.requires_recall).collect();
        if cold_turns.is_empty() { return 0.0; }
        let sum: f64 = cold_turns.iter().map(|t| t.keyword_score).sum();
        sum / cold_turns.len() as f64
    }

    /// Total tokens consumed
    pub fn total_tokens(&self) -> usize {
        self.turns.iter().map(|t| t.prompt_tokens + t.completion_tokens).sum()
    }

    /// Total prompt tokens
    pub fn total_prompt_tokens(&self) -> usize {
        self.turns.iter().map(|t| t.prompt_tokens).sum()
    }

    /// Total LLM calls
    pub fn total_llm_calls(&self) -> u32 {
        self.turns.iter().map(|t| t.llm_calls).sum()
    }

    /// Average latency per turn
    pub fn avg_latency_ms(&self) -> f64 {
        if self.turns.is_empty() { return 0.0; }
        let sum: u64 = self.turns.iter().map(|t| t.latency_ms).sum();
        sum as f64 / self.turns.len() as f64
    }

    /// Error rate
    pub fn error_rate(&self) -> f64 {
        if self.turns.is_empty() { return 0.0; }
        let errors = self.turns.iter().filter(|t| t.error.is_some()).count();
        errors as f64 / self.turns.len() as f64
    }

    pub fn avg_rubric_composite(&self) -> f64 {
        if self.turns.is_empty() {
            return 0.0;
        }
        self.turns.iter().map(|t| t.rubric_composite).sum::<f64>() / self.turns.len() as f64
    }

    pub fn rubric_pass_rate(&self) -> f64 {
        if self.turns.is_empty() {
            return 0.0;
        }
        let n = self.turns.iter().filter(|t| t.rubric_pass).count();
        n as f64 / self.turns.len() as f64
    }

    pub fn total_llm_http_requests(&self) -> u64 {
        self.turns.iter().map(|t| t.llm_http_requests as u64).sum()
    }

    pub fn total_wall_clock_ms(&self) -> u64 {
        self.turns.iter().map(|t| t.wall_clock_ms).sum()
    }

    /// Metrics broken down by domain
    pub fn by_domain(&self, tasks: &[super::tasks::EvalTask]) -> HashMap<String, DomainMetrics> {
        let mut domain_map: HashMap<String, Vec<&TurnMetrics>> = HashMap::new();
        for turn in &self.turns {
            if let Some(task) = tasks.iter().find(|t| t.id == turn.task_id) {
                domain_map.entry(task.domain.clone()).or_default().push(turn);
            }
        }
        domain_map.into_iter().map(|(domain, turns)| {
            let keyword_avg = turns.iter().map(|t| t.keyword_score).sum::<f64>() / turns.len() as f64;
            let recall_turns: Vec<&&TurnMetrics> = turns.iter().filter(|t| t.requires_recall).collect();
            let recall_avg = if recall_turns.is_empty() { 0.0 } else {
                recall_turns.iter().map(|t| t.keyword_score).sum::<f64>() / recall_turns.len() as f64
            };
            let tokens: usize = turns.iter().map(|t| t.prompt_tokens + t.completion_tokens).sum();
            (domain.clone(), DomainMetrics {
                domain: domain,
                turn_count: turns.len(),
                avg_keyword_score: keyword_avg,
                avg_recall_score: recall_avg,
                total_tokens: tokens,
            })
        }).collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainMetrics {
    pub domain: String,
    pub turn_count: usize,
    pub avg_keyword_score: f64,
    pub avg_recall_score: f64,
    pub total_tokens: usize,
}

/// Blend keyword score, grounding, tool shape, and optional LLM judge into \[0,1\].
pub fn turn_rubric_composite(keyword_score: f64, extras: &RubricExtras) -> f64 {
    let mut sum = keyword_score;
    let mut n = 1usize;
    // Always include concision/relevance: penalizes verbose, weakly aligned turns.
    sum += extras.concision_relevance_score;
    n += 1;
    if extras.grounding_applicable {
        sum += extras.grounding_score;
        n += 1;
    }
    if let Some(tp) = extras.tool_pass {
        sum += if tp { 1.0 } else { 0.0 };
        n += 1;
    }
    if let Some(j) = extras.llm_judge_pass {
        sum += if j { 1.0 } else { 0.0 };
        n += 1;
    }
    sum / n as f64
}

/// Score a response against expected keywords (case-insensitive)
pub fn score_keywords(response: &str, expected: &[String]) -> f64 {
    if expected.is_empty() {
        return 1.0;
    }
    let response_lower = response.to_lowercase();
    let hits = expected
        .iter()
        .filter(|kw| response_lower.contains(&kw.to_lowercase()))
        .count();
    hits as f64 / expected.len() as f64
}

/// Comparative report between baseline and HSM-II
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub baseline: RunnerSummary,
    pub hsm: RunnerSummary,
    pub improvement: ImprovementMetrics,
    pub domain_breakdown: Vec<DomainComparison>,
    pub verdict: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunnerSummary {
    pub name: String,
    pub avg_keyword_score: f64,
    pub avg_recall_score: f64,
    pub avg_cold_score: f64,
    pub avg_rubric_composite: f64,
    pub rubric_pass_rate: f64,
    pub total_tokens: usize,
    pub total_prompt_tokens: usize,
    pub total_llm_calls: u32,
    pub total_llm_http_requests: u64,
    pub total_wall_clock_ms: u64,
    pub avg_latency_ms: f64,
    pub error_rate: f64,
    pub total_duration_ms: u64,
}

impl RunnerSummary {
    pub fn from_metrics(m: &RunnerMetrics) -> Self {
        Self {
            name: m.runner_name.clone(),
            avg_keyword_score: m.avg_keyword_score(),
            avg_recall_score: m.avg_recall_score(),
            avg_cold_score: m.avg_cold_score(),
            avg_rubric_composite: m.avg_rubric_composite(),
            rubric_pass_rate: m.rubric_pass_rate(),
            total_tokens: m.total_tokens(),
            total_prompt_tokens: m.total_prompt_tokens(),
            total_llm_calls: m.total_llm_calls(),
            total_llm_http_requests: m.total_llm_http_requests(),
            total_wall_clock_ms: m.total_wall_clock_ms(),
            avg_latency_ms: m.avg_latency_ms(),
            error_rate: m.error_rate(),
            total_duration_ms: m.total_duration_ms,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImprovementMetrics {
    /// Keyword score improvement (positive = HSM better)
    pub keyword_score_delta: f64,
    pub keyword_score_pct: f64,
    /// Recall score improvement
    pub recall_score_delta: f64,
    pub recall_score_pct: f64,
    /// Token reduction (positive = HSM uses fewer)
    pub token_reduction: f64,
    pub token_reduction_pct: f64,
    /// Prompt token reduction
    pub prompt_token_reduction: f64,
    pub prompt_token_reduction_pct: f64,
    /// LLM call reduction
    pub llm_call_reduction: f64,
    pub llm_call_reduction_pct: f64,
    /// Latency change
    pub latency_delta_ms: f64,
    pub latency_delta_pct: f64,
    /// Rubric composite (quality blend incl. grounding/tools/judge)
    pub rubric_composite_delta: f64,
    pub rubric_pass_rate_delta: f64,
    pub llm_http_requests_delta: f64,
    pub wall_clock_ms_delta: f64,
    /// True when at least one side reports non-zero token usage.
    pub token_metrics_available: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomainComparison {
    pub domain: String,
    pub baseline_keyword_score: f64,
    pub hsm_keyword_score: f64,
    pub improvement_pct: f64,
    pub baseline_tokens: usize,
    pub hsm_tokens: usize,
    pub token_reduction_pct: f64,
}

/// Generate a full comparison report
pub fn compare(
    baseline: &RunnerMetrics,
    hsm: &RunnerMetrics,
    tasks: &[super::tasks::EvalTask],
) -> ComparisonReport {
    let b = RunnerSummary::from_metrics(baseline);
    let h = RunnerSummary::from_metrics(hsm);

    let keyword_delta = h.avg_keyword_score - b.avg_keyword_score;
    let recall_delta = h.avg_recall_score - b.avg_recall_score;

    let token_metrics_available = !(b.total_tokens == 0 && h.total_tokens == 0);
    let token_reduction = if token_metrics_available {
        b.total_tokens as f64 - h.total_tokens as f64
    } else {
        0.0
    };
    let prompt_reduction = if token_metrics_available {
        b.total_prompt_tokens as f64 - h.total_prompt_tokens as f64
    } else {
        0.0
    };
    let call_reduction = b.total_llm_calls as f64 - h.total_llm_calls as f64;
    let latency_delta = h.avg_latency_ms - b.avg_latency_ms;
    let rubric_delta = h.avg_rubric_composite - b.avg_rubric_composite;
    let rubric_pass_delta = h.rubric_pass_rate - b.rubric_pass_rate;
    let http_delta = h.total_llm_http_requests as f64 - b.total_llm_http_requests as f64;
    let wall_delta = h.total_wall_clock_ms as f64 - b.total_wall_clock_ms as f64;

    let safe_pct = |delta: f64, base: f64| if base.abs() < 1e-9 { 0.0 } else { (delta / base) * 100.0 };

    let improvement = ImprovementMetrics {
        keyword_score_delta: keyword_delta,
        keyword_score_pct: safe_pct(keyword_delta, b.avg_keyword_score),
        recall_score_delta: recall_delta,
        recall_score_pct: safe_pct(recall_delta, b.avg_recall_score),
        token_reduction,
        token_reduction_pct: safe_pct(token_reduction, b.total_tokens as f64),
        prompt_token_reduction: prompt_reduction,
        prompt_token_reduction_pct: safe_pct(prompt_reduction, b.total_prompt_tokens as f64),
        llm_call_reduction: call_reduction,
        llm_call_reduction_pct: safe_pct(call_reduction, b.total_llm_calls as f64),
        latency_delta_ms: latency_delta,
        latency_delta_pct: safe_pct(latency_delta, b.avg_latency_ms),
        rubric_composite_delta: rubric_delta,
        rubric_pass_rate_delta: rubric_pass_delta,
        llm_http_requests_delta: http_delta,
        wall_clock_ms_delta: wall_delta,
        token_metrics_available,
    };

    // Domain breakdown
    let b_domains = baseline.by_domain(tasks);
    let h_domains = hsm.by_domain(tasks);
    let mut domain_breakdown = Vec::new();
    for (domain, bd) in &b_domains {
        if let Some(hd) = h_domains.get(domain) {
            domain_breakdown.push(DomainComparison {
                domain: domain.clone(),
                baseline_keyword_score: bd.avg_keyword_score,
                hsm_keyword_score: hd.avg_keyword_score,
                improvement_pct: safe_pct(hd.avg_keyword_score - bd.avg_keyword_score, bd.avg_keyword_score),
                baseline_tokens: bd.total_tokens,
                hsm_tokens: hd.total_tokens,
                token_reduction_pct: safe_pct(bd.total_tokens as f64 - hd.total_tokens as f64, bd.total_tokens as f64),
            });
        }
    }
    domain_breakdown.sort_by(|a, b| b.improvement_pct.partial_cmp(&a.improvement_pct).unwrap_or(std::cmp::Ordering::Equal));

    // Verdict
    let mut components = vec![improvement.keyword_score_pct, improvement.recall_score_pct];
    if improvement.token_metrics_available {
        components.push(improvement.token_reduction_pct);
    }
    let overall_improvement = if components.is_empty() {
        0.0
    } else {
        components.iter().sum::<f64>() / components.len() as f64
    };
    let verdict = if overall_improvement >= 30.0 {
        format!("VALIDATED: {:.1}% average improvement across quality, recall, and cost. Exceeds 30% threshold.", overall_improvement)
    } else if overall_improvement >= 15.0 {
        format!("PROMISING: {:.1}% average improvement. Approaching but not yet at 30% threshold.", overall_improvement)
    } else if overall_improvement > 0.0 {
        format!("MARGINAL: {:.1}% average improvement. Below fundability threshold.", overall_improvement)
    } else {
        format!("NO IMPROVEMENT: {:.1}% — HSM-II did not outperform baseline.", overall_improvement)
    };

    ComparisonReport {
        baseline: b,
        hsm: h,
        improvement,
        domain_breakdown,
        verdict,
    }
}

/// Print a formatted comparison report to stdout
pub fn print_report(report: &ComparisonReport) {
    println!("\n{:=<90}", "");
    println!("  HSM-II vs Baseline — Comparative Evaluation Report");
    println!("{:=<90}", "");

    println!("\n{:<40} {:>20} {:>20}", "", "Baseline", "HSM-II");
    println!("{:-<90}", "");
    println!("{:<40} {:>19.1}% {:>19.1}%", "Avg keyword score", report.baseline.avg_keyword_score * 100.0, report.hsm.avg_keyword_score * 100.0);
    println!("{:<40} {:>19.1}% {:>19.1}%", "Avg recall score (cross-session)", report.baseline.avg_recall_score * 100.0, report.hsm.avg_recall_score * 100.0);
    println!("{:<40} {:>19.1}% {:>19.1}%", "Avg cold score (no recall needed)", report.baseline.avg_cold_score * 100.0, report.hsm.avg_cold_score * 100.0);
    println!("{:<40} {:>20} {:>20}", "Total tokens", report.baseline.total_tokens, report.hsm.total_tokens);
    println!("{:<40} {:>20} {:>20}", "Total prompt tokens", report.baseline.total_prompt_tokens, report.hsm.total_prompt_tokens);
    println!("{:<40} {:>20} {:>20}", "Total LLM calls", report.baseline.total_llm_calls, report.hsm.total_llm_calls);
    println!("{:<40} {:>18.0}ms {:>18.0}ms", "Avg latency/turn", report.baseline.avg_latency_ms, report.hsm.avg_latency_ms);
    println!("{:<40} {:>19.1}% {:>19.1}%", "Error rate", report.baseline.error_rate * 100.0, report.hsm.error_rate * 100.0);
    println!("{:<40} {:>20.3} {:>20.3}", "Avg rubric composite", report.baseline.avg_rubric_composite, report.hsm.avg_rubric_composite);
    println!("{:<40} {:>19.1}% {:>19.1}%", "Rubric pass rate", report.baseline.rubric_pass_rate * 100.0, report.hsm.rubric_pass_rate * 100.0);
    println!("{:<40} {:>20} {:>20}", "Total LLM HTTP reqs", report.baseline.total_llm_http_requests, report.hsm.total_llm_http_requests);
    println!("{:<40} {:>20} {:>20}", "Total wall-clock (turns ms)", report.baseline.total_wall_clock_ms, report.hsm.total_wall_clock_ms);

    println!("\n--- Improvements (positive = HSM-II better) ---");
    println!("{:<40} {:>+10.1}% (delta: {:>+.3})", "Keyword quality", report.improvement.keyword_score_pct, report.improvement.keyword_score_delta);
    println!("{:<40} {:>+10.1}% (delta: {:>+.3})", "Cross-session recall", report.improvement.recall_score_pct, report.improvement.recall_score_delta);
    println!(
        "{:<40} {:>+10.1}% ({:+} total tok vs baseline; + = HSM lower)",
        "Total tokens (Δ)",
        report.improvement.token_reduction_pct,
        report.improvement.token_reduction as i64,
    );
    println!(
        "{:<40} {:>+10.1}% ({:+} prompt tok vs baseline; + = HSM lower)",
        "Prompt tokens (Δ)",
        report.improvement.prompt_token_reduction_pct,
        report.improvement.prompt_token_reduction as i64,
    );
    println!("{:<40} {:>+10.1}% ({:.0} calls saved)", "LLM call reduction", report.improvement.llm_call_reduction_pct, report.improvement.llm_call_reduction);
    println!("{:<40} {:>+10.1}% ({:>+.0}ms)", "Latency", report.improvement.latency_delta_pct, report.improvement.latency_delta_ms);
    println!("{:<40} {:>+10.3} (Δ pass rate {:>+.3})", "Rubric composite", report.improvement.rubric_composite_delta, report.improvement.rubric_pass_rate_delta);
    println!("{:<40} {:>+10.1} ({:>+.0} extra reqs)", "LLM HTTP requests", report.improvement.llm_http_requests_delta, report.improvement.llm_http_requests_delta);
    println!("{:<40} {:>+10.0}ms", "Wall-clock (sum turns)", report.improvement.wall_clock_ms_delta);
    if !report.improvement.token_metrics_available {
        println!("{:<40} {}", "Token accounting", "unavailable (excluded from verdict)");
    }

    if !report.domain_breakdown.is_empty() {
        println!("\n--- Domain Breakdown ---");
        println!("{:<25} {:>12} {:>12} {:>12} {:>12}", "Domain", "Base Score", "HSM Score", "Quality +%", "Token -%");
        println!("{:-<75}", "");
        for d in &report.domain_breakdown {
            println!("{:<25} {:>11.1}% {:>11.1}% {:>+11.1}% {:>+11.1}%",
                d.domain, d.baseline_keyword_score * 100.0, d.hsm_keyword_score * 100.0,
                d.improvement_pct, d.token_reduction_pct);
        }
    }

    println!("\n{:=<90}", "");
    println!("  VERDICT: {}", report.verdict);
    println!("{:=<90}\n", "");
}
