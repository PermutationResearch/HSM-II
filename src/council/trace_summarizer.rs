//! Trace Summarizer for Council Deliberation
//!
//! Distills stigmergic traces, directives, and policy shifts into concise
//! bullet points for council consumption.

use crate::agent::AgentId;
use crate::social_memory::{DataSensitivity, PromiseStatus, SocialMemory};
use crate::stigmergic_policy::{StigmergicMemory, TraceKind};

/// Summary of recent stigmergic activity for council consumption
#[derive(Clone, Debug, Default)]
pub struct TraceSummary {
    /// Agents with high reliability scores (trusted agents)
    pub trusted_agents: Vec<(AgentId, f64)>,
    /// Recent promise outcomes (kept vs broken)
    pub recent_promise_outcomes: Vec<PromiseOutcomeSummary>,
    /// Agents restricted from sensitive data access
    pub restricted_shares: Vec<RestrictedShare>,
    /// Active routing directives affecting this task type
    pub active_directives: Vec<DirectiveSummary>,
    /// Recent policy shifts
    pub recent_policy_shifts: Vec<PolicyShiftSummary>,
    /// High-confidence traces relevant to the proposal
    pub relevant_traces: Vec<TraceBullet>,
}

#[derive(Clone, Debug)]
pub struct PromiseOutcomeSummary {
    pub agent_id: AgentId,
    pub task_key: String,
    pub status: PromiseStatus,
    pub quality_score: f64,
    pub when: u64,
}

#[derive(Clone, Debug)]
pub struct RestrictedShare {
    pub owner: AgentId,
    pub target: AgentId,
    pub max_sensitivity: DataSensitivity,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct DirectiveSummary {
    pub directive_id: String,
    pub routing_hint: String,
    pub priority: i32,
    pub condition_summary: String,
}

#[derive(Clone, Debug)]
pub struct PolicyShiftSummary {
    pub shift_id: String,
    pub description: String,
    pub scope: String,
    pub confidence: f64,
}

#[derive(Clone, Debug)]
pub struct TraceBullet {
    pub trace_id: String,
    pub agent_id: AgentId,
    pub kind: TraceKind,
    pub summary: String,
    pub confidence: Option<f64>,
}

/// Summarizer that distills stigmergic data for council deliberation
pub struct TraceSummarizer {
    /// Maximum age of traces to include (in ticks)
    pub max_trace_age: u64,
    /// Minimum confidence for trace inclusion
    pub min_confidence: f64,
    /// Maximum items per category
    pub max_items: usize,
}

impl Default for TraceSummarizer {
    fn default() -> Self {
        Self {
            max_trace_age: 1000,
            min_confidence: 0.3,
            max_items: 5,
        }
    }
}

impl TraceSummarizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_age(mut self, ticks: u64) -> Self {
        self.max_trace_age = ticks;
        self
    }

    pub fn with_min_confidence(mut self, confidence: f64) -> Self {
        self.min_confidence = confidence;
        self
    }

    pub fn with_max_items(mut self, items: usize) -> Self {
        self.max_items = items;
        self
    }

    /// Generate comprehensive summary for council deliberation
    pub fn summarize_for_council(
        &self,
        stigmergic_memory: &StigmergicMemory,
        social_memory: Option<&SocialMemory>,
        current_tick: u64,
        task_key: Option<&str>,
    ) -> TraceSummary {
        TraceSummary {
            trusted_agents: self.identify_trusted_agents(social_memory, self.max_items),
            recent_promise_outcomes: self.summarize_recent_promises(
                social_memory,
                task_key,
                self.max_items,
            ),
            restricted_shares: self.identify_restricted_shares(social_memory, self.max_items),
            active_directives: self.summarize_active_directives(
                stigmergic_memory,
                task_key,
                self.max_items,
            ),
            recent_policy_shifts: self.summarize_recent_policy_shifts(
                stigmergic_memory,
                current_tick,
                self.max_items,
            ),
            relevant_traces: self.select_relevant_traces(
                stigmergic_memory,
                current_tick,
                task_key,
                self.max_items,
            ),
        }
    }

