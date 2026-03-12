use crate::investigation_tools::ToolCallRecord;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub investigation_session_id: Option<String>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub evidence_chain_count: usize,
    pub claim_count: usize,
    pub evidence_count: usize,
    pub coverage: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceRequirements {
    pub require_investigation_session: bool,
    pub min_tool_calls: usize,
    pub min_evidence_chains: usize,
    pub min_claims: usize,
    pub min_evidence: usize,
    pub min_coverage: f64,
}

impl Default for EvidenceRequirements {
    fn default() -> Self {
        Self {
            require_investigation_session: true,
            min_tool_calls: 1,
            min_evidence_chains: 1,
            min_claims: 1,
            min_evidence: 1,
            min_coverage: 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceValidation {
    pub ok: bool,
    pub reasons: Vec<String>,
}

pub struct EvidenceContract {
    requirements: EvidenceRequirements,
}

impl EvidenceContract {
    pub fn new(requirements: EvidenceRequirements) -> Self {
        Self { requirements }
    }

    pub fn validate(&self, bundle: &EvidenceBundle) -> EvidenceValidation {
        let mut reasons = Vec::new();

        if self.requirements.require_investigation_session
            && bundle
                .investigation_session_id
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
        {
            reasons.push("missing investigation session id".to_string());
        }

        if bundle.tool_calls.len() < self.requirements.min_tool_calls {
            reasons.push(format!(
                "insufficient tool-call audit trail: {} < {}",
                bundle.tool_calls.len(),
                self.requirements.min_tool_calls
            ));
        }

        if bundle.evidence_chain_count < self.requirements.min_evidence_chains {
            reasons.push(format!(
                "insufficient evidence chains: {} < {}",
                bundle.evidence_chain_count, self.requirements.min_evidence_chains
            ));
        }

        if bundle.claim_count < self.requirements.min_claims {
            reasons.push(format!(
                "insufficient claims: {} < {}",
                bundle.claim_count, self.requirements.min_claims
            ));
        }

        if bundle.evidence_count < self.requirements.min_evidence {
            reasons.push(format!(
                "insufficient evidence items: {} < {}",
                bundle.evidence_count, self.requirements.min_evidence
            ));
        }

        if bundle.coverage < self.requirements.min_coverage {
            reasons.push(format!(
                "insufficient evidence coverage: {:.2} < {:.2}",
                bundle.coverage, self.requirements.min_coverage
            ));
        }

        EvidenceValidation {
            ok: reasons.is_empty(),
            reasons,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_call() -> ToolCallRecord {
        ToolCallRecord {
            id: "tc1".to_string(),
            tool_name: "read_file".to_string(),
            arguments: json!({"path":"x"}),
            result_value: Some(json!({"ok":true})),
            error_message: None,
            started_at: "2026-02-25T00:00:00Z".to_string(),
            completed_at: "2026-02-25T00:00:01Z".to_string(),
        }
    }

    #[test]
    fn rejects_missing_evidence_contract() {
        let c = EvidenceContract::new(EvidenceRequirements::default());
        let out = c.validate(&EvidenceBundle {
            investigation_session_id: None,
            tool_calls: vec![],
            evidence_chain_count: 0,
            claim_count: 0,
            evidence_count: 0,
            coverage: 0.0,
        });
        assert!(!out.ok);
        assert!(!out.reasons.is_empty());
    }

    #[test]
    fn accepts_good_bundle() {
        let c = EvidenceContract::new(EvidenceRequirements::default());
        let out = c.validate(&EvidenceBundle {
            investigation_session_id: Some("sess".to_string()),
            tool_calls: vec![sample_call()],
            evidence_chain_count: 1,
            claim_count: 1,
            evidence_count: 2,
            coverage: 1.5,
        });
        assert!(out.ok);
    }
}
