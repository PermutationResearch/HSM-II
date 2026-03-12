//! Debate mode council - structured deliberation with pros/cons.
//!
//! Debate mode is designed for complex decisions requiring thorough evaluation.
//! It uses structured argumentation with opening statements, rebuttals, and synthesis.

use super::{
    CouncilDecision, CouncilDecisionMetadata, CouncilEvidence, CouncilEvidenceKind, CouncilId,
    CouncilMember, CouncilMode, CouncilStatus, Decision, ExecutionPlan, Proposal,
    TraceSummarizer,
};
use crate::agent::{AgentId, Role};
use crate::social_memory::SocialMemory;
use crate::stigmergic_policy::StigmergicMemory;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// A council operating in debate mode
pub struct DebateCouncil {
    council_id: CouncilId,
    members: Vec<CouncilMember>,
    rounds: Vec<DebateRound>,
    status: CouncilStatus,
}

impl DebateCouncil {
    pub fn new(council_id: CouncilId, members: Vec<CouncilMember>) -> Self {
        Self {
            council_id,
            members,
            rounds: Vec::new(),
            status: CouncilStatus::NotStarted,
        }
    }

    pub fn status(&self) -> CouncilStatus {
        self.status.clone()
    }

    /// Evaluate a proposal through structured debate
    pub async fn evaluate(
        &mut self,
        proposal: &Proposal,
        mode: CouncilMode,
    ) -> anyhow::Result<CouncilDecision> {
        self.status = CouncilStatus::InProgress {
            step: "opening_statements".to_string(),
            progress_pct: 0.0,
        };

        // Phase 1: Opening statements (each agent presents initial position)
        let mut opening_args = Vec::new();
        for member in self.members.iter() {
            let stance = self.determine_initial_stance(member, proposal);
            let evidence = self.select_relevant_evidence(proposal, 2);
            let argument = Argument {
                agent_id: member.agent_id,
                stance: stance.clone(),
                content: self
                    .generate_opening_statement(member, proposal, &stance, &evidence)
                    .await,
                evidence,
                round: 0,
                responding_to: None,
            };
            opening_args.push(argument);
        }

        let round0 = DebateRound {
            number: 0,
            phase: DebatePhase::Opening,
            arguments: opening_args,
        };
        self.rounds.push(round0);

        self.status = CouncilStatus::InProgress {
            step: "rebuttal".to_string(),
            progress_pct: 0.33,
        };

        // Phase 2: Rebuttal (critics and explorers challenge positions)
        let mut rebuttals = Vec::new();
        for member in self.members.iter() {
            if matches!(member.role, Role::Critic | Role::Explorer) {
                // Find arguments to rebut
                let targets: Vec<_> = self.rounds[0]
                    .arguments
                    .iter()
                    .filter(|a| a.agent_id != member.agent_id)
                    .collect();

                for target in targets.iter().take(2) {
                    let evidence = self.select_rebuttal_evidence(proposal, target);
                    let rebuttal = Argument {
                        agent_id: member.agent_id,
                        stance: Stance::Against,
                        content: self
                            .generate_rebuttal(member, proposal, target, &evidence)
                            .await,
                        evidence,
                        round: 1,
                        responding_to: Some(target.agent_id),
                    };
                    rebuttals.push(rebuttal);
                }
            }
        }

        let round1 = DebateRound {
            number: 1,
            phase: DebatePhase::Rebuttal,
            arguments: rebuttals,
        };
        self.rounds.push(round1);

        self.status = CouncilStatus::InProgress {
            step: "synthesis".to_string(),
            progress_pct: 0.66,
        };

        // Phase 3: Synthesis (architects and chroniclers summarize)
        let synthesis = self.generate_synthesis(proposal, &self.members).await;

        let round2 = DebateRound {
            number: 2,
            phase: DebatePhase::Synthesis,
            arguments: vec![synthesis],
        };
        self.rounds.push(round2);

        // Final decision based on debate analysis
        let decision = self.render_decision(proposal, &self.members);

        self.status = CouncilStatus::Completed {
            decision: decision.decision.clone(),
        };

        Ok(CouncilDecision {
            council_id: self.council_id,
            proposal_id: proposal.id.clone(),
            decision: decision.decision,
            confidence: decision.confidence,
            participating_agents: self.members.iter().map(|m| m.agent_id).collect(),
            execution_plan: decision.execution_plan,
            decided_at: current_timestamp(),
            mode_used: mode,
            metadata: self.collect_decision_metadata(proposal),
        })
    }