    fn identify_trusted_agents(
        &self,
        social_memory: Option<&SocialMemory>,
        max_items: usize,
    ) -> Vec<(AgentId, f64)> {
        let Some(memory) = social_memory else {
            return Vec::new();
        };

        let mut agents: Vec<(AgentId, f64)> = memory
            .reputations
            .values()
            .map(|rep| {
                let score = rep.reliability_score();
                (rep.agent_id, score)
            })
            .filter(|(_, score)| *score >= 0.7)
            .collect();

        agents.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        agents.truncate(max_items);
        agents
    }

    fn summarize_recent_promises(
        &self,
        social_memory: Option<&SocialMemory>,
        task_key: Option<&str>,
        max_items: usize,
    ) -> Vec<PromiseOutcomeSummary> {
        let Some(memory) = social_memory else {
            return Vec::new();
        };

        let mut promises: Vec<PromiseOutcomeSummary> = memory
            .promises
            .values()
            .filter(|p| p.resolved_at.is_some())
            .filter(|p| task_key.map(|tk| p.task_key.contains(tk)).unwrap_or(true))
            .map(|p| PromiseOutcomeSummary {
                agent_id: p.promiser,
                task_key: p.task_key.clone(),
                status: p.status.clone(),
                quality_score: p.quality_score.unwrap_or(0.5),
                when: p.resolved_at.unwrap_or(0),
            })
            .collect();

        promises.sort_by(|a, b| b.when.cmp(&a.when));
        promises.truncate(max_items);
        promises
    }

    fn identify_restricted_shares(
        &self,
        social_memory: Option<&SocialMemory>,
        max_items: usize,
    ) -> Vec<RestrictedShare> {
        let Some(memory) = social_memory else {
            return Vec::new();
        };

        memory
            .share_policies
            .values()
            .filter(|policy| policy.max_sensitivity < DataSensitivity::Confidential)
            .take(max_items)
            .map(|policy| RestrictedShare {
                owner: policy.owner,
                target: policy.target,
                max_sensitivity: policy.max_sensitivity.clone(),
                reason: policy
                    .notes
                    .clone()
                    .unwrap_or_else(|| "Security policy".to_string()),
            })
            .collect()
    }

    fn summarize_active_directives(
        &self,
        stigmergic_memory: &StigmergicMemory,
        task_key: Option<&str>,
        max_items: usize,
    ) -> Vec<DirectiveSummary> {
        let mut directives: Vec<DirectiveSummary> = stigmergic_memory
            .directives
            .values()
            .filter(|d| task_key.map(|tk| d.task_key.contains(tk)).unwrap_or(true))
            .map(|d| DirectiveSummary {
                directive_id: d.task_key.clone(),
                routing_hint: format!("{:?}", d.preferred_agent),
                priority: (d.confidence * 100.0) as i32,
                condition_summary: format!("When {} -> {:?}", d.task_key, d.preferred_agent),
            })
            .collect();

        directives.sort_by(|a, b| b.priority.cmp(&a.priority));
        directives.truncate(max_items);
        directives
    }

    fn summarize_recent_policy_shifts(
        &self,
        stigmergic_memory: &StigmergicMemory,
        current_tick: u64,
        max_items: usize,
    ) -> Vec<PolicyShiftSummary> {
        let mut shifts: Vec<PolicyShiftSummary> = stigmergic_memory
            .policy_shifts
            .iter()
            .filter(|s| current_tick.saturating_sub(s.updated_at) < self.max_trace_age)
            .map(|s| PolicyShiftSummary {
                shift_id: s.id.clone(),
                description: s.rationale.clone(),
                scope: format!("{} -> {:?}", s.category, s.target_agent),
                confidence: s.confidence,
            })
            .collect();

        shifts.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        shifts.truncate(max_items);
        shifts
    }

