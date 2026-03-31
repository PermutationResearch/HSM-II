//! Phase 1: Assemble a chronologically-ordered trajectory from
//! stigmergic traces, experiences, and beliefs within the replay horizon.

use crate::hyper_stigmergy::{Belief, Experience, ExperienceOutcome};
use crate::stigmergic_policy::{StigmergicMemory, TraceKind};

use super::{DreamConfig, TrajectoryElement};

/// Assemble a chronological trajectory from all available data sources.
///
/// The trajectory interleaves three data streams:
/// 1. StigmergicTrace records (agent actions with outcomes)
/// 2. Experience records (system-level outcome observations)
/// 3. Belief updates (epistemic state changes)
///
/// Each element carries an eligibility weight that decays exponentially
/// from the present backward — recent events get more credit.
pub fn assemble_trajectory(
    config: &DreamConfig,
    stigmergic_memory: &StigmergicMemory,
    experiences: &[Experience],
    beliefs: &[Belief],
) -> Vec<TrajectoryElement> {
    let current_tick = stigmergic_memory.last_applied_tick;
    let horizon_start = current_tick.saturating_sub(config.replay_horizon);

    let mut trajectory: Vec<TrajectoryElement> = Vec::new();

    // Stream 1: Stigmergic traces (primary data source)
    for trace in &stigmergic_memory.traces {
        if trace.tick < horizon_start {
            continue;
        }
        let age = current_tick.saturating_sub(trace.tick);
        let eligibility = config.eligibility_lambda.powi(age as i32);

        trajectory.push(TrajectoryElement {
            tick: trace.tick,
            trace_kind: trace.kind.clone(),
            agent_id: trace.agent_id,
            task_key: trace.task_key.clone(),
            outcome_score: trace.outcome_score,
            embedding: None,
            eligibility,
        });
    }

    // Stream 2: Experience outcomes (system-level markers)
    for exp in experiences {
        if exp.tick < horizon_start {
            continue;
        }
        let valence = match &exp.outcome {
            ExperienceOutcome::Positive { coherence_delta } => Some(*coherence_delta),
            ExperienceOutcome::Negative { coherence_delta } => Some(*coherence_delta),
            ExperienceOutcome::Neutral => Some(0.0),
        };
        let age = current_tick.saturating_sub(exp.tick);
        let eligibility = config.eligibility_lambda.powi(age as i32);

        trajectory.push(TrajectoryElement {
            tick: exp.tick,
            trace_kind: TraceKind::Observation,
            agent_id: 0, // system-level event, no specific agent
            task_key: None,
            outcome_score: valence,
            embedding: exp.embedding.clone(),
            eligibility,
        });
    }

    // Stream 3: Belief updates (epistemic events)
    for belief in beliefs {
        if belief.updated_at < horizon_start {
            continue;
        }
        // Belief confidence changes are epistemic events.
        // High-confidence beliefs with no contradictions are positive;
        // contradicted beliefs are negative.
        let valence = if belief.contradicting_evidence.is_empty() {
            belief.confidence * 0.5
        } else {
            -(belief.contradicting_evidence.len() as f64 * 0.1)
        };
        let age = current_tick.saturating_sub(belief.updated_at);
        let eligibility = config.eligibility_lambda.powi(age as i32);

        trajectory.push(TrajectoryElement {
            tick: belief.updated_at,
            trace_kind: TraceKind::Observation,
            agent_id: 0,
            task_key: None,
            outcome_score: Some(valence),
            embedding: None,
            eligibility,
        });
    }

    // Sort chronologically (stable sort preserves insertion order for same tick)
    trajectory.sort_by_key(|e| e.tick);
    trajectory
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hyper_stigmergy::BeliefSource;
    use crate::social_memory::DataSensitivity;
    use crate::stigmergic_policy::StigmergicTrace;
    use std::collections::HashMap;

    fn make_trace(tick: u64, kind: TraceKind, agent_id: u64, score: Option<f64>) -> StigmergicTrace {
        StigmergicTrace {
            id: format!("t_{}", tick),
            agent_id,
            model_id: "test".into(),
            task_key: Some("task_a".into()),
            kind,
            summary: "test trace".into(),
            success: Some(true),
            outcome_score: score,
            sensitivity: DataSensitivity::Internal,
            planned_tool: None,
            recorded_at: tick,
            tick,
            metadata: HashMap::new(),
        }
    }

    fn make_experience(tick: u64, outcome: ExperienceOutcome) -> Experience {
        Experience {
            id: tick as usize,
            description: "test".into(),
            context: "ctx".into(),
            abstract_l0: None,
            overview_l1: None,
            outcome,
            timestamp: tick,
            tick,
            embedding: None,
        }
    }

    fn make_belief(updated_at: u64, confidence: f64, contradictions: usize) -> Belief {
        Belief {
            id: updated_at as usize,
            content: "test belief".into(),
            abstract_l0: None,
            overview_l1: None,
            confidence,
            source: BeliefSource::Observation,
            supporting_evidence: vec![],
            contradicting_evidence: (0..contradictions).map(|i| format!("c{}", i)).collect(),
            created_at: 0,
            updated_at,
            update_count: 1,
            owner_namespace: None,
            supersedes_belief_id: None,
            evidence_belief_ids: vec![],
            human_committed: false,
        }
    }

    #[test]
    fn test_empty_trajectory() {
        let config = DreamConfig::default();
        let mem = StigmergicMemory::default();
        let traj = assemble_trajectory(&config, &mem, &[], &[]);
        assert!(traj.is_empty());
    }

    #[test]
    fn test_trajectory_chronological_order() {
        let config = DreamConfig {
            replay_horizon: 1000,
            ..DreamConfig::default()
        };
        let mut mem = StigmergicMemory::default();
        mem.last_applied_tick = 100;
        mem.traces = vec![
            make_trace(50, TraceKind::QueryPlanned, 1, Some(0.5)),
            make_trace(30, TraceKind::PromiseMade, 2, Some(0.3)),
            make_trace(80, TraceKind::DeliveryRecorded, 1, Some(0.8)),
        ];

        let traj = assemble_trajectory(&config, &mem, &[], &[]);
        assert_eq!(traj.len(), 3);
        assert_eq!(traj[0].tick, 30);
        assert_eq!(traj[1].tick, 50);
        assert_eq!(traj[2].tick, 80);
    }

    #[test]
    fn test_trajectory_horizon_filter() {
        let config = DreamConfig {
            replay_horizon: 50,
            ..DreamConfig::default()
        };
        let mut mem = StigmergicMemory::default();
        mem.last_applied_tick = 100;
        mem.traces = vec![
            make_trace(10, TraceKind::PromiseMade, 1, None), // too old
            make_trace(60, TraceKind::QueryPlanned, 1, None), // within horizon
            make_trace(90, TraceKind::DeliveryRecorded, 1, None), // within horizon
        ];

        let traj = assemble_trajectory(&config, &mem, &[], &[]);
        assert_eq!(traj.len(), 2);
        assert_eq!(traj[0].tick, 60);
    }

    #[test]
    fn test_trajectory_eligibility_decay() {
        let config = DreamConfig {
            replay_horizon: 1000,
            eligibility_lambda: 0.9,
            ..DreamConfig::default()
        };
        let mut mem = StigmergicMemory::default();
        mem.last_applied_tick = 100;
        mem.traces = vec![
            make_trace(100, TraceKind::PromiseMade, 1, None), // age 0 -> elig ~1.0
            make_trace(90, TraceKind::PromiseMade, 1, None),  // age 10 -> elig ~0.35
        ];

        let traj = assemble_trajectory(&config, &mem, &[], &[]);
        assert_eq!(traj.len(), 2);
        // Most recent (tick=100) should have higher eligibility
        let recent = traj.iter().find(|e| e.tick == 100).unwrap();
        let older = traj.iter().find(|e| e.tick == 90).unwrap();
        assert!(recent.eligibility > older.eligibility);
        assert!((recent.eligibility - 1.0).abs() < 1e-9); // 0.9^0 = 1.0
    }

    #[test]
    fn test_trajectory_interleaves_streams() {
        let config = DreamConfig {
            replay_horizon: 1000,
            ..DreamConfig::default()
        };
        let mut mem = StigmergicMemory::default();
        mem.last_applied_tick = 100;
        mem.traces = vec![
            make_trace(50, TraceKind::QueryPlanned, 1, Some(0.5)),
        ];

        let experiences = vec![
            make_experience(60, ExperienceOutcome::Positive { coherence_delta: 0.3 }),
        ];
        let beliefs = vec![
            make_belief(70, 0.8, 0),
        ];

        let traj = assemble_trajectory(&config, &mem, &experiences, &beliefs);
        assert_eq!(traj.len(), 3);
        // Should be chronological: tick 50 (trace), 60 (exp), 70 (belief)
        assert_eq!(traj[0].tick, 50);
        assert_eq!(traj[1].tick, 60);
        assert_eq!(traj[2].tick, 70);
    }

    #[test]
    fn test_experience_valence_mapping() {
        let config = DreamConfig {
            replay_horizon: 1000,
            ..DreamConfig::default()
        };
        let mut mem = StigmergicMemory::default();
        mem.last_applied_tick = 100;

        let experiences = vec![
            make_experience(50, ExperienceOutcome::Positive { coherence_delta: 0.5 }),
            make_experience(60, ExperienceOutcome::Negative { coherence_delta: -0.3 }),
            make_experience(70, ExperienceOutcome::Neutral),
        ];

        let traj = assemble_trajectory(&config, &mem, &experiences, &[]);
        assert_eq!(traj.len(), 3);
        assert!((traj[0].outcome_score.unwrap() - 0.5).abs() < 1e-9);
        assert!((traj[1].outcome_score.unwrap() - (-0.3)).abs() < 1e-9);
        assert!((traj[2].outcome_score.unwrap()).abs() < 1e-9);
    }

    #[test]
    fn test_belief_valence_mapping() {
        let config = DreamConfig {
            replay_horizon: 1000,
            ..DreamConfig::default()
        };
        let mut mem = StigmergicMemory::default();
        mem.last_applied_tick = 100;

        let beliefs = vec![
            make_belief(50, 0.8, 0), // no contradictions -> 0.8 * 0.5 = 0.4
            make_belief(60, 0.8, 3), // 3 contradictions -> -(3 * 0.1) = -0.3
        ];

        let traj = assemble_trajectory(&config, &mem, &[], &beliefs);
        assert_eq!(traj.len(), 2);
        assert!((traj[0].outcome_score.unwrap() - 0.4).abs() < 1e-9);
        assert!((traj[1].outcome_score.unwrap() - (-0.3)).abs() < 1e-9);
    }
}
