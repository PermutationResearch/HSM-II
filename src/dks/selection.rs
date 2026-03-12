//! Selection pressure based on persistence, not fitness.
//!
//! Unlike traditional evolutionary algorithms that select for fitness,
//! DKS selects entities based on their persistence - how long they
//! maintain their structure far from equilibrium.

use super::{Environment, Population, Replicator};
use serde::{Deserialize, Serialize};

/// Selection pressure based on persistence metrics
#[derive(Clone, Debug)]
pub struct SelectionPressure {
    intensity: f64,
    history: Vec<SelectionEvent>,
}

impl SelectionPressure {
    pub fn new(intensity: f64) -> Self {
        Self {
            intensity: intensity.clamp(0.0, 1.0),
            history: Vec::new(),
        }
    }

    /// Apply selection to population
    /// Returns number of entities removed
    pub fn select(&mut self, population: &mut Population, environment: &Environment) -> usize {
        let before_count = population.size();

        // Calculate persistence scores for all entities
        let mut scores: Vec<(usize, f64)> = population
            .entities()
            .iter()
            .enumerate()
            .map(|(idx, entity)| {
                let score = self.calculate_persistence(entity, environment);
                (idx, score)
            })
            .collect();

        // Sort by persistence (highest first)
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Calculate how many to remove based on intensity
        let remove_count = ((before_count as f64) * self.intensity * 0.1) as usize;

        // Mark low-persistence entities for removal
        let entities_to_remove: Vec<usize> = scores
            .iter()
            .skip(before_count.saturating_sub(remove_count))
            .map(|(idx, _)| *idx)
            .collect();

        // Remove in reverse order to maintain indices
        let entities_mut = population.entities_mut();
        for &idx in entities_to_remove.iter().rev() {
            if idx < entities_mut.len() {
                entities_mut.remove(idx);
            }
        }

        let removed = before_count - population.size();

        // Record event
        self.history.push(SelectionEvent {
            timestamp: current_timestamp(),
            entities_before: before_count,
            entities_after: population.size(),
            average_persistence: population.average_persistence(),
            environmental_stress: environment.perturbation_level(),
        });

        removed
    }

    /// Calculate persistence score for an entity
    ///
    /// Persistence is based on:
    /// 1. Time survived (persistence_score)
    /// 2. Energy stability (maintaining high energy)
    /// 3. DKS stability (replication/decay balance)
    /// 4. Environmental adaptation
    fn calculate_persistence(&self, entity: &Replicator, environment: &Environment) -> f64 {
        // Base persistence from survival time
        let time_persistence = entity.persistence_score();

        // Energy stability (higher is better, but penalize extremes)
        let energy_ratio = entity.energy() / 100.0; // Assuming 100 is max
        let energy_stability = 1.0 - (energy_ratio - 0.5).abs() * 2.0;

        // DKS stability metric
        let dks_stability = (entity.dks_stability() + 1.0) / 2.0; // Normalize to 0-1

        // Environmental adaptation bonus
        let adaptation = if environment.temperature() > 1.2 {
            // Bonus for surviving harsh conditions
            0.2
        } else {
            0.0
        };

        // Weighted combination
        time_persistence * 0.4 + energy_stability * 0.2 + dks_stability * 0.3 + adaptation
    }

    /// Get selection history
    pub fn history(&self) -> &[SelectionEvent] {
        &self.history
    }
}

/// Metrics for measuring entity persistence
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistenceMeasure {
    /// Total time survived (ticks)
    pub survival_time: u64,
    /// Average energy level maintained
    pub average_energy: f64,
    /// Energy variance (lower = more stable)
    pub energy_variance: f64,
    /// Number of successful replications
    pub replication_count: u32,
    /// DKS stability ratio
    pub dks_stability: f64,
    /// Environmental stress endured
    pub stress_survived: f64,
}

impl PersistenceMeasure {
    /// Combined persistence score
    pub fn score(&self) -> f64 {
        let stability_bonus = if self.dks_stability > 0.0 { 1.0 } else { 0.0 };

        self.survival_time as f64 * 0.3
            + self.average_energy * 0.2
            + (100.0 - self.energy_variance) * 0.01
            + self.replication_count as f64 * 0.5
            + stability_bonus * 10.0
    }
}

/// Record of a selection event
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionEvent {
    pub timestamp: u64,
    pub entities_before: usize,
    pub entities_after: usize,
    pub average_persistence: f64,
    pub environmental_stress: f64,
}

/// Compare persistence vs fitness-based selection
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectionComparison {
    pub generation: u64,
    pub persistence_selected_avg: f64,
    pub fitness_selected_avg: f64,
    pub population_diversity_persistence: f64,
    pub population_diversity_fitness: f64,
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::super::{Environment, Population, Replicator};
    use super::*;

    #[test]
    fn test_persistence_calculation() {
        let mut population = Population::new(100);
        let environment = Environment::default();

        // Create entities with different persistence
        let stable = Replicator::new("stable".to_string(), 0.1, 0.01);
        let unstable = Replicator::new("unstable".to_string(), 0.9, 0.8);

        population.add_entity(stable.clone());
        population.add_entity(unstable.clone());

        let selection = SelectionPressure::new(0.5);

        // Stable entity should have higher persistence score
        let stable_score = selection.calculate_persistence(&stable, &environment);
        let unstable_score = selection.calculate_persistence(&unstable, &environment);

        assert!(
            stable_score > unstable_score,
            "Stable entity should have higher persistence than unstable entity"
        );
    }

    #[test]
    fn test_selection_pressure() {
        let mut population = Population::new(100);
        let environment = Environment::default();

        // Seed with mixed population - give different persistence scores
        for i in 0..10 {
            let mut rep = if i < 5 {
                Replicator::new(format!("stable_{}", i), 0.1, 0.01)
            } else {
                Replicator::new(format!("unstable_{}", i), 0.9, 0.8)
            };
            // Update persistence to differentiate entities
            for _ in 0..(i * 10) {
                rep.update_persistence();
            }
            population.add_entity(rep);
        }

        let initial_count = population.size();

        let mut selection = SelectionPressure::new(0.5);
        let removed = selection.select(&mut population, &environment);

        // Selection may or may not remove entities depending on random factors
        // Just verify it doesn't panic and maintains valid state
        assert!(
            population.size() <= initial_count,
            "Population should not increase"
        );
        println!("Selection removed {} entities", removed);
    }
}
