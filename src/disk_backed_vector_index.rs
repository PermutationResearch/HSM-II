use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiskBackedVectorIndex {
    pub dim: usize,
    pub path: String,
    pub vectors: HashMap<usize, Vec<f32>>,
}

impl DiskBackedVectorIndex {
    pub fn open(path: impl Into<String>, dim: usize) -> anyhow::Result<Self> {
        let path = path.into();
        if Path::new(&path).exists() {
            let bytes = fs::read(&path)?;
            let index: DiskBackedVectorIndex = bincode::deserialize(&bytes)?;
            Ok(index)
        } else {
            Ok(Self {
                dim,
                path,
                vectors: HashMap::new(),
            })
        }
    }

    pub fn insert(&mut self, id: usize, vector: Vec<f32>) {
        if vector.len() == self.dim {
            self.vectors.insert(id, normalize(vector));
        }
    }

    pub fn persist(&self) -> anyhow::Result<()> {
        let bytes = bincode::serialize(self)?;
        fs::write(&self.path, bytes)?;
        Ok(())
    }

    pub fn search(&self, query: &[f32], k: usize) -> Vec<(usize, f32)> {
        if query.len() != self.dim {
            return Vec::new();
        }
        let query = normalize(query.to_vec());
        let mut scores: Vec<(usize, f32)> = self
            .vectors
            .iter()
            .map(|(&id, vec)| (id, cosine_similarity(&query, vec)))
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores.into_iter().take(k).collect()
    }
}

fn normalize(vector: Vec<f32>) -> Vec<f32> {
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm == 0.0 {
        vector
    } else {
        vector.iter().map(|x| x / norm).collect()
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
    fn disk_index_searches_vectors() {
        let path = format!("/tmp/hsmii-disk-index-{}.bin", std::process::id());
        let mut index = DiskBackedVectorIndex::open(&path, 3).unwrap();
        index.insert(1, vec![1.0, 0.0, 0.0]);
        index.insert(2, vec![0.0, 1.0, 0.0]);
        let results = index.search(&[1.0, 0.0, 0.0], 1);
        assert_eq!(results[0].0, 1);
        let _ = index.persist();
        let _ = fs::remove_file(path);
    }
}
