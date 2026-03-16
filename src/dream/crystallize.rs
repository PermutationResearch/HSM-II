//! Phase 3: Crystallize detected motifs into CrystallizedPattern structs.
//! Matches against existing patterns to reinforce, or creates new ones.
//! Also generates proto-skills from high-confidence positive patterns.

use std::collections::HashMap;

use crate::agent::Role;
use crate::stigmergic_policy::TraceKind;

use super::{
    current_timestamp, CrystallizedPattern, DetectedMotif, DreamConfig, ProtoSkill, TemporalMotif,
};

/// Crystallize detected motifs into patterns.
/// Returns (new_patterns, count_reinforced).
pub fn crystallize(
    config: &DreamConfig,
    existing_patterns: &mut Vec<CrystallizedPattern>,
    motifs: &[DetectedMotif],
    dream_generation: u64,
) -> (Vec<CrystallizedPattern>, usize) {
    let mut new_patterns = Vec::new();
    let mut reinforced = 0;

    for motif in motifs {
        // Try to match against existing patterns
        if let Some(existing) = find_matching_pattern(existing_patterns, motif) {
            // Reinforce: increase observation count, boost confidence
            existing.observation_count += motif.observation_count;
            existing.confidence =
                bayesian_update(existing.confidence, motif_raw_confidence(motif));
            existing.last_reinforced_generation = dream_generation;
            existing.last_reinforced_at = current_timestamp();
            existing.persistence_score += 0.1; // DKS: being reinforced = persisting
            // Update valence as running average
            let total_obs = existing.observation_count as f64;
            existing.valence = existing.valence * ((total_obs - 1.0) / total_obs)
                + motif.avg_outcome * (1.0 / total_obs);
            reinforced += 1;
        } else {
            // Crystallize new pattern
            let pattern = CrystallizedPattern {
                id: format!("dream_{}_{}", dream_generation, new_patterns.len()),
                narrative: generate_narrative(motif),
                embedding: generate_embedding(motif),
                motif: TemporalMotif {
                    trace_sequence: motif.trace_sequence.clone(),
                    typical_duration_ticks: compute_typical_duration(motif),
                    associated_task_keys: extract_task_keys(motif),
                    transition_weights: compute_transition_weights(motif),
                    min_match_length: (motif.trace_sequence.len() / 2).max(2),
                },
                valence: motif.avg_outcome,
                confidence: motif_raw_confidence(motif),
                observation_count: motif.observation_count,
                role_affinity: extract_role_affinity(motif),
                origin_generation: dream_generation,
                last_reinforced_generation: dream_generation,
                temporal_reach: config.replay_horizon / 4, // reasonable default
                persistence_score: 1.0,
                created_at: current_timestamp(),
                last_reinforced_at: current_timestamp(),
            };
            new_patterns.push(pattern.clone());
            existing_patterns.push(pattern);
        }
    }

    (new_patterns, reinforced)
}

/// Generate proto-skills from high-confidence positive patterns.
pub fn generate_proto_skills(
    config: &DreamConfig,
    patterns: &[CrystallizedPattern],
) -> Vec<ProtoSkill> {
    patterns
        .iter()
        .filter(|p| {
            p.valence > 0.1
                && p.confidence >= config.proto_skill_confidence_threshold
                && p.observation_count >= config.proto_skill_min_observations
                && p.persistence_score > 0.3
        })
        .map(|p| ProtoSkill {
            name: format!("dream_{}", &p.id),
            description: p.narrative.clone(),
            source_pattern_id: p.id.clone(),
            source_dream_generation: p.origin_generation,
            initial_confidence: p.confidence * 0.8, // discount for transfer
            associated_task_keys: p.motif.associated_task_keys.clone(),
            embedding: Some(p.embedding.clone()),
        })
        .collect()
}

// ── Internal helpers ────────────────────────────────────────────────────

fn find_matching_pattern<'a>(
    patterns: &'a mut [CrystallizedPattern],
    motif: &DetectedMotif,
) -> Option<&'a mut CrystallizedPattern> {
    patterns.iter_mut().find(|p| {
        // Match if the trace sequences are sufficiently similar
        let seq_overlap = sequence_overlap(&p.motif.trace_sequence, &motif.trace_sequence);
        seq_overlap >= 0.6
    })
}

fn sequence_overlap(a: &[TraceKind], b: &[TraceKind]) -> f64 {
    let min_len = a.len().min(b.len());
    if min_len == 0 {
        return 0.0;
    }
    let matches = a
        .iter()
        .zip(b.iter())
        .filter(|(x, y)| x.as_str() == y.as_str())
        .count();
    matches as f64 / min_len as f64
}

fn bayesian_update(prior: f64, new_evidence: f64) -> f64 {
    // Simple Bayesian confidence update: weighted combination
    let alpha = 0.3; // weight on new evidence
    ((1.0 - alpha) * prior + alpha * new_evidence).clamp(0.01, 0.99)
}

