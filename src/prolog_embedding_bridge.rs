//! Prolog-Embedding Bridge: Grounding symbolic facts in neural embeddings.
//!
//! This module bridges the symbolic-neural gap by:
//! 1. Converting Prolog facts to vector embeddings
//! 2. Enabling semantic similarity search over facts
//! 3. Supporting neural retrieval of symbolic facts for LLM prompts
//! 4. Providing bidirectional translation between embeddings and Prolog terms
//!
//! This creates a tight integration where Prolog facts are grounded in
//! the same embedding space as skills, memories, and LLM context.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;
use crate::prolog_engine::{Atom, PrologEngine};
use crate::skill::SkillBank;

/// An embedded Prolog fact with vector representation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddedFact {
    /// Original Prolog fact as text
    pub fact_text: String,
    /// Predicate name
    pub predicate: String,
    /// Arguments as strings
    pub args: Vec<String>,
    /// Vector embedding of the fact
    pub embedding: Vec<f32>,
    /// Semantic category of the fact
    pub category: FactCategory,
    /// Confidence score (0-1)
    pub confidence: f64,
    /// Timestamp when fact was embedded
    pub embedded_at: u64,
}

/// Categories for semantic organization of facts
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FactCategory {
    Agent,
    Edge,
    Belief,
    Ontology,
    Skill,
    Federation,
    JW, // JoulWork
    SystemState,
    Unknown,
}

impl FactCategory {
    /// Determine category from predicate name
    pub fn from_predicate(pred: &str) -> Self {
        match pred {
            p if p.contains("agent") => FactCategory::Agent,
            p if p.contains("edge") || p.contains("cluster") => FactCategory::Edge,
            p if p.contains("belief") || p.contains("contradiction") => FactCategory::Belief,
            p if p.contains("ontology") || p.contains("concept") => FactCategory::Ontology,
            p if p.contains("skill") || p.contains("identity_bridge") => FactCategory::Skill,
            p if p.contains("federation")
                || p.contains("remote_")
                || p.contains("trust_")
                || p.contains("cross_system") =>
            {
                FactCategory::Federation
            }
            p if p.contains("jw")
                || p.contains("global_jw")
                || p.contains("productive")
                || p.contains("high_jw")
                || p.contains("low_jw") =>
            {
                FactCategory::JW
            }
            p if p.contains("coherence")
                || p.contains("tick")
                || p.contains("count")
                || p.contains("plateau")
                || p.contains("stable") =>
            {
                FactCategory::SystemState
            }
            _ => FactCategory::Unknown,
        }
    }
}

/// Semantic index for embedded Prolog facts
#[derive(Clone, Debug, Default)]
pub struct FactEmbeddingIndex {
    /// All embedded facts
    facts: Vec<EmbeddedFact>,
    /// Index by category
    by_category: HashMap<FactCategory, Vec<usize>>,
    /// Index by predicate
    by_predicate: HashMap<String, Vec<usize>>,
}

impl FactEmbeddingIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a fact to the index
    pub fn add_fact(&mut self, fact: EmbeddedFact) {
        let idx = self.facts.len();

        // Index by category
        self.by_category
            .entry(fact.category.clone())
            .or_default()
            .push(idx);

        // Index by predicate
        self.by_predicate
            .entry(fact.predicate.clone())
            .or_default()
            .push(idx);

        self.facts.push(fact);
    }

    /// Find facts semantically similar to a query embedding
    pub fn search_similar(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Vec<(f64, &EmbeddedFact)> {
        let mut scored: Vec<(f64, &EmbeddedFact)> = self
            .facts
            .iter()
            .map(|fact| {
                let similarity = cosine_similarity(query_embedding, &fact.embedding);
                (similarity, fact)
            })
            .collect();

        // Sort by similarity (highest first)
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        scored.truncate(top_k);
        scored
    }

    /// Search facts by category
    pub fn search_by_category(&self, category: &FactCategory) -> Vec<&EmbeddedFact> {
        self.by_category
            .get(category)
            .map(|indices| indices.iter().filter_map(|&i| self.facts.get(i)).collect())
            .unwrap_or_default()
    }

    /// Search facts by predicate
    pub fn search_by_predicate(&self, predicate: &str) -> Vec<&EmbeddedFact> {
        self.by_predicate
            .get(predicate)
            .map(|indices| indices.iter().filter_map(|&i| self.facts.get(i)).collect())
            .unwrap_or_default()
    }

    /// Get all facts
    pub fn all_facts(&self) -> &[EmbeddedFact] {
        &self.facts
    }

    /// Clear all facts
    pub fn clear(&mut self) {
        self.facts.clear();
        self.by_category.clear();
        self.by_predicate.clear();
    }
}

