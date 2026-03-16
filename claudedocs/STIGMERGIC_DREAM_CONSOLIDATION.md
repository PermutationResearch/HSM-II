# Stigmergic Dream Consolidation: Architectural Design

## The Innovation

**Name:** Stigmergic Dream Consolidation Engine (`StigmergicDreamEngine`)

**One-sentence summary:** An offline experience replay system that detects
recurring temporal patterns across agent traces, compresses them into
crystallized knowledge structures, and deposits them back into the
stigmergic field as discoverable hyperedges -- giving the system the
equivalent of biological sleep consolidation.

---

## 1. Problem Statement: What HSM-II Cannot Yet Do

After examining the full codebase across 25+ source files, the following
learning architecture emerges:

**Existing learning signals and where they act:**

| Signal | Source | Consumer | Timescale |
|--------|--------|----------|-----------|
| Skill credit | `apply_skill_credit()` | SkillBank confidence/EMA | Immediate (per tick) |
| Coherence delta | `global_coherence()` | LivingPrompt.evolve() | Immediate (per tick) |
| Experience outcome | `ExperienceOutcome` | `distill_from_experiences()` | Delayed (every 10 ticks) |
| Consensus verdict | `ConsensusEngine` | Skill promote/suspend | Delayed (every 10 ticks) |
| Trace outcome_score | `StigmergicTrace` | `apply_cycle()` bid_bias | Per-cycle |
| AutoContext generation | `AutoContextLoop` | Playbooks, Hints | Per-generation |
| Counterfactual credit | `batch_runner.rs` | DecisionCredit | Offline batch only |

**The gap:** Every learning signal is consumed at the tick boundary and then
discarded. There is no mechanism that:

1. Re-examines the *temporal sequence* of traces to find recurring
   multi-step patterns
2. Detects which *combinations* of trace-kinds, across which *agents*, over
   what *time windows*, reliably produce good or bad outcomes
3. Compresses those temporal patterns into compact, transferable knowledge
4. Deposits that compressed knowledge back into the stigmergic substrate
   where other agents can discover it through existing field-sensing
5. Applies survival pressure to the compressed knowledge so only genuinely
   persistent patterns survive

In biological terms: the system has waking cognition but no sleep. It has
short-term plasticity but no long-term potentiation through experience
replay. The counterfactual replay system in `batch_runner.rs` (line 1710+)
proves the concept works, but it runs only in offline batch experiments and
never writes back into the live system.

---

## 2. Why This Innovation Is Uniquely Stigmergic

Dream consolidation in HSM-II is not a generic "experience replay buffer"
that any agent framework could bolt on. It is deeply entangled with the
stigmergic paradigm in five ways that make it irreproducible elsewhere:

**1. Stigmergic deposition of compressed knowledge.** The output of a
dream cycle is not a parameter update or a database record. It is a new
hyperedge in the shared graph -- a `DreamTrail` edge with temporal
semantics. This edge is discoverable by any agent whose `sense_field()`
(from `StigmergicEntity`, line 145 of `dks/stigmergic_entity.rs`) sweeps
over the relevant vertices. No explicit communication is needed. The
knowledge transfer is *indirect and emergent*, exactly like ant pheromone
trails.

**2. DKS survival pressure on crystallized patterns.** The dream memory
itself is an ecological population subject to DKS dynamics. Patterns that
are not reinforced by new observations decay. Only the most persistent
patterns survive. This uses the same `SelectionPressure` logic (from
`dks/selection.rs`) that governs entity survival, applied at a higher
level of abstraction.

**3. Temporal credit assignment via eligibility traces.** The dream engine
retroactively adjusts `outcome_score` on existing `StigmergicTrace` records
(from `stigmergic_policy.rs`) based on whether those traces participated
in subsequently-detected positive or negative motifs. This is TD(lambda)-
style temporal credit assignment operating over the stigmergic trace buffer
-- credit flows backward through the pheromone trail.

**4. Kuramoto coherence as the global quality signal.** The Kuramoto order
parameter R (from `kuramoto.rs`) serves as the holistic quality measure
that determines whether a dream cycle's crystallized patterns are
beneficial. This is unique because R measures the *synchronization* of the
entire agent population, not just any single agent's performance.

**5. Multi-system integration that no other framework has.** The dream
engine simultaneously reads from `StigmergicMemory.traces`,
`HyperStigmergicMorphogenesis.experiences`, `beliefs`, and `SkillBank`;
and writes back into `world.edges` (hyperedges), `StigmergicTrace.
outcome_score` (retroactive credit), `LivingPrompt` (consolidated wisdom),
and `CASS` (proto-skills). No other framework has all seven of these
subsystems to read from and write to.

---

## 3. Detailed Architectural Design

### 3.1 Module Structure

```
src/dream/
    mod.rs             -- Core types, DreamConfig, public API
    engine.rs          -- StigmergicDreamEngine, dream() orchestration
    trajectory.rs      -- Trajectory assembly from multiple data sources
    motif.rs           -- Temporal motif detection via sliding window clustering
    crystallize.rs     -- Pattern crystallization and narrative compression
    deposit.rs         -- Stigmergic deposition and temporal credit assignment
    survival.rs        -- DKS-style survival pressure on pattern population
```

### 3.2 Core Types (`src/dream/mod.rs`)

