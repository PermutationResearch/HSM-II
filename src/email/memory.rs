//! Email memory with semantic search.

use super::Email;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Email memory system with semantic indexing
pub struct EmailMemory {
    emails: HashMap<String, Email>,
    threads: HashMap<String, ConversationThread>,
    // In production, would have semantic index here
    embeddings: HashMap<String, Vec<f32>>,
}

impl EmailMemory {
    pub fn new() -> Self {
        Self {
            emails: HashMap::new(),
            threads: HashMap::new(),
            embeddings: HashMap::new(),
        }
    }

    /// Store an email in memory
    pub fn store_email(&mut self, email: Email) {
        let thread_id = email.thread_id.clone();

        // Store email
        self.emails.insert(email.id.clone(), email.clone());

        // Update thread
        self.threads
            .entry(thread_id.clone())
            .or_insert_with(|| ConversationThread {
                id: thread_id,
                emails: Vec::new(),
                summary: String::new(),
            })
            .emails
            .push(email.clone());

        // Generate and store embedding (placeholder)
        let embedding = self.generate_embedding(&email);
        self.embeddings.insert(email.id.clone(), embedding);
    }

    /// Get email by ID
    pub fn get_email(&self, id: &str) -> Option<&Email> {
        self.emails.get(id)
    }

    /// Get conversation thread
    pub fn get_thread(&self, thread_id: &str) -> Option<&ConversationThread> {
        self.threads.get(thread_id)
    }

    /// Semantic search over emails
    pub async fn semantic_search(&self, query: &str) -> anyhow::Result<Vec<Email>> {
        let query_embedding = self.generate_query_embedding(query);

        let mut scored: Vec<(String, f32)> = self
            .embeddings
            .iter()
            .map(|(id, emb)| {
                let score = cosine_similarity(&query_embedding, emb);
                (id.clone(), score)
            })
            .collect();

        // Sort by similarity
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Return top results
        Ok(scored
            .into_iter()
            .take(10)
            .filter_map(|(id, _)| self.emails.get(&id).cloned())
            .collect())
    }

    /// Get email statistics
    pub fn stats(&self) -> super::EmailStats {
        use super::EmailStats;

        let mut categorized = HashMap::new();
        categorized.insert("total".to_string(), self.emails.len());
        categorized.insert("threads".to_string(), self.threads.len());

        EmailStats {
            total_processed: self.emails.len(),
            categorized,
            avg_response_time: 0.0,
        }
    }

    fn generate_embedding(&self, email: &Email) -> Vec<f32> {
        // Simple hash-based embedding for placeholder
        let text = format!("{} {}", email.subject, email.body);
        let mut embedding = vec![0.0f32; 384];

        for (i, byte) in text.bytes().enumerate() {
            embedding[i % 384] += (byte as f32) / 255.0;
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

    fn generate_query_embedding(&self, query: &str) -> Vec<f32> {
        // Same as email embedding for compatibility
        let mut embedding = vec![0.0f32; 384];

        for (i, byte) in query.bytes().enumerate() {
            embedding[i % 384] += (byte as f32) / 255.0;
        }

        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut embedding {
                *x /= norm;
            }
        }

        embedding
    }
}

impl Default for EmailMemory {
    fn default() -> Self {
        Self::new()
    }
}

/// Conversation thread
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationThread {
    pub id: String,
    pub emails: Vec<Email>,
    pub summary: String,
}

/// Cosine similarity between vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
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