    fn select_relevant_traces(
        &self,
        stigmergic_memory: &StigmergicMemory,
        current_tick: u64,
        task_key: Option<&str>,
        max_items: usize,
    ) -> Vec<TraceBullet> {
        let mut traces: Vec<TraceBullet> = stigmergic_memory
            .traces
            .iter()
            .filter(|t| current_tick.saturating_sub(t.tick) < self.max_trace_age)
            .filter(|t| {
                t.outcome_score
                    .map(|score| score >= self.min_confidence)
                    .unwrap_or(true)
            })
            .filter(|t| {
                task_key
                    .and_then(|tk| t.task_key.as_ref().map(|k| k.contains(tk)))
                    .unwrap_or(true)
            })
            .map(|t| TraceBullet {
                trace_id: t.id.clone(),
                agent_id: t.agent_id,
                kind: t.kind.clone(),
                summary: t.summary.clone(),
                confidence: t.outcome_score,
            })
            .collect();

        traces.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap()
                .then_with(|| b.trace_id.cmp(&a.trace_id))
        });
        traces.truncate(max_items);
        traces
    }

    /// Format summary as bullet points for LLM prompt injection
    pub fn to_bullet_points(&self, summary: &TraceSummary) -> String {
        let mut lines = Vec::new();

        if !summary.trusted_agents.is_empty() {
            lines.push("## Trusted Agents (High Reliability)".to_string());
            for (agent_id, score) in &summary.trusted_agents {
                lines.push(format!(
                    "- Agent {}: {:.0}% reliability",
                    agent_id,
                    score * 100.0
                ));
            }
            lines.push(String::new());
        }

        if !summary.recent_promise_outcomes.is_empty() {
            lines.push("## Recent Promise Outcomes".to_string());
            for outcome in &summary.recent_promise_outcomes {
                let status_str = match outcome.status {
                    PromiseStatus::Kept => "Kept",
                    PromiseStatus::Broken => "Broken",
                    PromiseStatus::Cancelled => "Cancelled",
                    PromiseStatus::Pending => "Pending",
                };
                lines.push(format!(
                    "- Agent {} on '{}': {} (quality: {:.0}%)",
                    outcome.agent_id,
                    outcome.task_key,
                    status_str,
                    outcome.quality_score * 100.0
                ));
            }
            lines.push(String::new());
        }

        if !summary.restricted_shares.is_empty() {
            lines.push("## Data Sharing Restrictions".to_string());
            for restriction in &summary.restricted_shares {
                lines.push(format!(
                    "- Agent {} -> Agent {}: Max {:?} access ({})",
                    restriction.owner,
                    restriction.target,
                    restriction.max_sensitivity,
                    restriction.reason
                ));
            }
            lines.push(String::new());
        }

        if !summary.active_directives.is_empty() {
            lines.push("## Active Routing Directives".to_string());
            for directive in &summary.active_directives {
                lines.push(format!(
                    "- [{} priority] {} (ID: {})",
                    directive.priority, directive.condition_summary, directive.directive_id
                ));
            }
            lines.push(String::new());
        }

        if !summary.recent_policy_shifts.is_empty() {
            lines.push("## Recent Policy Shifts".to_string());
            for shift in &summary.recent_policy_shifts {
                lines.push(format!(
                    "- [{}] {} (confidence: {:.0}%)",
                    shift.scope,
                    shift.description,
                    shift.confidence * 100.0
                ));
            }
            lines.push(String::new());
        }

        if !summary.relevant_traces.is_empty() {
            lines.push("## Relevant Stigmergic Traces".to_string());
            for trace in &summary.relevant_traces {
                let conf_str = trace
                    .confidence
                    .map(|c| format!(" ({:.0}% confidence)", c * 100.0))
                    .unwrap_or_default();
                lines.push(format!(
                    "- [{:?}] Agent {}: {}{}",
                    trace.kind, trace.agent_id, trace.summary, conf_str
                ));
            }
        }

        if lines.is_empty() {
            "No significant stigmergic context available.".to_string()
        } else {
            lines.join("\n")
        }
    }

    /// Get a one-line summary for quick reference
    pub fn to_one_liner(&self, summary: &TraceSummary) -> String {
        format!(
            "Stigmergic context: {} trusted agents, {} recent promises, {} relevant traces, {} active directives",
            summary.trusted_agents.len(),
            summary.recent_promise_outcomes.len(),
            summary.relevant_traces.len(),
            summary.active_directives.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_summarizer_default() {
        let summarizer = TraceSummarizer::default();
        assert_eq!(summarizer.max_trace_age, 1000);
        assert_eq!(summarizer.min_confidence, 0.3);
        assert_eq!(summarizer.max_items, 5);
    }

    #[test]
    fn test_to_bullet_points_empty() {
        let summarizer = TraceSummarizer::default();
        let summary = TraceSummary::default();
        let output = summarizer.to_bullet_points(&summary);
        assert_eq!(output, "No significant stigmergic context available.");
    }
}
