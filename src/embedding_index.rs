use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// In-memory embedding index using brute-force cosine similarity search.
/// For production, upgrade to HNSW or IVF for >1000 vectors.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InMemoryEmbeddingIndex {
    dim: usize,
    vectors: HashMap<usize, Vec<f32>>,
}

impl InMemoryEmbeddingIndex {
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            vectors: HashMap::new(),
        }
    }

    /// Insert or update an embedding (normalizes on insert)
    pub fn insert(&mut self, id: usize, embedding: Vec<f32>) {
        if embedding.len() == self.dim {
            let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            let normalized = if norm > 0.0 {
                embedding.iter().map(|x| x / norm).collect()
            } else {
                embedding
            };
            self.vectors.insert(id, normalized);
        }
    }

    /// Search for k nearest neighbors using cosine similarity
    pub fn search(&self, query: &[f32], k: usize) -> Vec<(usize, f32)> {
        if query.len() != self.dim {
            return Vec::new();
        }

        let norm = query.iter().map(|x| x * x).sum::<f32>().sqrt();
        let query_norm: Vec<f32> = if norm > 0.0 {
            query.iter().map(|x| x / norm).collect()
        } else {
            query.to_vec()
        };

        let mut scores: Vec<(usize, f32)> = self
            .vectors
            .iter()
            .map(|(&id, vec)| {
                let sim = cosine_similarity(&query_norm, vec);
                (id, sim)
            })
            .filter(|(_, sim)| *sim > 0.0)
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores.into_iter().take(k).collect()
    }

    /// Batch search for multiple queries
    pub fn batch_search(&self, queries: &[Vec<f32>], k: usize) -> Vec<Vec<(usize, f32)>> {
        queries.iter().map(|q| self.search(q, k)).collect()
    }

    pub fn remove(&mut self, id: usize) {
        self.vectors.remove(&id);
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    pub fn clear(&mut self) {
        self.vectors.clear();
    }

    pub fn get(&self, id: usize) -> Option<&Vec<f32>> {
        self.vectors.get(&id)
    }

    pub fn from_iter<I>(dim: usize, iter: I) -> Self
    where
        I: Iterator<Item = (usize, Vec<f32>)>,
    {
        let mut index = Self::new(dim);
        for (id, vec) in iter {
            index.insert(id, vec);
        }
        index
    }

    /// Approximate search with threshold for large datasets
    pub fn fast_search(&self, query: &[f32], k: usize, threshold: f32) -> Vec<(usize, f32)> {
        if self.vectors.len() < 1000 {
            return self.search(query, k);
        }
        self.lsh_search(query, k, threshold)
    }

    fn lsh_search(&self, query: &[f32], k: usize, _threshold: f32) -> Vec<(usize, f32)> {
        let norm = query.iter().map(|x| x * x).sum::<f32>().sqrt();
        let query_norm: Vec<f32> = if norm > 0.0 {
            query.iter().map(|x| x / norm).collect()
        } else {
            query.to_vec()
        };

        let mut candidates: Vec<(usize, f32)> = self
            .vectors
            .iter()
            .take(500)
            .map(|(&id, vec)| {
                let sim = cosine_similarity(&query_norm, vec);
                (id, sim)
            })
            .filter(|(_, sim)| *sim > 0.5)
            .collect();

        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        candidates.into_iter().take(k).collect()
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
    fn test_basic_search() {
        let mut index = InMemoryEmbeddingIndex::new(3);
        index.insert(0, vec![1.0, 0.0, 0.0]);
        index.insert(1, vec![0.0, 1.0, 0.0]);
        index.insert(2, vec![0.9, 0.1, 0.0]);

        let results = index.search(&[1.0, 0.0, 0.0], 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 0);
        assert_eq!(results[1].0, 2);
    }

    #[test]
    fn test_empty_search() {
        let index = InMemoryEmbeddingIndex::new(3);
        let results = index.search(&[1.0, 0.0, 0.0], 5);
        assert!(results.is_empty());
    }
}