    fn determine_initial_stance(&self, member: &CouncilMember, proposal: &Proposal) -> Stance {
        // Role-based stance determination
        match member.role {
            Role::Architect => {
                if proposal.description.contains("structure")
                    || proposal.description.contains("coherence")
                {
                    Stance::For
                } else {
                    Stance::Neutral
                }
            }
            Role::Catalyst => {
                if proposal.description.contains("innovation")
                    || proposal.description.contains("improve")
                {
                    Stance::For
                } else {
                    Stance::Cautious
                }
            }
            Role::Critic => Stance::Against, // Critics default to skeptical
            Role::Explorer => Stance::Curious,
            Role::Chronicler => Stance::Neutral,
            Role::Coder => {
                if proposal.description.contains("code")
                    || proposal.description.contains("implement")
                    || proposal.description.contains("tool")
                {
                    Stance::For
                } else {
                    Stance::Neutral
                }
            }
        }
    }

    async fn generate_opening_statement(
        &self,
        member: &CouncilMember,
        proposal: &Proposal,
        stance: &Stance,
        evidence: &[CouncilEvidence],
    ) -> String {
        // Include trace summary context if available
        let trace_context = proposal
            .stigmergic_context
            .as_ref()
            .and_then(|ctx| {
                if ctx.graph_snapshot_bullets.is_empty() {
                    None
                } else {
                    Some(format!(
                        "\n\nStigmergic Context:\n{}",
                        ctx.graph_snapshot_bullets.join("\n")
                    ))
                }
            })
            .unwrap_or_default();
        
        // In a real implementation, this would call an LLM
        format!(
            "[Agent {} as {:?}] {} - {}{}{}",
            member.agent_id,
            member.role,
            stance.as_str(),
            self.stance_reasoning(stance, member.role, proposal),
            self.format_evidence_suffix(evidence),
            trace_context,
        )
    }

    async fn generate_rebuttal(
        &self,
        member: &CouncilMember,
        proposal: &Proposal,
        target: &Argument,
        evidence: &[CouncilEvidence],
    ) -> String {
        format!(
            "[Agent {} as {:?}] Rebuttal to Agent {}: Consider the risks of {}.{}",
            member.agent_id,
            member.role,
            target.agent_id,
            proposal.title,
            self.format_evidence_suffix(evidence),
        )
    }

    async fn generate_synthesis(&self, proposal: &Proposal, members: &[CouncilMember]) -> Argument {
        // Count stances
        let mut for_count = 0;
        let mut against_count = 0;
        let mut neutral_count = 0;

        for round in &self.rounds {
            for arg in &round.arguments {
                match arg.stance {
                    Stance::For => for_count += 1,
                    Stance::Against => against_count += 1,
                    _ => neutral_count += 1,
                }
            }
        }

        let overall_stance = if for_count > against_count * 2 {
            Stance::For
        } else if against_count > for_count {
            Stance::Against
        } else {
            Stance::Cautious
        };

        // Find chronicler or architect for synthesis
        let synthesizer = members
            .iter()
            .find(|m| matches!(m.role, Role::Chronicler | Role::Architect))
            .map(|m| m.agent_id)
            .unwrap_or(members[0].agent_id);
        let evidence = self.collect_round_evidence(4);
        let graph_bullets = proposal
            .stigmergic_context
            .as_ref()
            .map(|ctx| ctx.graph_snapshot_bullets.join(" | "))
            .filter(|bullets| !bullets.is_empty())
            .unwrap_or_default();

        Argument {
            agent_id: synthesizer,
            stance: overall_stance,
            content: format!(
                "Synthesis: {} arguments for, {} against, {} neutral. Proposal '{}' requires careful consideration of implementation details. {}{}",
                for_count,
                against_count,
                neutral_count,
                proposal.title,
                if graph_bullets.is_empty() {
                    String::new()
                } else {
                    format!("Graph snapshot: {}. ", graph_bullets)
                },
                self.format_evidence_suffix(&evidence),
            ),
            evidence,
            round: 2,
            responding_to: None,
        }
    }