```rust
//! Stigmergic Dream Consolidation -- offline experience replay that
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
    /// Minimum confidence + observation count to promote to proto-skill. Default: 0.7.
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
/// This is the primary output of a dream cycle -- a compressed,
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
    /// DKS persistence score -- patterns survive when this stays positive
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
```

### 3.3 Trajectory Assembly (`src/dream/trajectory.rs`)

```rust
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
/// from the present backward -- recent events get more credit.
pub fn assemble_trajectory(
    config: &DreamConfig,
    stigmergic_memory: &StigmergicMemory,
    experiences: &[Experience],
    beliefs: &[Belief],
) -> Vec<TrajectoryElement> {
    let current_tick = stigmergic_memory.last_applied_tick;
    let horizon_start = current_tick.saturating_sub(config.replay_horizon);

    let mut trajectory: Vec<TrajectoryElement> = Vec::new();

    // Stream 1: Stigmergic traces (primary)
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
            agent_id: 0,
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
        // High-confidence beliefs with supporting evidence are positive;
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
```

### 3.4 Temporal Motif Detection (`src/dream/motif.rs`)

```rust
//! Phase 2: Detect recurring temporal motifs via sliding window
//! extraction and agglomerative clustering in feature space.
//!
//! The key insight is that *sequences* of traces, not individual traces,
//! are the unit of learning. A "PromiseMade -> QueryPlanned -> QueryExecuted
//! -> DeliveryRecorded -> PromiseResolved" sequence with positive outcomes
//! is a fundamentally different signal than any single trace in isolation.

use std::collections::HashMap;

use crate::agent::AgentId;
use crate::stigmergic_policy::TraceKind;

use super::{
    trace_kind_index, DreamConfig, DetectedMotif, MotifCluster,
    TraceWindow, TrajectoryElement, TRACE_KIND_COUNT,
};

/// Extract sliding windows from trajectory and cluster them
/// to find recurring temporal motifs.
pub fn detect_motifs(
    config: &DreamConfig,
    trajectory: &[TrajectoryElement],
) -> Vec<DetectedMotif> {
    if trajectory.len() < config.motif_window_size {
        return Vec::new();
    }

    // Step 1: Extract sliding windows with feature encoding
    let windows = extract_windows(config, trajectory);
    if windows.is_empty() {
        return Vec::new();
    }

    // Step 2: Agglomerative clustering by cosine similarity
    let clusters = cluster_windows(&windows, config.cluster_threshold);

    // Step 3: Convert qualifying clusters to detected motifs
    clusters
        .into_iter()
        .filter(|c| c.members.len() >= config.min_observations as usize)
        .map(|cluster| {
            let n = cluster.members.len() as f64;
            let avg_outcome = cluster.members.iter()
                .map(|w| w.outcome)
                .sum::<f64>() / n;
            let outcome_variance = cluster.members.iter()
                .map(|w| (w.outcome - avg_outcome).powi(2))
                .sum::<f64>() / n;
            let canonical = extract_canonical_sequence(&cluster);

            DetectedMotif {
                trace_sequence: canonical,
                observation_count: cluster.members.len() as u64,
                avg_outcome,
                outcome_variance,
                member_windows: cluster.members,
                centroid: cluster.centroid,
            }
        })
        .collect()
}

/// Encode each sliding window as a fixed-size feature vector.
///
/// The feature vector has dimensionality:
///   TRACE_KIND_COUNT (8)  -- trace kind histogram
///   + 1                   -- agent diversity (Shannon entropy)
///   + 1                   -- normalized temporal span
///   + 1                   -- outcome trajectory slope
///   + 1                   -- eligibility-weighted outcome
///   = 12 features
///
/// This dimensionality is deliberately small. The purpose is not
/// high-dimensional embedding but rather capturing the *shape* of
/// a trace subsequence for clustering.
fn extract_windows(
    config: &DreamConfig,
    trajectory: &[TrajectoryElement],
) -> Vec<TraceWindow> {
    let w = config.motif_window_size;
    let mut windows = Vec::with_capacity(trajectory.len().saturating_sub(w));

    for i in 0..=trajectory.len().saturating_sub(w) {
        let window = &trajectory[i..i + w];
        let outcome = window_outcome(window);
        let features = encode_window(config, window);

        windows.push(TraceWindow {
            start_idx: i,
            elements: window.to_vec(),
            outcome,
            features,
        });
    }

    windows
}

/// Compute a composite outcome for a window.
/// Later elements in the window get more weight (they are closer to
/// the outcome we are attributing). This implements a simple causal
/// assumption: traces just before an outcome are more responsible for it.
fn window_outcome(window: &[TrajectoryElement]) -> f64 {
    let mut weighted_sum = 0.0;
    let mut weight_total = 0.0;

    for (i, elem) in window.iter().enumerate() {
        let position_weight = (i + 1) as f64; // later = more weight
        let score = elem.outcome_score.unwrap_or(0.0);
        weighted_sum += score * position_weight * elem.eligibility;
        weight_total += position_weight * elem.eligibility;
    }

    if weight_total > 1e-9 {
        weighted_sum / weight_total
    } else {
        0.0
    }
}

fn encode_window(config: &DreamConfig, window: &[TrajectoryElement]) -> Vec<f64> {
    let mut features = Vec::with_capacity(TRACE_KIND_COUNT + 4);

    // Feature 1: Trace kind histogram (8 bins)
    let mut kind_hist = [0.0f64; TRACE_KIND_COUNT];
    for elem in window {
        kind_hist[trace_kind_index(&elem.trace_kind)] += 1.0;
    }
    let total = kind_hist.iter().sum::<f64>().max(1.0);
    for val in &kind_hist {
        features.push(val / total);
    }

    // Feature 2: Agent diversity (Shannon entropy)
    let mut agent_counts: HashMap<AgentId, usize> = HashMap::new();
    for elem in window {
        *agent_counts.entry(elem.agent_id).or_default() += 1;
    }
    let n = window.len() as f64;
    let entropy: f64 = agent_counts
        .values()
        .map(|&c| {
            let p = c as f64 / n;
            if p > 0.0 { -p * p.ln() } else { 0.0 }
        })
        .sum();
    features.push(entropy);

    // Feature 3: Normalized temporal span
    let span = if window.len() > 1 {
        (window.last().unwrap().tick.saturating_sub(window[0].tick)) as f64
    } else {
        0.0
    };
    features.push(span / config.replay_horizon.max(1) as f64);

    // Feature 4: Outcome trajectory slope (linear regression)
    let outcomes: Vec<f64> = window
        .iter()
        .filter_map(|e| e.outcome_score)
        .collect();
    let slope = if outcomes.len() > 1 {
        linear_regression_slope(&outcomes)
    } else {
        0.0
    };
    features.push(slope);

    // Feature 5: Eligibility-weighted outcome
    let elig_sum: f64 = window.iter().map(|e| e.eligibility).sum();
    let elig_outcome: f64 = window
        .iter()
        .map(|e| e.eligibility * e.outcome_score.unwrap_or(0.0))
        .sum::<f64>()
        / elig_sum.max(1e-9);
    features.push(elig_outcome);

    features
}

fn linear_regression_slope(values: &[f64]) -> f64 {
    let n = values.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let x_mean = (n - 1.0) / 2.0;
    let y_mean = values.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (i, &y) in values.iter().enumerate() {
        let x = i as f64;
        num += (x - x_mean) * (y - y_mean);
        den += (x - x_mean) * (x - x_mean);
    }
    if den.abs() < 1e-12 { 0.0 } else { num / den }
}

/// Cosine similarity between two feature vectors.
fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a < 1e-12 || norm_b < 1e-12 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Simple agglomerative clustering: merge closest pair until
/// all inter-cluster similarities fall below threshold.
fn cluster_windows(windows: &[TraceWindow], threshold: f64) -> Vec<MotifCluster> {
    let mut clusters: Vec<MotifCluster> = windows
        .iter()
        .map(|w| MotifCluster {
            members: vec![w.clone()],
            centroid: w.features.clone(),
        })
        .collect();

    loop {
        if clusters.len() < 2 {
            break;
        }

        // Find most similar pair
        let mut best_sim = f64::NEG_INFINITY;
        let mut best_i = 0;
        let mut best_j = 1;

        for i in 0..clusters.len() {
            for j in (i + 1)..clusters.len() {
                let sim = cosine_similarity(&clusters[i].centroid, &clusters[j].centroid);
                if sim > best_sim {
                    best_sim = sim;
                    best_i = i;
                    best_j = j;
                }
            }
        }

        if best_sim < threshold {
            break;
        }

        // Merge best_j into best_i
        let merged_members = {
            let mut m = clusters[best_i].members.clone();
            m.extend(clusters[best_j].members.clone());
            m
        };
        let merged_centroid = compute_centroid(&merged_members);
        clusters[best_i] = MotifCluster {
            members: merged_members,
            centroid: merged_centroid,
        };
        clusters.remove(best_j);
    }

    clusters
}

fn compute_centroid(members: &[TraceWindow]) -> Vec<f64> {
    if members.is_empty() {
        return Vec::new();
    }
    let dim = members[0].features.len();
    let n = members.len() as f64;
    let mut centroid = vec![0.0; dim];
    for m in members {
        for (i, &v) in m.features.iter().enumerate() {
            centroid[i] += v;
        }
    }
    for v in &mut centroid {
        *v /= n;
    }
    centroid
}

/// Extract the canonical trace sequence from a cluster's centroid.
/// Uses the most common trace kind at each position across members.
fn extract_canonical_sequence(cluster: &MotifCluster) -> Vec<TraceKind> {
    if cluster.members.is_empty() {
        return Vec::new();
    }
    let window_size = cluster.members[0].elements.len();
    let mut canonical = Vec::with_capacity(window_size);

    for pos in 0..window_size {
        let mut kind_counts: HashMap<String, usize> = HashMap::new();
        for member in &cluster.members {
            if pos < member.elements.len() {
                let key = format!("{:?}", member.elements[pos].trace_kind);
                *kind_counts.entry(key).or_default() += 1;
            }
        }
        // Pick the most common kind at this position
        let most_common = kind_counts
            .into_iter()
            .max_by_key(|&(_, count)| count)
            .map(|(kind_str, _)| TraceKind::from_str(&kind_str))
            .unwrap_or(TraceKind::Observation);
        canonical.push(most_common);
    }

    canonical
}
```