/// Bridge between Prolog engine and embedding space
#[derive(Clone)]
pub struct PrologEmbeddingBridge {
    /// Embedding engine for generating vectors
    embedding_dim: usize,
    /// Fact index for semantic retrieval
    fact_index: FactEmbeddingIndex,
    /// Cache for recently embedded facts
    cache: HashMap<String, Vec<f32>>,
}

impl PrologEmbeddingBridge {
    pub fn new() -> Self {
        Self {
            embedding_dim: 384,
            fact_index: FactEmbeddingIndex::new(),
            cache: HashMap::new(),
        }
    }

    /// Build the embedding index from a Prolog engine's facts
    pub fn index_engine_facts(&mut self, engine: &PrologEngine) {
        self.fact_index.clear();

        // Get all facts from the engine via reflection
        let facts = self.extract_facts_from_engine(engine);

        for atom in facts {
            let fact_text = format!("{:?}", atom);
            let embedding = self.embed_fact(&atom);
            let category = FactCategory::from_predicate(&atom.predicate);

            let args: Vec<String> = atom.args.iter().map(|t| format!("{:?}", t)).collect();

            let embedded_fact = EmbeddedFact {
                fact_text: fact_text.clone(),
                predicate: atom.predicate.clone(),
                args,
                embedding,
                category,
                confidence: 1.0,
                embedded_at: current_timestamp(),
            };

            self.cache
                .insert(fact_text.clone(), embedded_fact.embedding.clone());
            self.fact_index.add_fact(embedded_fact);
        }
    }

    /// Extract facts from Prolog engine
    fn extract_facts_from_engine(&self, _engine: &PrologEngine) -> Vec<Atom> {
        // In a real implementation, we would access engine.facts directly
        // For now, return empty - the engine will need to expose facts
        vec![]
    }

    /// Embed a Prolog atom as a vector
    pub fn embed_fact(&self, atom: &Atom) -> Vec<f32> {
        let text = format!("{:?}", atom);

        // Check cache first
        if let Some(cached) = self.cache.get(&text) {
            return cached.clone();
        }

        // Generate embedding
        let embedding = self.text_to_embedding(&text);
        embedding
    }

    /// Embed a query for semantic search
    pub fn embed_query(&self, query: &str) -> Vec<f32> {
        self.text_to_embedding(query)
    }

    /// Find facts relevant to a natural language query
    pub fn find_relevant_facts(&self, query: &str, top_k: usize) -> Vec<(f64, &EmbeddedFact)> {
        let query_embedding = self.embed_query(query);
        self.fact_index.search_similar(&query_embedding, top_k)
    }

    /// Find facts similar to a given fact
    pub fn find_similar_facts(
        &self,
        fact: &EmbeddedFact,
        top_k: usize,
    ) -> Vec<(f64, &EmbeddedFact)> {
        self.fact_index
            .search_similar(&fact.embedding, top_k)
            .into_iter()
            .filter(|(_, f)| f.fact_text != fact.fact_text)
            .collect()
    }

