//! Multi-Agent Council System with Debate, Orchestrate, and Simple modes.
//!
//! The council provides three coordination modes:
//! - **Debate**: Full deliberation with pros/cons, structured for complex decisions
//! - **Orchestrate**: Hierarchical command with sub-task delegation for urgent/complex tasks  
//! - **Simple**: Direct voting with minimal overhead for routine decisions
//!
//! Mode selection is automatic based on decision complexity, urgency, and agent availability.
//! Integration with RooDB persists council decisions; LARS cascades trigger council formation.

use crate::agent::{AgentId, Role};
use crate::graph_runtime::GraphToolKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub mod debate;
pub mod llm_deliberation;
pub mod mode_switcher;
pub mod orchestrate;
pub mod ralph;
pub mod simple;
pub mod trace_summarizer;

pub use debate::{Argument, DebateCouncil, DebateRound};
pub use llm_deliberation::{
    DebatePhase, LLMArgument, LLMDebateCouncil, LLMDeliberationConfig, Stance,
};
pub use mode_switcher::{
    CouncilMode, ModeConfig, ModeScoreBreakdown, ModeSelectionReport, ModeSwitchEvent, ModeSwitcher,
};
pub use orchestrate::{Command, OrchestratorCouncil, SubTask};
pub use ralph::{
    AgentConfig, RalphConfig, RalphCouncil, RalphIteration, RalphState, RalphVerdict,
    RalphVerdict as RalphDecision,
};
pub use simple::{SimpleCouncil, Vote};
pub use trace_summarizer::{TraceSummarizer, TraceSummary, TraceBullet};

/// Unique identifier for a council session
pub type CouncilId = uuid::Uuid;

/// A proposal being evaluated by the council
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Proposal {
    pub id: String,
    pub title: String,
    pub description: String,
    pub proposer: AgentId,
    pub proposed_at: u64,
    /// Estimated complexity score (0-1)
    pub complexity: f64,
    /// Time sensitivity (0-1, higher = more urgent)
    pub urgency: f64,
    /// Required roles for proper evaluation
    pub required_roles: Vec<Role>,
    #[serde(default)]
    pub task_key: Option<String>,
    #[serde(default)]
    pub stigmergic_context: Option<StigmergicCouncilContext>,
}