### 3.5 Pattern Crystallization (`src/dream/crystallize.rs`)

```rust
//! Phase 3: Crystallize detected motifs into CrystallizedPattern structs.
//! Matches against existing patterns to reinforce, or creates new ones.

use std::collections::HashMap;

use crate::agent::Role;
use crate::skill::SkillBank;

use super::{
    current_timestamp, CrystallizedPattern, DreamConfig, DetectedMotif,
    ProtoSkill, TemporalMotif,
};

/// Crystallize detected motifs into patterns.
/// Returns (new_patterns, count_reinforced).
pub fn crystallize(
    config: &DreamConfig,
    existing_patterns: &mut Vec<CrystallizedPattern>,
    motifs: &[DetectedMotif],
    dream_generation: u64,
    _skill_bank: &SkillBank,
) -> (Vec<CrystallizedPattern>, usize) {
    let mut new_patterns = Vec::new();
    let mut reinforced = 0;

    for motif in motifs {
        // Try to match against existing patterns
        if let Some(existing) = find_matching_pattern(existing_patterns, motif) {
            // Reinforce: increase observation count, boost confidence
            existing.observation_count += motif.observation_count;
            existing.confidence = bayesian_update(
                existing.confidence,
                motif_raw_confidence(motif),
            );
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
                temporal_reach: 0, // Set by caller from config
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

fn sequence_overlap(a: &[crate::stigmergic_policy::TraceKind], b: &[crate::stigmergic_policy::TraceKind]) -> f64 {
    let min_len = a.len().min(b.len());
    if min_len == 0 {
        return 0.0;
    }
    let matches = a.iter().zip(b.iter()).filter(|(x, y)| x == y).count();
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
    let sequence_desc: Vec<&str> = motif
        .trace_sequence
        .iter()
        .map(|k| k.as_str())
        .collect();

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
    // In production, this would use the same embedding engine as CASS.
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

    for (t, _) in motif.trace_sequence.windows(2).enumerate() {
        let expected = &motif.trace_sequence[t..t + 2];
        let matches = motif.member_windows.iter().filter(|w| {
            w.elements.len() > t + 1
                && format!("{:?}", w.elements[t].trace_kind) == format!("{:?}", expected[0])
                && format!("{:?}", w.elements[t + 1].trace_kind) == format!("{:?}", expected[1])
        }).count();
        weights[t] = matches as f64 / n_members.max(1.0);
    }
    weights
}

fn extract_role_affinity(motif: &DetectedMotif) -> HashMap<Role, f64> {
    // In the full implementation, this would look up agent roles.
    // For now, aggregate agent participation by ID as a proxy.
    let mut affinity = HashMap::new();
    let total_elig: f64 = motif
        .member_windows
        .iter()
        .flat_map(|w| w.elements.iter())
        .map(|e| e.eligibility)
        .sum();
    if total_elig > 0.0 {
        // Default: assign to Architect as a placeholder
        affinity.insert(Role::Architect, total_elig);
    }
    affinity
}
```