fn motif_raw_confidence(motif: &DetectedMotif) -> f64 {
    // Confidence from observation count + outcome consistency
    let count_factor = 1.0 - (1.0 / (motif.observation_count as f64 + 1.0));
    let consistency_factor = 1.0 / (1.0 + motif.outcome_variance);
    (count_factor * 0.6 + consistency_factor * 0.4).clamp(0.01, 0.99)
}

fn generate_narrative(motif: &DetectedMotif) -> String {
    let sequence_desc: Vec<&str> = motif.trace_sequence.iter().map(|k| k.as_str()).collect();

    let valence_word = if motif.avg_outcome > 0.1 {
        "beneficial"
    } else if motif.avg_outcome < -0.1 {
        "harmful"
    } else {
        "neutral"
    };

    format!(
        "Recurring {} pattern (observed {} times, variance {:.3}): [{}] -> avg outcome {:.3}",
        valence_word,
        motif.observation_count,
        motif.outcome_variance,
        sequence_desc.join(" -> "),
        motif.avg_outcome,
    )
}

fn generate_embedding(motif: &DetectedMotif) -> Vec<f32> {
    // Use the centroid of the motif's member windows as a float32 embedding.
    motif.centroid.iter().map(|&v| v as f32).collect()
}

fn compute_typical_duration(motif: &DetectedMotif) -> u64 {
    if motif.member_windows.is_empty() {
        return 0;
    }
    let total: u64 = motif
        .member_windows
        .iter()
        .map(|w| {
            let first_tick = w.elements.first().map(|e| e.tick).unwrap_or(0);
            let last_tick = w.elements.last().map(|e| e.tick).unwrap_or(0);
            last_tick.saturating_sub(first_tick)
        })
        .sum();
    total / motif.member_windows.len() as u64
}

