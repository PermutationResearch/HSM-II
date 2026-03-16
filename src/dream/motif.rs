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
    trace_kind_index, DreamConfig, DetectedMotif, MotifCluster, TraceWindow, TrajectoryElement,
    TRACE_KIND_COUNT,
};

/// Extract sliding windows from trajectory and cluster them
/// to find recurring temporal motifs.
pub fn detect_motifs(config: &DreamConfig, trajectory: &[TrajectoryElement]) -> Vec<DetectedMotif> {
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
            let avg_outcome = cluster.members.iter().map(|w| w.outcome).sum::<f64>() / n;
            let outcome_variance = cluster
                .members
                .iter()
                .map(|w| (w.outcome - avg_outcome).powi(2))
                .sum::<f64>()
                / n;
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
///   TRACE_KIND_COUNT (8)  — trace kind histogram
///   + 1                   — agent diversity (Shannon entropy)
///   + 1                   — normalized temporal span
///   + 1                   — outcome trajectory slope
///   + 1                   — eligibility-weighted outcome
///   = 12 features
///
/// This dimensionality is deliberately small. The purpose is not
/// high-dimensional embedding but rather capturing the *shape* of
/// a trace subsequence for clustering.
fn extract_windows(config: &DreamConfig, trajectory: &[TrajectoryElement]) -> Vec<TraceWindow> {
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
            if p > 0.0 {
                -p * p.ln()
            } else {
                0.0
            }
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
    if den.abs() < 1e-12 {
        0.0
    } else {
        num / den
    }
}

/// Cosine similarity between two feature vectors.
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
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
                let key = member.elements[pos].trace_kind.as_str().to_string();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_element(tick: u64, kind: TraceKind, score: Option<f64>) -> TrajectoryElement {
        TrajectoryElement {
            tick,
            trace_kind: kind,
            agent_id: 1,
            task_key: None,
            outcome_score: score,
            embedding: None,
            eligibility: 1.0,
        }
    }

    #[test]
    fn test_detect_motifs_too_short() {
        let config = DreamConfig {
            motif_window_size: 4,
            ..DreamConfig::default()
        };
        let traj = vec![
            make_element(1, TraceKind::PromiseMade, Some(0.5)),
            make_element(2, TraceKind::PromiseResolved, Some(0.6)),
        ];
        let motifs = detect_motifs(&config, &traj);
        assert!(motifs.is_empty());
    }

    #[test]
    fn test_window_outcome_position_weighted() {
        let window = vec![
            make_element(1, TraceKind::PromiseMade, Some(0.0)),
            make_element(2, TraceKind::PromiseMade, Some(0.0)),
            make_element(3, TraceKind::PromiseMade, Some(1.0)), // later, gets more weight
        ];
        let outcome = window_outcome(&window);
        // Position weights: 1, 2, 3. Weighted sum: 0*1 + 0*2 + 1.0*3 = 3.0
        // Total weight: 1+2+3 = 6.0. Outcome: 3.0/6.0 = 0.5
        assert!((outcome - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_encode_window_feature_dimensions() {
        let config = DreamConfig::default();
        let window = vec![
            make_element(1, TraceKind::PromiseMade, Some(0.5)),
            make_element(2, TraceKind::QueryPlanned, Some(0.3)),
            make_element(3, TraceKind::QueryExecuted, Some(0.7)),
            make_element(4, TraceKind::DeliveryRecorded, Some(0.9)),
            make_element(5, TraceKind::PromiseResolved, Some(1.0)),
            make_element(6, TraceKind::PromiseMade, Some(0.4)),
            make_element(7, TraceKind::PromiseMade, Some(0.5)),
            make_element(8, TraceKind::PromiseMade, Some(0.6)),
        ];
        let features = encode_window(&config, &window);
        // 8 (histogram) + 1 (entropy) + 1 (span) + 1 (slope) + 1 (elig_outcome) = 12
        assert_eq!(features.len(), 12);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-9);
    }

    #[test]
    fn test_linear_regression_slope_ascending() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let slope = linear_regression_slope(&values);
        assert!((slope - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_linear_regression_slope_flat() {
        let values = vec![3.0, 3.0, 3.0, 3.0];
        let slope = linear_regression_slope(&values);
        assert!(slope.abs() < 1e-9);
    }

    #[test]
    fn test_clustering_identical_windows() {
        // Create several identical windows — should cluster together
        let features = vec![0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.1, 0.05, 0.3, 0.4];
        let windows: Vec<TraceWindow> = (0..5)
            .map(|i| TraceWindow {
                start_idx: i,
                elements: vec![make_element(i as u64, TraceKind::PromiseMade, Some(0.5))],
                outcome: 0.5,
                features: features.clone(),
            })
            .collect();

        let clusters = cluster_windows(&windows, 0.7);
        // All should be in one cluster since they're identical
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].members.len(), 5);
    }

    #[test]
    fn test_detect_motifs_finds_recurring_pattern() {
        let config = DreamConfig {
            motif_window_size: 3,
            min_observations: 2,
            cluster_threshold: 0.8,
            replay_horizon: 1000,
            ..DreamConfig::default()
        };

        // Create a trajectory with a repeated pattern:
        // PromiseMade -> QueryPlanned -> PromiseResolved
        // repeated several times
        let mut traj = Vec::new();
        for cycle in 0..5 {
            let base = cycle * 3;
            traj.push(make_element(base as u64, TraceKind::PromiseMade, Some(0.5)));
            traj.push(make_element(
                (base + 1) as u64,
                TraceKind::QueryPlanned,
                Some(0.6),
            ));
            traj.push(make_element(
                (base + 2) as u64,
                TraceKind::PromiseResolved,
                Some(0.8),
            ));
        }

        let motifs = detect_motifs(&config, &traj);
        // Should detect at least one motif (the repeated pattern)
        assert!(
            !motifs.is_empty(),
            "Expected to detect motifs from repeating pattern"
        );
        // The most observed motif should have observation_count >= min_observations
        let max_obs = motifs.iter().map(|m| m.observation_count).max().unwrap();
        assert!(max_obs >= config.min_observations);
    }
}