### 3.6 Stigmergic Deposition (`src/dream/deposit.rs`)

```rust
//! Phase 4: Deposit crystallized patterns back into the stigmergic substrate.
//!
//! This is the core stigmergic innovation. Two operations:
//!
//! 1. Write DreamTrail hyperedges into the world graph. These are
//!    discoverable by agents through their existing sense_field() method
//!    in StigmergicEntity. No new sensing code is needed -- agents
//!    already scan all hyperedges.
//!
//! 2. Apply retroactive temporal credit to existing StigmergicTrace
//!    records. Traces that participated in positive motifs have their
//!    outcome_score boosted; traces in negative motifs are weakened.
//!    This is the stigmergic equivalent of synaptic potentiation
//!    during sleep.

use std::collections::HashMap;

use crate::hyper_stigmergy::HyperEdge;
use crate::stigmergic_policy::{StigmergicMemory, StigmergicTrace};

use super::{current_timestamp, CrystallizedPattern, DreamConfig};

/// Deposit patterns into the stigmergic substrate.
/// Returns (traces_boosted, traces_weakened, dream_trails_deposited).
pub fn deposit_stigmergic(
    config: &DreamConfig,
    patterns: &[CrystallizedPattern],
    stigmergic_memory: &mut StigmergicMemory,
    world_edges: &mut Vec<HyperEdge>,
) -> (usize, usize, usize) {
    let mut boosted = 0;
    let mut weakened = 0;
    let mut deposited = 0;

    for pattern in patterns {
        // Operation 1: Create DreamTrail hyperedge for qualifying patterns
        if pattern.confidence >= config.deposition_confidence_threshold
            && pattern.observation_count >= config.min_observations
        {
            let mut tags = HashMap::new();
            tags.insert("type".to_string(), "DreamTrail".to_string());
            tags.insert(
                "dream_gen".to_string(),
                pattern.origin_generation.to_string(),
            );
            tags.insert("valence".to_string(), format!("{:.3}", pattern.valence));
            tags.insert("confidence".to_string(), format!("{:.3}", pattern.confidence));
            tags.insert("narrative".to_string(), pattern.narrative.clone());
            tags.insert(
                "observations".to_string(),
                pattern.observation_count.to_string(),
            );
            tags.insert(
                "motif".to_string(),
                pattern
                    .motif
                    .trace_sequence
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
            );

            // Participants: agents who appeared in the motif's role affinity.
            // This makes the DreamTrail discoverable by those agents' field sensors.
            let participants: Vec<u64> = pattern
                .role_affinity
                .keys()
                .enumerate()
                .map(|(i, _)| (i + 1) as u64)
                .collect();

            // If we have no participants, use a sentinel so the edge is still stored
            let participants = if participants.is_empty() {
                vec![0]
            } else {
                participants
            };

            world_edges.push(HyperEdge {
                participants,
                weight: pattern.valence.abs() * pattern.confidence,
                emergent: true,
                age: 0,
                tags,
                created_at: current_timestamp(),
                embedding: Some(pattern.embedding.clone()),
                scope: None,
                provenance: None,
                trust_tags: Some(vec!["dream_consolidation".to_string()]),
                origin_system: None,
                knowledge_layer: None,
            });
            deposited += 1;
        }

        // Operation 2: Retroactive temporal credit assignment
        let current_tick = stigmergic_memory.last_applied_tick;
        for trace in stigmergic_memory.traces.iter_mut() {
            if let Some(match_strength) = trace_matches_pattern(trace, pattern) {
                let age = current_tick.saturating_sub(trace.tick);
                let eligibility = config.eligibility_lambda.powi(age as i32);

                let credit = pattern.valence
                    * pattern.confidence
                    * match_strength
                    * eligibility;

                if let Some(ref mut score) = trace.outcome_score {
                    let old = *score;
                    if credit > 0.0 {
                        *score = (*score + credit * config.positive_trace_boost)
                            .clamp(-1.0, 1.0);
                        if *score > old {
                            boosted += 1;
                        }
                    } else if credit < 0.0 {
                        *score = (*score + credit * config.negative_trace_decay)
                            .clamp(-1.0, 1.0);
                        if *score < old {
                            weakened += 1;
                        }
                    }
                }
            }
        }
    }

    (boosted, weakened, deposited)
}

/// Check if a trace matches a pattern's motif and return match strength [0, 1].
fn trace_matches_pattern(
    trace: &StigmergicTrace,
    pattern: &CrystallizedPattern,
) -> Option<f64> {
    // Check if trace kind appears in the motif sequence
    let kind_present = pattern
        .motif
        .trace_sequence
        .contains(&trace.kind);

    if !kind_present {
        return None;
    }

    // Base match: 1/sequence_length (being part of the sequence)
    let base_match = 1.0 / pattern.motif.trace_sequence.len().max(1) as f64;

    // Task key bonus: stronger match if task keys overlap
    let task_bonus = trace
        .task_key
        .as_ref()
        .map(|tk| {
            if pattern.motif.associated_task_keys.iter().any(|at| at == tk) {
                0.5
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);

    Some((base_match + task_bonus).min(1.0))
}
```

