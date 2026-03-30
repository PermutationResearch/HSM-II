use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::federation::types::SystemId;
use crate::hyper_stigmergy::Belief;
use crate::skill::Skill;

/// Propagation strategy for distributing state changes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PropagationStrategy {
    /// Broadcast to all connected peers
    Broadcast,
    /// Only propagate to peers above trust threshold
    TrustGated { min_trust: f64 },
    /// Epidemic/gossip-style probabilistic propagation
    Gossip { fanout: usize, rounds: usize },
    /// Targeted propagation to specific systems
    Targeted { targets: Vec<SystemId> },
}

/// A propagation envelope wrapping any distributable payload
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropagationEnvelope {
    pub origin: SystemId,
    pub hop_count: u32,
    pub max_hops: u32,
    pub timestamp: u64,
    pub payload: PropagationPayload,
    pub signature: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PropagationPayload {
    BeliefUpdate(Belief),
    SkillUpdate(Skill),
    TrustUpdate {
        from: SystemId,
        to: SystemId,
        score: f64,
    },
    WorldCheckpoint {
        tick: u64,
        coherence: f64,
        edge_count: usize,
    },
    CouncilDecision {
        id: String,
        proposal_id: String,
        decision_json: String,
    },
}

/// Propagation engine managing outbound distribution
pub struct PropagationEngine {
    pub local_system: SystemId,
    pub strategy: PropagationStrategy,
    pub max_hops: u32,
    pub outbox: Vec<PropagationEnvelope>,
    /// Track seen envelope IDs to prevent loops
    pub seen: HashMap<String, u64>,
}

impl PropagationEngine {
    pub fn new(local_system: SystemId, strategy: PropagationStrategy) -> Self {
        Self {
            local_system,
            strategy,
            max_hops: 5,
            outbox: Vec::new(),
            seen: HashMap::new(),
        }
    }

    /// Enqueue a payload for propagation
    pub fn enqueue(&mut self, payload: PropagationPayload) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let envelope = PropagationEnvelope {
            origin: self.local_system.clone(),
            hop_count: 0,
            max_hops: self.max_hops,
            timestamp: now,
            payload,
            signature: None,
        };

        let id = Self::envelope_id(&envelope);
        self.seen.insert(id, now);
        self.outbox.push(envelope);
    }

    /// Drain the outbox, returning envelopes to send
    pub fn drain_outbox(&mut self) -> Vec<PropagationEnvelope> {
        std::mem::take(&mut self.outbox)
    }

    /// Process an incoming envelope from a peer.
    /// Returns true if this is new (not seen before) and should be further propagated.
    pub fn receive(&mut self, envelope: &PropagationEnvelope) -> bool {
        // Reject if hop limit exceeded
        if envelope.hop_count >= envelope.max_hops {
            return false;
        }

        let id = Self::envelope_id(envelope);

        // Reject if already seen
        if self.seen.contains_key(&id) {
            return false;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.seen.insert(id, now);

        // Re-enqueue for further propagation based on strategy
        match &self.strategy {
            PropagationStrategy::Broadcast => {
                let mut forwarded = envelope.clone();
                forwarded.hop_count += 1;
                self.outbox.push(forwarded);
            }
            PropagationStrategy::TrustGated { .. } => {
                // Trust filtering happens at the send layer; here we just forward
                let mut forwarded = envelope.clone();
                forwarded.hop_count += 1;
                self.outbox.push(forwarded);
            }
            PropagationStrategy::Gossip { rounds, .. } => {
                // Only re-propagate if we haven't exceeded gossip rounds
                let effective_rounds = *rounds as u32;
                if envelope.hop_count + 1 < effective_rounds.min(envelope.max_hops) {
                    let mut forwarded = envelope.clone();
                    forwarded.hop_count += 1;
                    self.outbox.push(forwarded);
                }
            }
            PropagationStrategy::Targeted { .. } => {
                // Targeted envelopes are not further propagated by intermediate nodes
            }
        }

        true
    }

    /// Evict old entries from the seen set to prevent unbounded growth.
    /// Removes entries older than `max_age_ms` milliseconds.
    pub fn gc_seen(&mut self, max_age_ms: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.seen.retain(|_, ts| now.saturating_sub(*ts) < max_age_ms);
    }

    /// Get the envelope ID for dedup (origin + timestamp + payload discriminant hash)
    fn envelope_id(envelope: &PropagationEnvelope) -> String {
        let mut hasher = DefaultHasher::new();
        envelope.origin.hash(&mut hasher);
        envelope.timestamp.hash(&mut hasher);
        // Discriminant tag for payload type
        let tag = match &envelope.payload {
            PropagationPayload::BeliefUpdate(b) => format!("belief:{}", b.id),
            PropagationPayload::SkillUpdate(s) => format!("skill:{}", s.id),
            PropagationPayload::TrustUpdate { from, to, .. } => {
                format!("trust:{}:{}", from, to)
            }
            PropagationPayload::WorldCheckpoint { tick, .. } => {
                format!("world:{}", tick)
            }
            PropagationPayload::CouncilDecision { id, .. } => {
                format!("council:{}", id)
            }
        };
        tag.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine(strategy: PropagationStrategy) -> PropagationEngine {
        PropagationEngine::new("sys-local".to_string(), strategy)
    }

    fn dummy_belief() -> Belief {
        Belief {
            id: 1,
            content: "test belief".to_string(),
            confidence: 0.9,
            source: crate::hyper_stigmergy::BeliefSource::Observation,
            supporting_evidence: vec![],
            contradicting_evidence: vec![],
            created_at: 100,
            updated_at: 100,
            update_count: 0,
            abstract_l0: None,
            overview_l1: None,
            owner_namespace: None,
            supersedes_belief_id: None,
            evidence_belief_ids: Vec::new(),
            human_committed: false,
        }
    }

    #[test]
    fn enqueue_and_drain() {
        let mut engine = make_engine(PropagationStrategy::Broadcast);
        engine.enqueue(PropagationPayload::BeliefUpdate(dummy_belief()));
        let envelopes = engine.drain_outbox();
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].hop_count, 0);
        assert_eq!(envelopes[0].origin, "sys-local");
        // Outbox should be empty after drain
        assert!(engine.drain_outbox().is_empty());
    }

    #[test]
    fn receive_dedup() {
        let mut engine = make_engine(PropagationStrategy::Broadcast);
        let envelope = PropagationEnvelope {
            origin: "sys-remote".to_string(),
            hop_count: 0,
            max_hops: 5,
            timestamp: 12345,
            payload: PropagationPayload::BeliefUpdate(dummy_belief()),
            signature: None,
        };
        assert!(engine.receive(&envelope));
        // Same envelope again should be rejected
        assert!(!engine.receive(&envelope));
    }

    #[test]
    fn receive_hop_limit() {
        let mut engine = make_engine(PropagationStrategy::Broadcast);
        let envelope = PropagationEnvelope {
            origin: "sys-remote".to_string(),
            hop_count: 5,
            max_hops: 5,
            timestamp: 99999,
            payload: PropagationPayload::WorldCheckpoint {
                tick: 10,
                coherence: 0.8,
                edge_count: 42,
            },
            signature: None,
        };
        assert!(!engine.receive(&envelope));
    }

    #[test]
    fn targeted_no_forward() {
        let mut engine = make_engine(PropagationStrategy::Targeted {
            targets: vec!["sys-a".to_string()],
        });
        let envelope = PropagationEnvelope {
            origin: "sys-remote".to_string(),
            hop_count: 0,
            max_hops: 5,
            timestamp: 55555,
            payload: PropagationPayload::TrustUpdate {
                from: "a".to_string(),
                to: "b".to_string(),
                score: 0.7,
            },
            signature: None,
        };
        engine.receive(&envelope);
        // Targeted strategy should not re-enqueue for forwarding
        assert!(engine.drain_outbox().is_empty());
    }
}
