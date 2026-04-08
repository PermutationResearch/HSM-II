//! Rich Goal primitive with assignee (IC/DRI), escalation chain, and artifact output.
//!
//! This is the in-memory representation used by the Intelligence Layer.  The
//! Postgres-backed `company_os::GoalRow` remains the durable store; this struct
//! is the live working copy enriched with runtime context.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Identifiers ──────────────────────────────────────────────────────────────

pub type GoalId = String;
pub type AgentRef = String; // Maps to company_os agent name or HSM-II agent ID

// ── GoalStatus ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    /// Newly created, not yet assigned or started.
    Open,
    /// Assigned and actively being worked on.
    InProgress,
    /// Composition attempted but failed — needs DRI intervention.
    CompositionFailed,
    /// Blocked on external dependency or missing capability.
    Blocked { reason: String },
    /// Escalated up the chain because the current owner couldn't resolve.
    Escalated { level: usize },
    /// Successfully completed with artifacts.
    Done,
    /// Cancelled by a DRI or the Intelligence Layer.
    Cancelled { reason: String },
}

impl GoalStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, GoalStatus::Done | GoalStatus::Cancelled { .. })
    }

    pub fn is_actionable(&self) -> bool {
        matches!(
            self,
            GoalStatus::Open | GoalStatus::InProgress | GoalStatus::CompositionFailed
        )
    }
}

// ── GoalPriority ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl Default for GoalPriority {
    fn default() -> Self {
        GoalPriority::Medium
    }
}

// ── GoalAssignee ─────────────────────────────────────────────────────────────

/// Who owns this goal — an IC (deep specialist) or a DRI (outcome owner).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "role_type", rename_all = "snake_case")]
pub enum GoalAssignee {
    /// Individual Contributor — works within a single capability domain.
    Ic {
        agent_ref: AgentRef,
        capability_id: String,
    },
    /// Directly Responsible Individual — cross-cutting authority.
    Dri { agent_ref: AgentRef, domain: String },
    /// Player-Coach — builds + mentors.
    PlayerCoach {
        agent_ref: AgentRef,
        mentee_refs: Vec<AgentRef>,
    },
    /// Unassigned — the Intelligence Layer will route it.
    Unassigned,
}

impl GoalAssignee {
    pub fn agent_ref(&self) -> Option<&str> {
        match self {
            GoalAssignee::Ic { agent_ref, .. }
            | GoalAssignee::Dri { agent_ref, .. }
            | GoalAssignee::PlayerCoach { agent_ref, .. } => Some(agent_ref),
            GoalAssignee::Unassigned => None,
        }
    }
}

// ── EscalationChain ──────────────────────────────────────────────────────────

/// Defines who gets notified/assigned when a goal can't be resolved at the
/// current level.  The Intelligence Layer walks this chain on composition failure.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EscalationChain {
    pub levels: Vec<EscalationLevel>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EscalationLevel {
    /// Who to escalate to.
    pub assignee: GoalAssignee,
    /// Max time (seconds) at this level before auto-escalating to the next.
    pub timeout_secs: u64,
    /// Action to take: reassign, notify, spawn sub-goal, etc.
    #[serde(default = "default_escalation_action")]
    pub action: EscalationAction,
}

fn default_escalation_action() -> EscalationAction {
    EscalationAction::Reassign
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalationAction {
    /// Reassign the goal to the escalation-level assignee.
    Reassign,
    /// Notify the assignee but keep current owner.
    Notify,
    /// Spawn a new sub-goal owned by the escalation assignee.
    SpawnSubGoal { sub_title: String },
    /// Pause the goal and flag for human review.
    HumanReview,
}

impl EscalationChain {
    pub fn new() -> Self {
        Self { levels: Vec::new() }
    }

    pub fn push(&mut self, level: EscalationLevel) {
        self.levels.push(level);
    }

    /// Get the next level to escalate to, given the current escalation depth.
    pub fn next_level(&self, current_depth: usize) -> Option<&EscalationLevel> {
        self.levels.get(current_depth)
    }
}

// ── ArtifactOutput ───────────────────────────────────────────────────────────

/// Machine-readable output produced when a goal completes. Feeds back into
/// the world model so downstream goals can consume it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactOutput {
    /// Unique artifact ID (UUID).
    pub id: String,
    /// What kind of artifact this is.
    pub kind: ArtifactKind,
    /// Short human-readable summary.
    pub summary: String,
    /// Structured payload (JSON).
    pub payload: serde_json::Value,
    /// Unix timestamp when produced.
    pub produced_at: u64,
    /// Which goal produced this artifact.
    pub source_goal_id: GoalId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// Code commit, PR, or deployment.
    Code { repo: String, ref_id: String },
    /// Research report or analysis document.
    Report,
    /// Data file, dataset, or export.
    Data { format: String },
    /// Configuration change.
    Config,
    /// Decision record (from council or DRI).
    Decision,
    /// Customer-facing output (email, message, etc.).
    CustomerOutput,
    /// Budget/spend change.
    Financial,
    /// Generic artifact.
    Other { label: String },
}

