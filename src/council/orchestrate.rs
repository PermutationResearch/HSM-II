//! Orchestrate mode council - hierarchical command with sub-task delegation.
//!
//! Orchestrate mode is designed for urgent or complex tasks requiring
//! clear chains of command and parallel sub-task execution.

use super::{
    CouncilDecision, CouncilId, CouncilMember, CouncilMode, CouncilStatus, Decision, ExecutionPlan,
    ExecutionStep, Proposal,
};
use crate::agent::{AgentId, Role};
use serde::{Deserialize, Serialize};

/// A council operating in orchestrate mode
pub struct OrchestratorCouncil {
    council_id: CouncilId,
    members: Vec<CouncilMember>,
    command_chain: Vec<Command>,
    subtasks: Vec<SubTask>,
    status: CouncilStatus,
}

impl OrchestratorCouncil {
    pub fn new(council_id: CouncilId, members: Vec<CouncilMember>) -> Self {
        Self {
            council_id,
            members,
            command_chain: Vec::new(),
            subtasks: Vec::new(),
            status: CouncilStatus::NotStarted,
        }
    }

    pub fn status(&self) -> CouncilStatus {
        self.status.clone()
    }

    /// Evaluate a proposal through hierarchical orchestration
    pub async fn evaluate(
        &mut self,
        proposal: &Proposal,
        mode: CouncilMode,
    ) -> anyhow::Result<CouncilDecision> {
        self.status = CouncilStatus::InProgress {
            step: "command_assignment".to_string(),
            progress_pct: 0.0,
        };

        // Step 1: Select commander (typically Architect or highest expertise)
        let commander_id = self.select_commander_id(&self.members);

        // Step 2: Decompose proposal into subtasks
        self.status = CouncilStatus::InProgress {
            step: "task_decomposition".to_string(),
            progress_pct: 0.25,
        };

        self.subtasks = self.decompose_into_subtasks(proposal, &self.members);

        // Step 3: Assign subtasks to appropriate agents
        self.status = CouncilStatus::InProgress {
            step: "subtask_assignment".to_string(),
            progress_pct: 0.5,
        };

        let members_clone = self.members.clone();
        self.assign_subtasks(&members_clone, proposal);

        // Step 4: Create command chain
        let priority = self.calculate_priority(proposal);
        let command = Command {
            commander_id,
            proposal_id: proposal.id.clone(),
            priority,
            subtask_ids: self.subtasks.iter().map(|st| st.id.clone()).collect(),
            deadline: proposal.proposed_at + 3600, // 1 hour default
        };
        self.command_chain.push(command);

        self.status = CouncilStatus::InProgress {
            step: "execution_planning".to_string(),
            progress_pct: 0.75,
        };

        // Step 5: Generate execution plan
        let execution_plan = self.create_execution_plan();

        self.status = CouncilStatus::Completed {
            decision: Decision::Approve,
        };

        Ok(CouncilDecision {
            council_id: self.council_id,
            proposal_id: proposal.id.clone(),
            decision: Decision::Approve,
            confidence: 0.85,
            participating_agents: self.members.iter().map(|m| m.agent_id).collect(),
            execution_plan: Some(execution_plan),
            decided_at: current_timestamp(),
            mode_used: mode,
            metadata: proposal
                .stigmergic_context
                .as_ref()
                .map(|ctx| ctx.audit_metadata())
                .unwrap_or_default(),
        })
    }

    fn select_commander_id(&self, members: &[CouncilMember]) -> AgentId {
        // Prefer Architect with highest expertise
        members
            .iter()
            .filter(|m| matches!(m.role, Role::Architect))
            .max_by(|a, b| a.expertise_score.partial_cmp(&b.expertise_score).unwrap())
            .or_else(|| {
                members
                    .iter()
                    .max_by(|a, b| a.expertise_score.partial_cmp(&b.expertise_score).unwrap())
            })
            .map(|m| m.agent_id)
            .unwrap_or(members[0].agent_id)
    }

