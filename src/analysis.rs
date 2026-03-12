use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;
use std::collections::HashMap;

pub struct HypergraphAnalysis;

impl HypergraphAnalysis {
    pub fn degree_centrality(world: &HyperStigmergicMorphogenesis) -> HashMap<u64, usize> {
        let mut centrality = HashMap::new();
        for agent in &world.agents {
            let degree = world
                .adjacency
                .get(&agent.id)
                .map(|edges| edges.len())
                .unwrap_or(0);
            centrality.insert(agent.id, degree);
        }
        centrality
    }
}

#[derive(Clone, Debug)]
pub struct DensityMetrics {
    pub overall_density: f64,
    pub emergent_ratio: f64,
    pub avg_hyperedge_size: f64,
}
