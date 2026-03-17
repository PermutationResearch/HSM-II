//! Phase 4: Stigmergic deposition and temporal credit assignment.
//!
//! This is where dream consolidation writes back into the live system:
//! 1. Deposit DreamTrail hyperedges into the shared graph (discoverable via sense_field)
//! 2. Retroactively boost/decay outcome_scores on existing StigmergicTraces
//!    using TD(λ)-style eligibility traces

use std::collections::HashMap;

use crate::agent::AgentId;
use crate::federation::types::KnowledgeLayer;
use crate::hyper_stigmergy::{HyperEdge, HyperStigmergicMorphogenesis};

use super::{current_timestamp, CrystallizedPattern, DreamConfig};

/// Result of the deposition phase.
#[derive(Clone, Debug, Default)]
pub struct DepositionResult {
    pub dream_trails_deposited: usize,
    pub traces_boosted: usize,
    pub traces_weakened: usize,
}

/// Deposit crystallized patterns into the stigmergic field as hyperedges
/// and retroactively adjust trace outcome scores.
///
/// Takes the whole `world` as a single mutable reference to avoid
/// borrow-splitting issues when the caller also needs to pass
/// `world.stigmergic_memory`.
pub fn deposit(
    config: &DreamConfig,
    patterns: &[CrystallizedPattern],
    world: &mut HyperStigmergicMorphogenesis,
) -> DepositionResult {
    let mut result = DepositionResult::default();

    // Phase 4a: Deposit DreamTrail hyperedges
    for pattern in patterns {
        if pattern.confidence < config.deposition_confidence_threshold {
            continue;
        }

        // Collect unique agent IDs that participated in this pattern's observations
        let participants = collect_participants(pattern, &world.agents);

        let mut tags = HashMap::new();
        tags.insert("dream_pattern_id".to_string(), pattern.id.clone());
        tags.insert("dream_generation".to_string(), pattern.origin_generation.to_string());
        tags.insert("valence".to_string(), format!("{:.3}", pattern.valence));
        tags.insert("confidence".to_string(), format!("{:.3}", pattern.confidence));
        tags.insert("observations".to_string(), pattern.observation_count.to_string());
        tags.insert("type".to_string(), "dream_trail".to_string());
        // Encode the trace sequence for discoverability
        let sequence_str: Vec<&str> = pattern
            .motif
            .trace_sequence
            .iter()
            .map(|k| k.as_str())
            .collect();
        tags.insert("trace_sequence".to_string(), sequence_str.join("->"));

        let edge = HyperEdge {
            participants,
            weight: pattern.valence * pattern.confidence,
            emergent: true, // dream-discovered patterns are emergent
            age: 0,
            tags,
            created_at: current_timestamp(),
            embedding: Some(pattern.embedding.clone()),
            creator: None,
            scope: None,
            provenance: None,
            trust_tags: Some(vec!["dream_consolidation".to_string()]),
            origin_system: None,
            knowledge_layer: Some(KnowledgeLayer::Distilled),
        };

        world.edges.push(edge);
        result.dream_trails_deposited += 1;
    }

    // Phase 4b: Temporal credit assignment via eligibility traces
    // Boost/decay outcome_scores on existing traces based on pattern participation
    let current_tick = world.stigmergic_memory.last_applied_tick;

    for pattern in patterns {
        if pattern.confidence < config.deposition_confidence_threshold {
            continue;
        }

        let is_positive = pattern.valence > 0.0;
        let credit_magnitude = if is_positive {
            config.positive_trace_boost * pattern.confidence
        } else {
            config.negative_trace_decay * pattern.confidence
        };

        // Walk through traces and apply credit to those matching the pattern's motif
        for trace in world.stigmergic_memory.traces.iter_mut() {
            if trace.tick + config.replay_horizon < current_tick {
                continue; // outside replay horizon
            }

            // Check if this trace's kind appears in the pattern's motif
            let matches_motif = pattern
                .motif
                .trace_sequence
                .iter()
                .any(|k| k.as_str() == trace.kind.as_str());

            if !matches_motif {
                continue;
            }

            // Check if the trace's task overlaps with the pattern's associated tasks
            let task_matches = trace.task_key.as_ref().map_or(true, |tk| {
                pattern.motif.associated_task_keys.is_empty()
                    || pattern.motif.associated_task_keys.contains(tk)
            });

            if !task_matches {
                continue;
            }

            // Apply TD(λ)-style eligibility decay based on temporal distance
            let age = current_tick.saturating_sub(trace.tick);
            let eligibility = config.eligibility_lambda.powi(age as i32);
            let credit = credit_magnitude * eligibility;

            let current_score = trace.outcome_score.unwrap_or(0.0);
            if is_positive {
                trace.outcome_score = Some((current_score + credit).min(1.0));
                result.traces_boosted += 1;
            } else {
                trace.outcome_score = Some((current_score - credit).max(-1.0));
                result.traces_weakened += 1;
            }
        }
    }

    result
}

