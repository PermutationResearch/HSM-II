use serde::{Deserialize, Serialize};

use super::trust::TrustGraph;
use super::types::{SharedEdge, SystemId};

/// Mediates conflicts between contradicting edges from different systems.
/// Uses a weighted combination of trust, empirical evidence, and consistency.
#[derive(Clone, Debug)]
pub struct ConflictMediator {
    pub trust_weight: f64,
    pub empirical_weight: f64,
    pub consistency_weight: f64,
}

impl Default for ConflictMediator {
    fn default() -> Self {
        Self {
            trust_weight: 0.4,
            empirical_weight: 0.35,
            consistency_weight: 0.25,
        }
    }
}

/// How a conflict between two edges was resolved.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ConflictResolution {
    /// Keep both edges as parallel hypotheses.
    KeepBoth,
    /// Prefer the first edge.
    PreferA { reason: String },
    /// Prefer the second edge.
    PreferB { reason: String },
    /// Merge into a blended edge.
    Merge { merged_weight: f64 },
}

/// Record of a detected and resolved conflict.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub edge_a_id: String,
    pub edge_b_id: String,
    pub resolution: ConflictResolution,
    pub resolved_at: u64,
}

impl ConflictMediator {
    /// Detect conflicts among shared edges.
    /// Two edges conflict if they share the same vertices but have opposing weights
    /// or contradicting trust tags.
    pub fn detect_conflicts(edges: &[SharedEdge]) -> Vec<(usize, usize)> {
        let mut conflicts = Vec::new();

        for i in 0..edges.len() {
            for j in (i + 1)..edges.len() {
                if Self::edges_conflict(&edges[i], &edges[j]) {
                    conflicts.push((i, j));
                }
            }
        }

        conflicts
    }

    /// Check if two edges are in conflict.
    fn edges_conflict(a: &SharedEdge, b: &SharedEdge) -> bool {
        // Same vertices (order-independent)
        let mut a_verts = a.vertices.clone();
        let mut b_verts = b.vertices.clone();
        a_verts.sort();
        b_verts.sort();

        if a_verts != b_verts {
            return false;
        }

        // Same edge type but different origins
        if a.edge_type != b.edge_type {
            return false;
        }

        // Conflicting if weights diverge significantly (one positive, one negative-ish)
        let weight_diff = (a.weight - b.weight).abs();
        if weight_diff > 0.5 {
            return true;
        }

        // Conflicting trust tags (e.g., "verified" vs "disputed")
        let a_has_disputed = a
            .trust_tags
            .iter()
            .any(|t| t.contains("disputed") || t.contains("reject"));
        let b_has_disputed = b
            .trust_tags
            .iter()
            .any(|t| t.contains("disputed") || t.contains("reject"));
        let a_has_verified = a
            .trust_tags
            .iter()
            .any(|t| t.contains("verified") || t.contains("accept"));
        let b_has_verified = b
            .trust_tags
            .iter()
            .any(|t| t.contains("verified") || t.contains("accept"));

        (a_has_disputed && b_has_verified) || (a_has_verified && b_has_disputed)
    }

    /// Resolve a conflict between two edges using weighted evaluation.
    pub fn resolve(
        &self,
        edge_a: &SharedEdge,
        edge_b: &SharedEdge,
        local_system: &SystemId,
        trust_graph: &TrustGraph,
    ) -> ConflictResolution {
        // Trust score of origin systems
        let trust_a = trust_graph.get_trust(local_system, &edge_a.provenance.origin_system);
        let trust_b = trust_graph.get_trust(local_system, &edge_b.provenance.origin_system);
        let trust_score = trust_a - trust_b; // positive favors A

        // Empirical evidence (usage and success counts)
        let empirical_a = if edge_a.usage_count > 0 {
            edge_a.success_count as f64 / edge_a.usage_count as f64
        } else {
            0.5
        };
        let empirical_b = if edge_b.usage_count > 0 {
            edge_b.success_count as f64 / edge_b.usage_count as f64
        } else {
            0.5
        };
        let empirical_score = empirical_a - empirical_b;

        // Consistency: more contributing systems = more consistent
        let consistency_a = edge_a.contributing_systems.len() as f64;
        let consistency_b = edge_b.contributing_systems.len() as f64;
        let consistency_score = if consistency_a + consistency_b > 0.0 {
            (consistency_a - consistency_b) / (consistency_a + consistency_b)
        } else {
            0.0
        };

        let composite = self.trust_weight * trust_score
            + self.empirical_weight * empirical_score
            + self.consistency_weight * consistency_score;

        if composite.abs() < 0.1 {
            // Too close to call — keep both as parallel hypotheses
            ConflictResolution::KeepBoth
        } else if composite > 0.3 {
            ConflictResolution::PreferA {
                reason: format!("composite score {:.3} favors A (trust={:.2}, empirical={:.2}, consistency={:.2})",
                    composite, trust_score, empirical_score, consistency_score),
            }
        } else if composite < -0.3 {
            ConflictResolution::PreferB {
                reason: format!("composite score {:.3} favors B (trust={:.2}, empirical={:.2}, consistency={:.2})",
                    composite, trust_score, empirical_score, consistency_score),
            }
        } else {
            // Moderate difference — merge with blended weight
            let merged_weight =
                edge_a.weight * (0.5 + composite / 2.0) + edge_b.weight * (0.5 - composite / 2.0);
            ConflictResolution::Merge { merged_weight }
        }
    }
}
