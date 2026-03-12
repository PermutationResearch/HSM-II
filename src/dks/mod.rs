//! Dynamic Kinetic Stability (DKS) - Self-replicating entities driven by persistence.
//!
//! DKS implements entities that:
//! 1. Self-replicate using available energy/resources
//! 2. Exist far from thermodynamic equilibrium
//! 3. Are selected based on persistence (stability), not fitness
//! 4. Generate complexity through non-equilibrium dynamics
//!
//! Integration with RooDB persists entity states and population metrics.
//! LARS cascades can trigger DKS evolution cycles.

use serde::{Deserialize, Serialize};

pub mod flux;
pub mod multifractal;
pub mod population;
pub mod replicator;
pub mod selection;
pub mod stigmergic_entity;

pub use flux::{Environment, Flux, FluxType};
pub use multifractal::{compositionality_measure, MultifractalSpectrum, MultiscaleDKS};
pub use population::{EvolutionParameters, Population, PopulationStats};
pub use replicator::{Metabolism, Replicator, ReplicatorState, Resource};
pub use selection::{PersistenceMeasure, SelectionEvent, SelectionPressure};
pub use stigmergic_entity::{
    CognitiveState, FieldReading, StigmergicAction, StigmergicDKS, StigmergicEdgeType,
    StigmergicEntity, StigmergicMemory, StigmergicPattern, StigmergicPopulation, StigmergicStats,
    StigmergicTickResult,
};

use crate::metrics_dks_ext::AdaptiveCoupling;

/// Unique identifier for a DKS entity
pub type EntityId = uuid::Uuid;

/// Configuration for a DKS system
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DKSConfig {
    /// Base replication rate
    pub base_replication_rate: f64,
    /// Base decay rate
    pub base_decay_rate: f64,
    /// Energy cost per replication
    pub replication_energy_cost: f64,
    /// Energy gain per resource unit
    pub resource_energy_conversion: f64,
    /// Maximum population size
    pub max_population: usize,
    /// Selection pressure intensity
    pub selection_intensity: f64,
    /// Environmental fluctuation rate
    pub flux_rate: f64,
}

impl Default for DKSConfig {
    fn default() -> Self {
        Self {
            base_replication_rate: 0.1,
            base_decay_rate: 0.05,
            replication_energy_cost: 10.0,
            resource_energy_conversion: 1.0,
            max_population: 1000,
            selection_intensity: 0.3,
            flux_rate: 0.01,
        }
    }
}

/// Main DKS system managing entities, environment, and evolution
#[derive(Clone, Debug)]
pub struct DKSSystem {
    config: DKSConfig,
    population: Population,
    environment: Environment,
    flux: Flux,
    selection: SelectionPressure,
    generation: u64,
    /// Adaptive ecological coupling coefficient (ηC) with online learning
    pub(crate) adaptive_coupling: AdaptiveCoupling,
}

impl DKSSystem {
    pub fn new(config: DKSConfig) -> Self {
        let population = Population::new(config.max_population);
        let environment = Environment::default();
        let flux = Flux::new(config.flux_rate);
        let selection = SelectionPressure::new(config.selection_intensity);

        Self {
            config,
            population,
            environment,
            flux,
            selection,
            generation: 0,
            adaptive_coupling: AdaptiveCoupling::new(),
        }
    }

    /// Initialize with seed entities
    pub fn seed(&mut self, count: usize) {
        for i in 0..count {
            let replicator = Replicator::new(
                format!("seed_{}", i),
                self.config.base_replication_rate,
                self.config.base_decay_rate,
            );
            self.population.add_entity(replicator);
        }
    }

    /// Execute one generation of the DKS cycle
    pub fn tick(&mut self) -> DKSTickResult {
        self.generation += 1;

        // 1. Apply environmental flux (energy/resource changes)
        self.flux.apply(&mut self.environment);

        // 2. Entities metabolize resources
        self.population
            .metabolize(&self.environment, self.config.resource_energy_conversion);

        // 3. Entities replicate if they have sufficient energy
        let new_entities = self.population.replicate(
            self.config.replication_energy_cost,
            self.config.max_population,
        );

        // 4. Natural decay
        self.population.decay();

        // 5. Selection based on persistence
        let selected = self
            .selection
            .select(&mut self.population, &self.environment);

        DKSTickResult {
            generation: self.generation,
            new_entities,
            decayed_entities: 0, // Tracked internally
            selected_entities: selected,
            population_size: self.population.size(),
            total_energy: self.population.total_energy(),
            average_persistence: self.population.average_persistence(),
        }
    }

    /// Run evolution for multiple generations
    pub fn evolve(&mut self, generations: usize) -> Vec<DKSTickResult> {
        let mut results = Vec::with_capacity(generations);
        for _ in 0..generations {
            results.push(self.tick());
        }
        results
    }

