use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::agent::{Agent, AgentId};
use crate::graph_runtime::{GraphRuntime, GraphToolKind};
use crate::social_memory::{DataSensitivity, SocialMemory};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TraceKind {
    PromiseMade,
    PromiseResolved,
    DeliveryRecorded,
    QueryPlanned,
    QueryExecuted,
    MemoryShared,
    CouncilDecision,
    Observation,
}

impl TraceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PromiseMade => "PromiseMade",
            Self::PromiseResolved => "PromiseResolved",
            Self::DeliveryRecorded => "DeliveryRecorded",
            Self::QueryPlanned => "QueryPlanned",
            Self::QueryExecuted => "QueryExecuted",
            Self::MemoryShared => "MemoryShared",
            Self::CouncilDecision => "CouncilDecision",
            Self::Observation => "Observation",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "PromiseMade" => Self::PromiseMade,
            "PromiseResolved" => Self::PromiseResolved,
            "DeliveryRecorded" => Self::DeliveryRecorded,
            "QueryPlanned" => Self::QueryPlanned,
            "QueryExecuted" => Self::QueryExecuted,
            "MemoryShared" => Self::MemoryShared,
            "CouncilDecision" => Self::CouncilDecision,
            _ => Self::Observation,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StigmergicTrace {
    pub id: String,
    pub agent_id: AgentId,
    pub model_id: String,
    pub task_key: Option<String>,
    pub kind: TraceKind,
    pub summary: String,
    pub success: Option<bool>,
    pub outcome_score: Option<f64>,
    pub sensitivity: DataSensitivity,
    pub planned_tool: Option<String>,
    pub recorded_at: u64,
    pub tick: u64,
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoutingDirective {
    pub task_key: String,
    pub preferred_agent: Option<AgentId>,
    pub preferred_tool: String,
    pub minimum_sensitivity: DataSensitivity,
    pub confidence: f64,
    pub rationale: String,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyShift {
    pub id: String,
    pub category: String,
    pub target_agent: Option<AgentId>,
    pub target_task: Option<String>,
    pub value: String,
    pub confidence: f64,
    pub rationale: String,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StigmergicMemory {
    pub traces: Vec<StigmergicTrace>,
    pub directives: HashMap<String, RoutingDirective>,
    pub policy_shifts: Vec<PolicyShift>,
    pub next_trace_id: u64,
    pub next_policy_id: u64,
    pub last_applied_tick: u64,
}

impl StigmergicMemory {
    pub fn record_trace(
        &mut self,
        agent_id: AgentId,
        model_id: impl Into<String>,
        task_key: Option<&str>,
        kind: TraceKind,
        summary: impl Into<String>,
        success: Option<bool>,
        outcome_score: Option<f64>,
        sensitivity: DataSensitivity,
        planned_tool: Option<GraphToolKind>,
        recorded_at: u64,
        tick: u64,
        metadata: HashMap<String, String>,
    ) -> String {
        let trace_id = format!("trace-{}", self.next_trace_id);
        self.next_trace_id += 1;
        self.traces.push(StigmergicTrace {
            id: trace_id.clone(),
            agent_id,
            model_id: model_id.into(),
            task_key: task_key.map(|s| s.to_string()),
            kind,
            summary: summary.into(),
            success,
            outcome_score,
            sensitivity,
            planned_tool: planned_tool.map(|tool| GraphRuntime::tool_name(&tool).to_string()),
            recorded_at,
            tick,
            metadata,
        });
        self.trim_traces();
        trace_id
    }

    pub fn directive_for(&self, task_key: &str) -> Option<&RoutingDirective> {
        self.directives.get(task_key)
    }

    pub fn preferred_agent_for(&self, task_key: &str) -> Option<AgentId> {
        self.directive_for(task_key)
            .and_then(|directive| directive.preferred_agent)
    }

    pub fn preferred_tool_for(&self, task_key: &str) -> Option<GraphToolKind> {
        self.directive_for(task_key)
            .and_then(|directive| GraphRuntime::parse_tool_name(&directive.preferred_tool))
    }

    pub fn is_agent_restricted(&self, agent_id: AgentId) -> bool {
        self.policy_shifts.iter().any(|shift| {
            shift.category == "share_restriction"
                && shift.target_agent == Some(agent_id)
                && shift.confidence >= 0.6
        })
    }

    pub fn apply_cycle(&mut self, agents: &mut [Agent], social_memory: &SocialMemory, tick: u64) {
        self.last_applied_tick = tick;

        let mut task_keys: BTreeSet<String> = self
            .traces
            .iter()
            .filter_map(|trace| trace.task_key.clone())
            .collect();
        for promise in social_memory.promises.values() {
            task_keys.insert(promise.task_key.clone());
        }
        for reputation in social_memory.reputations.values() {
            for task_key in reputation.capability_profiles.keys() {
                task_keys.insert(task_key.clone());
            }
        }

        self.directives.clear();
        let candidates: Vec<(AgentId, f64)> =
            agents.iter().map(|agent| (agent.id, agent.jw)).collect();
        for task_key in task_keys {
            let sensitivity = max_sensitivity_for_task(social_memory, &task_key);
            let recommendation = social_memory.recommend_delegate(
                &candidates,
                Some(task_key.as_str()),
                None,
                Some(sensitivity.clone()),
            );
            let tool_plan = GraphRuntime::plan(&task_key);
            let confidence = recommendation
                .as_ref()
                .map(|candidate| candidate.score)
                .unwrap_or(0.45)
                .clamp(0.0, 1.0);
            let rationale = recommendation
                .as_ref()
                .map(|candidate| {
                    format!(
                        "delegate={} observed={:.2} capability={:.2} collaboration={:.2}; tool={}",
                        candidate.agent_id,
                        candidate.components.observed_score,
                        candidate.components.capability_score,
                        candidate.components.collaboration_score,
                        GraphRuntime::tool_name(&tool_plan.tool)
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "cold-start directive from graph runtime; tool={}",
                        GraphRuntime::tool_name(&tool_plan.tool)
                    )
                });
            self.directives.insert(
                task_key.clone(),
                RoutingDirective {
                    task_key,
                    preferred_agent: recommendation.as_ref().map(|candidate| candidate.agent_id),
                    preferred_tool: GraphRuntime::tool_name(&tool_plan.tool).to_string(),
                    minimum_sensitivity: sensitivity,
                    confidence,
                    rationale,
                    updated_at: tick,
                },
            );
        }

        for agent in agents {
            let reputation = social_memory.reputation_score(agent.id, agent.jw);
            let recent_failures = self
                .traces
                .iter()
                .rev()
                .take(64)
                .filter(|trace| trace.agent_id == agent.id && trace.success == Some(false))
                .count() as f64;
            let recent_successes = self
                .traces
                .iter()
                .rev()
                .take(64)
                .filter(|trace| trace.agent_id == agent.id && trace.success == Some(true))
                .count() as f64;
            let trace_delta = ((recent_successes - recent_failures) * 0.05).clamp(-0.25, 0.25);
            agent.bid_bias = (0.7 + reputation + trace_delta).clamp(0.25, 1.8);
        }

        self.policy_shifts.clear();
        let restricted_agents: HashSet<AgentId> = social_memory
            .reputations
            .values()
            .filter(|reputation| reputation.unsafe_shares > reputation.safe_shares)
            .map(|reputation| reputation.agent_id)
            .collect();
        for agent_id in restricted_agents {
            self.push_policy_shift(
                "share_restriction",
                Some(agent_id),
                None,
                "confidential-and-secret-blocked",
                0.8,
                "unsafe sharing exceeded safe sharing in observed history",
                tick,
            );
        }

        let unstable_tasks: Vec<String> = self
            .directives
            .values()
            .filter(|directive| directive.confidence < 0.5)
            .map(|directive| directive.task_key.clone())
            .collect();
        for task_key in unstable_tasks {
            self.push_policy_shift(
                "council_review",
                None,
                Some(task_key),
                "require-multi-agent-review",
                0.65,
                "delegation confidence is low; route through council-style review",
                tick,
            );
        }
    }

    fn push_policy_shift(
        &mut self,
        category: &str,
        target_agent: Option<AgentId>,
        target_task: Option<String>,
        value: &str,
        confidence: f64,
        rationale: &str,
        updated_at: u64,
    ) {
        let id = format!("policy-{}", self.next_policy_id);
        self.next_policy_id += 1;
        self.policy_shifts.push(PolicyShift {
            id,
            category: category.to_string(),
            target_agent,
            target_task,
            value: value.to_string(),
            confidence,
            rationale: rationale.to_string(),
            updated_at,
        });
    }

    fn trim_traces(&mut self) {
        const MAX_TRACES: usize = 2048;
        if self.traces.len() > MAX_TRACES {
            let excess = self.traces.len() - MAX_TRACES;
            self.traces.drain(0..excess);
        }
    }
}

fn max_sensitivity_for_task(social_memory: &SocialMemory, task_key: &str) -> DataSensitivity {
    social_memory
        .promises
        .values()
        .filter(|promise| promise.task_key == task_key)
        .map(|promise| promise.sensitivity.clone())
        .max()
        .unwrap_or(DataSensitivity::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Drives, Role};

    #[test]
    fn cycle_creates_directives_and_policy_shifts() {
        let mut memory = StigmergicMemory::default();
        let mut agents = vec![
            Agent {
                id: 1,
                drives: Drives {
                    curiosity: 0.8,
                    harmony: 0.7,
                    growth: 0.9,
                    transcendence: 0.5,
                },
                learning_rate: 0.05,
                description: String::new(),
                role: Role::Architect,
                bid_bias: 1.0,
                jw: 0.9,
            },
            Agent {
                id: 2,
                drives: Drives {
                    curiosity: 0.6,
                    harmony: 0.6,
                    growth: 0.7,
                    transcendence: 0.4,
                },
                learning_rate: 0.05,
                description: String::new(),
                role: Role::Coder,
                bid_bias: 1.0,
                jw: 0.4,
            },
        ];
        let mut social = SocialMemory::default();
        social.record_delivery(1, "compile code", true, 0.9, true, true, 1, &[2]);
        social.record_delivery(2, "compile code", false, 0.2, false, false, 1, &[1]);
        memory.record_trace(
            1,
            "model-a",
            Some("compile code"),
            TraceKind::DeliveryRecorded,
            "compiled successfully",
            Some(true),
            Some(0.9),
            DataSensitivity::Internal,
            Some(GraphToolKind::CypherLikeQuery),
            1,
            1,
            HashMap::new(),
        );

        memory.apply_cycle(&mut agents, &social, 2);
        assert!(memory.directive_for("compile code").is_some());
        assert!(memory.preferred_agent_for("compile code").is_some());
        assert!(agents[0].bid_bias >= agents[1].bid_bias);
        assert!(memory.is_agent_restricted(2));
    }
}
