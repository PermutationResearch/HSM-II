//! Semantic search for code navigation.

use super::{CodeIndex, ParsedUnit, TopicId};

/// Semantic search engine for code
pub struct SemanticSearch {
    // In production, would use a vector database
}

impl SemanticSearch {
    pub fn new() -> Self {
        Self {}
    }

    /// Build search index from code index
    pub fn build(&mut self, _index: &CodeIndex) {
        // In production, build optimized search structures
    }

    /// Search for code by natural language query
    pub fn search(&self, query: &str, index: &CodeIndex, limit: usize) -> Vec<SearchResult> {
        let intent = self.parse_query_intent(query);
        let query_embedding = text_to_embedding(query);

        let mut scored: Vec<ScoredUnit> = Vec::new();

        for (id, unit) in index.units() {
            let mut score = 0.0;

            // Semantic similarity
            if let Some(ref unit_emb) = unit.embedding {
                score += cosine_similarity(&query_embedding, unit_emb) * 0.5;
            }

            // Keyword matching
            score += self.keyword_match_score(query, unit) * 0.3;

            // Intent-specific boosting
            score += self.intent_boost(&intent, unit) * 0.2;

            // Topic relevance
            if let Some(topic) = unit.topic {
                if intent.preferred_topics.contains(&topic) {
                    score += 0.1;
                }
            }

            scored.push(ScoredUnit {
                unit_id: id.clone(),
                score,
                unit,
            });
        }

        // Sort by score and take top results
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        scored
            .into_iter()
            .take(limit)
            .map(|s| SearchResult {
                unit_id: s.unit_id,
                name: s.unit.name.clone(),
                file_path: s.unit.file_path.clone(),
                line_start: s.unit.line_start,
                line_end: s.unit.line_end,
                unit_type: format!("{:?}", s.unit.unit_type),
                relevance_score: s.score,
                snippet: truncate(&s.unit.content, 200),
                documentation: s.unit.documentation.clone(),
            })
            .collect()
    }

    /// Parse query to understand intent
    fn parse_query_intent(&self, query: &str) -> QueryIntent {
        let query_lower = query.to_lowercase();

        let mut intent = QueryIntent::default();

        // Detect intent keywords
        if query_lower.contains("how to") || query_lower.contains("usage") {
            intent.action_type = Some(ActionType::Usage);
        } else if query_lower.contains("definition") || query_lower.contains("what is") {
            intent.action_type = Some(ActionType::Definition);
        } else if query_lower.contains("example") {
            intent.action_type = Some(ActionType::Example);
        } else if query_lower.contains("implementation") {
            intent.action_type = Some(ActionType::Implementation);
        }

        // Detect target types
        if query_lower.contains("function") || query_lower.contains("method") {
            intent.target_types.push(UnitTypeFilter::Function);
        }
        if query_lower.contains("struct") || query_lower.contains("class") {
            intent.target_types.push(UnitTypeFilter::Struct);
        }
        if query_lower.contains("trait") || query_lower.contains("interface") {
            intent.target_types.push(UnitTypeFilter::Trait);
        }

        // Extract likely keywords
        intent.keywords = query_lower
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .map(|w| w.to_string())
            .collect();

        intent
    }

    /// Score keyword matches
    fn keyword_match_score(&self, query: &str, unit: &ParsedUnit) -> f64 {
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let mut matches = 0;

        // Check name
        let name_lower = unit.name.to_lowercase();
        for word in &query_words {
            if name_lower.contains(word) {
                matches += 3; // Name matches weighted higher
            }
        }

        // Check signature
        let sig_lower = unit.signature.to_lowercase();
        for word in &query_words {
            if sig_lower.contains(word) {
                matches += 1;
            }
        }

        // Check documentation
        if let Some(ref doc) = unit.documentation {
            let doc_lower = doc.to_lowercase();
            for word in &query_words {
                if doc_lower.contains(word) {
                    matches += 2;
                }
            }
        }

        (matches as f64 / (query_words.len() * 3) as f64).min(1.0)
    }

    /// Boost score based on query intent
    fn intent_boost(&self, intent: &QueryIntent, unit: &ParsedUnit) -> f64 {
        let mut boost = 0.0;

        // Type matching
        if !intent.target_types.is_empty() {
            let unit_type_str = format!("{:?}", unit.unit_type).to_lowercase();
            for target_type in &intent.target_types {
                let target_str = format!("{:?}", target_type).to_lowercase();
                if unit_type_str.contains(&target_str) {
                    boost += 0.5;
                }
            }
        }

        // Documentation presence for certain intents
        if let Some(ActionType::Usage) = intent.action_type {
            if unit.documentation.is_some() {
                boost += 0.3;
            }
        }

        boost
    }
}

impl Default for SemanticSearch {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
struct ScoredUnit<'a> {
    unit_id: String,
    score: f64,
    unit: &'a ParsedUnit,
}

/// Search result
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub unit_id: String,
    pub name: String,
    pub file_path: String,
    pub line_start: usize,
    pub line_end: usize,
    pub unit_type: String,
    pub relevance_score: f64,
    pub snippet: String,
    pub documentation: Option<String>,
}

/// Parsed query intent
#[derive(Clone, Debug, Default)]
pub struct QueryIntent {
    pub action_type: Option<ActionType>,
    pub target_types: Vec<UnitTypeFilter>,
    pub keywords: Vec<String>,
    pub preferred_topics: Vec<TopicId>,
}

#[derive(Clone, Debug)]
pub enum ActionType {
    Usage,
    Definition,
    Example,
    Implementation,
    Related,
}

#[derive(Clone, Debug)]
pub enum UnitTypeFilter {
    Function,
    Struct,
    Trait,
    Module,
}

/// Calculate cosine similarity
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

/// Convert text to embedding
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

/// Truncate string to max length
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}
