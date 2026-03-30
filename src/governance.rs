//! Governance defaults: RACI-style accountability and incident playbooks.
//! These are **specification hooks** for operators; enforcement remains in policy / auth layers.

/// Short RACI summary for belief sources, supervision, federation, tools, and model changes.
pub const RACI_SUMMARY: &str = r#"HSM-II governance defaults (adapt per org):
- Belief sources: Researcher/owner (A) approves automated sources above org thresholds; Operator (R) runs ingestion; Audit (C) reviews quarterly.
- Supervision-loop weights / exploration: ML/Platform (A); Runtime (R); Security (C) on autonomy increases.
- Federation trust edges: Security (A) for peer onboarding; SRE (R) for partition behavior; Legal (C) for data scope.
- Tool deployment & allowlists: Security (A); Dev (R); SRE (C) for prod.
- Model upgrades: Model owner (A); Infra (R); Compliance (C).
"#;

/// Incident response outline when coherence optima diverge from task success or state forks.
pub const INCIDENT_PLAYBOOK: &str = r#"1. Freeze mutations: set HSM_FREEZE_MUTATIONS=1 (operator) or stop supervision ticks.
2. Snapshot world + decision_log for audit; note tick_count and schema format_version.
3. Resolve narrative conflicts via explicit supersedes_belief_id / owner_namespace rules — avoid silent merges.
4. Roll back to last known-good embedded snapshot if integrity checks fail.
5. Post-mortem: compare held-out task metrics vs internal coherence; adjust GuardrailWeights and exploration epsilon.
"#;

/// Federation operator expectations (conflict + partition).
pub const FEDERATION_OPERATIONS: &str = r#"Trust edges are scoped grants — not blanket write access. Under partition, prefer read-only degrade until quorum-based merge; document merge strategy per artifact type (beliefs vs edges)."#;