// ── Goal ─────────────────────────────────────────────────────────────────────

/// The rich Goal primitive used by the Intelligence Layer.
///
/// This is the live working copy; the durable store is `company_os::GoalRow`
/// in Postgres. The Intelligence Layer syncs between the two.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Goal {
    pub id: GoalId,
    pub title: String,
    pub description: String,

    /// Parent goal (hierarchical decomposition).
    pub parent_id: Option<GoalId>,
    /// Child sub-goals spawned from this one.
    pub child_ids: Vec<GoalId>,

    /// Who owns this goal.
    pub assignee: GoalAssignee,
    pub status: GoalStatus,
    pub priority: GoalPriority,

    /// Which capabilities are needed to accomplish this goal.
    pub required_capabilities: Vec<String>,

    /// Escalation chain for when composition fails.
    pub escalation: EscalationChain,
    /// Current depth in the escalation chain (0 = not escalated).
    pub escalation_depth: usize,

    /// Artifacts produced by this goal.
    pub artifacts: Vec<ArtifactOutput>,

    /// Key results / success criteria (from ops_config or company_os).
    pub key_results: Vec<KeyResult>,

    /// Metadata.
    pub created_at: u64,
    pub updated_at: u64,
    /// How many composition attempts the Intelligence Layer has made.
    pub composition_attempts: u32,
    /// Tags for filtering and world-model indexing.
    pub tags: Vec<String>,
    /// Arbitrary metadata from external systems.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyResult {
    pub metric: String,
    pub target: serde_json::Value,
    pub current: Option<serde_json::Value>,
}

impl Goal {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        let now = now_secs();
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            parent_id: None,
            child_ids: Vec::new(),
            assignee: GoalAssignee::Unassigned,
            status: GoalStatus::Open,
            priority: GoalPriority::default(),
            required_capabilities: Vec::new(),
            escalation: EscalationChain::new(),
            escalation_depth: 0,
            artifacts: Vec::new(),
            key_results: Vec::new(),
            created_at: now,
            updated_at: now,
            composition_attempts: 0,
            tags: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_assignee(mut self, assignee: GoalAssignee) -> Self {
        self.assignee = assignee;
        self
    }

    pub fn with_priority(mut self, priority: GoalPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.required_capabilities = caps;
        self
    }

    pub fn with_escalation(mut self, chain: EscalationChain) -> Self {
        self.escalation = chain;
        self
    }

    pub fn add_artifact(&mut self, artifact: ArtifactOutput) {
        self.artifacts.push(artifact);
        self.updated_at = now_secs();
    }

    pub fn mark_done(&mut self) {
        self.status = GoalStatus::Done;
        self.updated_at = now_secs();
    }

    pub fn mark_failed(&mut self) {
        self.status = GoalStatus::CompositionFailed;
        self.updated_at = now_secs();
    }

    pub fn escalate(&mut self) -> Option<&EscalationLevel> {
        let next = self.escalation.next_level(self.escalation_depth)?;
        self.escalation_depth += 1;
        self.status = GoalStatus::Escalated {
            level: self.escalation_depth,
        };
        self.updated_at = now_secs();
        Some(next)
    }

    pub fn is_stale(&self, max_age_secs: u64) -> bool {
        let now = now_secs();
        now.saturating_sub(self.updated_at) > max_age_secs
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
