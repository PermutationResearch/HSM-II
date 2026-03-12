use super::{ActionKind, ProposedAction};
use crate::council::{
    CouncilMember, ModeConfig, ModeSelectionReport, ModeSwitcher, Proposal as CouncilProposal,
};
use crate::ouroboros_compat::phase2_risk_gate::RiskAssessment;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouncilBridgeConfig {
    pub min_confidence_for_approval: f64,
    pub min_evidence_coverage_for_approval: f64,
    pub mode_config: ModeConfig,
}

impl Default for CouncilBridgeConfig {
    fn default() -> Self {
        Self {
            min_confidence_for_approval: 0.65,
            min_evidence_coverage_for_approval: 1.0,
            mode_config: ModeConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouncilGatePlan {
    pub council_required: bool,
    pub proposal: Option<CouncilProposal>,
    pub mode_report: Option<ModeSelectionReport>,
    pub min_confidence_for_approval: f64,
    pub min_evidence_coverage_for_approval: f64,
}

pub struct CouncilBridge {
    config: CouncilBridgeConfig,
    mode_switcher: ModeSwitcher,
}

impl CouncilBridge {
    pub fn new(config: CouncilBridgeConfig) -> Self {
        let mode_switcher = ModeSwitcher::new(config.mode_config.clone());
        Self {
            config,
            mode_switcher,
        }
    }

    pub fn plan(
        &self,
        action: &ProposedAction,
        risk: &RiskAssessment,
        members: &[CouncilMember],
    ) -> CouncilGatePlan {
        if !risk.council_required {
            return CouncilGatePlan {
                council_required: false,
                proposal: None,
                mode_report: None,
                min_confidence_for_approval: self.config.min_confidence_for_approval,
                min_evidence_coverage_for_approval: self.config.min_evidence_coverage_for_approval,
            };
        }

        let proposal = self.to_proposal(action, risk);
        let report = self
            .mode_switcher
            .select_mode_with_report(&proposal, members);
        CouncilGatePlan {
            council_required: true,
            proposal: Some(proposal),
            mode_report: Some(report),
            min_confidence_for_approval: self.config.min_confidence_for_approval,
            min_evidence_coverage_for_approval: self.config.min_evidence_coverage_for_approval,
        }
    }

    fn to_proposal(&self, action: &ProposedAction, risk: &RiskAssessment) -> CouncilProposal {
        let mut proposal = CouncilProposal::new(
            &action.id,
            &action.title,
            &action.description,
            action.actor_id.parse::<u64>().unwrap_or(0),
        );
        proposal.proposed_at = now_secs();
        proposal.urgency = match action.kind {
            ActionKind::FederationSync => 0.75,
            ActionKind::SelfModification => 0.70,
            ActionKind::ExternalWrite => 0.68,
            ActionKind::InvestigationQuery => 0.45,
            ActionKind::MemoryMutation => 0.50,
            ActionKind::ReadOnly => 0.25,
        };
        proposal.complexity = ((risk.score + proposal.urgency) / 2.0).clamp(0.0, 1.0);
        proposal.estimate_complexity();
        proposal
    }

    pub fn should_approve(
        &self,
        council_confidence: f64,
        evidence_coverage: f64,
        policy_allows_execution: bool,
    ) -> bool {
        policy_allows_execution
            && council_confidence >= self.config.min_confidence_for_approval
            && evidence_coverage >= self.config.min_evidence_coverage_for_approval
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Role;
    use crate::ouroboros_compat::phase2_risk_gate::{RiskAssessment, RiskLevel};

    #[test]
    fn requires_council_for_high_risk_actions() {
        let bridge = CouncilBridge::new(CouncilBridgeConfig::default());
        let action = ProposedAction {
            id: "id".to_string(),
            title: "Mutate".to_string(),
            description: "Perform self modification and federation changes".to_string(),
            actor_id: "1".to_string(),
            kind: ActionKind::SelfModification,
            target_path: Some("src/main.rs".to_string()),
            target_peer: None,
            metadata: Default::default(),
        };
        let risk = RiskAssessment {
            level: RiskLevel::High,
            score: 0.9,
            reasons: vec!["high".to_string()],
            council_required: true,
        };
        let members = vec![CouncilMember {
            agent_id: 1,
            role: Role::Architect,
            expertise_score: 0.9,
            participation_weight: 1.0,
        }];
        let plan = bridge.plan(&action, &risk, &members);
        assert!(plan.council_required);
        assert!(plan.mode_report.is_some());
    }
}
