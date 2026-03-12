//! Population management for DKS entities.
//!
//! Handles entity lifecycle, reproduction, and population-level statistics.

use super::{Environment, Replicator};
use serde::{Deserialize, Serialize};

/// A population of replicator entities
#[derive(Clone, Debug)]
pub struct Population {
    entities: Vec<Replicator>,
    max_size: usize,
    _generation_count: u64,
}

impl Population {
    pub fn new(max_size: usize) -> Self {
        Self {
            entities: Vec::new(),
            max_size,
            _generation_count: 0,
        }
    }

    /// Add an entity to the population
    pub fn add_entity(&mut self, entity: Replicator) {
        if self.entities.len() < self.max_size {
            self.entities.push(entity);
        }
    }

    /// Get all entities (immutable)
    pub fn entities(&self) -> &[Replicator] {
        &self.entities
    }

    /// Get mutable access to entities
    pub fn entities_mut(&mut self) -> &mut Vec<Replicator> {
        &mut self.entities
    }

    /// Current population size
    pub fn size(&self) -> usize {
        self.entities.len()
    }

    /// Total energy in population
    pub fn total_energy(&self) -> f64 {
        self.entities.iter().map(|e| e.energy()).sum()
    }

    /// Average persistence score
    pub fn average_persistence(&self) -> f64 {
        if self.entities.is_empty() {
            return 0.0;
        }
        self.entities
            .iter()
            .map(|e| e.persistence_score())
            .sum::<f64>()
            / self.entities.len() as f64
    }

    /// Run metabolism for all entities
    pub fn metabolize(&mut self, environment: &Environment, conversion_rate: f64) {
        for entity in &mut self.entities {
            // Get available resources based on entity's metabolism
            let resources = environment.available_resources(entity);
            entity.metabolize(resources, conversion_rate);
        }
    }

    /// Handle replication across population
    pub fn replicate(&mut self, energy_cost: f64, max_population: usize) -> usize {
        let mut new_entities = Vec::new();
        let mut current_size = self.entities.len();

        for entity in &mut self.entities {
            entity.update_persistence();

            if entity.should_replicate(energy_cost) {
                if let Some(child) = entity.replicate() {
                    entity.pay_replication_cost(energy_cost);
                    new_entities.push(child);
                    current_size += 1;

                    // Stop if at capacity
                    if current_size >= max_population {
                        break;
                    }
                }
            }
        }

        let count = new_entities.len();
        self.entities.extend(new_entities);

        // Enforce max population (remove lowest persistence if over)
        if self.entities.len() > max_population {
            // Sort by persistence, keep top max_population
            self.entities.sort_by(|a, b| {
                b.persistence_score()
                    .partial_cmp(&a.persistence_score())
                    .unwrap()
            });
            self.entities.truncate(max_population);
        }

        count
    }

    /// Apply decay to all entities
    pub fn decay(&mut self) {
        for entity in &mut self.entities {
            entity.decay();
        }

        // Remove decayed entities
        self.entities
            .retain(|e| e.state() != &super::replicator::ReplicatorState::Decayed);
    }

    /// Get population statistics
    pub fn stats(&self) -> PopulationStats {
        if self.entities.is_empty() {
            return PopulationStats::default();
        }

        let total_energy: f64 = self.entities.iter().map(|e| e.energy()).sum();
        let total_persistence: f64 = self.entities.iter().map(|e| e.persistence_score()).sum();
        let avg_replication: f64 = self
            .entities
            .iter()
            .map(|e| e.replication_rate())
            .sum::<f64>()
            / self.entities.len() as f64;
        let avg_decay: f64 =
            self.entities.iter().map(|e| e.decay_rate()).sum::<f64>() / self.entities.len() as f64;

        // Count by generation
        let mut generation_counts: std::collections::HashMap<u64, usize> =
            std::collections::HashMap::new();
        for entity in &self.entities {
            *generation_counts.entry(entity.generation()).or_insert(0) += 1;
        }

        PopulationStats {
            size: self.entities.len(),
            total_energy,
            average_energy: total_energy / self.entities.len() as f64,
            average_persistence: total_persistence / self.entities.len() as f64,
            average_replication_rate: avg_replication,
            average_decay_rate: avg_decay,
            generation_counts,
            max_generation: self
                .entities
                .iter()
                .map(|e| e.generation())
                .max()
                .unwrap_or(0),
        }
    }
}

/// Population-level statistics
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PopulationStats {
    pub size: usize,
    pub total_energy: f64,
    pub average_energy: f64,
    pub average_persistence: f64,
    pub average_replication_rate: f64,
    pub average_decay_rate: f64,
    pub generation_counts: std::collections::HashMap<u64, usize>,
    pub max_generation: u64,
}

/// Parameters controlling evolution dynamics
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvolutionParameters {
    /// Base mutation rate for replication parameters
    pub mutation_rate: f64,
    /// Selection strength (how strongly persistence affects survival)
    pub selection_strength: f64,
    /// Carrying capacity of environment
    pub carrying_capacity: usize,
    /// Energy influx rate
    pub energy_influx: f64,
}

impl Default for EvolutionParameters {
    fn default() -> Self {
        Self {
            mutation_rate: 0.05,
            selection_strength: 0.3,
            carrying_capacity: 1000,
            energy_influx: 100.0,
        }
    }
}
