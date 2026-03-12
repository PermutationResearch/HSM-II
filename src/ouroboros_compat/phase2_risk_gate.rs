use super::{ActionKind, ProposedAction};
use crate::ouroboros_compat::phase1_policy::{PolicyDecision, PolicyVerdict};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub level: RiskLevel,
    pub score: f64,
    pub reasons: Vec<String>,
    pub council_required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskGateConfig {
    pub high_risk_kinds: HashSet<ActionKind>,
    pub high_risk_score_threshold: f64,
}

impl Default for RiskGateConfig {
    fn default() -> Self {
        let high_risk_kinds = [
            ActionKind::SelfModification,
            ActionKind::ExternalWrite,
            ActionKind::FederationSync,
        ]
        .into_iter()
        .collect();
        Self {
            high_risk_kinds,
            high_risk_score_threshold: 0.7,
        }
    }
}

pub struct RiskGate {
    config: RiskGateConfig,
}

impl RiskGate {
    pub fn new(config: RiskGateConfig) -> Self {
        Self { config }
    }

    pub fn assess(&self, action: &ProposedAction, verdict: &PolicyVerdict) -> RiskAssessment {
        let mut score: f64 = 0.05;
        let mut reasons = Vec::new();

        if self.config.high_risk_kinds.contains(&action.kind) {
            score += 0.65;
            reasons.push("action kind is high-risk".to_string());
        }

        if matches!(verdict.decision, PolicyDecision::ReviewRequired) {
            score += 0.2;
            reasons.push("constitution policy requested review".to_string());
        }

        if matches!(verdict.decision, PolicyDecision::Deny) {
            score = 1.0;
            reasons.push("constitution policy denied action".to_string());
        }

        if action
            .metadata
            .get("touches_external_system")
            .map(|v| v == "true")
            .unwrap_or(false)
        {
            score += 0.15;
            reasons.push("touches external system".to_string());
        }

        let score = score.clamp(0.0, 1.0);
        let level = if score >= self.config.high_risk_score_threshold {
            RiskLevel::High
        } else {
            RiskLevel::Low
        };
        let council_required = level == RiskLevel::High;

        RiskAssessment {
            level,
            score,
            reasons,
            council_required,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ouroboros_compat::phase1_policy::PolicyVerdict;

    #[test]
    fn marks_federation_sync_high_risk() {
        let gate = RiskGate::new(RiskGateConfig::default());
        let action = ProposedAction {
            id: "x".to_string(),
            title: "sync".to_string(),
            description: "sync".to_string(),
            actor_id: "a".to_string(),
            kind: ActionKind::FederationSync,
            target_path: None,
            target_peer: Some("peer".to_string()),
            metadata: Default::default(),
        };
        let out = gate.assess(
            &action,
            &PolicyVerdict {
                decision: PolicyDecision::Allow,
                reasons: vec![],
            },
        );
        assert_eq!(out.level, RiskLevel::High);
        assert!(out.council_required);
    }
}
