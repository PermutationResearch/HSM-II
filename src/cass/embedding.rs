//! Embedding engine for semantic skill representation.
//!
//! Generates and manipulates vector embeddings for skills and queries.

use crate::skill::Skill;
use serde::{Deserialize, Serialize};

/// Vector embedding for a skill
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SkillEmbedding {
    pub skill_id: String,
    pub vector: Vec<f32>,
    pub dimension: usize,
    pub model_version: String,
}

impl SkillEmbedding {
    pub fn new(skill_id: String, vector: Vec<f32>) -> Self {
        let dimension = vector.len();
        Self {
            skill_id,
            vector,
            dimension,
            model_version: "cass-v1".to_string(),
        }
    }

    /// Calculate similarity with another embedding
    pub fn similarity(&self, other: &SkillEmbedding, metric: SimilarityMetric) -> f64 {
        match metric {
            SimilarityMetric::Cosine => self.cosine_similarity(&other.vector),
            SimilarityMetric::Euclidean => self.euclidean_similarity(&other.vector),
            SimilarityMetric::DotProduct => self.dot_product(&other.vector),
        }
    }

    fn cosine_similarity(&self, other: &[f32]) -> f64 {
        if self.vector.len() != other.len() {
            return 0.0;
        }

        let dot: f32 = self
            .vector
            .iter()
            .zip(other.iter())
            .map(|(a, b)| a * b)
            .sum();
        let norm_a: f32 = self.vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = other.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        (dot / (norm_a * norm_b)) as f64
    }

    fn euclidean_similarity(&self, other: &[f32]) -> f64 {
        if self.vector.len() != other.len() {
            return 0.0;
        }

        let dist_sq: f32 = self
            .vector
            .iter()
            .zip(other.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum();

        let dist = dist_sq.sqrt();
        // Convert distance to similarity (0-1)
        1.0 / (1.0 + dist as f64)
    }

    fn dot_product(&self, other: &[f32]) -> f64 {
        self.vector
            .iter()
            .zip(other.iter())
            .map(|(a, b)| (*a * *b) as f64)
            .sum::<f64>()
    }
}

/// Similarity metrics for comparing embeddings
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum SimilarityMetric {
    Cosine,
    Euclidean,
    DotProduct,
}

/// Embedding engine for generating skill embeddings
pub struct EmbeddingEngine {
    dimension: usize,
    // In a real implementation, this would contain an ML model or API client
}

impl EmbeddingEngine {
    pub fn new() -> Self {
        Self {
            dimension: 384, // Typical small embedding dimension
        }
    }

    /// Generate embedding for a skill
    ///
    /// In production, this would call an embedding API or local model.
    /// For now, we use a deterministic hash-based embedding.
    pub async fn embed_skill(&self, skill: &Skill) -> anyhow::Result<SkillEmbedding> {
        let text = format!(
            "{} {} {:?}",
            skill.title,
            skill.principle,
            skill
                .when_to_apply
                .iter()
                .map(|c| &c.predicate)
                .collect::<Vec<_>>()
        );

        let vector = self.text_to_embedding(&text);

        Ok(SkillEmbedding::new(skill.id.clone(), vector))
    }

    /// Generate embedding for arbitrary text
    pub async fn embed_text(&self, text: &str) -> anyhow::Result<SkillEmbedding> {
        let vector = self.text_to_embedding(text);
        Ok(SkillEmbedding::new("query".to_string(), vector))
    }

    /// Deterministic text-to-embedding conversion
    ///
    /// This is a placeholder implementation. In production, use:
    /// - sentence-transformers (all-MiniLM-L6-v2)
    /// - OpenAI embeddings API
    /// - Ollama local embeddings
    fn text_to_embedding(&self, text: &str) -> Vec<f32> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Simple word hashing for deterministic embeddings
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut embedding = vec![0.0f32; self.dimension];

        for (i, word) in words.iter().enumerate() {
            let mut hasher = DefaultHasher::new();
            word.hash(&mut hasher);
            let hash = hasher.finish();

            // Distribute hash values across dimensions
            for d in 0..self.dimension {
                let bit = ((hash >> (d % 64)) & 1) as f32;
                embedding[d] += bit * (1.0 / (i + 1) as f32);
            }
        }

        // Normalize
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut embedding {
                *x /= norm;
            }
        }

        embedding
    }

    /// Batch embed multiple skills
    pub async fn embed_skills_batch(
        &self,
        skills: &[Skill],
    ) -> anyhow::Result<Vec<SkillEmbedding>> {
        let mut embeddings = Vec::with_capacity(skills.len());
        for skill in skills {
            embeddings.push(self.embed_skill(skill).await?);
        }
        Ok(embeddings)
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

impl Default for EmbeddingEngine {
    fn default() -> Self {
        Self::new()
    }
}