### 3.7 DKS Survival Pressure (`src/dream/survival.rs`)

```rust
//! Phase 5: Apply DKS-style survival pressure to the pattern population.
//!
//! Crystallized patterns are themselves entities in an ecological system.
//! They compete for survival. Patterns not reinforced by new observations
//! decay. Only the most persistent patterns survive.
//!
//! This mirrors biological sleep consolidation: not all memories
//! experienced during the day survive the night. Only those that
//! are replayed (reinforced) persist.

use super::{CrystallizedPattern, DreamConfig, current_timestamp};

/// Apply survival pressure to patterns. Returns count of decayed patterns.
pub fn apply_survival_pressure(
    config: &DreamConfig,
    patterns: &mut Vec<CrystallizedPattern>,
    current_generation: u64,
) -> usize {
    // Step 1: Apply base decay to all patterns
    for pattern in patterns.iter_mut() {
        pattern.persistence_score -= config.pattern_decay_rate;

        // Recently reinforced patterns resist decay
        let generations_since_reinforcement =
            current_generation.saturating_sub(pattern.last_reinforced_generation);
        if generations_since_reinforcement <= 2 {
            // Reinforced within last 2 dream cycles: partial decay resistance
            pattern.persistence_score += config.pattern_decay_rate * 0.5;
        }

        // High-observation patterns are harder to kill
        let observation_bonus = (pattern.observation_count as f64).ln().max(0.0) * 0.005;
        pattern.persistence_score += observation_bonus;
    }

    // Step 2: Remove patterns below survival threshold
    let before = patterns.len();
    patterns.retain(|p| p.persistence_score > 0.0);
    let removed_by_decay = before - patterns.len();

    // Step 3: Enforce maximum population (keep highest persistence)
    let removed_by_cap = if patterns.len() > config.max_patterns {
        patterns.sort_by(|a, b| {
            b.persistence_score
                .partial_cmp(&a.persistence_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let excess = patterns.len() - config.max_patterns;
        patterns.truncate(config.max_patterns);
        excess
    } else {
        0
    };

    removed_by_decay + removed_by_cap
}
```

### 3.8 The Dream Engine (`src/dream/engine.rs`)

