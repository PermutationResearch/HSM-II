use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HnswLikeIndex {
    pub dim: usize,
    pub m: usize,
    pub vectors: HashMap<usize, Vec<f32>>,
    pub neighbors: HashMap<usize, Vec<usize>>,
}

impl HnswLikeIndex {
    pub fn new(dim: usize, m: usize) -> Self {
        Self {
            dim,
            m: m.max(2),
            vectors: HashMap::new(),
            neighbors: HashMap::new(),
        }
    }

    pub fn insert(&mut self, id: usize, vector: Vec<f32>) {
        if vector.len() != self.dim {
            return;
        }
        let vector = normalize(vector);
        let mut nearest: Vec<(usize, f32)> = self
            .vectors
            .iter()
            .map(|(&other_id, other)| (other_id, cosine_similarity(&vector, other)))
            .collect();
        nearest.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let linked: Vec<usize> = nearest.into_iter().take(self.m).map(|(id, _)| id).collect();

        self.vectors.insert(id, vector);
        self.neighbors.insert(id, linked.clone());
        for other in linked {
            self.neighbors.entry(other).or_default().push(id);
            if self.neighbors[&other].len() > self.m {
                let mut trimmed = self.neighbors[&other].clone();
                trimmed.truncate(self.m);
                self.neighbors.insert(other, trimmed);
            }
        }
    }

    pub fn search(&self, query: &[f32], k: usize) -> Vec<(usize, f32)> {
        if query.len() != self.dim || self.vectors.is_empty() {
            return Vec::new();
        }
        let query = normalize(query.to_vec());
        let start = *self.vectors.keys().next().unwrap();
        let mut frontier = vec![start];
        let mut visited = HashMap::new();

        while let Some(current) = frontier.pop() {
            let Some(vec) = self.vectors.get(&current) else {
                continue;
            };
            let score = cosine_similarity(&query, vec);
            visited.insert(current, score);
            if let Some(neighbors) = self.neighbors.get(&current) {
                for neighbor in neighbors {
                    if !visited.contains_key(neighbor) {
                        frontier.push(*neighbor);
                    }
                }
            }
            if visited.len() >= self.m * 8 {
                break;
            }
        }

        let mut scored: Vec<(usize, f32)> = visited.into_iter().collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored.into_iter().take(k).collect()
    }
}

fn normalize(vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        vector
    } else {
        vector.into_iter().map(|x| x / norm).collect()
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hnsw_like_search_returns_nearest() {
        let mut index = HnswLikeIndex::new(3, 4);
        index.insert(1, vec![1.0, 0.0, 0.0]);
        index.insert(2, vec![0.0, 1.0, 0.0]);
        index.insert(3, vec![0.9, 0.1, 0.0]);
        let results = index.search(&[1.0, 0.0, 0.0], 2);
        assert_eq!(results[0].0, 1);
    }
}
