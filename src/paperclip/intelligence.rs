//! Intelligence Layer — the composition engine that makes the company alive.
//!
//! Continuously monitors the world model for signals. When a signal appears
//! (budget overrun, capability failure, new market opportunity, stale goal),
//! it proactively composes a solution by:
//!   1. Resolving required capabilities from the registry
//!   2. Assigning to the best IC or spawning a sub-goal
//!   3. On failure, escalating to the appropriate DRI
//!
//! Runs as an async loop inside the unified runtime (not a separate process).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

use super::capability::CapabilityRegistry;
use super::dri::DriRegistry;
use super::goal::{EscalationAction, Goal, GoalAssignee, GoalId, GoalPriority, GoalStatus};
use super::org::OrgBlueprint;

// ── Signal ───────────────────────────────────────────────────────────────────

/// A signal from the world model that requires the Intelligence Layer's attention.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signal {
    pub id: String,
    pub kind: SignalKind,
    pub source: String,
    pub description: String,
    pub severity: f64, // 0.0–1.0
    pub timestamp: u64,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// A capability is degraded (reliability below target).
    CapabilityDegraded { capability_id: String },
    /// A goal has been stale for too long.
    GoalStale { goal_id: GoalId },
    /// Budget overrun detected.
    BudgetOverrun {
        agent_ref: String,
        overage_cents: i64,
    },
    /// A composition attempt failed.
    CompositionFailed { goal_id: GoalId, reason: String },
    /// Missing capability — no agent can handle a required primitive.
    MissingCapability { capability_id: String },
    /// External opportunity or threat detected (market signal).
    ExternalSignal { category: String },
    /// Coherence drop in the HSM-II world model.
    CoherenceDrop { current: f64, threshold: f64 },
    /// Agent performance anomaly.
    AgentAnomaly { agent_ref: String, metric: String },
    /// Custom signal from a DRI or Player-Coach.
    Custom { label: String },
}

// ── CompositionResult ────────────────────────────────────────────────────────

/// Outcome of the Intelligence Layer's attempt to compose a solution for a signal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompositionResult {
    pub signal_id: String,
    pub success: bool,
    pub goal_id: Option<GoalId>,
    pub assigned_to: Option<String>,
    pub capabilities_used: Vec<String>,
    pub escalated_to: Option<String>,
    pub message: String,
}

// ── IntelligenceLayer ────────────────────────────────────────────────────────

/// The core composition engine. Holds references to the capability registry,
/// DRI registry, and the live goal set.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IntelligenceLayer {
    pub capabilities: CapabilityRegistry,
    pub dri_registry: DriRegistry,
    /// Org taxonomy + default escalation from the last template `apply_to` (export merges with DRIs).
    #[serde(default)]
    pub org_blueprint: Option<OrgBlueprint>,
    pub goals: HashMap<GoalId, Goal>,
    /// Pending signals waiting to be processed.
    pub signal_queue: Vec<Signal>,
    /// Processed signals (last N for audit).
    pub signal_history: Vec<Signal>,
    /// Configuration.
    pub config: IntelligenceConfig,
    /// Stats.
    pub stats: IntelligenceStats,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntelligenceConfig {
    /// Max seconds a goal can be stale before generating a signal.
    pub goal_stale_threshold_secs: u64,
    /// Max composition attempts before escalating.
    pub max_composition_attempts: u32,
    /// How many historical signals to keep.
    pub signal_history_limit: usize,
    /// Coherence threshold below which to generate a signal.
    pub coherence_alert_threshold: f64,
}