    /// Generate neural context for LLM prompt from relevant facts
    pub fn generate_neural_context(&self, query: &str, max_facts: usize) -> String {
        let relevant = self.find_relevant_facts(query, max_facts);

        if relevant.is_empty() {
            return String::new();
        }

        let mut context = String::from("### Symbolic Facts (Semantically Retrieved)\n");

        for (score, fact) in relevant {
            context.push_str(&format!(
                "- [{} | {:.2}] {}\n",
                format!("{:?}", fact.category).to_lowercase(),
                score,
                fact.fact_text
            ));
        }

        context.push('\n');
        context
    }

    /// Synthesize new facts by finding patterns in similar facts
    pub fn synthesize_facts(&self, seed_fact: &EmbeddedFact) -> Vec<String> {
        let similar = self.find_similar_facts(seed_fact, 5);

        // Simple pattern synthesis: find common predicates among similar facts
        let mut predicate_counts: HashMap<String, usize> = HashMap::new();
        for (_, fact) in &similar {
            *predicate_counts.entry(fact.predicate.clone()).or_insert(0) += 1;
        }

        // Suggest new facts based on common patterns
        let mut synthesized = Vec::new();
        for (pred, count) in predicate_counts {
            if count >= 2 {
                synthesized.push(format!(
                    "Pattern detected: {} facts share predicate '{}'",
                    count, pred
                ));
            }
        }

        synthesized
    }

    /// Convert text to embedding vector using hash-based deterministic approach
    fn text_to_embedding(&self, text: &str) -> Vec<f32> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let words: Vec<&str> = text.split_whitespace().collect();
        let mut embedding = vec![0.0f32; self.embedding_dim];

        // Use n-grams for better semantic capture
        for i in 0..words.len() {
            // Unigrams
            let mut hasher = DefaultHasher::new();
            words[i].hash(&mut hasher);
            let hash = hasher.finish();
            self.add_hash_to_embedding(&mut embedding, hash, i as f32 + 1.0);

            // Bigrams
            if i + 1 < words.len() {
                let mut hasher = DefaultHasher::new();
                format!("{}_{}", words[i], words[i + 1]).hash(&mut hasher);
                let hash = hasher.finish();
                self.add_hash_to_embedding(&mut embedding, hash, (i as f32 + 1.0) * 1.5);
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

    fn add_hash_to_embedding(&self, embedding: &mut [f32], hash: u64, weight: f32) {
        for d in 0..self.embedding_dim {
            let bit = ((hash >> (d % 64)) & 1) as f32;
            embedding[d] += bit * (1.0 / weight);
        }
    }

    /// Get the fact index for direct access
    pub fn fact_index(&self) -> &FactEmbeddingIndex {
        &self.fact_index
    }

    /// Get mutable fact index
    pub fn fact_index_mut(&mut self) -> &mut FactEmbeddingIndex {
        &mut self.fact_index
    }

    /// Get embedding dimension
    pub fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }
}

impl Default for PrologEmbeddingBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Enhanced reasoning braid that uses neural-semantic retrieval
#[derive(Clone)]
pub struct NeuralSymbolicBraid {
    bridge: PrologEmbeddingBridge,
    /// Query history for context
    query_history: Vec<String>,
    /// Semantic coherence threshold for fact retrieval
    coherence_threshold: f64,
}

impl NeuralSymbolicBraid {
    pub fn new() -> Self {
        Self {
            bridge: PrologEmbeddingBridge::new(),
            query_history: Vec::new(),
            coherence_threshold: 0.6,
        }
    }

    /// Build the bridge from world state
    pub fn build_from_world(
        &mut self,
        world: &HyperStigmergicMorphogenesis,
        skill_bank: &SkillBank,
    ) {
        let engine = PrologEngine::from_world(world, skill_bank);
        self.bridge.index_engine_facts(&engine);
    }

