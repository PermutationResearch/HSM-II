//! Simple mode council - direct voting with minimal overhead.
//!
//! Simple mode is designed for routine decisions requiring quick consensus.
//! Agents vote based on role-weighted preferences without full deliberation.

use super::{
    CouncilDecision, CouncilId, CouncilMember, CouncilMode, CouncilStatus, Decision, ExecutionPlan,
    ExecutionStep, Proposal,
};
use crate::agent::{AgentId, Role};
use serde::{Deserialize, Serialize};

/// A council operating in simple mode
pub struct SimpleCouncil {
    council_id: CouncilId,
    members: Vec<CouncilMember>,
    votes: Vec<Vote>,
    status: CouncilStatus,
}

impl SimpleCouncil {
    pub fn new(council_id: CouncilId, members: Vec<CouncilMember>) -> Self {
        Self {
            council_id,
            members,
            votes: Vec::new(),
            status: CouncilStatus::NotStarted,
        }
    }

    pub fn status(&self) -> CouncilStatus {
        self.status.clone()
    }

    /// Evaluate a proposal through simple voting
    pub async fn evaluate(
        &mut self,
        proposal: &Proposal,
        mode: CouncilMode,
    ) -> anyhow::Result<CouncilDecision> {
        self.status = CouncilStatus::InProgress {
            step: "voting".to_string(),
            progress_pct: 0.5,
        };

        // Collect votes from all members
        let mut votes = Vec::new();
        for member in self.members.iter() {
            let vote = self.cast_vote(member, proposal);
            votes.push(vote);
        }

        self.votes = votes;

        // Calculate weighted result
        let (decision, confidence) = self.tally_votes();

        self.status = CouncilStatus::Completed {
            decision: decision.clone(),
        };

        let execution_plan = if matches!(decision, Decision::Approve) {
            Some(self.create_simple_plan(proposal))
        } else {
            None
        };

        Ok(CouncilDecision {
            council_id: self.council_id,
            proposal_id: proposal.id.clone(),
            decision,
            confidence,
            participating_agents: self.members.iter().map(|m| m.agent_id).collect(),
            execution_plan,
            decided_at: current_timestamp(),
            mode_used: mode,
            metadata: proposal
                .stigmergic_context
                .as_ref()
                .map(|ctx| ctx.audit_metadata())
                .unwrap_or_default(),
        })
    }

    fn cast_vote(&self, member: &CouncilMember, proposal: &Proposal) -> Vote {
        // Role-based voting heuristics
        let mut approval_probability = match member.role {
            Role::Architect => {
                if proposal.description.contains("structure")
                    || proposal.description.contains("improve")
                {
                    0.8
                } else {
                    0.6
                }
            }
            Role::Catalyst => {
                if proposal.description.contains("innovation")
                    || proposal.description.contains("experiment")
                {
                    0.9
                } else {
                    0.7
                }
            }
            Role::Critic => {
                if proposal.complexity > 0.7 {
                    0.4 // Skeptical of complex proposals
                } else {
                    0.6
                }
            }
            Role::Explorer => {
                if proposal.description.contains("explore") || proposal.description.contains("new")
                {
                    0.85
                } else {
                    0.65
                }
            }
            Role::Chronicler => {
                if proposal.description.contains("document")
                    || proposal.description.contains("record")
                {
                    0.9
                } else {
                    0.7
                }
            }
            Role::Coder => {
                if proposal.description.contains("code")
                    || proposal.description.contains("implement")
                    || proposal.description.contains("tool")
                    || proposal.description.contains("debug")
                {
                    0.85
                } else {
                    0.6
                }
            }
        };

        if let Some(context) = &proposal.stigmergic_context {
            if context.require_council_review {
                approval_probability *= 0.9;
            }
            if Some(member.agent_id) == context.preferred_agent {
                approval_probability = (approval_probability + 0.15 * context.confidence).min(0.98);
            } else if context.preferred_agent.is_some() && context.confidence > 0.65 {
                approval_probability = (approval_probability - 0.05 * context.confidence).max(0.05);
            }
        }

        // Add some randomness based on expertise (experts more certain)
        let certainty = member.expertise_score;
        let final_prob = approval_probability * certainty + 0.5 * (1.0 - certainty);

        let value = if final_prob > 0.5 {
            VoteValue::Approve
        } else {
            VoteValue::Reject
        };

        Vote {
            agent_id: member.agent_id,
            value,
            weight: member.participation_weight,
            reasoning: format!("{:?} vote based on role expertise", member.role),
        }
    }

    fn tally_votes(&self) -> (Decision, f64) {
        let mut approve_weight = 0.0;
        let mut _reject_weight = 0.0;
        let mut total_weight = 0.0;

        for vote in &self.votes {
            total_weight += vote.weight;
            match vote.value {
                VoteValue::Approve => approve_weight += vote.weight,
                VoteValue::Reject => _reject_weight += vote.weight,
                VoteValue::Abstain => {}
            }
        }

        if total_weight == 0.0 {
            return (
                Decision::Defer {
                    reason: "No votes cast".to_string(),
                },
                0.0,
            );
        }

        let approval_ratio = approve_weight / total_weight;

        if approval_ratio > 0.66 {
            (Decision::Approve, approval_ratio)
        } else if approval_ratio < 0.33 {
            (Decision::Reject, 1.0 - approval_ratio)
        } else {
            (
                Decision::Defer {
                    reason: "No clear majority".to_string(),
                },
                0.5,
            )
        }
    }

    fn create_simple_plan(&self, proposal: &Proposal) -> ExecutionPlan {
        ExecutionPlan {
            steps: vec![
                ExecutionStep {
                    sequence: 1,
                    description: format!("Execute: {}", proposal.title),
                    assigned_agent: proposal
                        .stigmergic_context
                        .as_ref()
                        .and_then(|ctx| ctx.preferred_agent)
                        .or(Some(proposal.proposer)),
                    dependencies: vec![],
                },
                ExecutionStep {
                    sequence: 2,
                    description: "Verify completion".to_string(),
                    assigned_agent: Some(proposal.proposer),
                    dependencies: vec![1],
                },
            ],
            estimated_duration_ms: 30000,
            rollback_strategy: Some("Manual revert".to_string()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vote {
    pub agent_id: AgentId,
    pub value: VoteValue,
    pub weight: f64,
    pub reasoning: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum VoteValue {
    Approve,
    Reject,
    Abstain,
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
