use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::federation::types::SystemId;
use crate::hyper_stigmergy::Belief;

/// Partition state detected by the federation
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PartitionState {
    /// All peers reachable
    Connected,
    /// Some peers unreachable
    PartialPartition { unreachable: Vec<SystemId> },
    /// This node is isolated (no peers reachable)
    Isolated,
}

/// Conflict resolution strategy when partitions heal
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MergeStrategy {
    /// Last-writer-wins based on timestamp
    LastWriterWins,
    /// Higher confidence value wins for beliefs
    HighestConfidence,
    /// Union merge (keep all unique entries)
    UnionMerge,
    /// Require manual council resolution
    CouncilResolve,
}

/// Per-peer liveness state
#[derive(Clone, Debug)]
pub struct PeerState {
    pub system_id: SystemId,
    pub last_heartbeat: u64,
    pub missed_heartbeats: u32,
    pub reachable: bool,
}

/// Track peer liveness and detect partitions
pub struct PartitionDetector {
    pub local_system: SystemId,
    pub peers: HashMap<SystemId, PeerState>,
    pub heartbeat_interval_ms: u64,
    pub failure_threshold: u32,
    pub current_state: PartitionState,
}

impl PartitionDetector {
    pub fn new(
        local_system: SystemId,
        heartbeat_interval_ms: u64,
        failure_threshold: u32,
    ) -> Self {
        Self {
            local_system,
            peers: HashMap::new(),
            heartbeat_interval_ms,
            failure_threshold,
            current_state: PartitionState::Connected,
        }
    }

    /// Register a peer for heartbeat tracking
    pub fn add_peer(&mut self, peer: SystemId) {
        self.peers.insert(
            peer.clone(),
            PeerState {
                system_id: peer,
                last_heartbeat: 0,
                missed_heartbeats: 0,
                reachable: true,
            },
        );
    }

    /// Remove a peer from tracking
    pub fn remove_peer(&mut self, peer: &SystemId) {
        self.peers.remove(peer);
    }

    /// Record heartbeat received from a peer
    pub fn heartbeat_received(&mut self, from: &SystemId, timestamp: u64) {
        if let Some(state) = self.peers.get_mut(from) {
            state.last_heartbeat = timestamp;
            state.missed_heartbeats = 0;
            state.reachable = true;
        }
    }

    /// Check for partition by evaluating heartbeat deadlines.
    /// Call this periodically (e.g. every heartbeat_interval_ms).
    pub fn detect(&mut self, now_ms: u64) -> PartitionState {
        if self.peers.is_empty() {
            self.current_state = PartitionState::Connected;
            return self.current_state.clone();
        }

        let mut unreachable = Vec::new();

        for state in self.peers.values_mut() {
            // Calculate how many heartbeat intervals have been missed
            if state.last_heartbeat == 0 {
                // Never received a heartbeat; count based on total elapsed time
                if now_ms > self.heartbeat_interval_ms * (self.failure_threshold as u64) {
                    state.missed_heartbeats = self.failure_threshold + 1;
                }
            } else {
                let elapsed = now_ms.saturating_sub(state.last_heartbeat);
                let expected_beats = elapsed / self.heartbeat_interval_ms.max(1);
                state.missed_heartbeats = expected_beats as u32;
            }

            if state.missed_heartbeats > self.failure_threshold {
                state.reachable = false;
                unreachable.push(state.system_id.clone());
            } else {
                state.reachable = true;
            }
        }

        self.current_state = if unreachable.is_empty() {
            PartitionState::Connected
        } else if unreachable.len() == self.peers.len() {
            PartitionState::Isolated
        } else {
            PartitionState::PartialPartition { unreachable }
        };

        self.current_state.clone()
    }

    /// Get list of currently reachable peers
    pub fn reachable_peers(&self) -> Vec<SystemId> {
        self.peers
            .values()
            .filter(|s| s.reachable)
            .map(|s| s.system_id.clone())
            .collect()
    }

    /// Get list of currently unreachable peers
    pub fn unreachable_peers(&self) -> Vec<SystemId> {
        self.peers
            .values()
            .filter(|s| !s.reachable)
            .map(|s| s.system_id.clone())
            .collect()
    }

    /// Check whether any partition is currently detected
    pub fn is_partitioned(&self) -> bool {
        !matches!(self.current_state, PartitionState::Connected)
    }
}

/// Merge engine for healing partitions — reconciles divergent state
pub struct PartitionMerger {
    pub strategy: MergeStrategy,
}

impl PartitionMerger {
    pub fn new(strategy: MergeStrategy) -> Self {
        Self { strategy }
    }

