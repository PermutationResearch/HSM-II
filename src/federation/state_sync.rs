use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::federation::types::SystemId;

/// Vector clock for causal ordering of state updates
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorClock {
    pub clocks: HashMap<SystemId, u64>,
}

impl VectorClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the logical clock for the given system
    pub fn increment(&mut self, system: &SystemId) {
        *self.clocks.entry(system.clone()).or_insert(0) += 1;
    }

    /// Merge another vector clock into this one (element-wise max)
    pub fn merge(&mut self, other: &VectorClock) {
        for (k, v) in &other.clocks {
            let entry = self.clocks.entry(k.clone()).or_insert(0);
            *entry = (*entry).max(*v);
        }
    }

    /// Returns true if self causally happens-before other.
    /// self < other iff for all systems, self[s] <= other[s] AND at least one is strictly less.
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut at_least_one_less = false;

        // Check all keys in self
        for (k, v) in &self.clocks {
            let other_v = other.clocks.get(k).copied().unwrap_or(0);
            if *v > other_v {
                return false;
            }
            if *v < other_v {
                at_least_one_less = true;
            }
        }

        // Check keys in other that are not in self (those are implicitly 0 in self)
        for (k, v) in &other.clocks {
            if !self.clocks.contains_key(k) && *v > 0 {
                at_least_one_less = true;
            }
        }

        at_least_one_less
    }

    /// Returns true if the two clocks are concurrent (neither happens-before the other)
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self)
    }

    /// Get the clock value for a specific system (0 if not present)
    pub fn get(&self, system: &SystemId) -> u64 {
        self.clocks.get(system).copied().unwrap_or(0)
    }
}

/// State digest for efficient comparison between peers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateDigest {
    pub system: SystemId,
    pub tick: u64,
    pub belief_hash: u64,
    pub skill_hash: u64,
    pub trust_hash: u64,
    pub world_hash: u64,
    pub clock: VectorClock,
}

/// Sync request/response messages between two systems
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SyncMessage {
    /// Send digest for comparison
    DigestExchange(StateDigest),
    /// Request specific subsystem data since a given tick
    RequestDelta { subsystem: String, since_tick: u64 },
    /// Response with delta data (serialized as JSON string)
    DeltaResponse { subsystem: String, data: String },
    /// Acknowledge sync complete
    Ack { tick: u64 },
}

/// Anti-entropy sync engine that periodically reconciles state with peers
pub struct StateSyncEngine {
    pub local_system: SystemId,
    pub clock: VectorClock,
    pub peer_digests: HashMap<SystemId, StateDigest>,
    pub sync_interval_ticks: u64,
    pub last_sync_tick: u64,
}

impl StateSyncEngine {
    pub fn new(local_system: SystemId, sync_interval: u64) -> Self {
        let mut clock = VectorClock::new();
        clock.increment(&local_system);

        Self {
            local_system,
            clock,
            peer_digests: HashMap::new(),
            sync_interval_ticks: sync_interval,
            last_sync_tick: 0,
        }
    }

    /// Generate current state digest from local counts.
    /// Uses simple hash combining count + tick as a fast fingerprint.
    pub fn local_digest(
        &self,
        tick: u64,
        belief_count: usize,
        skill_count: usize,
        trust_count: usize,
    ) -> StateDigest {
        StateDigest {
            system: self.local_system.clone(),
            tick,
            belief_hash: Self::hash_pair(belief_count as u64, tick),
            skill_hash: Self::hash_pair(skill_count as u64, tick),
            trust_hash: Self::hash_pair(trust_count as u64, tick),
            world_hash: Self::hash_pair((belief_count + skill_count + trust_count) as u64, tick),
            clock: self.clock.clone(),
        }
    }

    /// Compare local digest with a peer digest, returning subsystem names that differ
    pub fn diff(&self, peer_digest: &StateDigest, local_digest: &StateDigest) -> Vec<String> {
        let mut out_of_sync = Vec::new();

        if peer_digest.belief_hash != local_digest.belief_hash {
            out_of_sync.push("beliefs".to_string());
        }
        if peer_digest.skill_hash != local_digest.skill_hash {
            out_of_sync.push("skills".to_string());
        }
        if peer_digest.trust_hash != local_digest.trust_hash {
            out_of_sync.push("trust".to_string());
        }
        if peer_digest.world_hash != local_digest.world_hash {
            out_of_sync.push("world".to_string());
        }

        out_of_sync
    }

    /// Returns true if enough ticks have elapsed since the last sync
    pub fn should_sync(&self, current_tick: u64) -> bool {
        current_tick.saturating_sub(self.last_sync_tick) >= self.sync_interval_ticks
    }