    fn render_decision(&self, proposal: &Proposal, members: &[CouncilMember]) -> DecisionResult {
        // Analyze all arguments to make decision
        let mut for_weight = 0.0;
        let mut against_weight = 0.0;

        for round in &self.rounds {
            for arg in &round.arguments {
                let member = members.iter().find(|m| m.agent_id == arg.agent_id);
                let weight = member.map(|m| m.participation_weight).unwrap_or(1.0);

                match arg.stance {
                    Stance::For => for_weight += weight,
                    Stance::Against => against_weight += weight,
                    Stance::Cautious => against_weight += weight * 0.5,
                    _ => {}
                }
            }
        }

        let total = for_weight + against_weight;
        let approval_ratio = if total > 0.0 { for_weight / total } else { 0.5 };

        let (decision, confidence) = if approval_ratio > 0.7 {
            (Decision::Approve, approval_ratio)
        } else if approval_ratio < 0.3 {
            (Decision::Reject, 1.0 - approval_ratio)
        } else {
            (
                Decision::Defer {
                    reason: "Insufficient consensus after debate".to_string(),
                },
                0.5,
            )
        };

        let execution_plan = if matches!(decision, Decision::Approve) {
            Some(self.create_execution_plan(proposal, members))
        } else {
            None
        };

        DecisionResult {
            decision,
            confidence,
            execution_plan,
        }
    }

    fn create_execution_plan(
        &self,
        proposal: &Proposal,
        members: &[CouncilMember],
    ) -> ExecutionPlan {
        let steps = vec![
            ExecutionStep {
                sequence: 1,
                description: format!("Initialize: {}", proposal.title),
                assigned_agent: Some(proposal.proposer),
                dependencies: vec![],
            },
            ExecutionStep {
                sequence: 2,
                description: "Validate implementation requirements".to_string(),
                assigned_agent: members
                    .iter()
                    .find(|m| matches!(m.role, Role::Critic))
                    .map(|m| m.agent_id),
                dependencies: vec![1],
            },
            ExecutionStep {
                sequence: 3,
                description: "Document changes".to_string(),
                assigned_agent: members
                    .iter()
                    .find(|m| matches!(m.role, Role::Chronicler))
                    .map(|m| m.agent_id),
                dependencies: vec![2],
            },
        ];

        ExecutionPlan {
            steps,
            estimated_duration_ms: 60000,
            rollback_strategy: Some("Revert to previous state".to_string()),
        }
    }

    fn stance_reasoning(&self, stance: &Stance, role: Role, _proposal: &Proposal) -> String {
        match stance {
            Stance::For => format!("As {:?}, I see this enhancing our capabilities", role),
            Stance::Against => format!("As {:?}, I have concerns about risks", role),
            Stance::Neutral => format!("As {:?}, I need more information", role),
            Stance::Cautious => format!("As {:?}, I support with reservations", role),
            Stance::Curious => format!("As {:?}, I want to explore implications", role),
        }
    }

    fn select_relevant_evidence(
        &self,
        proposal: &Proposal,
        max_items: usize,
    ) -> Vec<CouncilEvidence> {
        let mut evidence = proposal
            .stigmergic_context
            .as_ref()
            .map(|ctx| ctx.all_evidence())
            .unwrap_or_default();
        evidence.sort_by_key(|item| match item.kind {
            CouncilEvidenceKind::Directive => 0,
            CouncilEvidenceKind::Trace => 1,
            CouncilEvidenceKind::PolicyShift => 2,
            CouncilEvidenceKind::GraphQuery => 3,
        });
        evidence.truncate(max_items);
        evidence
    }

    fn select_rebuttal_evidence(
        &self,
        proposal: &Proposal,
        target: &Argument,
    ) -> Vec<CouncilEvidence> {
        let mut evidence = target.evidence.clone();
        if evidence.len() < 2 {
            for item in self.select_relevant_evidence(proposal, 3) {
                if evidence.iter().any(|existing| existing.id == item.id) {
                    continue;
                }
                evidence.push(item);
                if evidence.len() >= 2 {
                    break;
                }
            }
        }
        evidence
    }