```rust
//! StigmergicDreamEngine: the top-level orchestrator that runs one
//! complete dream cycle by composing all phases.

use std::time::Instant;

use crate::hyper_stigmergy::{Belief, Experience, HyperEdge};
use crate::skill::SkillBank;
use crate::stigmergic_policy::StigmergicMemory;

use super::{
    crystallize, deposit, motif, survival, trajectory,
    CrystallizedPattern, DreamConfig, DreamCycleResult, ProtoSkill,
};

/// The Stigmergic Dream Consolidation Engine.
///
/// Orchestrates periodic offline experience replay that:
/// 1. Replays recent trace trajectories
/// 2. Detects recurring temporal motifs
/// 3. Crystallizes motifs into transferable patterns
/// 4. Deposits patterns as DreamTrail hyperedges
/// 5. Applies DKS survival pressure to the pattern population
/// 6. Generates proto-skills for CASS integration
pub struct StigmergicDreamEngine {
    pub config: DreamConfig,
    /// All surviving crystallized patterns
    pub patterns: Vec<CrystallizedPattern>,
    /// Dream generation counter
    pub generation: u64,
}

impl StigmergicDreamEngine {
    pub fn new(config: DreamConfig) -> Self {
        Self {
            config,
            patterns: Vec::new(),
            generation: 0,
        }
    }

    /// Should a dream cycle run at this tick?
    pub fn should_dream(&self, tick: u64) -> bool {
        tick > 0 && tick % self.config.dream_interval == 0
    }

    /// Execute one complete dream cycle.
    ///
    /// Reads from: stigmergic_memory, experiences, beliefs, skill_bank
    /// Writes to:  world_edges (DreamTrail hyperedges),
    ///             stigmergic_memory.traces (retroactive credit),
    ///             self.patterns (crystallized knowledge)
    pub fn dream(
        &mut self,
        stigmergic_memory: &mut StigmergicMemory,
        experiences: &[Experience],
        beliefs: &[Belief],
        skill_bank: &SkillBank,
        world_edges: &mut Vec<HyperEdge>,
        coherence: f64,
    ) -> DreamCycleResult {
        self.generation += 1;
        let start = Instant::now();

        // Phase 1: Assemble trajectory
        let traj = trajectory::assemble_trajectory(
            &self.config,
            stigmergic_memory,
            experiences,
            beliefs,
        );

        // Phase 2: Detect temporal motifs
        let motifs = motif::detect_motifs(&self.config, &traj);

        // Phase 3: Crystallize motifs into patterns
        let (new_patterns, reinforced) = crystallize::crystallize(
            &self.config,
            &mut self.patterns,
            &motifs,
            self.generation,
            skill_bank,
        );

        // Set temporal reach on new patterns
        for p in self.patterns.iter_mut() {
            if p.temporal_reach == 0 {
                p.temporal_reach = self.config.replay_horizon;
            }
        }

        // Phase 4: Stigmergic deposition
        let (boosted, weakened, deposited) = deposit::deposit_stigmergic(
            &self.config,
            &self.patterns,
            stigmergic_memory,
            world_edges,
        );

        // Phase 5: DKS survival pressure
        let decayed = survival::apply_survival_pressure(
            &self.config,
            &mut self.patterns,
            self.generation,
        );

        // Phase 6: Generate proto-skills
        let proto_skills = crystallize::generate_proto_skills(
            &self.config,
            &self.patterns,
        );

        DreamCycleResult {
            dream_generation: self.generation,
            traces_replayed: traj.len(),
            motifs_detected: motifs.len(),
            patterns_crystallized: new_patterns.len(),
            patterns_reinforced: reinforced,
            patterns_decayed: decayed,
            traces_boosted: boosted,
            traces_weakened: weakened,
            dream_trails_deposited: deposited,
            proto_skills_generated: proto_skills.len(),
            dream_duration_ms: start.elapsed().as_millis() as u64,
            coherence_at_dream: coherence,
            total_patterns_alive: self.patterns.len(),
        }
    }

    /// Get proto-skills from current patterns (for CASS injection).
    pub fn proto_skills(&self) -> Vec<ProtoSkill> {
        crystallize::generate_proto_skills(&self.config, &self.patterns)
    }

    /// Number of surviving patterns.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// Get top patterns by persistence score.
    pub fn top_patterns(&self, n: usize) -> Vec<&CrystallizedPattern> {
        let mut sorted: Vec<&CrystallizedPattern> = self.patterns.iter().collect();
        sorted.sort_by(|a, b| {
            b.persistence_score
                .partial_cmp(&a.persistence_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted.into_iter().take(n).collect()
    }

    /// Serialize patterns for persistence to disk.
    pub fn serialize_patterns(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.patterns)
    }

    /// Load patterns from serialized JSON.
    pub fn load_patterns(&mut self, json: &str) -> Result<(), serde_json::Error> {
        self.patterns = serde_json::from_str(json)?;
        Ok(())
    }
}
```

---

## 4. Integration with Existing Systems

### 4.1 Conductor Integration

The dream engine slots into the Conductor (defined in
`/Users/cno/hyper-stigmergic-morphogenesisII/.claude/worktrees/exciting-heisenberg/src/conductor.rs`)
as an optional field, exactly like `autocontext: Option<AutoContextLoop>`.

**Changes to `Conductor` struct (conductor.rs, around line 17):**

```rust
pub struct Conductor {
    // ... existing fields unchanged ...
    pub autocontext: Option<AutoContextLoop>,
    /// Dream consolidation engine (optional; runs every N ticks)
    pub dream_engine: Option<StigmergicDreamEngine>,
}
```

**New builder method:**

```rust
pub fn with_dream_engine(mut self, engine: StigmergicDreamEngine) -> Self {
    self.dream_engine = Some(engine);
    self
}
```