    fn decompose_into_subtasks(
        &self,
        proposal: &Proposal,
        _members: &[CouncilMember],
    ) -> Vec<SubTask> {
        let mut subtasks = Vec::new();

        // Standard decomposition pattern
        subtasks.push(SubTask {
            id: format!("{}_analysis", proposal.id),
            description: format!("Analyze requirements for: {}", proposal.title),
            required_role: Role::Critic,
            assigned_agent: None,
            dependencies: vec![],
            estimated_effort: 0.2,
        });

        subtasks.push(SubTask {
            id: format!("{}_design", proposal.id),
            description: format!("Design implementation for: {}", proposal.title),
            required_role: Role::Architect,
            assigned_agent: None,
            dependencies: vec![format!("{}_analysis", proposal.id)],
            estimated_effort: 0.3,
        });

        subtasks.push(SubTask {
            id: format!("{}_implement", proposal.id),
            description: format!("Implement: {}", proposal.title),
            required_role: Role::Catalyst,
            assigned_agent: None,
            dependencies: vec![format!("{}_design", proposal.id)],
            estimated_effort: 0.4,
        });

        subtasks.push(SubTask {
            id: format!("{}_document", proposal.id),
            description: format!("Document changes for: {}", proposal.title),
            required_role: Role::Chronicler,
            assigned_agent: None,
            dependencies: vec![format!("{}_implement", proposal.id)],
            estimated_effort: 0.1,
        });

        subtasks
    }

    fn assign_subtasks(&mut self, members: &[CouncilMember], proposal: &Proposal) {
        let preferred_agent = proposal
            .stigmergic_context
            .as_ref()
            .and_then(|ctx| ctx.preferred_agent);
        for subtask in &mut self.subtasks {
            if let Some(agent_id) = preferred_agent {
                if let Some(preferred_member) = members.iter().find(|member| {
                    member.agent_id == agent_id && member.role == subtask.required_role
                }) {
                    subtask.assigned_agent = Some(preferred_member.agent_id);
                    continue;
                }
            }

            // Find best matching agent for this subtask
            let best_match = members
                .iter()
                .filter(|m| m.role == subtask.required_role)
                .max_by(|a, b| a.expertise_score.partial_cmp(&b.expertise_score).unwrap());

            if let Some(agent) = best_match {
                subtask.assigned_agent = Some(agent.agent_id);
            }
        }
    }

    fn calculate_priority(&self, proposal: &Proposal) -> Priority {
        if proposal.urgency > 0.8 {
            Priority::Critical
        } else if proposal.urgency > 0.5 {
            Priority::High
        } else if proposal.urgency > 0.3 {
            Priority::Medium
        } else {
            Priority::Low
        }
    }

    fn create_execution_plan(&self) -> ExecutionPlan {
        let steps: Vec<ExecutionStep> = self
            .subtasks
            .iter()
            .enumerate()
            .map(|(i, st)| ExecutionStep {
                sequence: i + 1,
                description: st.description.clone(),
                assigned_agent: st.assigned_agent,
                dependencies: st
                    .dependencies
                    .iter()
                    .filter_map(|dep_id| self.subtasks.iter().position(|s| s.id == *dep_id))
                    .map(|idx| idx + 1)
                    .collect(),
            })
            .collect();

        let total_effort: f64 = self.subtasks.iter().map(|st| st.estimated_effort).sum();

        ExecutionPlan {
            steps,
            estimated_duration_ms: (total_effort * 60000.0) as u64,
            rollback_strategy: Some("Commander-initiated rollback".to_string()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubTask {
    pub id: String,
    pub description: String,
    pub required_role: Role,
    pub assigned_agent: Option<AgentId>,
    pub dependencies: Vec<String>,
    pub estimated_effort: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Command {
    pub commander_id: AgentId,
    pub proposal_id: String,
    pub priority: Priority,
    pub subtask_ids: Vec<String>,
    pub deadline: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
