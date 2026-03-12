use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::types::SystemId;
use crate::agent::Agent;

/// Directed weighted trust graph over federated system instances.
/// Trust scores gate import permissions and weight remote consensus votes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrustGraph {
    /// (from_system, to_system) -> trust edge
    pub edges: HashMap<(SystemId, SystemId), TrustEdge>,
    /// Default trust for unknown systems.
    pub default_trust: f64,
    /// Per-tick decay rate for trust scores.
    pub decay_rate: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrustEdge {
    pub score: f64,
    pub successful_imports: u64,
    pub failed_imports: u64,
    pub last_interaction: u64,
}

/// Policy thresholds for trust-gated operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrustPolicy {
    pub min_import_trust: f64,
    pub min_vote_trust: f64,
    pub promotion_trust: f64,
}

impl Default for TrustPolicy {
    fn default() -> Self {
        Self {
            min_import_trust: 0.3,
            min_vote_trust: 0.4,
            promotion_trust: 0.6,
        }
    }
}

impl Default for TrustGraph {
    fn default() -> Self {
        Self {
            edges: HashMap::new(),
            default_trust: 0.3,
            decay_rate: 0.001,
        }
    }
}

impl TrustGraph {
    pub fn new(default_trust: f64, decay_rate: f64) -> Self {
        Self {
            edges: HashMap::new(),
            default_trust,
            decay_rate,
        }
    }

    /// Get trust score from one system to another. Returns default if unknown.
    pub fn get_trust(&self, from: &SystemId, to: &SystemId) -> f64 {
        self.edges
            .get(&(from.clone(), to.clone()))
            .map(|e| e.score)
            .unwrap_or(self.default_trust)
    }

    /// Record a successful import from a remote system (Bayesian update).
    pub fn record_success(&mut self, from: &SystemId, to: &SystemId, tick: u64) {
        let edge = self
            .edges
            .entry((from.clone(), to.clone()))
            .or_insert_with(|| TrustEdge {
                score: self.default_trust,
                successful_imports: 0,
                failed_imports: 0,
                last_interaction: tick,
            });

        edge.successful_imports += 1;
        edge.last_interaction = tick;

        // Bayesian update: score = (successes + prior) / (successes + failures + 2*prior)
        let prior = 2.0;
        edge.score = (edge.successful_imports as f64 + prior)
            / (edge.successful_imports as f64 + edge.failed_imports as f64 + 2.0 * prior);
    }

    /// Record a failed import (rejected edge, conflict, etc.).
    pub fn record_failure(&mut self, from: &SystemId, to: &SystemId, tick: u64) {
        let edge = self
            .edges
            .entry((from.clone(), to.clone()))
            .or_insert_with(|| TrustEdge {
                score: self.default_trust,
                successful_imports: 0,
                failed_imports: 0,
                last_interaction: tick,
            });

        edge.failed_imports += 1;
        edge.last_interaction = tick;

        let prior = 2.0;
        edge.score = (edge.successful_imports as f64 + prior)
            / (edge.successful_imports as f64 + edge.failed_imports as f64 + 2.0 * prior);
    }

    /// Apply time-based decay to all trust scores (edges not recently active drift toward default).
    pub fn decay_all(&mut self, current_tick: u64) {
        for edge in self.edges.values_mut() {
            let age = current_tick.saturating_sub(edge.last_interaction);
            if age > 100 {
                // Slowly decay toward default
                let decay = self.decay_rate * (age as f64 / 100.0).min(5.0);
                let diff = edge.score - self.default_trust;
                edge.score -= diff * decay;
                edge.score = edge.score.clamp(0.0, 1.0);
            }
        }
    }

    /// Check if trust from one system to another meets a threshold.
    pub fn meets_threshold(&self, from: &SystemId, to: &SystemId, threshold: f64) -> bool {
        self.get_trust(from, to) >= threshold
    }

    /// Add a new peer with default trust.
    pub fn bootstrap_peer(&mut self, local_system: &SystemId, remote_system: &SystemId, tick: u64) {
        let key = (local_system.clone(), remote_system.clone());
        self.edges.entry(key).or_insert_with(|| TrustEdge {
            score: self.default_trust,
            successful_imports: 0,
            failed_imports: 0,
            last_interaction: tick,
        });
    }

    /// Apply re-specialization noise to agents when groupthink is detected.
    /// This is called by the CorrelationMonitor when bid correlation is too high.
    pub fn apply_respec_noise(agents: &mut [Agent]) {
        for agent in agents.iter_mut() {
            // Inject noise into bid_bias to break groupthink
            let noise = (rand::random::<f64>() - 0.5) * 0.4;
            agent.bid_bias = (agent.bid_bias + noise).clamp(0.1, 5.0);
        }
    }
}
