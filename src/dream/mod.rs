//! Stigmergic Dream Consolidation — offline experience replay that
//! crystallizes temporal patterns into stigmergic field structures.
//!
//! Architecture:
//!   1. Trajectory Assembly: sequence traces chronologically with outcome tags
//!   2. Temporal Motif Detection: sliding window + clustering in feature space
//!   3. Crystallization: compress motif clusters into transferable patterns
//!   4. Stigmergic Deposition: write DreamTrail hyperedges, boost/decay traces
//!   5. DKS Survival Pressure: only persistent patterns survive
//!   6. Proto-Skill Generation: promote high-confidence patterns to CASS

pub mod crystallize;
pub mod deposit;
pub mod engine;
pub mod motif;
pub mod survival;
pub mod trajectory;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agent::{AgentId, Role};
use crate::stigmergic_policy::TraceKind;

// Re-exports
pub use engine::StigmergicDreamEngine;

// ── Configuration ──────────────────────────────────────────────────────

/// Configuration for the dream consolidation engine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DreamConfig {
    /// How often to dream (in ticks). Default: 50.
    pub dream_interval: u64,
    /// How far back to replay (in ticks). Default: 200.
    pub replay_horizon: u64,
    /// Sliding window size for motif detection. Default: 8.
    pub motif_window_size: usize,
    /// Minimum cosine similarity for trace subsequences to cluster. Default: 0.7.
    pub cluster_threshold: f64,
    /// Minimum observations before crystallizing a motif. Default: 3.
    pub min_observations: u64,
    /// Eligibility trace decay factor (lambda). Default: 0.9.
    pub eligibility_lambda: f64,
    /// Maximum crystallized patterns to maintain. Default: 256.
    pub max_patterns: usize,
    /// DKS-style decay rate for patterns (survival pressure). Default: 0.02.
    pub pattern_decay_rate: f64,
    /// Boost factor applied to traces participating in positive motifs. Default: 0.15.
    pub positive_trace_boost: f64,
    /// Decay factor applied to traces participating in negative motifs. Default: 0.10.
    pub negative_trace_decay: f64,
    /// Minimum confidence to deposit a DreamTrail hyperedge. Default: 0.5.
    pub deposition_confidence_threshold: f64,
    /// Minimum confidence to promote to proto-skill. Default: 0.7.
    pub proto_skill_confidence_threshold: f64,
    /// Minimum observations to promote to proto-skill. Default: 5.
    pub proto_skill_min_observations: u64,
}

impl Default for DreamConfig {
    fn default() -> Self {
        Self {
            dream_interval: 50,
            replay_horizon: 200,
            motif_window_size: 8,
            cluster_threshold: 0.7,
            min_observations: 3,
            eligibility_lambda: 0.9,
            max_patterns: 256,
            pattern_decay_rate: 0.02,
            positive_trace_boost: 0.15,
            negative_trace_decay: 0.10,
            deposition_confidence_threshold: 0.5,
            proto_skill_confidence_threshold: 0.7,
            proto_skill_min_observations: 5,
        }
    }
}

// ── Crystallized Pattern ────────────────────────────────────────────────

/// A crystallized pattern extracted from dream consolidation.
///
/// This is the primary output of a dream cycle — a compressed,
/// transferable representation of a temporal motif that was
/// observed across multiple traces/experiences.
///
/// Patterns are themselves subject to DKS survival pressure:
/// those that are not reinforced by new observations decay
/// and are eventually pruned.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrystallizedPattern {
    pub id: String,
    /// Human-readable narrative of what this pattern represents
    pub narrative: String,
    /// Compressed embedding in the same vector space as skill/trace embeddings
    pub embedding: Vec<f32>,
    /// The trace sequence template that defines this motif
    pub motif: TemporalMotif,
    /// Outcome valence: positive = beneficial pattern, negative = harmful
    pub valence: f64,
    /// Confidence based on observation count and outcome consistency
    pub confidence: f64,
    /// How many distinct trace subsequences matched this motif
    pub observation_count: u64,
    /// Which agent roles participated most in this pattern
    pub role_affinity: HashMap<Role, f64>,
    /// Dream cycle generation that first crystallized this pattern
    pub origin_generation: u64,
    /// Most recent dream generation that reinforced this pattern
    pub last_reinforced_generation: u64,
    /// Eligibility reach: how far back in ticks this pattern's
    /// temporal credit should extend
    pub temporal_reach: u64,
    /// DKS persistence score — patterns survive when this stays positive
    pub persistence_score: f64,
    /// Timestamps
    pub created_at: u64,
    pub last_reinforced_at: u64,
}

// ── Temporal Motif ──────────────────────────────────────────────────────

/// A temporal motif: an ordered sequence of trace-kind transitions
/// that recurs across the experience trajectory.
///
/// This is the "fingerprint" of a behavioral pattern in time.
/// Unlike a Skill (which is a static principle), a TemporalMotif
/// encodes the *sequence* and *timing* of actions that produce
/// an outcome.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemporalMotif {
    /// Ordered sequence of trace kinds that define the motif
    pub trace_sequence: Vec<TraceKind>,
    /// Typical time span (in ticks) from first to last element
    pub typical_duration_ticks: u64,
    /// Task keys associated with this motif
    pub associated_task_keys: Vec<String>,
    /// Transition weights: probability of each transition in the sequence
    pub transition_weights: Vec<f64>,
    /// Minimum subsequence match length for activation
    pub min_match_length: usize,
}

// ── Dream Cycle Result ──────────────────────────────────────────────────