fn extract_task_keys(motif: &DetectedMotif) -> Vec<String> {
    let mut keys: Vec<String> = motif
        .member_windows
        .iter()
        .flat_map(|w| w.elements.iter().filter_map(|e| e.task_key.clone()))
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

fn compute_transition_weights(motif: &DetectedMotif) -> Vec<f64> {
    if motif.trace_sequence.len() < 2 {
        return Vec::new();
    }
    // For each transition in the canonical sequence, compute how often
    // that transition actually appeared in member windows
    let n_transitions = motif.trace_sequence.len() - 1;
    let mut weights = vec![0.0; n_transitions];
    let n_members = motif.member_windows.len() as f64;

    for t in 0..n_transitions {
        let expected_from = motif.trace_sequence[t].as_str();
        let expected_to = motif.trace_sequence[t + 1].as_str();
        let matches = motif
            .member_windows
            .iter()
            .filter(|w| {
                w.elements.len() > t + 1
                    && w.elements[t].trace_kind.as_str() == expected_from
                    && w.elements[t + 1].trace_kind.as_str() == expected_to
            })
            .count();
        weights[t] = matches as f64 / n_members.max(1.0);
    }
    weights
}

fn extract_role_affinity(_motif: &DetectedMotif) -> HashMap<Role, f64> {
    // We only have agent_id, not Role, in TrajectoryElements.
    // Return empty for now — the engine can enrich this with agent->role mapping.
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_motif(
        sequence: Vec<TraceKind>,
        obs: u64,
        avg_outcome: f64,
        variance: f64,
    ) -> DetectedMotif {
        DetectedMotif {
            trace_sequence: sequence,
            observation_count: obs,
            avg_outcome,
            outcome_variance: variance,
            member_windows: vec![],
            centroid: vec![0.5; 12],
        }
    }

    #[test]
    fn test_crystallize_new_pattern() {
        let config = DreamConfig::default();
        let mut patterns = Vec::new();
        let motifs = vec![make_motif(
            vec![TraceKind::PromiseMade, TraceKind::PromiseResolved],
            5,
            0.7,
            0.05,
        )];

        let (new, reinforced) = crystallize(&config, &mut patterns, &motifs, 1);
        assert_eq!(new.len(), 1);
        assert_eq!(reinforced, 0);
        assert_eq!(patterns.len(), 1);
        assert!(patterns[0].narrative.contains("beneficial"));
    }

    #[test]
    fn test_crystallize_reinforces_existing() {
        let config = DreamConfig::default();
        let mut patterns = vec![CrystallizedPattern {
            id: "existing_0".into(),
            narrative: "test".into(),
            embedding: vec![0.5; 12],
            motif: TemporalMotif {
                trace_sequence: vec![TraceKind::PromiseMade, TraceKind::PromiseResolved],
                typical_duration_ticks: 5,
                associated_task_keys: vec![],
                transition_weights: vec![0.9],
                min_match_length: 2,
            },
            valence: 0.5,
            confidence: 0.6,
            observation_count: 10,
            role_affinity: HashMap::new(),
            origin_generation: 0,
            last_reinforced_generation: 0,
            temporal_reach: 50,
            persistence_score: 0.8,
            created_at: 100,
            last_reinforced_at: 100,
        }];

        // Same sequence — should reinforce
        let motifs = vec![make_motif(
            vec![TraceKind::PromiseMade, TraceKind::PromiseResolved],
            3,
            0.8,
            0.02,
        )];

        let (new, reinforced) = crystallize(&config, &mut patterns, &motifs, 2);
        assert_eq!(new.len(), 0);
        assert_eq!(reinforced, 1);
        assert_eq!(patterns[0].observation_count, 13); // 10 + 3
        assert!(patterns[0].persistence_score > 0.8); // boosted
    }

    #[test]
    fn test_generate_proto_skills_filters() {
        let config = DreamConfig {
            proto_skill_confidence_threshold: 0.7,
            proto_skill_min_observations: 5,
            ..DreamConfig::default()
        };

        let patterns = vec![
            // Should qualify
            CrystallizedPattern {
                id: "good".into(),
                narrative: "Good pattern".into(),
                embedding: vec![],
                motif: TemporalMotif {
                    trace_sequence: vec![],
                    typical_duration_ticks: 0,
                    associated_task_keys: vec!["task_a".into()],
                    transition_weights: vec![],
                    min_match_length: 2,
                },
                valence: 0.5,       // positive
                confidence: 0.8,    // above threshold
                observation_count: 10, // above threshold
                role_affinity: HashMap::new(),
                origin_generation: 1,
                last_reinforced_generation: 3,
                temporal_reach: 50,
                persistence_score: 0.5, // above 0.3
                created_at: 0,
                last_reinforced_at: 0,
            },
            // Should NOT qualify (low confidence)
            CrystallizedPattern {
                id: "weak".into(),
                narrative: "Weak pattern".into(),
                embedding: vec![],
                motif: TemporalMotif {
                    trace_sequence: vec![],
                    typical_duration_ticks: 0,
                    associated_task_keys: vec![],
                    transition_weights: vec![],
                    min_match_length: 2,
                },
                valence: 0.5,
                confidence: 0.3, // below threshold
                observation_count: 10,
                role_affinity: HashMap::new(),
                origin_generation: 1,
                last_reinforced_generation: 1,
                temporal_reach: 50,
                persistence_score: 0.5,
                created_at: 0,
                last_reinforced_at: 0,
            },
        ];

        let skills = generate_proto_skills(&config, &patterns);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source_pattern_id, "good");
        assert!((skills[0].initial_confidence - 0.64).abs() < 1e-9); // 0.8 * 0.8
    }

    #[test]
    fn test_sequence_overlap() {
        let a = vec![
            TraceKind::PromiseMade,
            TraceKind::QueryPlanned,
            TraceKind::PromiseResolved,
        ];
        let b = vec![
            TraceKind::PromiseMade,
            TraceKind::QueryPlanned,
            TraceKind::PromiseResolved,
        ];
        assert!((sequence_overlap(&a, &b) - 1.0).abs() < 1e-9);

        let c = vec![
            TraceKind::PromiseMade,
            TraceKind::DeliveryRecorded,
            TraceKind::PromiseResolved,
        ];
        assert!((sequence_overlap(&a, &c) - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_bayesian_update() {
        let updated = bayesian_update(0.5, 0.8);
        // (0.7 * 0.5 + 0.3 * 0.8) = 0.35 + 0.24 = 0.59
        assert!((updated - 0.59).abs() < 1e-9);
    }

    #[test]
    fn test_motif_raw_confidence() {
        let motif = make_motif(vec![], 10, 0.5, 0.0);
        let conf = motif_raw_confidence(&motif);
        // count_factor: 1 - 1/11 ≈ 0.909
        // consistency_factor: 1/(1+0) = 1.0
        // total: 0.909 * 0.6 + 1.0 * 0.4 = 0.5454 + 0.4 = 0.9454
        assert!(conf > 0.9);
    }

    #[test]
    fn test_narrative_valence_words() {
        let positive = generate_narrative(&make_motif(
            vec![TraceKind::PromiseMade],
            5,
            0.5,
            0.1,
        ));
        assert!(positive.contains("beneficial"));

        let negative = generate_narrative(&make_motif(
            vec![TraceKind::PromiseMade],
            5,
            -0.5,
            0.1,
        ));
        assert!(negative.contains("harmful"));

        let neutral = generate_narrative(&make_motif(
            vec![TraceKind::PromiseMade],
            5,
            0.0,
            0.1,
        ));
        assert!(neutral.contains("neutral"));
    }
}
