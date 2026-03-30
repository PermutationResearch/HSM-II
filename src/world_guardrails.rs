//! Multi-objective guardrails, exploration, and belief explainability — mitigates Goodharting on coherence alone.
//!
//! Env overrides:
//! - `HSM_GUARD_COHERENCE_W` / `HSM_GUARD_NOVELTY_W` / `HSM_GUARD_DISSENT_W` — weights for supervision mutation scoring (default 0.5 / 0.3 / 0.2).
//! - `HSM_EXPLORATION_EPSILON` — probability of picking a random mutation among candidates (default `0.08`).
//! - `HSM_BELIEF_EXPLAIN_SLO_CONF` — automated beliefs above this confidence need evidence (default `0.88`).
//! - `HSM_BELIEF_MIN_EVIDENCE` — min supporting strings + evidence_belief_ids for that SLO (default `2`).
//!
//! `HSM_FREEZE_MUTATIONS=1` — skip applying supervision mutations (early return).

/// Weights for mutation supervision: **never** collapse to a single scalar before logging components.
#[derive(Clone, Debug)]
pub struct GuardrailWeights {
    pub w_coherence: f32,
    pub w_novelty: f32,
    pub w_dissent: f32,
}

impl GuardrailWeights {
    pub fn from_env() -> Self {
        let parse = |key: &str, d: f32| -> f32 {
            std::env::var(key)
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|v: &f32| *v >= 0.0)
                .unwrap_or(d)
        };
        Self {
            w_coherence: parse("HSM_GUARD_COHERENCE_W", 0.5),
            w_novelty: parse("HSM_GUARD_NOVELTY_W", 0.3),
            w_dissent: parse("HSM_GUARD_DISSENT_W", 0.2),
        }
    }

    /// Pareto-style composite for **ranking only**; components must still be stored separately.
    pub fn composite_rank(&self, coherence_term: f32, novelty_term: f32, dissent_term: f32) -> f32 {
        self.w_coherence * coherence_term
            + self.w_novelty * novelty_term
            + self.w_dissent * dissent_term
    }
}

/// ε-greedy exploration for mutation selection (caps local-improvement lock-in).
pub fn exploration_epsilon() -> f64 {
    std::env::var("HSM_EXPLORATION_EPSILON")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|e: &f64| *e >= 0.0 && *e <= 1.0)
        .unwrap_or(0.08)
}

/// Normalized spread of simulated coherences across candidates (0 = agreement, higher = more dissent / exploration signal).
pub fn dissent_from_simulations(sim_coherences: &[f32]) -> f32 {
    if sim_coherences.len() < 2 {
        return 0.0;
    }
    let n = sim_coherences.len() as f32;
    let mean: f32 = sim_coherences.iter().copied().sum::<f32>() / n;
    let var: f32 = sim_coherences
        .iter()
        .map(|x| (x - mean).powi(2))
        .sum::<f32>()
        / n;
    // σ in [0, ~0.5] typical; map gently to [0, 1]
    (var * 4.0).min(1.0)
}

/// Clamp automated high-confidence beliefs that lack explicit evidence (procedural / explainability SLO).
pub fn apply_belief_explainability_cap(
    mut confidence: f64,
    supporting_evidence_len: usize,
    evidence_belief_ids_len: usize,
    user_provided: bool,
) -> f64 {
    let slo: f64 = std::env::var("HSM_BELIEF_EXPLAIN_SLO_CONF")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|v: &f64| *v > 0.0 && *v <= 1.0)
        .unwrap_or(0.88);
    let min_evidence: usize = std::env::var("HSM_BELIEF_MIN_EVIDENCE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);

    let automated = !user_provided;
    let evidence_total = supporting_evidence_len + evidence_belief_ids_len;
    if automated && confidence >= slo && evidence_total < min_evidence {
        confidence = confidence.min(0.72);
    }
    confidence
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dissent_is_zero_for_uniform_values() {
        let d = dissent_from_simulations(&[0.5, 0.5, 0.5]);
        assert!((d - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn explainability_cap_applies_to_automated_high_confidence() {
        let capped = apply_belief_explainability_cap(0.95, 0, 0, false);
        assert!(capped <= 0.72);
    }

    #[test]
    fn explainability_cap_not_applied_when_evidence_present() {
        let kept = apply_belief_explainability_cap(0.95, 2, 0, false);
        assert!((kept - 0.95).abs() < f64::EPSILON);
    }
}