impl Default for IntelligenceConfig {
    fn default() -> Self {
        Self {
            goal_stale_threshold_secs: 3600, // 1 hour
            max_composition_attempts: 3,
            signal_history_limit: 1000,
            coherence_alert_threshold: 0.3,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IntelligenceStats {
    pub signals_processed: u64,
    pub compositions_attempted: u64,
    pub compositions_succeeded: u64,
    pub compositions_failed: u64,
    pub escalations: u64,
    pub goals_created: u64,
    pub goals_completed: u64,
}

impl IntelligenceLayer {
    pub fn new() -> Self {
        Self {
            capabilities: CapabilityRegistry::with_defaults(),
            ..Default::default()
        }
    }

    // ── Signal ingestion ─────────────────────────────────────────────────────

    /// Emit a signal into the queue for processing on the next tick.
    pub fn emit_signal(&mut self, signal: Signal) {
        debug!(signal_id = %signal.id, kind = ?signal.kind, "signal emitted");
        self.signal_queue.push(signal);
    }

    /// Scan the world state and generate signals for anomalies.
    ///
    /// Called by the unified runtime's heartbeat loop with the current world state.
    pub fn scan_world(&mut self, coherence: f64, _tick: u64) {
        // 1. Coherence drop
        if coherence < self.config.coherence_alert_threshold {
            self.emit_signal(Signal {
                id: uuid_v4(),
                kind: SignalKind::CoherenceDrop {
                    current: coherence,
                    threshold: self.config.coherence_alert_threshold,
                },
                source: "world_model".into(),
                description: format!(
                    "Coherence {:.3} below threshold {:.3}",
                    coherence, self.config.coherence_alert_threshold
                ),
                severity: 0.8,
                timestamp: now_secs(),
                metadata: HashMap::new(),
            });
        }

        // 2. Stale goals
        let stale_ids: Vec<GoalId> = self
            .goals
            .values()
            .filter(|g| {
                g.status.is_actionable() && g.is_stale(self.config.goal_stale_threshold_secs)
            })
            .map(|g| g.id.clone())
            .collect();

        for goal_id in stale_ids {
            self.emit_signal(Signal {
                id: uuid_v4(),
                kind: SignalKind::GoalStale {
                    goal_id: goal_id.clone(),
                },
                source: "intelligence_layer".into(),
                description: format!("Goal {} stale", goal_id),
                severity: 0.5,
                timestamp: now_secs(),
                metadata: HashMap::new(),
            });
        }

        // 3. Degraded capabilities (collect first, then emit to avoid borrow conflict)
        let degraded_signals: Vec<Signal> = self
            .capabilities
            .all()
            .filter(|cap| cap.metrics.total_invocations > 10 && !cap.meets_target())
            .map(|cap| Signal {
                id: uuid_v4(),
                kind: SignalKind::CapabilityDegraded {
                    capability_id: cap.id.clone(),
                },
                source: "capability_monitor".into(),
                description: format!(
                    "Capability {} reliability {:.1}% < target {:.1}%",
                    cap.name,
                    cap.metrics.reliability() * 100.0,
                    cap.target.reliability * 100.0
                ),
                severity: 0.6,
                timestamp: now_secs(),
                metadata: HashMap::new(),
            })
            .collect();

        for signal in degraded_signals {
            self.emit_signal(signal);
        }
    }

    // ── Tick: process all pending signals ────────────────────────────────────

    /// Process all pending signals. Returns composition results.
    ///
    /// Called by the unified runtime's heartbeat loop.
    pub fn tick(&mut self) -> Vec<CompositionResult> {
        let signals: Vec<Signal> = self.signal_queue.drain(..).collect();
        let mut results = Vec::new();

        for signal in signals {
            let result = self.process_signal(&signal);
            self.stats.signals_processed += 1;

            // Archive signal
            self.signal_history.push(signal);
            if self.signal_history.len() > self.config.signal_history_limit {
                self.signal_history.remove(0);
            }

            results.push(result);
        }

        results
    }

    // ── Core composition logic ───────────────────────────────────────────────

    fn process_signal(&mut self, signal: &Signal) -> CompositionResult {
        match &signal.kind {
            SignalKind::CompositionFailed { goal_id, reason } => {
                self.handle_composition_failure(signal, goal_id, reason)
            }
            SignalKind::GoalStale { goal_id } => self.handle_stale_goal(signal, goal_id),
            SignalKind::MissingCapability { capability_id } => {
                self.handle_missing_capability(signal, capability_id)
            }
            SignalKind::CapabilityDegraded { capability_id } => {
                self.handle_degraded_capability(signal, capability_id)
            }
            SignalKind::CoherenceDrop { current, threshold } => {
                self.handle_coherence_drop(signal, *current, *threshold)
            }
            SignalKind::BudgetOverrun {
                agent_ref,
                overage_cents,
            } => self.handle_budget_overrun(signal, agent_ref, *overage_cents),
            _ => {
                // For external/custom/anomaly signals: create a goal and route to DRI
                self.create_goal_from_signal(signal)
            }
        }
    }

    fn handle_composition_failure(
        &mut self,
        signal: &Signal,
        goal_id: &str,
        reason: &str,
    ) -> CompositionResult {
        self.stats.compositions_failed += 1;

        if let Some(goal) = self.goals.get_mut(goal_id) {
            goal.composition_attempts += 1;

            if goal.composition_attempts >= self.config.max_composition_attempts {
                // Escalate
                return self.escalate_goal(signal, goal_id);
            }

            goal.status = GoalStatus::CompositionFailed;
            info!(
                goal_id,
                reason,
                attempts = goal.composition_attempts,
                "composition failed, will retry"
            );
        }

        CompositionResult {
            signal_id: signal.id.clone(),
            success: false,
            goal_id: Some(goal_id.to_string()),
            assigned_to: None,
            capabilities_used: Vec::new(),
            escalated_to: None,
            message: format!("Composition failed: {reason}"),
        }
    }

    fn handle_stale_goal(&mut self, signal: &Signal, goal_id: &str) -> CompositionResult {
        // Try to re-compose: check if capabilities are now available
        let caps_needed: Vec<String> = self
            .goals
            .get(goal_id)
            .map(|g| g.required_capabilities.clone())
            .unwrap_or_default();

        let (found, missing) = self.capabilities.resolve(&caps_needed);

        if missing.is_empty() && !found.is_empty() {
            // Capabilities available — try to assign to an IC
            let agent_ref = found[0].provider_agents.first().cloned();
            if let Some(ref agent) = agent_ref {
                if let Some(goal) = self.goals.get_mut(goal_id) {
                    goal.assignee = GoalAssignee::Ic {
                        agent_ref: agent.clone(),
                        capability_id: found[0].id.clone(),
                    };
                    goal.status = GoalStatus::InProgress;
                    goal.updated_at = now_secs();
                    self.stats.compositions_attempted += 1;
                    self.stats.compositions_succeeded += 1;
                }
            }

            CompositionResult {
                signal_id: signal.id.clone(),
                success: true,
                goal_id: Some(goal_id.to_string()),
                assigned_to: agent_ref,
                capabilities_used: found.iter().map(|c| c.id.clone()).collect(),
                escalated_to: None,
                message: "Re-composed stale goal".into(),
            }
        } else {
            // Still can't compose — escalate
            self.escalate_goal(signal, goal_id)
        }
    }

    fn handle_missing_capability(
        &mut self,
        signal: &Signal,
        capability_id: &str,
    ) -> CompositionResult {
        // Route to DRI for "new capability development"
        let dri = self
            .dri_registry
            .find_for_domain("capability_development")
            .or_else(|| self.dri_registry.active().into_iter().next());

        let dri_ref = dri.map(|d| d.agent_ref.clone());

        let goal = Goal::new(
            uuid_v4(),
            format!("Develop missing capability: {capability_id}"),
        )
        .with_description(signal.description.clone())
        .with_priority(GoalPriority::High)
        .with_assignee(match &dri_ref {
            Some(ref r) => GoalAssignee::Dri {
                agent_ref: r.clone(),
                domain: "capability_development".into(),
            },
            None => GoalAssignee::Unassigned,
        });

        let goal_id = goal.id.clone();
        self.goals.insert(goal_id.clone(), goal);
        self.stats.goals_created += 1;
        self.stats.escalations += 1;

        info!(capability_id, dri = ?dri_ref, "missing capability → DRI goal created");

        CompositionResult {
            signal_id: signal.id.clone(),
            success: true,
            goal_id: Some(goal_id),
            assigned_to: None,
            capabilities_used: Vec::new(),
            escalated_to: dri_ref,
            message: format!("Created development goal for missing capability {capability_id}"),
        }
    }

    fn handle_degraded_capability(
        &mut self,
        signal: &Signal,
        capability_id: &str,
    ) -> CompositionResult {
        // Route to Player-Coach for capabilities stack
        let dri = self
            .dri_registry
            .find_for_domain("capabilities")
            .or_else(|| self.dri_registry.find_for_domain("engineering"));

        let dri_ref = dri.map(|d| d.agent_ref.clone());

        let goal = Goal::new(
            uuid_v4(),
            format!("Investigate degraded capability: {capability_id}"),
        )
        .with_description(signal.description.clone())
        .with_priority(GoalPriority::Medium)
        .with_assignee(match &dri_ref {
            Some(ref r) => GoalAssignee::Dri {
                agent_ref: r.clone(),
                domain: "capabilities".into(),
            },
            None => GoalAssignee::Unassigned,
        });

        let goal_id = goal.id.clone();
        self.goals.insert(goal_id.clone(), goal);
        self.stats.goals_created += 1;

        CompositionResult {
            signal_id: signal.id.clone(),
            success: true,
            goal_id: Some(goal_id),
            assigned_to: None,
            capabilities_used: Vec::new(),
            escalated_to: dri_ref,
            message: format!("Created investigation goal for degraded {capability_id}"),
        }
    }

    fn handle_coherence_drop(
        &mut self,
        signal: &Signal,
        current: f64,
        threshold: f64,
    ) -> CompositionResult {
        let dri = self
            .dri_registry
            .find_for_domain("crisis")
            .or_else(|| self.dri_registry.active().into_iter().next());

        let dri_ref = dri.map(|d| d.agent_ref.clone());

        let goal = Goal::new(uuid_v4(), "Restore world model coherence")
            .with_description(format!(
                "Coherence dropped to {current:.3} (threshold: {threshold:.3})"
            ))
            .with_priority(GoalPriority::Critical)
            .with_assignee(match &dri_ref {
                Some(ref r) => GoalAssignee::Dri {
                    agent_ref: r.clone(),
                    domain: "crisis".into(),
                },
                None => GoalAssignee::Unassigned,
            });

        let goal_id = goal.id.clone();
        self.goals.insert(goal_id.clone(), goal);
        self.stats.goals_created += 1;
        self.stats.escalations += 1;

        warn!(current, threshold, dri = ?dri_ref, "coherence drop → crisis DRI");

        CompositionResult {
            signal_id: signal.id.clone(),
            success: true,
            goal_id: Some(goal_id),
            assigned_to: None,
            capabilities_used: Vec::new(),
            escalated_to: dri_ref,
            message: format!("Crisis goal created for coherence drop ({current:.3})"),
        }
    }

    fn handle_budget_overrun(
        &mut self,
        signal: &Signal,
        agent_ref: &str,
        overage_cents: i64,
    ) -> CompositionResult {
        let dri = self
            .dri_registry
            .find_for_domain("cost_optimization")
            .or_else(|| self.dri_registry.find_for_domain("finance"));

        let dri_ref = dri.map(|d| d.agent_ref.clone());

        let goal = Goal::new(uuid_v4(), format!("Resolve budget overrun: {agent_ref}"))
            .with_description(format!(
                "Agent {} is ${:.2} over budget",
                agent_ref,
                overage_cents as f64 / 100.0
            ))
            .with_priority(GoalPriority::High)
            .with_assignee(match &dri_ref {
                Some(ref r) => GoalAssignee::Dri {
                    agent_ref: r.clone(),
                    domain: "cost_optimization".into(),
                },
                None => GoalAssignee::Unassigned,
            });

        let goal_id = goal.id.clone();
        self.goals.insert(goal_id.clone(), goal);
        self.stats.goals_created += 1;

        CompositionResult {
            signal_id: signal.id.clone(),
            success: true,
            goal_id: Some(goal_id),
            assigned_to: None,
            capabilities_used: Vec::new(),
            escalated_to: dri_ref,
            message: format!("Budget overrun goal created for {agent_ref}"),
        }
    }

    fn create_goal_from_signal(&mut self, signal: &Signal) -> CompositionResult {
        let domains: Vec<String> = signal
            .metadata
            .get("domains")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let dri = self.dri_registry.route_goal(&domains);
        let dri_ref = dri.map(|d| d.agent_ref.clone());

        let goal = Goal::new(uuid_v4(), signal.description.clone())
            .with_priority(if signal.severity > 0.7 {
                GoalPriority::High
            } else {
                GoalPriority::Medium
            })
            .with_assignee(match &dri_ref {
                Some(ref r) => GoalAssignee::Dri {
                    agent_ref: r.clone(),
                    domain: domains.first().cloned().unwrap_or_default(),
                },
                None => GoalAssignee::Unassigned,
            });

        let goal_id = goal.id.clone();
        self.goals.insert(goal_id.clone(), goal);
        self.stats.goals_created += 1;

        CompositionResult {
            signal_id: signal.id.clone(),
            success: true,
            goal_id: Some(goal_id),
            assigned_to: None,
            capabilities_used: Vec::new(),
            escalated_to: dri_ref,
            message: format!("Goal created from signal: {}", signal.description),
        }
    }

    fn escalate_goal(&mut self, signal: &Signal, goal_id: &str) -> CompositionResult {
        self.stats.escalations += 1;

        let escalation_level = if let Some(goal) = self.goals.get_mut(goal_id) {
            goal.escalate()
                .map(|l| (l.assignee.clone(), l.action.clone()))
        } else {
            None
        };

        if let Some((assignee, action)) = escalation_level {
            let dri_ref = assignee.agent_ref().map(|s| s.to_string());

            match action {
                EscalationAction::Reassign => {
                    if let Some(goal) = self.goals.get_mut(goal_id) {
                        goal.assignee = assignee;
                        goal.updated_at = now_secs();
                    }
                }
                EscalationAction::SpawnSubGoal { ref sub_title } => {
                    let sub_goal = Goal::new(uuid_v4(), sub_title.clone())
                        .with_assignee(assignee)
                        .with_priority(GoalPriority::High);
                    let sub_id = sub_goal.id.clone();
                    if let Some(parent) = self.goals.get_mut(goal_id) {
                        parent.child_ids.push(sub_id.clone());
                    }
                    self.goals.insert(sub_id, sub_goal);
                    self.stats.goals_created += 1;
                }
                EscalationAction::Notify => {
                    info!(goal_id, dri = ?dri_ref, "escalation: notifying DRI");
                }
                EscalationAction::HumanReview => {
                    if let Some(goal) = self.goals.get_mut(goal_id) {
                        goal.status = GoalStatus::Blocked {
                            reason: "Awaiting human review".into(),
                        };
                    }
                }
            }

            info!(goal_id, dri = ?dri_ref, "goal escalated");

            CompositionResult {
                signal_id: signal.id.clone(),
                success: false,
                goal_id: Some(goal_id.to_string()),
                assigned_to: None,
                capabilities_used: Vec::new(),
                escalated_to: dri_ref,
                message: format!("Goal {goal_id} escalated to DRI"),
            }
        } else {
            // No more escalation levels — block for human review
            if let Some(goal) = self.goals.get_mut(goal_id) {
                goal.status = GoalStatus::Blocked {
                    reason: "Escalation chain exhausted — needs human review".into(),
                };
            }

            warn!(goal_id, "escalation chain exhausted");

            CompositionResult {
                signal_id: signal.id.clone(),
                success: false,
                goal_id: Some(goal_id.to_string()),
                assigned_to: None,
                capabilities_used: Vec::new(),
                escalated_to: None,
                message: format!(
                    "Goal {goal_id}: escalation chain exhausted, blocked for human review"
                ),
            }
        }
    }

    // ── Goal management ──────────────────────────────────────────────────────

    pub fn add_goal(&mut self, goal: Goal) -> GoalId {
        let id = goal.id.clone();
        self.goals.insert(id.clone(), goal);
        self.stats.goals_created += 1;
        id
    }

    pub fn get_goal(&self, id: &str) -> Option<&Goal> {
        self.goals.get(id)
    }

    pub fn get_goal_mut(&mut self, id: &str) -> Option<&mut Goal> {
        self.goals.get_mut(id)
    }

    pub fn complete_goal(&mut self, id: &str) {
        if let Some(goal) = self.goals.get_mut(id) {
            goal.mark_done();
            self.stats.goals_completed += 1;
        }
    }

    pub fn list_goals(&self) -> Vec<&Goal> {
        self.goals.values().collect()
    }

    pub fn actionable_goals(&self) -> Vec<&Goal> {
        self.goals
            .values()
            .filter(|g| g.status.is_actionable())
            .collect()
    }

    /// Summary for API / dashboard.
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "stats": self.stats,
            "goals": {
                "total": self.goals.len(),
                "open": self.goals.values().filter(|g| matches!(g.status, GoalStatus::Open)).count(),
                "in_progress": self.goals.values().filter(|g| matches!(g.status, GoalStatus::InProgress)).count(),
                "done": self.goals.values().filter(|g| matches!(g.status, GoalStatus::Done)).count(),
                "blocked": self.goals.values().filter(|g| matches!(g.status, GoalStatus::Blocked{..})).count(),
                "escalated": self.goals.values().filter(|g| matches!(g.status, GoalStatus::Escalated{..})).count(),
            },
            "capabilities": {
                "total": self.capabilities.len(),
                "health": self.capabilities.health(),
            },
            "dris": {
                "total": self.dri_registry.len(),
                "active": self.dri_registry.active().len(),
            },
            "signal_queue": self.signal_queue.len(),
        })
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}
