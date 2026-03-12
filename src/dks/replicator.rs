//! Replicator entities - the fundamental units of DKS.
//!
//! Replicators are entities that:
//! - Consume energy to maintain structure
//! - Replicate when energy exceeds thresholds
//! - Decay spontaneously (thermodynamic reality)
//! - Inherit and mutate parameters

use super::EntityId;
use serde::{Deserialize, Serialize};

/// A self-replicating entity in the DKS system
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replicator {
    id: EntityId,
    /// Generation number (0 = seed, increases with each replication)
    generation: u64,
    /// Current energy level
    energy: f64,
    /// Rate at which this entity produces offspring
    replication_rate: f64,
    /// Rate at which this entity loses energy/structure
    decay_rate: f64,
    /// State of this entity
    state: ReplicatorState,
    /// Metabolism configuration
    metabolism: Metabolism,
    /// Accumulated persistence score
    persistence_score: f64,
    /// Time since last replication
    ticks_since_replication: u64,
    /// Parent entity ID (None for seed entities)
    parent_id: Option<EntityId>,
}

impl Replicator {
    pub fn new(_name: String, replication_rate: f64, decay_rate: f64) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            generation: 0,
            energy: 50.0, // Starting energy
            replication_rate: replication_rate.clamp(0.0, 1.0),
            decay_rate: decay_rate.clamp(0.0, 1.0),
            state: ReplicatorState::Active,
            metabolism: Metabolism::default(),
            persistence_score: 0.0,
            ticks_since_replication: 0,
            parent_id: None,
        }
    }

    /// Create offspring with inherited (and possibly mutated) traits
    pub fn replicate(&self) -> Option<Self> {
        if self.state != ReplicatorState::Active {
            return None;
        }

        // Mutation rates
        let mutation_rate = 0.05;

        let child = Self {
            id: uuid::Uuid::new_v4(),
            generation: self.generation + 1,
            energy: 25.0, // Offspring starts with half energy
            replication_rate: self.mutate_trait(self.replication_rate, mutation_rate),
            decay_rate: self.mutate_trait(self.decay_rate, mutation_rate),
            state: ReplicatorState::Active,
            metabolism: self.metabolism.clone(),
            persistence_score: 0.0,
            ticks_since_replication: 0,
            parent_id: Some(self.id),
        };

        Some(child)
    }

    /// Apply decay (energy loss)
    pub fn decay(&mut self) {
        let decay_amount = self.energy * self.decay_rate;
        self.energy -= decay_amount;

        if self.energy <= 0.0 {
            self.energy = 0.0;
            self.state = ReplicatorState::Decayed;
        }
    }

    /// Metabolize resources from environment into energy
    pub fn metabolize(&mut self, available_resources: f64, conversion_rate: f64) {
        if self.state != ReplicatorState::Active {
            return;
        }

        let metabolic_efficiency = self.metabolism.efficiency;
        let energy_gained = available_resources * conversion_rate * metabolic_efficiency;
        self.energy += energy_gained;

        // Cap energy at max storage
        self.energy = self.energy.min(self.metabolism.max_energy_storage);
    }

    /// Calculate if this entity should replicate this tick
    pub fn should_replicate(&self, energy_cost: f64) -> bool {
        if self.state != ReplicatorState::Active {
            return false;
        }

        // Need sufficient energy
        if self.energy < energy_cost * 2.0 {
            return false;
        }

        // Probabilistic replication based on rate
        let roll: f64 = rand::random();
        roll < self.replication_rate
    }

    /// Deduct replication cost
    pub fn pay_replication_cost(&mut self, cost: f64) {
        self.energy -= cost;
        self.ticks_since_replication = 0;
    }

    /// Update persistence score (called each tick)
    pub fn update_persistence(&mut self) {
        if self.state == ReplicatorState::Active {
            self.persistence_score += 1.0;
            self.ticks_since_replication += 1;

            // Bonus for maintaining high energy
            if self.energy > 50.0 {
                self.persistence_score += 0.1;
            }
        }
    }

    /// Getters
    pub fn id(&self) -> EntityId {
        self.id
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn energy(&self) -> f64 {
        self.energy
    }
    pub fn replication_rate(&self) -> f64 {
        self.replication_rate
    }
    pub fn decay_rate(&self) -> f64 {
        self.decay_rate
    }
    pub fn persistence_score(&self) -> f64 {
        self.persistence_score
    }
    pub fn state(&self) -> &ReplicatorState {
        &self.state
    }
    pub fn parent_id(&self) -> Option<EntityId> {
        self.parent_id
    }
    pub fn metabolism(&self) -> &Metabolism {
        &self.metabolism
    }

    /// DKS stability metric for this entity
    pub fn dks_stability(&self) -> f64 {
        super::calculate_dks_stability(self.replication_rate, self.decay_rate)
    }

    fn mutate_trait(&self, value: f64, rate: f64) -> f64 {
        let mutation: f64 = (rand::random::<f64>() - 0.5) * 2.0 * rate;
        (value + mutation).clamp(0.01, 1.0)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ReplicatorState {
    Active,
    Dormant,
    Decayed,
}

/// Metabolism configuration for a replicator
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metabolism {
    /// Efficiency of converting resources to energy (0-1)
    pub efficiency: f64,
    /// Maximum energy storage capacity
    pub max_energy_storage: f64,
    /// Resource types this entity can metabolize
    pub preferred_resources: Vec<Resource>,
}

impl Default for Metabolism {
    fn default() -> Self {
        Self {
            efficiency: 0.5,
            max_energy_storage: 100.0,
            preferred_resources: vec![Resource::Energy],
        }
    }
}

/// Available resource types
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Resource {
    Energy,
    Information,
    Matter,
    Compute,
    Network,
}