    /// Query with neural enhancement - finds semantically relevant facts
    pub fn neural_query(&mut self, query: &str) -> NeuralQueryResult {
        self.query_history.push(query.to_string());

        // Get semantically relevant facts
        let relevant_facts = self.bridge.find_relevant_facts(query, 10);

        // Filter by coherence threshold
        let filtered: Vec<_> = relevant_facts
            .into_iter()
            .filter(|(score, _)| *score >= self.coherence_threshold)
            .collect();

        // Generate neural context
        let neural_context = self.bridge.generate_neural_context(query, 5);

        // Synthesize patterns
        let mut syntheses = Vec::new();
        for (_, fact) in &filtered {
            syntheses.extend(self.bridge.synthesize_facts(fact));
        }

        // Compute confidence before consuming filtered
        let confidence = filtered.first().map(|(s, _)| *s).unwrap_or(0.0);

        NeuralQueryResult {
            query: query.to_string(),
            relevant_facts: filtered.into_iter().map(|(s, f)| (s, f.clone())).collect(),
            neural_context,
            syntheses,
            confidence,
        }
    }

    /// Set coherence threshold
    pub fn set_coherence_threshold(&mut self, threshold: f64) {
        self.coherence_threshold = threshold.clamp(0.0, 1.0);
    }
}

impl Default for NeuralSymbolicBraid {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a neural-symbolic query
#[derive(Clone, Debug)]
pub struct NeuralQueryResult {
    pub query: String,
    pub relevant_facts: Vec<(f64, EmbeddedFact)>,
    pub neural_context: String,
    pub syntheses: Vec<String>,
    pub confidence: f64,
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() {
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

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Trait for embedding-aware Prolog engines
pub trait EmbeddingAwareProlog {
    /// Get embedded representation of all facts
    fn get_embedded_facts(&self) -> Vec<EmbeddedFact>;

    /// Find facts semantically similar to a query
    fn find_semantic_facts(&self, query: &str, top_k: usize) -> Vec<EmbeddedFact>;

    /// Inject neural context into query processing
    fn with_neural_context(&mut self, context: &str);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prolog_engine::Term;

    #[test]
    fn test_fact_category_from_predicate() {
        assert_eq!(
            FactCategory::from_predicate("agent_role"),
            FactCategory::Agent
        );
        assert_eq!(FactCategory::from_predicate("edge"), FactCategory::Edge);
        assert_eq!(
            FactCategory::from_predicate("belief_conflict"),
            FactCategory::Belief
        );
        assert_eq!(FactCategory::from_predicate("jw"), FactCategory::JW);
    }

    #[test]
    fn test_embedding_generation() {
        let bridge = PrologEmbeddingBridge::new();
        let atom = Atom::new("agent", vec![Term::Int(1), Term::atom("architect")]);
        let embedding = bridge.embed_fact(&atom);

        assert_eq!(embedding.len(), 384);

        // Check normalization
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01 || norm == 0.0);
    }

    #[test]
    fn test_fact_index_search() {
        let mut index = FactEmbeddingIndex::new();

        let fact1 = EmbeddedFact {
            fact_text: "agent(1, architect)".to_string(),
            predicate: "agent".to_string(),
            args: vec!["1".to_string(), "architect".to_string()],
            embedding: vec![1.0, 0.0, 0.0],
            category: FactCategory::Agent,
            confidence: 1.0,
            embedded_at: 0,
        };

        let fact2 = EmbeddedFact {
            fact_text: "agent(2, critic)".to_string(),
            predicate: "agent".to_string(),
            args: vec!["2".to_string(), "critic".to_string()],
            embedding: vec![0.0, 1.0, 0.0],
            category: FactCategory::Agent,
            confidence: 1.0,
            embedded_at: 0,
        };

        index.add_fact(fact1);
        index.add_fact(fact2);

        // Search by category
        let agents = index.search_by_category(&FactCategory::Agent);
        assert_eq!(agents.len(), 2);

        // Search by predicate
        let agent_preds = index.search_by_predicate("agent");
        assert_eq!(agent_preds.len(), 2);
    }
}
