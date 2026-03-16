//! Phase 5: DKS-style survival pressure on the pattern population.
//!
//! Patterns that are not reinforced by new observations decay.
//! Only the most persistent patterns survive. This mirrors the
//! SelectionPressure mechanism from dks/selection.rs, applied at
//! the meta-level of crystallized knowledge rather than agents.

use super::{CrystallizedPattern, DreamConfig};

/// Result of survival pressure application.
#[derive(Clone, Debug, Default)]
pub struct SurvivalResult {
    pub patterns_before: usize,
    pub patterns_decayed: usize,
    pub patterns_pruned: usize,
    pub patterns_after: usize,
}

/// Apply DKS-style survival pressure to the pattern population.
///
/// 1. Decay persistence_score for all patterns not reinforced this generation
/// 2. Prune patterns whose persistence_score falls below zero
/// 3. Enforce max_patterns cap by pruning weakest
pub fn apply_survival_pressure(
    config: &DreamConfig,
    patterns: &mut Vec<CrystallizedPattern>,
    current_generation: u64,
) -> SurvivalResult {
    let patterns_before = patterns.len();
    let mut decayed = 0;

    // Step 1: Apply decay to patterns not reinforced in this generation
    for pattern in patterns.iter_mut() {
        if pattern.last_reinforced_generation < current_generation {
            // Not reinforced this generation → decay
            let generations_since = current_generation - pattern.last_reinforced_generation;
            let decay = config.pattern_decay_rate * generations_since as f64;
            pattern.persistence_score -= decay;
            decayed += 1;
        }

        // Additional decay based on low confidence
        if pattern.confidence < 0.3 {
            pattern.persistence_score -= config.pattern_decay_rate * 0.5;
        }

        // Additional decay for inconsistent patterns (high variance implies unreliable)
        // We use a heuristic: low observation count with moderate confidence = uncertain
        if pattern.observation_count < config.min_observations * 2 && pattern.confidence < 0.5 {
            pattern.persistence_score -= config.pattern_decay_rate * 0.3;
        }
    }

    // Step 2: Prune patterns with non-positive persistence
    let before_prune = patterns.len();
    patterns.retain(|p| p.persistence_score > 0.0);
    let pruned_by_decay = before_prune - patterns.len();

    // Step 3: Enforce max_patterns cap
    let mut pruned_by_cap = 0;
    if patterns.len() > config.max_patterns {
        // Sort by persistence_score descending, keep top max_patterns
        patterns.sort_by(|a, b| {
            b.persistence_score
                .partial_cmp(&a.persistence_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pruned_by_cap = patterns.len() - config.max_patterns;
        patterns.truncate(config.max_patterns);
    }

    SurvivalResult {
        patterns_before,
        patterns_decayed: decayed,
        patterns_pruned: pruned_by_decay + pruned_by_cap,
        patterns_after: patterns.len(),
    }
}

/// Compute a composite "fitness" score for a pattern, used for ranking.
/// Higher is better. Combines persistence, confidence, observation count,
/// and valence magnitude.
pub fn pattern_fitness(pattern: &CrystallizedPattern) -> f64 {
    let persistence = pattern.persistence_score;
    let confidence = pattern.confidence;
    let observation_factor = (pattern.observation_count as f64).ln().max(0.0) / 5.0; // log scale
    let valence_factor = pattern.valence.abs(); // magnitude, not sign

    persistence * 0.4 + confidence * 0.3 + observation_factor * 0.2 + valence_factor * 0.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dream::TemporalMotif;
    use crate::stigmergic_policy::TraceKind;
    use std::collections::HashMap;

    fn make_pattern(
        id: &str,
        persistence: f64,
        confidence: f64,
        obs: u64,
        last_gen: u64,
    ) -> CrystallizedPattern {
        CrystallizedPattern {
            id: id.into(),
            narrative: "test".into(),
            embedding: vec![],
            motif: TemporalMotif {
                trace_sequence: vec![TraceKind::PromiseMade],
                typical_duration_ticks: 5,
                associated_task_keys: vec![],
                transition_weights: vec![],
                min_match_length: 2,
            },
            valence: 0.5,
            confidence,
            observation_count: obs,
            role_affinity: HashMap::new(),
            origin_generation: 1,
            last_reinforced_generation: last_gen,
            temporal_reach: 50,
            persistence_score: persistence,
            created_at: 0,
            last_reinforced_at: 0,
        }
    }

    #[test]
    fn test_survival_no_decay_when_reinforced() {
        let config = DreamConfig::default();
        let mut patterns = vec![
            make_pattern("fresh", 1.0, 0.8, 10, 5), // reinforced at gen 5
        ];

        let result = apply_survival_pressure(&config, &mut patterns, 5);
        assert_eq!(result.patterns_before, 1);
        assert_eq!(result.patterns_decayed, 0);
        assert_eq!(result.patterns_pruned, 0);
        assert_eq!(result.patterns_after, 1);
        assert!((patterns[0].persistence_score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_survival_decays_stale_patterns() {
        let config = DreamConfig {
            pattern_decay_rate: 0.1,
            ..DreamConfig::default()
        };
        let mut patterns = vec![
            make_pattern("stale", 0.5, 0.8, 10, 1), // last reinforced gen 1
        ];

        let result = apply_survival_pressure(&config, &mut patterns, 5);
        assert_eq!(result.patterns_decayed, 1);
        // Decay: 0.1 * (5-1) = 0.4
        // New persistence: 0.5 - 0.4 = 0.1
        assert!(patterns[0].persistence_score > 0.0);
        assert!(patterns[0].persistence_score < 0.5);
    }

    #[test]
    fn test_survival_prunes_dead_patterns() {
        let config = DreamConfig {
            pattern_decay_rate: 0.5,
            ..DreamConfig::default()
        };
        let mut patterns = vec![
            make_pattern("dying", 0.3, 0.8, 10, 1), // persistence 0.3
        ];

        // Decay of 0.5 * (10-1) = 4.5 will kill it
        let result = apply_survival_pressure(&config, &mut patterns, 10);
        assert_eq!(result.patterns_pruned, 1);
        assert_eq!(result.patterns_after, 0);
    }

    #[test]
    fn test_survival_enforces_max_patterns() {
        let config = DreamConfig {
            max_patterns: 3,
            pattern_decay_rate: 0.0, // no decay for this test
            ..DreamConfig::default()
        };

        let mut patterns = vec![
            make_pattern("a", 0.5, 0.8, 10, 5),
            make_pattern("b", 0.9, 0.8, 10, 5),
            make_pattern("c", 0.3, 0.8, 10, 5),
            make_pattern("d", 0.7, 0.8, 10, 5),
            make_pattern("e", 0.1, 0.8, 10, 5),
        ];

        let result = apply_survival_pressure(&config, &mut patterns, 5);
        assert_eq!(result.patterns_before, 5);
        assert_eq!(result.patterns_after, 3);
        // Should keep the top 3 by persistence: b(0.9), d(0.7), a(0.5)
        let ids: Vec<&str> = patterns.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"b"));
        assert!(ids.contains(&"d"));
        assert!(ids.contains(&"a"));
    }

    #[test]
    fn test_pattern_fitness_ordering() {
        let strong = make_pattern("strong", 1.0, 0.9, 50, 5);
        let weak = make_pattern("weak", 0.1, 0.2, 2, 1);

        assert!(pattern_fitness(&strong) > pattern_fitness(&weak));
    }

    #[test]
    fn test_low_confidence_extra_decay() {
        let config = DreamConfig {
            pattern_decay_rate: 0.02,
            ..DreamConfig::default()
        };
        let mut low_conf = vec![
            make_pattern("low_conf", 0.5, 0.2, 10, 5), // confidence 0.2 < 0.3
        ];
        let mut high_conf = vec![
            make_pattern("high_conf", 0.5, 0.8, 10, 5), // confidence 0.8
        ];

        apply_survival_pressure(&config, &mut low_conf, 5);
        apply_survival_pressure(&config, &mut high_conf, 5);

        // Low confidence should have extra decay applied
        assert!(low_conf[0].persistence_score < high_conf[0].persistence_score);
    }
}
