//! Environmental flux - non-equilibrium energy and resource dynamics.
//!
//! Flux drives the system away from thermodynamic equilibrium,
//! enabling the emergence of complex, self-organizing structures.

use super::{Replicator, Resource};
use serde::{Deserialize, Serialize};

/// Environmental flux - drives non-equilibrium dynamics
#[derive(Clone, Debug)]
pub struct Flux {
    rate: f64,
    current_cycle: u64,
}

impl Flux {
    pub fn new(rate: f64) -> Self {
        Self {
            rate: rate.clamp(0.0, 1.0),
            current_cycle: 0,
        }
    }

    /// Apply flux to the environment
    pub fn apply(&mut self, environment: &mut Environment) {
        self.current_cycle += 1;

        // Periodic energy influx
        let energy_pulse =
            100.0 * self.rate * (1.0 + 0.3 * (self.current_cycle as f64 / 10.0).sin());
        environment.add_energy(energy_pulse);

        // Random resource distribution
        let resource_types = [
            Resource::Energy,
            Resource::Information,
            Resource::Matter,
            Resource::Compute,
        ];
        for resource in &resource_types {
            if rand::random::<f64>() < self.rate {
                environment.distribute_resource(resource.clone(), 50.0 * self.rate);
            }
        }

        // Occasional shocks (environmental perturbations)
        if rand::random::<f64>() < self.rate * 0.1 {
            environment.apply_perturbation();
        }
    }
}

/// Types of environmental flux
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FluxType {
    /// Constant energy input
    Constant { rate: f64 },
    /// Periodic oscillation
    Periodic { amplitude: f64, period: u64 },
    /// Stochastic pulses
    Stochastic { mean: f64, variance: f64 },
    /// Punctuated equilibrium (rare large events)
    Punctuated {
        base_rate: f64,
        shock_probability: f64,
        shock_magnitude: f64,
    },
}

impl FluxType {
    /// Calculate current flux value at time t
    pub fn value(&self, t: u64) -> f64 {
        match self {
            FluxType::Constant { rate } => *rate,
            FluxType::Periodic { amplitude, period } => {
                let phase = (t % period) as f64 / *period as f64;
                amplitude * (phase * 2.0 * std::f64::consts::PI).sin()
            }
            FluxType::Stochastic { mean, variance } => {
                // Simple pseudo-random based on time
                let noise =
                    ((t.wrapping_mul(1103515245).wrapping_add(12345) % 1000) as f64 / 1000.0 - 0.5)
                        * 2.0;
                mean + noise * variance.sqrt()
            }
            FluxType::Punctuated {
                base_rate,
                shock_probability,
                shock_magnitude,
            } => {
                let shock_roll =
                    (t.wrapping_mul(1103515245).wrapping_add(12345) % 1000) as f64 / 1000.0;
                if shock_roll < *shock_probability {
                    base_rate + shock_magnitude
                } else {
                    *base_rate
                }
            }
        }
    }
}

/// Environment containing resources and energy
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Environment {
    available_energy: f64,
    resources: std::collections::HashMap<Resource, f64>,
    temperature: f64, // Affects reaction rates
    perturbation_level: f64,
}

impl Default for Environment {
    fn default() -> Self {
        let mut resources = std::collections::HashMap::new();
        resources.insert(Resource::Energy, 100.0);
        resources.insert(Resource::Information, 50.0);
        resources.insert(Resource::Matter, 200.0);

        Self {
            available_energy: 100.0,
            resources,
            temperature: 1.0,
            perturbation_level: 0.0,
        }
    }
}

impl Environment {
    /// Get available resources for a specific entity
    pub fn available_resources(&self, entity: &Replicator) -> f64 {
        // Match entity's preferred resources to available resources
        let metabolism = entity.metabolism();
        let mut total = 0.0;

        for resource in &metabolism.preferred_resources {
            if let Some(&amount) = self.resources.get(resource) {
                total += amount;
            }
        }

        total * self.temperature // Temperature affects availability
    }

    /// Consume resources
    pub fn consume_resources(&mut self, resource: &Resource, amount: f64) {
        if let Some(available) = self.resources.get_mut(resource) {
            *available = (*available - amount).max(0.0);
        }
    }

    /// Add energy to environment
    pub fn add_energy(&mut self, amount: f64) {
        self.available_energy += amount;
    }

    /// Distribute resource into environment
    pub fn distribute_resource(&mut self, resource: Resource, amount: f64) {
        *self.resources.entry(resource).or_insert(0.0) += amount;
    }

    /// Apply environmental perturbation
    pub fn apply_perturbation(&mut self) {
        self.perturbation_level = 1.0;
        self.temperature *= 1.5; // Heat up

        // Reduce some resources
        for (_, amount) in self.resources.iter_mut() {
            *amount *= 0.8;
        }
    }

    /// Decay perturbation over time
    pub fn decay_perturbation(&mut self) {
        self.perturbation_level *= 0.9;
        self.temperature = 1.0 + (self.temperature - 1.0) * 0.95;
    }

    /// Get total available resources
    pub fn total_resources(&self) -> f64 {
        self.resources.values().sum()
    }

    pub fn temperature(&self) -> f64 {
        self.temperature
    }
    pub fn perturbation_level(&self) -> f64 {
        self.perturbation_level
    }
}