    /// Get current population statistics
    pub fn stats(&self) -> PopulationStats {
        self.population.stats()
    }

    /// Current generation index.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Persistence values for current entities (non-negative, epsilon-shifted).
    pub fn persistence_values(&self) -> Vec<f64> {
        self.population
            .entities()
            .iter()
            .map(|e| e.persistence_score().max(0.0) + 1e-9)
            .collect()
    }

    /// Compute multifractal spectrum points (alpha, f(alpha)) from persistence.
    pub fn multifractal_spectrum_points(&self, max_points: usize) -> Vec<(f64, f64)> {
        let values = self.persistence_values();
        if values.len() < 4 {
            let p = self.population.average_persistence();
            return vec![(0.5, p)];
        }

        let mut box_sizes = vec![1usize, 2, 4, 8, 16, 32, 64];
        box_sizes.retain(|b| *b <= values.len());
        if box_sizes.is_empty() {
            box_sizes.push(1);
        }

        let spectrum = MultifractalSpectrum::from_persistence_distribution(&values, &box_sizes);
        let mut points: Vec<(f64, f64)> = spectrum
            .alpha_values
            .iter()
            .copied()
            .zip(spectrum.fractal_dimensions.iter().copied())
            .filter(|(a, f)| a.is_finite() && f.is_finite())
            .collect();

        points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if points.is_empty() {
            return vec![(0.5, self.population.average_persistence())];
        }

        let cap = max_points.max(1);
        if points.len() <= cap {
            return points;
        }

        let stride = ((points.len() as f64) / (cap as f64)).ceil() as usize;
        points.into_iter().step_by(stride).take(cap).collect()
    }

    /// Get reference to population for testing
    pub fn population(&self) -> &Population {
        &self.population
    }

    /// Persist current state to database
    pub async fn persist(&self, db: &crate::database::RooDb) -> anyhow::Result<()> {
        let timestamp = current_timestamp();

        // Save population snapshot
        let sql = format!(
            "INSERT INTO dks_snapshots (timestamp, generation, population_size, total_energy, avg_persistence) 
             VALUES ({}, {}, {}, {}, {})",
            timestamp,
            self.generation,
            self.population.size(),
            self.population.total_energy(),
            self.population.average_persistence()
        );
        db.execute(&sql).await?;

        // Save entity states
        for entity in self.population.entities() {
            let entity_sql = format!(
                "INSERT INTO dks_entities (snapshot_time, entity_id, generation, energy, 
                 replication_rate, decay_rate, persistence_score) 
                 VALUES ({}, '{}', {}, {}, {}, {}, {})",
                timestamp,
                entity.id(),
                entity.generation(),
                entity.energy(),
                entity.replication_rate(),
                entity.decay_rate(),
                entity.persistence_score()
            );
            db.execute(&entity_sql).await?;
        }

        Ok(())
    }

    /// Initialize DKS database schema
    pub async fn init_schema(db: &crate::database::RooDb) -> anyhow::Result<()> {
        let sql = r#"
            CREATE TABLE IF NOT EXISTS dks_snapshots (
                id BIGINT AUTO_INCREMENT PRIMARY KEY,
                timestamp BIGINT NOT NULL,
                generation BIGINT NOT NULL,
                population_size INT NOT NULL,
                total_energy DOUBLE NOT NULL,
                avg_persistence DOUBLE NOT NULL,
                INDEX idx_timestamp (timestamp),
                INDEX idx_generation (generation)
            );
            
            CREATE TABLE IF NOT EXISTS dks_entities (
                id BIGINT AUTO_INCREMENT PRIMARY KEY,
                snapshot_time BIGINT NOT NULL,
                entity_id VARCHAR(36) NOT NULL,
                generation BIGINT NOT NULL,
                energy DOUBLE NOT NULL,
                replication_rate DOUBLE NOT NULL,
                decay_rate DOUBLE NOT NULL,
                persistence_score DOUBLE NOT NULL,
                INDEX idx_snapshot (snapshot_time),
                INDEX idx_entity (entity_id)
            )
        "#;
        db.execute(sql).await?;
        Ok(())
    }
}

/// Result of a single DKS tick/generation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DKSTickResult {
    pub generation: u64,
    pub new_entities: usize,
    pub decayed_entities: usize,
    pub selected_entities: usize,
    pub population_size: usize,
    pub total_energy: f64,
    pub average_persistence: f64,
}

/// DKS stability metric: replication_rate / (decay_rate + ε) - 1
///
/// Positive values indicate sustainable self-replication
/// Negative values indicate population decline
/// Zero indicates equilibrium (DKS state)
pub fn calculate_dks_stability(replication_rate: f64, decay_rate: f64) -> f64 {
    replication_rate / (decay_rate + 1e-10) - 1.0
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