    /// Record that a sync round completed at the given tick
    pub fn mark_synced(&mut self, tick: u64) {
        self.last_sync_tick = tick;
        self.clock.increment(&self.local_system);
    }

    /// Store a received peer digest for future comparisons
    pub fn store_peer_digest(&mut self, digest: StateDigest) {
        self.clock.merge(&digest.clock);
        self.peer_digests.insert(digest.system.clone(), digest);
    }

    /// Generate sync messages for all peers that are out of date.
    /// Returns a list of (peer_system_id, messages_to_send).
    pub fn generate_sync_requests(
        &self,
        current_tick: u64,
        belief_count: usize,
        skill_count: usize,
        trust_count: usize,
    ) -> Vec<(SystemId, Vec<SyncMessage>)> {
        let local = self.local_digest(current_tick, belief_count, skill_count, trust_count);
        let mut requests = Vec::new();

        for (peer_id, peer_digest) in &self.peer_digests {
            let diffs = self.diff(peer_digest, &local);
            if diffs.is_empty() {
                continue;
            }

            let mut messages = vec![SyncMessage::DigestExchange(local.clone())];
            for subsystem in diffs {
                messages.push(SyncMessage::RequestDelta {
                    subsystem,
                    since_tick: peer_digest.tick,
                });
            }
            requests.push((peer_id.clone(), messages));
        }

        requests
    }

    /// Simple hash combining two u64 values for digest fingerprinting
    fn hash_pair(a: u64, b: u64) -> u64 {
        let mut hasher = DefaultHasher::new();
        a.hash(&mut hasher);
        b.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_clock_happens_before() {
        let mut a = VectorClock::new();
        let mut b = VectorClock::new();

        a.increment(&"sys-a".to_string());
        b.increment(&"sys-a".to_string());
        b.increment(&"sys-b".to_string());

        assert!(a.happens_before(&b));
        assert!(!b.happens_before(&a));
    }

    #[test]
    fn vector_clock_concurrent() {
        let mut a = VectorClock::new();
        let mut b = VectorClock::new();

        a.increment(&"sys-a".to_string());
        b.increment(&"sys-b".to_string());

        assert!(a.is_concurrent(&b));
    }

    #[test]
    fn vector_clock_merge() {
        let mut a = VectorClock::new();
        let mut b = VectorClock::new();

        a.increment(&"sys-a".to_string());
        a.increment(&"sys-a".to_string()); // a["sys-a"] = 2
        b.increment(&"sys-a".to_string()); // b["sys-a"] = 1
        b.increment(&"sys-b".to_string()); // b["sys-b"] = 1

        a.merge(&b);
        assert_eq!(a.get(&"sys-a".to_string()), 2);
        assert_eq!(a.get(&"sys-b".to_string()), 1);
    }

    #[test]
    fn vector_clock_equal_not_happens_before() {
        let mut a = VectorClock::new();
        a.increment(&"sys-a".to_string());
        let b = a.clone();
        assert!(!a.happens_before(&b));
        assert!(!b.happens_before(&a));
        // Equal clocks are not concurrent in the strict sense;
        // is_concurrent returns true because neither happens-before the other
        assert!(a.is_concurrent(&b));
    }

    #[test]
    fn should_sync_timing() {
        let engine = StateSyncEngine::new("sys-local".to_string(), 10);
        assert!(!engine.should_sync(5));
        assert!(engine.should_sync(10));
        assert!(engine.should_sync(15));
    }

    #[test]
    fn diff_detects_mismatches() {
        let engine = StateSyncEngine::new("sys-local".to_string(), 10);
        let local = engine.local_digest(100, 50, 30, 20);
        let mut peer = engine.local_digest(100, 50, 30, 20);
        // Tamper with peer's belief hash
        peer.belief_hash = 0;
        peer.system = "sys-peer".to_string();

        let diffs = engine.diff(&peer, &local);
        assert!(diffs.contains(&"beliefs".to_string()));
    }

    #[test]
    fn store_peer_digest_merges_clock() {
        let mut engine = StateSyncEngine::new("sys-local".to_string(), 10);
        let mut peer_clock = VectorClock::new();
        peer_clock.increment(&"sys-peer".to_string());
        peer_clock.increment(&"sys-peer".to_string());

        let digest = StateDigest {
            system: "sys-peer".to_string(),
            tick: 50,
            belief_hash: 1,
            skill_hash: 2,
            trust_hash: 3,
            world_hash: 4,
            clock: peer_clock,
        };

        engine.store_peer_digest(digest);
        assert_eq!(engine.clock.get(&"sys-peer".to_string()), 2);
        assert!(engine.peer_digests.contains_key(&"sys-peer".to_string()));
    }
}
