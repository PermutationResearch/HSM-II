use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct ExperimentStats {
    pub final_coherence: f64,
    pub emergent_edge_count: usize,
    pub total_edges: usize,
    pub ontology_concepts: usize,
    pub dynamic_concepts: usize,
    pub property_diversity: usize,
    pub embedding_coverage: f32,
    pub coherence_trajectory: Vec<f64>,
    pub clustering_coefficient: f64,
    pub avg_path_length: f64,
}

pub struct ExperimentConfig {
    pub initial_agents: usize,
    pub drive_bias: HashMap<String, f32>,
    pub ticks: usize,
    pub decay_rate: f64,
    pub seed: Option<u64>,
}

pub struct ExperimentHarness;

impl ExperimentHarness {
    pub fn run_variant(config: ExperimentConfig) -> ExperimentStats {
        let mut world = HyperStigmergicMorphogenesis::new(config.initial_agents);
        world.bias_drives(&config.drive_bias);
        world.decay_rate = config.decay_rate;

        let mut coherence_trajectory = Vec::with_capacity(config.ticks);

        for _ in 0..config.ticks {
            world.tick();
            coherence_trajectory.push(world.global_coherence());
        }

        ExperimentStats {
            final_coherence: world.global_coherence(),
            emergent_edge_count: world.emergent_edge_count(),
            total_edges: world.edges.len(),
            ontology_concepts: world.ontology.len(),
            dynamic_concepts: world.dynamic_concepts.len(),
            property_diversity: world.property_vertices.len(),
            embedding_coverage: world.calculate_embedding_coverage(),
            coherence_trajectory,
            clustering_coefficient: world.calculate_clustering_coefficient(),
            avg_path_length: 0.0,
        }
    }

    pub fn compare_variants(
        configs: Vec<(String, ExperimentConfig)>,
    ) -> Vec<(String, ExperimentStats)> {
        configs
            .into_iter()
            .map(|(name, config)| (name, Self::run_variant(config)))
            .collect()
    }
}
