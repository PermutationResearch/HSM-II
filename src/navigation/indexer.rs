//! Semantic indexer for code units with topic modeling.

use super::{ParsedUnit, TopicId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Semantic index of code units
pub struct CodeIndex {
    units: HashMap<String, ParsedUnit>,
    units_by_topic: HashMap<TopicId, Vec<String>>,
    topics: Vec<Topic>,
    file_units: HashMap<String, Vec<String>>, // file_path -> unit_ids
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Topic {
    id: TopicId,
    /// Top words describing this topic
    keywords: Vec<String>,
    /// Centroid embedding
    centroid: Vec<f32>,
}

impl CodeIndex {
    pub fn new() -> Self {
        Self {
            units: HashMap::new(),
            units_by_topic: HashMap::new(),
            topics: Vec::new(),
            file_units: HashMap::new(),
        }
    }

    /// Add a parsed unit to the index
    pub fn add_unit(&mut self, unit: ParsedUnit) {
        let unit_id = unit.id.clone();
        let file_path = unit.file_path.clone();

        self.units.insert(unit_id.clone(), unit);

        // Track by file
        self.file_units
            .entry(file_path)
            .or_insert_with(Vec::new)
            .push(unit_id);
    }

    /// Build topic model using simple k-means clustering
    pub fn build_topics(&mut self, num_topics: usize) {
        if self.units.is_empty() {
            return;
        }

        // Generate embeddings for all units
        self.generate_embeddings();

        // Simple k-means clustering
        let mut model = TopicModel::new(num_topics);
        model.fit(&self.units);

        // Assign topics
        for (unit_id, unit) in &mut self.units {
            let topic = model.assign_topic(&unit.embedding.as_ref().unwrap());
            unit.topic = Some(topic);

            self.units_by_topic
                .entry(topic)
                .or_insert_with(Vec::new)
                .push(unit_id.clone());
        }

        // Store topics
        self.topics = model.topics();
    }

    /// Get units by topic
    pub fn get_units_by_topic(&self, topic_id: TopicId) -> Vec<&ParsedUnit> {
        self.units_by_topic
            .get(&topic_id)
            .map(|ids| ids.iter().filter_map(|id| self.units.get(id)).collect())
            .unwrap_or_default()
    }

    /// Get related units (same topic or semantic similarity)
    pub fn get_related_units(&self, unit_id: &str, limit: usize) -> Vec<&ParsedUnit> {
        let unit = match self.units.get(unit_id) {
            Some(u) => u,
            None => return Vec::new(),
        };

        let topic = match unit.topic {
            Some(t) => t,
            None => return Vec::new(),
        };

        // Get units from same topic
        let mut related: Vec<&ParsedUnit> = self
            .units_by_topic
            .get(&topic)
            .map(|ids| {
                ids.iter()
                    .filter(|id| *id != unit_id)
                    .filter_map(|id| self.units.get(id))
                    .collect()
            })
            .unwrap_or_default();

        // Sort by similarity if embeddings available
        if let Some(ref embedding) = unit.embedding {
            related.sort_by(|a, b| {
                let sim_a = a
                    .embedding
                    .as_ref()
                    .map(|e| cosine_similarity(e, embedding))
                    .unwrap_or(0.0);
                let sim_b = b
                    .embedding
                    .as_ref()
                    .map(|e| cosine_similarity(e, embedding))
                    .unwrap_or(0.0);
                sim_b.partial_cmp(&sim_a).unwrap()
            });
        }

        related.truncate(limit);
        related
    }

    /// Get topic distribution for a file
    pub fn get_file_topic_distribution(
        &self,
        file_path: &std::path::Path,
    ) -> HashMap<TopicId, f64> {
        let path_str = file_path.to_string_lossy().to_string();

        let unit_ids = match self.file_units.get(&path_str) {
            Some(ids) => ids,
            None => return HashMap::new(),
        };

        let mut distribution: HashMap<TopicId, usize> = HashMap::new();
        let mut total = 0;

        for unit_id in unit_ids {
            if let Some(unit) = self.units.get(unit_id) {
                if let Some(topic) = unit.topic {
                    *distribution.entry(topic).or_insert(0) += 1;
                    total += 1;
                }
            }
        }

        // Convert to percentages
        distribution
            .into_iter()
            .map(|(topic, count)| (topic, count as f64 / total as f64))
            .collect()
    }

    /// Get all units
    pub fn units(&self) -> &HashMap<String, ParsedUnit> {
        &self.units
    }

    /// Get unit by ID
    pub fn get_unit(&self, id: &str) -> Option<&ParsedUnit> {
        self.units.get(id)
    }

    /// Get number of topics
    pub fn topic_count(&self) -> usize {
        self.topics.len()
    }

    /// Get topic keywords
    pub fn get_topic_keywords(&self, topic_id: TopicId) -> Vec<String> {
        self.topics
            .get(topic_id)
            .map(|t| t.keywords.clone())
            .unwrap_or_default()
    }

    fn generate_embeddings(&mut self) {
        for (_id, unit) in &mut self.units {
            let text = format!(
                "{} {} {} {:?}",
                unit.name,
                unit.signature,
                unit.documentation.as_deref().unwrap_or(""),
                unit.unit_type
            );
            unit.embedding = Some(text_to_embedding(&text));
        }
    }
}

impl Default for CodeIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple k-means topic model
pub struct TopicModel {
    num_topics: usize,
    centroids: Vec<Vec<f32>>,
}

impl TopicModel {
    pub fn new(num_topics: usize) -> Self {
        Self {
            num_topics,
            centroids: Vec::new(),
        }
    }

    pub fn fit(&mut self, units: &HashMap<String, ParsedUnit>) {
        // Collect embeddings
        let embeddings: Vec<Vec<f32>> =
            units.values().filter_map(|u| u.embedding.clone()).collect();

        if embeddings.len() < self.num_topics {
            return;
        }

        // Initialize centroids randomly
        self.centroids = embeddings.iter().take(self.num_topics).cloned().collect();

        // Run k-means iterations
        for _ in 0..10 {
            // Assign points to clusters
            let mut clusters: Vec<Vec<Vec<f32>>> = vec![Vec::new(); self.num_topics];

            for embedding in &embeddings {
                let closest = self.find_closest_centroid(embedding);
                clusters[closest].push(embedding.clone());
            }

            // Update centroids
            for i in 0..self.num_topics {
                if !clusters[i].is_empty() {
                    self.centroids[i] = self.compute_centroid(&clusters[i]);
                }
            }
        }
    }

    pub fn assign_topic(&self, embedding: &[f32]) -> TopicId {
        self.find_closest_centroid(embedding)
    }

    pub fn topics(&self) -> Vec<Topic> {
        self.centroids
            .iter()
            .enumerate()
            .map(|(i, centroid)| Topic {
                id: i,
                keywords: extract_keywords_from_centroid(centroid),
                centroid: centroid.clone(),
            })
            .collect()
    }

    fn find_closest_centroid(&self, embedding: &[f32]) -> usize {
        let mut best_idx = 0;
        let mut best_sim = -1.0f64;

        for (i, centroid) in self.centroids.iter().enumerate() {
            let sim = cosine_similarity(embedding, centroid);
            if sim > best_sim {
                best_sim = sim;
                best_idx = i;
            }
        }

        best_idx
    }

    fn compute_centroid(&self, points: &[Vec<f32>]) -> Vec<f32> {
        if points.is_empty() {
            return Vec::new();
        }

        let dim = points[0].len();
        let mut centroid = vec![0.0f32; dim];

        for point in points {
            for (i, &val) in point.iter().enumerate() {
                centroid[i] += val;
            }
        }

        for val in &mut centroid {
            *val /= points.len() as f32;
        }

        centroid
    }
}

/// Semantic index for fast similarity search
pub struct SemanticIndex {
    embeddings: Vec<(String, Vec<f32>)>,
    _dimension: usize,
}

impl SemanticIndex {
    pub fn new(dimension: usize) -> Self {
        Self {
            embeddings: Vec::new(),
            _dimension: dimension,
        }
    }

    pub fn add(&mut self, id: String, embedding: Vec<f32>) {
        self.embeddings.push((id, embedding));
    }

    pub fn search(&self, query: &[f32], top_k: usize) -> Vec<(String, f64)> {
        let mut results: Vec<(String, f64)> = self
            .embeddings
            .iter()
            .map(|(id, emb)| (id.clone(), cosine_similarity(query, emb)))
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        results.truncate(top_k);
        results
    }
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    (dot / (norm_a * norm_b)) as f64
}

/// Convert text to embedding vector
fn text_to_embedding(text: &str) -> Vec<f32> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let dimension = 128;
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut embedding = vec![0.0f32; dimension];

    for (i, word) in words.iter().enumerate() {
        let mut hasher = DefaultHasher::new();
        word.hash(&mut hasher);
        let hash = hasher.finish();

        for d in 0..dimension {
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

fn extract_keywords_from_centroid(_centroid: &[f32]) -> Vec<String> {
    // In production, would extract meaningful keywords from centroid
    vec!["topic".to_string()]
}