    fn collect_round_evidence(&self, max_items: usize) -> Vec<CouncilEvidence> {
        let mut evidence = Vec::new();
        let mut seen = BTreeSet::new();
        for round in &self.rounds {
            for argument in &round.arguments {
                for item in &argument.evidence {
                    if seen.insert(item.id.clone()) {
                        evidence.push(item.clone());
                        if evidence.len() >= max_items {
                            return evidence;
                        }
                    }
                }
            }
        }
        evidence
    }

    fn format_evidence_suffix(&self, evidence: &[CouncilEvidence]) -> String {
        if evidence.is_empty() {
            return String::new();
        }

        let fragments = evidence
            .iter()
            .map(|item| format!("{} says {}", item.id, item.summary))
            .collect::<Vec<_>>()
            .join("; ");
        format!(" Evidence: {fragments}")
    }

    fn collect_decision_metadata(&self, proposal: &Proposal) -> CouncilDecisionMetadata {
        let mut metadata = proposal
            .stigmergic_context
            .as_ref()
            .map(|ctx| ctx.audit_metadata())
            .unwrap_or_default();
        
        // Log all evidence cited in arguments
        for round in &self.rounds {
            for argument in &round.arguments {
                for evidence in &argument.evidence {
                    metadata.record_evidence(evidence);
                }
            }
        }
        
        // Add summary bullets from trace context
        if let Some(ref ctx) = proposal.stigmergic_context {
            for bullet in &ctx.graph_snapshot_bullets {
                if !metadata.graph_snapshot_bullets.contains(bullet) {
                    metadata.graph_snapshot_bullets.push(bullet.clone());
                }
            }
        }
        
        metadata.dedupe();
        metadata
    }

    /// Enrich proposal with trace summary for better deliberation
    pub fn enrich_with_trace_summary(
        &self,
        proposal: &mut Proposal,
        stigmergic_memory: &StigmergicMemory,
        social_memory: Option<&SocialMemory>,
        current_tick: u64,
    ) {
        let summarizer = TraceSummarizer::default();
        let summary = summarizer.summarize_for_council(
            stigmergic_memory,
            social_memory,
            current_tick,
            proposal.task_key.as_deref(),
        );
        
        // Create or update proposal context with summarized bullets
        let bullets = summarizer.to_bullet_points(&summary)
            .lines()
            .map(|s| s.to_string())
            .collect();
        
        if let Some(ref mut context) = proposal.stigmergic_context {
            context.graph_snapshot_bullets = bullets;
        } else {
            proposal.stigmergic_context = Some(super::StigmergicCouncilContext {
                preferred_agent: None,
                preferred_tool: None,
                confidence: 0.5,
                require_council_review: false,
                rationale: "Auto-populated from trace summary".to_string(),
                evidence: vec![],
                graph_snapshot_bullets: bullets,
                graph_queries: vec![],
            });
        }
    }

    /// Get formatted trace summary for prompt injection
    pub fn format_trace_summary_for_prompt(
        &self,
        stigmergic_memory: &StigmergicMemory,
        social_memory: Option<&SocialMemory>,
        current_tick: u64,
        task_key: Option<&str>,
    ) -> String {
        let summarizer = TraceSummarizer::default();
        let summary = summarizer.summarize_for_council(
            stigmergic_memory,
            social_memory,
            current_tick,
            task_key,
        );
        summarizer.to_bullet_points(&summary)
    }

}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DebateRound {
    pub number: usize,
    pub phase: DebatePhase,
    pub arguments: Vec<Argument>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Argument {
    pub agent_id: AgentId,
    pub stance: Stance,
    pub content: String,
    #[serde(default)]
    pub evidence: Vec<CouncilEvidence>,
    pub round: usize,
    pub responding_to: Option<AgentId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum DebatePhase {
    Opening,
    Rebuttal,
    Synthesis,
    FinalVote,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Stance {
    For,
    Against,
    Neutral,
    Cautious,
    Curious,
}

impl Stance {
    fn as_str(&self) -> &'static str {
        match self {
            Stance::For => "In favor",
            Stance::Against => "Opposed",
            Stance::Neutral => "Neutral",
            Stance::Cautious => "Cautiously supportive",
            Stance::Curious => "Exploratory",
        }
    }
}

struct DecisionResult {
    decision: Decision,
    confidence: f64,
    execution_plan: Option<ExecutionPlan>,
}

use super::ExecutionStep;

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