/// Diagnostic result of a single dream cycle.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DreamCycleResult {
    pub dream_generation: u64,
    pub traces_replayed: usize,
    pub motifs_detected: usize,
    pub patterns_crystallized: usize,
    pub patterns_reinforced: usize,
    pub patterns_decayed: usize,
    pub traces_boosted: usize,
    pub traces_weakened: usize,
    pub dream_trails_deposited: usize,
    pub proto_skills_generated: usize,
    pub dream_duration_ms: u64,
    pub coherence_at_dream: f64,
    pub total_patterns_alive: usize,
}

// ── Proto-Skill ──────────────────────────────────────────────────────────

/// A skill candidate generated from dream consolidation, ready for
/// injection into CASS or SkillBank.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtoSkill {
    pub name: String,
    pub description: String,
    pub source_pattern_id: String,
    pub source_dream_generation: u64,
    pub initial_confidence: f64,
    pub associated_task_keys: Vec<String>,
    pub embedding: Option<Vec<f32>>,
}

// ── Internal types used across submodules ─────────────────────────────

/// A single element in the replay trajectory.
#[derive(Clone, Debug)]
pub struct TrajectoryElement {
    pub tick: u64,
    pub trace_kind: TraceKind,
    pub agent_id: AgentId,
    pub task_key: Option<String>,
    pub outcome_score: Option<f64>,
    pub embedding: Option<Vec<f32>>,
    /// Eligibility weight: decays exponentially from present backward
    pub eligibility: f64,
}

/// A sliding window over the trajectory.
#[derive(Clone, Debug)]
pub struct TraceWindow {
    pub start_idx: usize,
    pub elements: Vec<TrajectoryElement>,
    /// Composite outcome for this window
    pub outcome: f64,
    /// Fixed-size feature vector encoding the window
    pub features: Vec<f64>,
}

/// A cluster of similar trace windows.
#[derive(Clone, Debug)]
pub struct MotifCluster {
    pub members: Vec<TraceWindow>,
    pub centroid: Vec<f64>,
}

/// A detected motif before crystallization.
#[derive(Clone, Debug)]
pub struct DetectedMotif {
    pub trace_sequence: Vec<TraceKind>,
    pub observation_count: u64,
    pub avg_outcome: f64,
    pub outcome_variance: f64,
    pub member_windows: Vec<TraceWindow>,
    pub centroid: Vec<f64>,
}

// ── Helpers ──────────────────────────────────────────────────────────────

pub fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Map TraceKind to a stable integer index for histogram encoding.
pub fn trace_kind_index(kind: &TraceKind) -> usize {
    match kind {
        TraceKind::PromiseMade => 0,
        TraceKind::PromiseResolved => 1,
        TraceKind::DeliveryRecorded => 2,
        TraceKind::QueryPlanned => 3,
        TraceKind::QueryExecuted => 4,
        TraceKind::MemoryShared => 5,
        TraceKind::CouncilDecision => 6,
        TraceKind::Observation => 7,
    }
}

pub const TRACE_KIND_COUNT: usize = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dream_config_defaults() {
        let cfg = DreamConfig::default();
        assert_eq!(cfg.dream_interval, 50);
        assert_eq!(cfg.replay_horizon, 200);
        assert_eq!(cfg.motif_window_size, 8);
        assert!((cfg.cluster_threshold - 0.7).abs() < 1e-9);
        assert_eq!(cfg.min_observations, 3);
        assert_eq!(cfg.max_patterns, 256);
    }

    #[test]
    fn test_trace_kind_index_exhaustive() {
        assert_eq!(trace_kind_index(&TraceKind::PromiseMade), 0);
        assert_eq!(trace_kind_index(&TraceKind::PromiseResolved), 1);
        assert_eq!(trace_kind_index(&TraceKind::DeliveryRecorded), 2);
        assert_eq!(trace_kind_index(&TraceKind::QueryPlanned), 3);
        assert_eq!(trace_kind_index(&TraceKind::QueryExecuted), 4);
        assert_eq!(trace_kind_index(&TraceKind::MemoryShared), 5);
        assert_eq!(trace_kind_index(&TraceKind::CouncilDecision), 6);
        assert_eq!(trace_kind_index(&TraceKind::Observation), 7);
    }

    #[test]
    fn test_crystallized_pattern_serde() {
        let pat = CrystallizedPattern {
            id: "test_1".into(),
            narrative: "Test pattern".into(),
            embedding: vec![0.1, 0.2, 0.3],
            motif: TemporalMotif {
                trace_sequence: vec![TraceKind::PromiseMade, TraceKind::PromiseResolved],
                typical_duration_ticks: 5,
                associated_task_keys: vec!["task_a".into()],
                transition_weights: vec![0.9],
                min_match_length: 2,
            },
            valence: 0.8,
            confidence: 0.75,
            observation_count: 10,
            role_affinity: HashMap::new(),
            origin_generation: 1,
            last_reinforced_generation: 3,
            temporal_reach: 50,
            persistence_score: 1.0,
            created_at: 100,
            last_reinforced_at: 300,
        };

        let json = serde_json::to_string(&pat).unwrap();
        let restored: CrystallizedPattern = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, "test_1");
        assert_eq!(restored.observation_count, 10);
        assert!((restored.valence - 0.8).abs() < 1e-9);
    }

    #[test]
    fn test_dream_cycle_result_default() {
        let r = DreamCycleResult::default();
        assert_eq!(r.dream_generation, 0);
        assert_eq!(r.traces_replayed, 0);
        assert_eq!(r.total_patterns_alive, 0);
    }

    #[test]
    fn test_current_timestamp_is_reasonable() {
        let ts = current_timestamp();
        // After 2024-01-01
        assert!(ts > 1_704_067_200);
    }
}