**Changes to `Conductor::tick()` (conductor.rs, after existing Phase 6, before federation sub-tick around line 345):**

```rust
// Phase 7: Dream consolidation (periodic offline replay)
let dream_result = if let Some(ref mut dream) = self.dream_engine {
    let world = self.world.read().await;
    let should_dream = dream.should_dream(world.tick_count);
    let tick_count = world.tick_count;

    if should_dream {
        drop(world);
        let mut world = self.world.write().await;
        let coherence = world.global_coherence();

        let result = dream.dream(
            &mut world.stigmergic_memory,
            &world.experiences,
            &world.beliefs,
            &world.skill_bank,
            &mut world.edges,
            coherence,
        );

        // Inject consolidated wisdom into LivingPrompt
        for pattern in dream.top_patterns(3) {
            rlm.living_prompt.add_insight(format!(
                "[Dream gen {}] {} (conf: {:.0}%, seen {} times, persistence: {:.2})",
                pattern.origin_generation,
                pattern.narrative,
                pattern.confidence * 100.0,
                pattern.observation_count,
                pattern.persistence_score,
            ));
            // Negative patterns become avoid-patterns
            if pattern.valence < -0.15 {
                rlm.living_prompt.add_avoid_pattern(
                    pattern.narrative.clone()
                );
            }
        }

        Some(result)
    } else {
        None
    }
} else {
    None
};
```

**Changes to `TickResult` (conductor.rs, around line 543):**

```rust
pub struct TickResult {
    // ... existing fields unchanged ...
    pub federation: Option<FederationTickResult>,
    pub autocontext: Option<LoopResult>,
    /// Dream consolidation result (only populated on dream-cycle ticks)
    pub dream: Option<DreamCycleResult>,
}
```

### 4.2 lib.rs Registration

**Additions to `/Users/cno/hyper-stigmergic-morphogenesisII/.claude/worktrees/exciting-heisenberg/src/lib.rs`:**

```rust
// Dream Consolidation -- offline experience replay with stigmergic crystallization
pub mod dream;
pub use dream::{
    CrystallizedPattern, DreamConfig, DreamCycleResult, ProtoSkill,
    StigmergicDreamEngine, TemporalMotif,
};
```

### 4.3 CASS Integration

Proto-skills generated by the dream engine can be injected into CASS
through the existing `CASS::add_skill()` method (defined in
`/Users/cno/hyper-stigmergic-morphogenesisII/.claude/worktrees/exciting-heisenberg/src/cass/mod.rs`,
line 124). The conversion from `ProtoSkill` to `Skill` uses the existing
`SkillSource` enum, which needs one new variant:

```rust
// In skill.rs, add to SkillSource enum:
pub enum SkillSource {
    // ... existing variants ...
    /// Crystallized from dream consolidation
    DreamConsolidation {
        dream_generation: u64,
        pattern_id: String,
    },
}
```

### 4.4 DKS Entity Field Sensing

No changes are needed to `StigmergicEntity::sense_field()` (defined in
`/Users/cno/hyper-stigmergic-morphogenesisII/.claude/worktrees/exciting-heisenberg/src/dks/stigmergic_entity.rs`,
line 145). DreamTrail hyperedges are standard `HyperEdge` structs with
`tags["type"] = "DreamTrail"`. The existing `infer_edge_type()` method
can be extended to recognize the DreamTrail tag:

```rust
// In stigmergic_entity.rs, extend infer_edge_type():
fn infer_edge_type(&self, edge: &HyperEdge) -> StigmergicEdgeType {
    if let Some(edge_type) = edge.tags.get("type") {
        match edge_type.as_str() {
            "DreamTrail" => StigmergicEdgeType::SuccessTrail,
            // ... existing matches ...
            _ => StigmergicEdgeType::StrategyAlignment,
        }
    } else {
        // ... existing fallback logic ...
    }
}
```

Alternatively, a new `StigmergicEdgeType::DreamTrail` variant can be added
for more explicit handling.

### 4.5 AutoContext Knowledge Base

Dream patterns can feed into the AutoContext `KnowledgeBase` (defined in
`/Users/cno/hyper-stigmergic-morphogenesisII/.claude/worktrees/exciting-heisenberg/src/autocontext/mod.rs`,
line 354) as Hints. Positive patterns become guidance hints; negative
patterns become avoidance hints.

```rust
// Conversion from CrystallizedPattern to Hint (in conductor tick):
for pattern in dream.top_patterns(5) {
    if let Some(ref mut ac) = self.autocontext {
        let hint = Hint::new(
            pattern.narrative.clone(),
            pattern.motif.associated_task_keys.join(" "),
            pattern.confidence,
        );
        ac.knowledge_base.upsert_hint(hint);
    }
}
```

### 4.6 Batch Runner / Metrics

The dream engine's `DreamCycleResult` struct is already `Serialize`/
`Deserialize`, making it directly usable in the metrics and batch
experiment pipeline. The existing `MetricsExperimentConfig` in
`/Users/cno/hyper-stigmergic-morphogenesisII/.claude/worktrees/exciting-heisenberg/src/metrics.rs`
can be extended with a `dream_enabled: bool` field.

### 4.7 Council Trace Summarizer

The `TraceSummarizer` (in
`/Users/cno/hyper-stigmergic-morphogenesisII/.claude/worktrees/exciting-heisenberg/src/council/trace_summarizer.rs`)
can be extended to include dream pattern summaries in council deliberation
context. This requires adding one method:

