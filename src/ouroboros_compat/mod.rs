//! Ouroboros compatibility layer on top of HSM-II.
//!
//! This module implements the phase foundation for migrating Ouroboros
//! semantics into HSM-II:
//! - Phase 1: constitution and identity policy checks
//! - Phase 2: risk assessment and council gating
//! - Phase 3: council mode bridge for high-risk actions
//! - Phase 4: evidence contract enforcement for investigations
//! - Phase 5: operational SLOs, federation mesh checks, and event-sourced memory

pub mod phase1_policy;
pub mod phase2_risk_gate;
pub mod phase3_council_bridge;
pub mod phase4_evidence_contract;
pub mod phase5_ops_memory;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Normalized action categories used by the migration layer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ActionKind {
    SelfModification,
    ExternalWrite,
    FederationSync,
    InvestigationQuery,
    MemoryMutation,
    ReadOnly,
}

/// Action descriptor passed through policy, risk, council, and evidence checks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProposedAction {
    pub id: String,
    pub title: String,
    pub description: String,
    pub actor_id: String,
    pub kind: ActionKind,
    pub target_path: Option<String>,
    pub target_peer: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl ProposedAction {
    pub fn is_high_risk_kind(&self) -> bool {
        matches!(
            self.kind,
            ActionKind::SelfModification | ActionKind::ExternalWrite | ActionKind::FederationSync
        )
    }
}