/// Collect unique agent IDs that should participate in this DreamTrail.
/// Uses role_affinity if available, otherwise collects from all active agents.
fn collect_participants(
    pattern: &CrystallizedPattern,
    agents: &[crate::agent::Agent],
) -> Vec<AgentId> {
    if !pattern.role_affinity.is_empty() {
        // Get agents whose roles match the pattern's role affinity
        agents
            .iter()
            .filter(|a| pattern.role_affinity.contains_key(&a.role))
            .map(|a| a.id)
            .collect()
    } else {
        // Default: include all agents (the pattern is system-wide)
        agents.iter().map(|a| a.id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dream::{CrystallizedPattern, TemporalMotif};
    use crate::social_memory::DataSensitivity;
    use crate::stigmergic_policy::{StigmergicTrace, TraceKind};

    fn make_pattern(
        id: &str,
        valence: f64,
        confidence: f64,
        sequence: Vec<TraceKind>,
    ) -> CrystallizedPattern {
        CrystallizedPattern {
            id: id.into(),
            narrative: "test".into(),
            embedding: vec![0.5; 12],
            motif: TemporalMotif {
                trace_sequence: sequence,
                typical_duration_ticks: 5,
                associated_task_keys: vec![],
                transition_weights: vec![],
                min_match_length: 2,
            },
            valence,
            confidence,
            observation_count: 10,
            role_affinity: HashMap::new(),
            origin_generation: 1,
            last_reinforced_generation: 1,
            temporal_reach: 50,
            persistence_score: 1.0,
            created_at: 0,
            last_reinforced_at: 0,
        }
    }

    fn make_trace(
        tick: u64,
        kind: TraceKind,
        score: Option<f64>,
    ) -> StigmergicTrace {
        StigmergicTrace {
            id: format!("t_{}", tick),
            agent_id: 1,
            model_id: "test".into(),
            task_key: None,
            kind,
            summary: "test".into(),
            success: None,
            outcome_score: score,
            sensitivity: DataSensitivity::Internal,
            planned_tool: None,
            recorded_at: tick,
            tick,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_deposit_creates_hyperedges() {
        let config = DreamConfig {
            deposition_confidence_threshold: 0.5,
            ..DreamConfig::default()
        };

        let patterns = vec![
            make_pattern(
                "pat_1",
                0.7,
                0.8,
                vec![TraceKind::PromiseMade, TraceKind::PromiseResolved],
            ),
        ];

        let mut world = HyperStigmergicMorphogenesis::new(0);
        world.stigmergic_memory.last_applied_tick = 100;

        let initial_edges = world.edges.len();
        let result = deposit(&config, &patterns, &mut world);

        assert_eq!(result.dream_trails_deposited, 1);
        assert_eq!(world.edges.len(), initial_edges + 1);
        let edge = world.edges.last().unwrap();
        assert!(edge.emergent);
        assert_eq!(
            edge.tags.get("type").unwrap(),
            "dream_trail"
        );
        assert_eq!(
            edge.knowledge_layer,
            Some(KnowledgeLayer::Distilled)
        );
    }

    #[test]
    fn test_deposit_skips_low_confidence() {
        let config = DreamConfig {
            deposition_confidence_threshold: 0.8,
            ..DreamConfig::default()
        };

        let patterns = vec![
            make_pattern("weak", 0.5, 0.3, vec![TraceKind::PromiseMade]),
        ];

        let mut world = HyperStigmergicMorphogenesis::new(0);

        let result = deposit(&config, &patterns, &mut world);
        assert_eq!(result.dream_trails_deposited, 0);
    }

    #[test]
    fn test_temporal_credit_boosts_matching_traces() {
        let config = DreamConfig {
            deposition_confidence_threshold: 0.5,
            positive_trace_boost: 0.2,
            replay_horizon: 1000,
            eligibility_lambda: 1.0, // no decay for test simplicity
            ..DreamConfig::default()
        };

        let patterns = vec![make_pattern(
            "pos",
            0.8, // positive valence
            0.9,
            vec![TraceKind::PromiseMade],
        )];

        let mut world = HyperStigmergicMorphogenesis::new(0);
        world.stigmergic_memory.last_applied_tick = 100;
        world.stigmergic_memory.traces = vec![
            make_trace(90, TraceKind::PromiseMade, Some(0.5)),     // matches
            make_trace(80, TraceKind::QueryPlanned, Some(0.5)),    // doesn't match kind
        ];

        let result = deposit(&config, &patterns, &mut world);
        assert!(result.traces_boosted > 0);

        // PromiseMade trace should be boosted
        let pm_trace = world.stigmergic_memory.traces.iter().find(|t| t.tick == 90).unwrap();
        assert!(pm_trace.outcome_score.unwrap() > 0.5);

        // QueryPlanned trace should be unchanged
        let qp_trace = world.stigmergic_memory.traces.iter().find(|t| t.tick == 80).unwrap();
        assert!((qp_trace.outcome_score.unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_temporal_credit_weakens_for_negative() {
        let config = DreamConfig {
            deposition_confidence_threshold: 0.5,
            negative_trace_decay: 0.15,
            replay_horizon: 1000,
            eligibility_lambda: 1.0,
            ..DreamConfig::default()
        };

        let patterns = vec![make_pattern(
            "neg",
            -0.6, // negative valence
            0.8,
            vec![TraceKind::QueryPlanned],
        )];

        let mut world = HyperStigmergicMorphogenesis::new(0);
        world.stigmergic_memory.last_applied_tick = 100;
        world.stigmergic_memory.traces = vec![
            make_trace(90, TraceKind::QueryPlanned, Some(0.5)),
        ];

        let result = deposit(&config, &patterns, &mut world);
        assert!(result.traces_weakened > 0);

        let trace = &world.stigmergic_memory.traces[0];
        assert!(trace.outcome_score.unwrap() < 0.5);
    }
}