```rust
// In TraceSummarizer:
pub fn summarize_dream_patterns(
    &self,
    patterns: &[CrystallizedPattern],
    max_items: usize,
) -> Vec<String> {
    patterns
        .iter()
        .filter(|p| p.confidence > self.min_confidence)
        .take(max_items)
        .map(|p| {
            format!(
                "- [Dream] {} (valence: {:.2}, confidence: {:.0}%, persistence: {:.2})",
                p.narrative,
                p.valence,
                p.confidence * 100.0,
                p.persistence_score,
            )
        })
        .collect()
}
```

---

## 5. Implementation Plan

### Phase 1: Core Engine (estimated: 1-2 days)

1. Create `src/dream/mod.rs` with all core types
2. Implement `src/dream/trajectory.rs` (trajectory assembly)
3. Implement `src/dream/motif.rs` (sliding window + clustering)
4. Implement `src/dream/crystallize.rs` (pattern creation and narrative)
5. Implement `src/dream/survival.rs` (DKS survival pressure)
6. Implement `src/dream/deposit.rs` (stigmergic deposition + credit)
7. Implement `src/dream/engine.rs` (orchestration)
8. Add `pub mod dream;` to `lib.rs`

**Test strategy:** Unit tests for each submodule. The motif detection
and clustering can be tested with synthetic trace sequences. Survival
pressure can be tested by running multiple cycles and verifying that
unreinforced patterns decay.

### Phase 2: Conductor Integration (estimated: 0.5 day)

1. Add `dream_engine: Option<StigmergicDreamEngine>` to `Conductor`
2. Add `with_dream_engine()` builder method
3. Add Phase 7 to `Conductor::tick()` after existing Phase 6
4. Add `dream: Option<DreamCycleResult>` to `TickResult`
5. Wire LivingPrompt injection of consolidated wisdom

**Test strategy:** Integration test that creates a Conductor with a dream
engine, runs 100+ ticks, and verifies that DreamTrail hyperedges appear
in the world graph.

### Phase 3: Subsystem Integration (estimated: 1 day)

1. Add `SkillSource::DreamConsolidation` variant to `skill.rs`
2. Wire proto-skill injection into SkillBank
3. Extend `StigmergicEntity::infer_edge_type()` for DreamTrail
4. Wire AutoContext Hint generation from dream patterns
5. Extend `TraceSummarizer` for dream pattern summaries
6. Add `dream_enabled` to `MetricsExperimentConfig`

**Test strategy:** End-to-end test in `batch_runner` mode with dream
enabled, verifying that patterns crystallize, deposit as hyperedges,
and influence subsequent tick behavior.

### Phase 4: Persistence and Observability (estimated: 0.5 day)

1. Add dream pattern JSON persistence to `~/.hsmii/dream/`
2. Wire into `AutoContextStore` for save/load lifecycle
3. Add dream metrics to the `/api/status` endpoint
4. Add `dream` TUI panel showing top patterns and dream cycle stats

### Phase 5: Tuning and Validation (estimated: 1 day)

1. Run batch experiments with varying `DreamConfig` parameters
2. Measure coherence growth rate with vs. without dream consolidation
3. Verify that DKS survival pressure keeps pattern count bounded
4. Verify compound learning (patterns referencing other patterns)
5. Verify that negative patterns propagate as avoid-patterns

---

## 6. Properties of the Resulting System

**Compound learning.** Each dream cycle produces patterns that become
inputs to future dream cycles. Pattern A from cycle 1 and pattern B from
cycle 2 can combine into a higher-order "A-then-B" motif in cycle 3.
This is meta-learning over the learning process itself.

**Temporal credit assignment.** The eligibility trace mechanism means
traces from 50+ ticks ago can retroactively receive credit for outcomes
that happened recently. This solves the credit assignment problem that
every other agent framework punts on.

**Self-pruning knowledge.** DKS survival pressure means the dream memory
never grows unboundedly. Only patterns repeatedly reinforced by new
evidence survive. Unused connections are pruned.

**Indirect transfer via stigmergy.** When agent A's experiences
crystallize into a DreamTrail hyperedge, agent B discovers that edge
through its existing field sensor. No explicit message passing required.

**Narrative inspectability.** Every crystallized pattern has a
human-readable narrative. The system's consolidated knowledge is not
opaque weights but readable text describing what it learned.

**Ecological integration.** Patterns compete for survival using the
same DKS dynamics that govern knowledge entities. The pattern population
is itself an evolving ecology, subject to the same far-from-equilibrium
persistence selection as everything else in HSM-II.

---

## 7. Complexity Assessment

**New code:** Approximately 900-1200 lines of Rust across 7 files.

**Existing code changes:** Approximately 40-60 lines across 5 existing
files (conductor.rs, lib.rs, skill.rs, stigmergic_entity.rs,
trace_summarizer.rs).

**New dependencies:** None. Uses only existing crate dependencies
(serde, HashMap, Vec, standard math).

**Risk:** Low. The dream engine is fully optional (`Option<StigmergicDreamEngine>`),
gated by a config flag, and never modifies existing behavior when disabled.
All mutations to the world graph and trace buffer are additive (new
hyperedges, score adjustments) rather than destructive.