    /// Merge two belief sets after a partition heals.
    /// Returns the reconciled belief set.
    pub fn merge_beliefs(&self, local: &[Belief], remote: &[Belief]) -> Vec<Belief> {
        // Build index of remote beliefs by id for fast lookup
        let remote_by_id: HashMap<usize, &Belief> =
            remote.iter().map(|b| (b.id, b)).collect();
        let local_by_id: HashMap<usize, &Belief> =
            local.iter().map(|b| (b.id, b)).collect();

        let mut merged: HashMap<usize, Belief> = HashMap::new();

        // Process all local beliefs
        for belief in local {
            if let Some(remote_belief) = remote_by_id.get(&belief.id) {
                // Belief exists on both sides: apply merge strategy
                let winner = self.resolve_belief(belief, remote_belief);
                merged.insert(belief.id, winner);
            } else {
                // Only exists locally
                merged.insert(belief.id, belief.clone());
            }
        }

        // Add remote-only beliefs
        for belief in remote {
            if !local_by_id.contains_key(&belief.id) {
                merged.insert(belief.id, belief.clone());
            }
        }

        let mut result: Vec<Belief> = merged.into_values().collect();
        result.sort_by_key(|b| b.id);
        result
    }

    /// Resolve a single conflicting belief using the configured strategy
    fn resolve_belief(&self, local: &Belief, remote: &Belief) -> Belief {
        match &self.strategy {
            MergeStrategy::LastWriterWins => {
                if remote.updated_at > local.updated_at {
                    remote.clone()
                } else {
                    local.clone()
                }
            }
            MergeStrategy::HighestConfidence => {
                if remote.confidence > local.confidence {
                    remote.clone()
                } else {
                    local.clone()
                }
            }
            MergeStrategy::UnionMerge => {
                // Take the version with higher update_count as base,
                // then union supporting/contradicting evidence
                let mut base = if remote.update_count > local.update_count {
                    remote.clone()
                } else {
                    local.clone()
                };

                // Union evidence sets
                let mut supporting: Vec<String> = local
                    .supporting_evidence
                    .iter()
                    .chain(remote.supporting_evidence.iter())
                    .cloned()
                    .collect();
                supporting.sort();
                supporting.dedup();

                let mut contradicting: Vec<String> = local
                    .contradicting_evidence
                    .iter()
                    .chain(remote.contradicting_evidence.iter())
                    .cloned()
                    .collect();
                contradicting.sort();
                contradicting.dedup();

                base.supporting_evidence = supporting;
                base.contradicting_evidence = contradicting;
                base.update_count = local.update_count.max(remote.update_count);
                base.updated_at = local.updated_at.max(remote.updated_at);
                let mut eids: Vec<usize> = local
                    .evidence_belief_ids
                    .iter()
                    .chain(remote.evidence_belief_ids.iter())
                    .copied()
                    .collect();
                eids.sort_unstable();
                eids.dedup();
                base.evidence_belief_ids = eids;
                base
            }
            MergeStrategy::CouncilResolve => {
                // Return a merged record with both sets of evidence,
                // but mark confidence as 0.0 to signal "needs council review"
                let mut merged = local.clone();
                merged.confidence = 0.0;

                let mut supporting: Vec<String> = local
                    .supporting_evidence
                    .iter()
                    .chain(remote.supporting_evidence.iter())
                    .cloned()
                    .collect();
                supporting.sort();
                supporting.dedup();

                let mut contradicting: Vec<String> = local
                    .contradicting_evidence
                    .iter()
                    .chain(remote.contradicting_evidence.iter())
                    .cloned()
                    .collect();
                contradicting.sort();
                contradicting.dedup();

                // Add a marker to contradicting evidence to flag for council
                contradicting.push("[COUNCIL_REVIEW_REQUIRED]".to_string());

                merged.supporting_evidence = supporting;
                merged.contradicting_evidence = contradicting;
                merged.updated_at = local.updated_at.max(remote.updated_at);
                let mut eids: Vec<usize> = local
                    .evidence_belief_ids
                    .iter()
                    .chain(remote.evidence_belief_ids.iter())
                    .copied()
                    .collect();
                eids.sort_unstable();
                eids.dedup();
                merged.evidence_belief_ids = eids;
                merged
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hyper_stigmergy::BeliefSource;

    fn make_belief(id: usize, confidence: f64, updated_at: u64) -> Belief {
        Belief {
            id,
            content: format!("belief-{}", id),
            confidence,
            source: BeliefSource::Observation,
            supporting_evidence: vec![format!("ev-{}", id)],
            contradicting_evidence: vec![],
            created_at: 100,
            updated_at,
            update_count: 1,
            abstract_l0: None,
            overview_l1: None,
            owner_namespace: None,
            supersedes_belief_id: None,
            evidence_belief_ids: Vec::new(),
            human_committed: false,
        }
    }

    #[test]
    fn partition_detection_connected() {
        let mut detector = PartitionDetector::new("local".to_string(), 1000, 3);
        detector.add_peer("peer-a".to_string());
        detector.heartbeat_received(&"peer-a".to_string(), 5000);

        let state = detector.detect(5500);
        assert_eq!(state, PartitionState::Connected);
    }

    #[test]
    fn partition_detection_partial() {
        let mut detector = PartitionDetector::new("local".to_string(), 1000, 3);
        detector.add_peer("peer-a".to_string());
        detector.add_peer("peer-b".to_string());

        detector.heartbeat_received(&"peer-a".to_string(), 1000);
        detector.heartbeat_received(&"peer-b".to_string(), 1000);

        // peer-b misses heartbeats for a long time
        let _state = detector.detect(6000); // 5 intervals elapsed since last heartbeat
        // peer-a also missed since 1000, elapsed = 5000, missed = 5 > 3
        // Both unreachable => Isolated
        assert!(detector.is_partitioned());
    }

    #[test]
    fn partition_detection_isolated() {
        let mut detector = PartitionDetector::new("local".to_string(), 1000, 2);
        detector.add_peer("peer-a".to_string());
        detector.add_peer("peer-b".to_string());
        // No heartbeats received, advance time past threshold
        let state = detector.detect(10000);
        assert_eq!(state, PartitionState::Isolated);
    }

    #[test]
    fn reachable_unreachable_lists() {
        let mut detector = PartitionDetector::new("local".to_string(), 1000, 3);
        detector.add_peer("peer-a".to_string());
        detector.add_peer("peer-b".to_string());

        detector.heartbeat_received(&"peer-a".to_string(), 9000);
        // peer-b never heartbeats

        detector.detect(10000);

        let reachable = detector.reachable_peers();
        let unreachable = detector.unreachable_peers();

        assert!(reachable.contains(&"peer-a".to_string()));
        assert!(unreachable.contains(&"peer-b".to_string()));
    }

    #[test]
    fn merge_last_writer_wins() {
        let merger = PartitionMerger::new(MergeStrategy::LastWriterWins);
        let local = vec![make_belief(1, 0.8, 100), make_belief(2, 0.5, 200)];
        let remote = vec![make_belief(1, 0.6, 300), make_belief(3, 0.9, 150)];

        let merged = merger.merge_beliefs(&local, &remote);
        assert_eq!(merged.len(), 3);

        // Belief 1: remote wins (updated_at 300 > 100)
        let b1 = merged.iter().find(|b| b.id == 1).unwrap();
        assert!((b1.confidence - 0.6).abs() < f64::EPSILON);

        // Belief 2: local only
        assert!(merged.iter().any(|b| b.id == 2));
        // Belief 3: remote only
        assert!(merged.iter().any(|b| b.id == 3));
    }

    #[test]
    fn merge_highest_confidence() {
        let merger = PartitionMerger::new(MergeStrategy::HighestConfidence);
        let local = vec![make_belief(1, 0.9, 100)];
        let remote = vec![make_belief(1, 0.7, 300)];

        let merged = merger.merge_beliefs(&local, &remote);
        let b1 = merged.iter().find(|b| b.id == 1).unwrap();
        assert!((b1.confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn merge_union() {
        let merger = PartitionMerger::new(MergeStrategy::UnionMerge);
        let mut local_belief = make_belief(1, 0.8, 100);
        local_belief.supporting_evidence = vec!["local-ev".to_string()];
        local_belief.evidence_belief_ids = vec![10, 11];
        let mut remote_belief = make_belief(1, 0.7, 200);
        remote_belief.supporting_evidence = vec!["remote-ev".to_string()];
        remote_belief.evidence_belief_ids = vec![11, 12];

        let merged = merger.merge_beliefs(&[local_belief], &[remote_belief]);
        let b1 = merged.iter().find(|b| b.id == 1).unwrap();
        assert_eq!(b1.supporting_evidence.len(), 2);
        assert!(b1.supporting_evidence.contains(&"local-ev".to_string()));
        assert!(b1.supporting_evidence.contains(&"remote-ev".to_string()));
        assert_eq!(b1.evidence_belief_ids, vec![10, 11, 12]);
    }

    #[test]
    fn merge_council_resolve() {
        let merger = PartitionMerger::new(MergeStrategy::CouncilResolve);
        let local = vec![make_belief(1, 0.8, 100)];
        let remote = vec![make_belief(1, 0.7, 200)];

        let merged = merger.merge_beliefs(&local, &remote);
        let b1 = merged.iter().find(|b| b.id == 1).unwrap();
        // Confidence zeroed for council review
        assert!((b1.confidence - 0.0).abs() < f64::EPSILON);
        assert!(b1
            .contradicting_evidence
            .contains(&"[COUNCIL_REVIEW_REQUIRED]".to_string()));
    }
}