impl Proposal {
    pub fn new(id: &str, title: &str, description: &str, proposer: AgentId) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            proposer,
            proposed_at: current_timestamp(),
            complexity: 0.5,
            urgency: 0.5,
            required_roles: vec![Role::Architect, Role::Catalyst, Role::Chronicler],
            task_key: Some(title.to_lowercase()),
            stigmergic_context: None,
        }
    }

    pub fn with_stigmergic_context(mut self, context: StigmergicCouncilContext) -> Self {
        self.stigmergic_context = Some(context);
        self
    }

    /// Estimate complexity based on description length and keywords
    pub fn estimate_complexity(&mut self) {
        let desc_lower = self.description.to_lowercase();
        let complexity_keywords = [
            "integrate",
            "federate",
            "synthesize",
            "recursive",
            "multi",
            "complex",
        ];
        let simple_keywords = ["simple", "routine", "standard", "basic", "minor"];

        let complex_count = complexity_keywords
            .iter()
            .filter(|kw| desc_lower.contains(*kw))
            .count();
        let simple_count = simple_keywords
            .iter()
            .filter(|kw| desc_lower.contains(*kw))
            .count();

        let length_factor = (self.description.len() as f64 / 500.0).min(1.0);
        let keyword_factor = (complex_count as f64 - simple_count as f64 * 0.5).max(0.0) / 5.0;

        self.complexity = (0.3 + length_factor * 0.4 + keyword_factor * 0.3).min(1.0);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StigmergicCouncilContext {
    pub preferred_agent: Option<AgentId>,
    pub preferred_tool: Option<GraphToolKind>,
    pub confidence: f64,
    pub require_council_review: bool,
    pub rationale: String,
    #[serde(default)]
    pub evidence: Vec<CouncilEvidence>,
    #[serde(default)]
    pub graph_snapshot_bullets: Vec<String>,
    #[serde(default)]
    pub graph_queries: Vec<CouncilGraphQuery>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CouncilEvidenceKind {
    Trace,
    Directive,
    PolicyShift,
    GraphQuery,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CouncilEvidence {
    pub id: String,
    pub kind: CouncilEvidenceKind,
    pub summary: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CouncilGraphQuery {
    pub purpose: String,
    pub query: String,
    #[serde(default)]
    pub evidence: Vec<CouncilEvidence>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct CouncilDecisionMetadata {
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub trace_ids: Vec<String>,
    #[serde(default)]
    pub directive_ids: Vec<String>,
    #[serde(default)]
    pub policy_shift_ids: Vec<String>,
    #[serde(default)]
    pub graph_queries: Vec<String>,
    #[serde(default)]
    pub graph_snapshot_bullets: Vec<String>,
}

impl CouncilDecisionMetadata {
    pub fn record_evidence(&mut self, evidence: &CouncilEvidence) {
        self.evidence_ids.push(evidence.id.clone());
        match evidence.kind {
            CouncilEvidenceKind::Trace => self.trace_ids.push(evidence.id.clone()),
            CouncilEvidenceKind::Directive => self.directive_ids.push(evidence.id.clone()),
            CouncilEvidenceKind::PolicyShift => self.policy_shift_ids.push(evidence.id.clone()),
            CouncilEvidenceKind::GraphQuery => {}
        }
    }

    pub fn record_query(&mut self, query: &CouncilGraphQuery) {
        self.graph_queries.push(query.query.clone());
        for evidence in &query.evidence {
            self.record_evidence(evidence);
        }
    }

    pub fn dedupe(&mut self) {
        dedupe_strings(&mut self.evidence_ids);
        dedupe_strings(&mut self.trace_ids);
        dedupe_strings(&mut self.directive_ids);
        dedupe_strings(&mut self.policy_shift_ids);
        dedupe_strings(&mut self.graph_queries);
        dedupe_strings(&mut self.graph_snapshot_bullets);
    }
}

impl StigmergicCouncilContext {
    pub fn all_evidence(&self) -> Vec<CouncilEvidence> {
        let mut combined = self.evidence.clone();
        for query in &self.graph_queries {
            combined.extend(query.evidence.clone());
        }

        let mut seen = BTreeSet::new();
        combined.retain(|evidence| seen.insert(evidence.id.clone()));
        combined
    }

    pub fn audit_metadata(&self) -> CouncilDecisionMetadata {
        let mut metadata = CouncilDecisionMetadata {
            graph_snapshot_bullets: self.graph_snapshot_bullets.clone(),
            ..CouncilDecisionMetadata::default()
        };
        for evidence in &self.evidence {
            metadata.record_evidence(evidence);
        }
        for query in &self.graph_queries {
            metadata.record_query(query);
        }
        metadata.dedupe();
        metadata
    }
}

/// Agent participating in a council with assigned role
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouncilMember {
    pub agent_id: AgentId,
    pub role: Role,
    pub expertise_score: f64,
    pub participation_weight: f64,
}

/// Outcome of a council evaluation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouncilDecision {
    pub council_id: CouncilId,
    pub proposal_id: String,
    pub decision: Decision,
    pub confidence: f64,
    pub participating_agents: Vec<AgentId>,
    pub execution_plan: Option<ExecutionPlan>,
    pub decided_at: u64,
    pub mode_used: CouncilMode,
    #[serde(default)]
    pub metadata: CouncilDecisionMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Decision {
    Approve,
    Reject,
    Amend { amended_proposal: Proposal },
    Defer { reason: String },
}

/// Plan for executing an approved proposal
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub steps: Vec<ExecutionStep>,
    pub estimated_duration_ms: u64,
    pub rollback_strategy: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionStep {
    pub sequence: usize,
    pub description: String,
    pub assigned_agent: Option<AgentId>,
    pub dependencies: Vec<usize>,
}

/// Unified council interface that dispatches to specific mode implementations
pub struct Council {
    pub id: CouncilId,
    pub mode: CouncilMode,
    pub members: Vec<CouncilMember>,
    pub proposal: Proposal,
    debate_instance: Option<DebateCouncil>,
    orchestrate_instance: Option<OrchestratorCouncil>,
    simple_instance: Option<SimpleCouncil>,
    llm_instance: Option<llm_deliberation::LLMDebateCouncil>,
    ralph_instance: Option<ralph::RalphCouncil>,
}

impl Council {
    pub fn new(mode: CouncilMode, proposal: Proposal, members: Vec<CouncilMember>) -> Self {
        let id = uuid::Uuid::new_v4();

        let mut council = Self {
            id,
            mode: mode.clone(),
            members: members.clone(),
            proposal,
            debate_instance: None,
            orchestrate_instance: None,
            simple_instance: None,
            llm_instance: None,
            ralph_instance: None,
        };

        match mode {
            CouncilMode::Debate => {
                council.debate_instance = Some(DebateCouncil::new(id, members));
            }
            CouncilMode::Orchestrate => {
                council.orchestrate_instance = Some(OrchestratorCouncil::new(id, members));
            }
            CouncilMode::Simple => {
                council.simple_instance = Some(SimpleCouncil::new(id, members));
            }
            CouncilMode::LLMDeliberation => {
                let config = llm_deliberation::LLMDeliberationConfig::default();
                council.llm_instance =
                    Some(llm_deliberation::LLMDebateCouncil::new(id, members, config));
            }
            CouncilMode::Ralph => {
                let config = ralph::RalphConfig::default();
                council.ralph_instance = Some(ralph::RalphCouncil::new(id, config));
            }
        }

        council
    }

    /// Create a council with explicit LLM configuration
    pub fn new_with_llm_config(
        mode: CouncilMode,
        proposal: Proposal,
        members: Vec<CouncilMember>,
        llm_config: llm_deliberation::LLMDeliberationConfig,
    ) -> Self {
        let id = uuid::Uuid::new_v4();

        let mut council = Self {
            id,
            mode: mode.clone(),
            members: members.clone(),
            proposal,
            debate_instance: None,
            orchestrate_instance: None,
            simple_instance: None,
            llm_instance: None,
            ralph_instance: None,
        };

        match mode {
            CouncilMode::Debate => {
                council.debate_instance = Some(DebateCouncil::new(id, members));
            }
            CouncilMode::Orchestrate => {
                council.orchestrate_instance = Some(OrchestratorCouncil::new(id, members));
            }
            CouncilMode::Simple => {
                council.simple_instance = Some(SimpleCouncil::new(id, members));
            }
            CouncilMode::LLMDeliberation => {
                council.llm_instance = Some(llm_deliberation::LLMDebateCouncil::new(
                    id, members, llm_config,
                ));
            }
            CouncilMode::Ralph => {
                let config = ralph::RalphConfig::default();
                council.ralph_instance = Some(ralph::RalphCouncil::new(id, config));
            }
        }

        council
    }

    /// Run the council evaluation to completion
    pub async fn evaluate(&mut self) -> anyhow::Result<CouncilDecision> {
        match self.mode {
            CouncilMode::Debate => {
                if let Some(debate) = &mut self.debate_instance {
                    debate.evaluate(&self.proposal, self.mode.clone()).await
                } else {
                    anyhow::bail!("Debate council not initialized")
                }
            }
            CouncilMode::Orchestrate => {
                if let Some(orch) = &mut self.orchestrate_instance {
                    orch.evaluate(&self.proposal, self.mode.clone()).await
                } else {
                    anyhow::bail!("Orchestrator council not initialized")
                }
            }
            CouncilMode::Simple => {
                if let Some(simple) = &mut self.simple_instance {
                    simple.evaluate(&self.proposal, self.mode.clone()).await
                } else {
                    anyhow::bail!("Simple council not initialized")
                }
            }
            CouncilMode::LLMDeliberation => {
                if let Some(llm) = &mut self.llm_instance {
                    llm.evaluate(&self.proposal, self.mode.clone()).await
                } else {
                    anyhow::bail!("LLM deliberation council not initialized")
                }
            }
            CouncilMode::Ralph => {
                if let Some(ralph) = &mut self.ralph_instance {
                    let task = &self.proposal.description;
                    let (_, decision) = ralph.execute(task).await?;
                    Ok(decision)
                } else {
                    anyhow::bail!("Ralph council not initialized")
                }
            }
        }
    }

    /// Get current status/progress of the council
    pub fn status(&self) -> CouncilStatus {
        match self.mode {
            CouncilMode::Debate => self
                .debate_instance
                .as_ref()
                .map(|d| d.status())
                .unwrap_or(CouncilStatus::NotStarted),
            CouncilMode::Orchestrate => self
                .orchestrate_instance
                .as_ref()
                .map(|o| o.status())
                .unwrap_or(CouncilStatus::NotStarted),
            CouncilMode::Simple => self
                .simple_instance
                .as_ref()
                .map(|s| s.status())
                .unwrap_or(CouncilStatus::NotStarted),
            CouncilMode::LLMDeliberation => self
                .llm_instance
                .as_ref()
                .map(|l| l.status())
                .unwrap_or(CouncilStatus::NotStarted),
            CouncilMode::Ralph => {
                // Ralph council doesn't have a status method, infer from state
                CouncilStatus::InProgress {
                    step: "ralph_loop".to_string(),
                    progress_pct: 0.5,
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CouncilStatus {
    NotStarted,
    InProgress { step: String, progress_pct: f64 },
    AwaitingQuorum,
    Completed { decision: Decision },
    Failed { error: String },
}

/// Factory for creating councils with automatic mode selection
pub struct CouncilFactory {
    mode_switcher: ModeSwitcher,
}

impl CouncilFactory {
    pub fn new(config: ModeConfig) -> Self {
        Self {
            mode_switcher: ModeSwitcher::new(config),
        }
    }

    /// Create a council with automatic mode selection
    pub fn create_council(
        &self,
        proposal: &Proposal,
        available_agents: Vec<CouncilMember>,
    ) -> anyhow::Result<Council> {
        let mode = self.mode_switcher.select_mode(proposal, &available_agents);
        Ok(Council::new(mode, proposal.clone(), available_agents))
    }

    /// Create a council with explicit mode override
    pub fn create_council_with_mode(
        &self,
        mode: CouncilMode,
        proposal: Proposal,
        available_agents: Vec<CouncilMember>,
    ) -> Council {
        Council::new(mode, proposal, available_agents)
    }
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert(value.clone()));
}
