//! Semantic graph of skill relationships.
//!
//! Models skills as nodes and their relationships as edges,
//! enabling skill chaining and discovery.

use super::{RelatedSkill, SkillChain, SkillEmbedding};
use crate::skill::Skill;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Graph representing semantic relationships between skills
pub struct SemanticGraph {
    nodes: HashMap<String, SkillNode>,
    edges: Vec<SemanticEdge>,
}

#[derive(Clone, Debug)]
pub struct SkillNode {
    pub skill_id: String,
    pub embedding: SkillEmbedding,
    pub usage_count: u64,
    pub success_rate: f64,
    pub centrality: f64, // PageRank-style importance
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SemanticEdge {
    source: String,
    target: String,
    edge_type: EdgeType,
    weight: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum EdgeType {
    /// Skills are semantically similar
    Similarity,
    /// Skills often used together
    Cooccurrence,
    /// One skill enables/depends on another
    Dependency,
    /// Skills are alternatives for similar goals
    Alternative,
    /// Skills form a sequence (A → B → C)
    Sequence,
}

impl SemanticGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    /// Add a skill node to the graph
    pub fn add_skill_node(&mut self, skill: &Skill, embedding: SkillEmbedding) {
        let node = SkillNode {
            skill_id: skill.id.clone(),
            embedding,
            usage_count: skill.usage_count,
            success_rate: if skill.usage_count > 0 {
                skill.success_count as f64 / skill.usage_count as f64
            } else {
                0.5
            },
            centrality: 0.0,
        };

        self.nodes.insert(skill.id.clone(), node);
    }

    /// Build relationships based on embedding similarity
    pub fn build_relationships(&mut self, embeddings: &HashMap<String, SkillEmbedding>) {
        let skill_ids: Vec<String> = self.nodes.keys().cloned().collect();

        for (i, id_a) in skill_ids.iter().enumerate() {
            for id_b in skill_ids.iter().skip(i + 1) {
                if let (Some(emb_a), Some(emb_b)) = (embeddings.get(id_a), embeddings.get(id_b)) {
                    let similarity = emb_a.similarity(emb_b, super::SimilarityMetric::Cosine);

                    if similarity > 0.7 {
                        self.add_edge(id_a.clone(), id_b.clone(), EdgeType::Similarity, similarity);
                    }
                }
            }
        }

        // Calculate centrality
        self.calculate_centrality();
    }

    /// Update relationships for a specific skill
    pub fn update_relationships(
        &mut self,
        skill_id: &str,
        embeddings: &HashMap<String, SkillEmbedding>,
    ) {
        // Get the embedding for this skill first
        let skill_embedding = self.nodes.get(skill_id).map(|node| node.embedding.clone());

        if skill_embedding.is_some() {
            // Remove old edges involving this skill
            self.edges
                .retain(|e| e.source != skill_id && e.target != skill_id);

            let skill_emb = skill_embedding.unwrap();

            // Add new edges based on current embeddings
            for (other_id, other_emb) in embeddings {
                if other_id == skill_id {
                    continue;
                }

                let similarity = skill_emb.similarity(other_emb, super::SimilarityMetric::Cosine);

                if similarity > 0.7 {
                    self.add_edge(
                        skill_id.to_string(),
                        other_id.clone(),
                        EdgeType::Similarity,
                        similarity,
                    );
                }
            }
        }

        self.calculate_centrality();
    }

    /// Get skills related to a given skill
    pub fn get_related(&self, skill_id: &str, limit: usize) -> Vec<RelatedSkill> {
        let mut related = Vec::new();

        for edge in &self.edges {
            if edge.source == skill_id {
                related.push(RelatedSkill {
                    skill_id: edge.target.clone(),
                    relationship: format!("{:?}", edge.edge_type),
                    strength: edge.weight,
                });
            } else if edge.target == skill_id {
                related.push(RelatedSkill {
                    skill_id: edge.source.clone(),
                    relationship: format!("{:?}", edge.edge_type),
                    strength: edge.weight,
                });
            }
        }

        // Sort by strength and take top
        related.sort_by(|a, b| b.strength.partial_cmp(&a.strength).unwrap());
        related.truncate(limit);

        related
    }

    /// Get centrality score for a skill
    pub fn centrality_score(&self, skill_id: &str) -> f64 {
        self.nodes
            .get(skill_id)
            .map(|n| n.centrality)
            .unwrap_or(0.0)
    }

    /// Boost a skill's importance after successful use
    pub fn boost_skill(&mut self, skill_id: &str) {
        if let Some(node) = self.nodes.get_mut(skill_id) {
            node.success_rate = (node.success_rate * node.usage_count as f64 + 1.0)
                / (node.usage_count as f64 + 1.0);
            node.usage_count += 1;
        }
    }

    /// Find a path of skills from start to goal
    pub fn find_path(
        &self,
        start_skill: &str,
        _goal_description: &str,
        _embeddings: &HashMap<String, SkillEmbedding>,
    ) -> Option<SkillChain> {
        // Simple greedy path finding
        // In production, use A* with learned heuristics

        let path = Vec::new();
        let mut visited = HashSet::new();
        let mut current = start_skill.to_string();

        visited.insert(current.clone());

        // Get start node
        if let Some(_node) = self.nodes.get(&current) {
            // Find skill by ID (in production, would query skill bank)
            // For now, return empty chain

            // Try to find a path of related skills
            for _ in 0..5 {
                // Max depth
                // Get neighbors sorted by relevance to goal
                let neighbors: Vec<_> = self
                    .edges
                    .iter()
                    .filter(|e| e.source == current || e.target == current)
                    .filter(|e| {
                        let neighbor_id = if e.source == current {
                            &e.target
                        } else {
                            &e.source
                        };
                        !visited.contains(neighbor_id)
                    })
                    .collect();

                if neighbors.is_empty() {
                    break;
                }

                // Pick best neighbor (simplified - would use goal embedding)
                let best = neighbors
                    .iter()
                    .max_by(|a, b| a.weight.partial_cmp(&b.weight).unwrap())
                    .unwrap();

                let next = if best.source == current {
                    &best.target
                } else {
                    &best.source
                };

                visited.insert(next.clone());
                current = next.clone();
            }
        }

        // Return empty chain (full implementation needs skill bank access)
        Some(SkillChain {
            skills: path,
            total_confidence: 0.5,
            estimated_effectiveness: 0.5,
        })
    }

    /// Get related skills list
    pub fn related_skills(&self, skill_id: &str, limit: usize) -> Vec<String> {
        self.get_related(skill_id, limit)
            .into_iter()
            .map(|r| r.skill_id)
            .collect()
    }

    /// Persist graph edges to database
    pub async fn persist(&self, db: &crate::database::RooDb) -> anyhow::Result<()> {
        for edge in &self.edges {
            let sql = format!(
                "INSERT INTO cass_graph_edges 
                 (source_skill, target_skill, edge_type, weight, created_at)
                 VALUES ('{}', '{}', '{:?}', {}, {})
                 ON DUPLICATE KEY UPDATE weight = VALUES(weight)",
                edge.source,
                edge.target,
                edge.edge_type,
                edge.weight,
                current_timestamp()
            );
            db.execute(&sql).await?;
        }
        Ok(())
    }

    fn add_edge(&mut self, source: String, target: String, edge_type: EdgeType, weight: f64) {
        self.edges.push(SemanticEdge {
            source,
            target,
            edge_type,
            weight,
        });
    }

    /// Calculate PageRank-style centrality
    fn calculate_centrality(&mut self) {
        let damping = 0.85;
        let iterations = 20;

        // Initialize
        let n = self.nodes.len() as f64;
        if n == 0.0 {
            return;
        }

        let initial_rank = 1.0 / n;
        for node in self.nodes.values_mut() {
            node.centrality = initial_rank;
        }

        // Iterate
        for _ in 0..iterations {
            let mut new_ranks = HashMap::new();

            for (id, _node) in &self.nodes {
                let mut rank = (1.0 - damping) / n;

                // Sum contributions from incoming edges
                for edge in &self.edges {
                    if edge.target == *id {
                        if let Some(source_node) = self.nodes.get(&edge.source) {
                            let out_degree = self
                                .edges
                                .iter()
                                .filter(|e| e.source == edge.source)
                                .count() as f64;
                            if out_degree > 0.0 {
                                rank += damping * source_node.centrality / out_degree;
                            }
                        }
                    }
                }

                new_ranks.insert(id.clone(), rank);
            }

            // Update
            for (id, rank) in new_ranks {
                if let Some(node) = self.nodes.get_mut(&id) {
                    node.centrality = rank;
                }
            }
        }
    }
}

impl Default for SemanticGraph {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
