use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::embedding_index::InMemoryEmbeddingIndex;
use crate::graph_runtime::GraphRuntime;
use crate::pi_tools::create_pi_tools;

const EMBEDDING_DIM: usize = 768;

/// Hindsight-inspired four-network memory architecture:
///   1. World facts — objective truths about the system
///   2. Experiences — timestamped agent events + outcomes
///   3. Entity summaries — synthesized profiles
///   4. Evolving beliefs — opinions with confidence
///
/// ZeroClaw-inspired hybrid retrieval:
///   - Vector (cosine similarity via InMemoryEmbeddingIndex)
///   - Keyword (BM25-style term frequency matching)
///   - Graph (entity/relationship traversal)
///   - Temporal (recency-weighted filtering)
///   - **Typed claims** — [`MemoryRetrievalIntent`] × [`MemoryNetwork`] × entity overlap ([`recall_claim_typed`])
///
/// Results fused via Reciprocal Rank Fusion (RRF). Claim channel weight: `HSM_TYPED_CLAIM_RRF_WEIGHT` (default 0.65).

// ── Memory Entry ──────────────────────────────────────────────────────

/// OpenViking-inspired L0/L1/L2 tiered context:
///   L0 (abstract_l0): ~50 token summary for quick relevance filtering
///   L1 (overview_l1): ~500 token overview for navigation/reranking
///   L2 (content):      full entry text for detailed processing
/// API-facing slice of [`MemoryEntry`] without embeddings.
#[derive(Clone, Debug, Serialize)]
pub struct MemoryEntryPreview {
    pub id: usize,
    pub content: String,
    pub abstract_l0: Option<String>,
    pub overview_l1: Option<String>,
    pub network: MemoryNetwork,
    pub entities: Vec<String>,
    pub tags: Vec<String>,
    pub timestamp: u64,
    pub tick: u64,
    pub importance: f64,
}

impl MemoryEntryPreview {
    fn from_entry(e: &MemoryEntry) -> Self {
        Self {
            id: e.id,
            content: e.content.clone(),
            abstract_l0: e.abstract_l0.clone(),
            overview_l1: e.overview_l1.clone(),
            network: e.network.clone(),
            entities: e.entities.clone(),
            tags: e.tags.clone(),
            timestamp: e.timestamp,
            tick: e.tick,
            importance: e.importance,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: usize,
    pub content: String,
    /// L0 abstract: single-sentence summary (~50 tokens) for rapid filtering
    #[serde(default)]
    pub abstract_l0: Option<String>,
    /// L1 overview: structured summary (~500 tokens) for navigation
    #[serde(default)]
    pub overview_l1: Option<String>,
    pub network: MemoryNetwork,
    pub entities: Vec<String>,
    pub tags: Vec<String>,
    pub timestamp: u64,
    pub tick: u64,
    pub access_count: u32,
    pub last_accessed: u64,
    pub importance: f64,
    pub embedding: Option<Vec<f32>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum MemoryNetwork {
    WorldFact,
    Experience,
    EntitySummary,
    Belief,
}

// ── Recall Result ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct RecallResult {
    pub entry: MemoryEntry,
    pub score: f64,
    pub strategy_scores: StrategyScores,
}

#[derive(Clone, Debug)]
pub struct StrategyScores {
    pub semantic: f64,
    pub keyword: f64,
    pub graph: f64,
    pub temporal: f64,
    /// Typed claim / [`MemoryNetwork`] alignment with [`classify_query_intent`] (channel raw score).
    pub claim_typed: f64,
    pub fused: f64,
}

// ── Typed claim retrieval (query intent × memory network) ─────────────

/// Lightweight query intent for memory ranking — no LLM. Drives which [`MemoryNetwork`] kinds are preferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryRetrievalIntent {
    /// Belief updates, contradictions, opinions
    BeliefRevision,
    /// Time-relative questions
    Temporal,
    /// Who/what/entity-centric lookups
    EntityCentric,
    /// Events, sessions, what happened
    Experiential,
    /// No strong signal
    General,
}

/// Classify query intent from cheap keyword / pattern heuristics (English-oriented).
pub fn classify_query_intent(query: &str) -> MemoryRetrievalIntent {
    let q = query.to_lowercase();
    if q.split_whitespace().count() < 2 && !q.contains('?') {
        return MemoryRetrievalIntent::General;
    }
    if matches_belief_revision(&q) {
        return MemoryRetrievalIntent::BeliefRevision;
    }
    if matches_temporal(&q) {
        return MemoryRetrievalIntent::Temporal;
    }
    if matches_entity_centric(&q) {
        return MemoryRetrievalIntent::EntityCentric;
    }
    if matches_experiential(&q) {
        return MemoryRetrievalIntent::Experiential;
    }
    MemoryRetrievalIntent::General
}

fn matches_belief_revision(q: &str) -> bool {
    const K: &[&str] = &[
        "believe",
        "belief",
        "opinion",
        "think that",
        "suppose",
        "revised",
        "revision",
        "updated view",
        "supersede",
        "supersed",
        "contradict",
        "contradiction",
        "no longer believe",
        "changed my mind",
        "corrected",
        "obsolete",
    ];
    K.iter().any(|k| q.contains(k))
}

fn matches_temporal(q: &str) -> bool {
    const K: &[&str] = &[
        "when ",
        "when did",
        "before ",
        "after ",
        "during ",
        "last week",
        "yesterday",
        "timeline",
        "chronolog",
        "at what time",
        "what date",
    ];
    K.iter().any(|k| q.contains(k))
}

fn matches_entity_centric(q: &str) -> bool {
    q.starts_with("who ")
        || q.starts_with("what is ")
        || q.starts_with("what are ")
        || q.contains("define ")
        || q.contains("definition of")
}

fn matches_experiential(q: &str) -> bool {
    const K: &[&str] = &[
        "happened",
        "we did",
        "i did",
        "session",
        "meeting",
        "event",
        "experience",
        "recall when",
        "remember when",
    ];
    K.iter().any(|k| q.contains(k))
}

/// Base relevance in [0, 1] of a [`MemoryNetwork`] row given intent.
pub fn network_claim_match(intent: MemoryRetrievalIntent, network: &MemoryNetwork) -> f64 {
    match intent {
        MemoryRetrievalIntent::BeliefRevision => match network {
            MemoryNetwork::Belief => 1.0,
            MemoryNetwork::WorldFact => 0.48,
            MemoryNetwork::EntitySummary => 0.42,
            MemoryNetwork::Experience => 0.35,
        },
        MemoryRetrievalIntent::Temporal => match network {
            MemoryNetwork::Experience => 1.0,
            MemoryNetwork::Belief => 0.72,
            MemoryNetwork::WorldFact => 0.62,
            MemoryNetwork::EntitySummary => 0.48,
        },
        MemoryRetrievalIntent::EntityCentric => match network {
            MemoryNetwork::EntitySummary => 1.0,
            MemoryNetwork::WorldFact => 0.88,
            MemoryNetwork::Belief => 0.55,
            MemoryNetwork::Experience => 0.4,
        },
        MemoryRetrievalIntent::Experiential => match network {
            MemoryNetwork::Experience => 1.0,
            MemoryNetwork::EntitySummary => 0.52,
            MemoryNetwork::WorldFact => 0.48,
            MemoryNetwork::Belief => 0.38,
        },
        MemoryRetrievalIntent::General => match network {
            MemoryNetwork::Belief => 0.78,
            MemoryNetwork::WorldFact => 0.76,
            MemoryNetwork::EntitySummary => 0.7,
            MemoryNetwork::Experience => 0.68,
        },
    }
}

fn entity_overlap_factor(query_entities: &[String], entry: &MemoryEntry) -> f64 {
    if query_entities.is_empty() {
        return 1.0;
    }
    let mut hits = 0_usize;
    for qe in query_entities {
        let ql = qe.to_lowercase();
        if entry.entities.iter().any(|e| {
            let el = e.to_lowercase();
            el == ql || el.contains(&ql) || ql.contains(&el)
        }) {
            hits += 1;
        }
    }
    0.65 + 0.35 * (hits as f64 / query_entities.len().max(1) as f64)
}

// ── OpenViking-inspired Progressive Recall ───────────────────────────

/// Which context tier to render when returning recall results.
/// Inspired by OpenViking's L0/L1/L2 hierarchical retrieval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextLevel {
    /// ~50 token abstract — for rapid relevance scanning
    L0,
    /// ~500 token overview — for navigation and reranking
    L1,
    /// Full content — for detailed processing
    L2,
}

/// Configuration for progressive token-budget-aware recall.
#[derive(Clone, Debug)]
pub struct RecallConfig {
    /// How many candidates to retrieve initially (broad sweep)
    pub k_initial: usize,
    /// How many results to return after reranking
    pub k_final: usize,
    /// Maximum total token budget for returned content
    pub token_budget: usize,
    /// Preferred context level; falls back to lower tiers to fit budget
    pub level: ContextLevel,
}

impl Default for RecallConfig {
    fn default() -> Self {
        Self {
            k_initial: 20,
            k_final: 5,
            token_budget: 2000,
            level: ContextLevel::L1,
        }
    }
}

/// A single result from progressive recall, with its rendered content
/// at the appropriate context level to fit the token budget.
#[derive(Clone, Debug)]
pub struct ProgressiveRecallResult {
    pub entry: MemoryEntry,
    pub score: f64,
    pub strategy_scores: StrategyScores,
    /// The text actually sent to the LLM (L0, L1, or L2 depending on budget)
    pub rendered_content: String,
    /// Which level was used for this result
    pub level: ContextLevel,
}

// ── Hierarchy Derivation (OpenViking L0/L1) ─────────────────────────

/// Derive L0 (abstract) and L1 (overview) from full content.
///
/// This is a lightweight, instant, local operation — no LLM calls.
/// L0: First sentence (or first 80 chars), capped at ~50 tokens.
/// L1: First 3 sentences (or first 500 chars) with key entity extraction.
pub fn derive_hierarchy(content: &str) -> (String, String) {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return (String::new(), String::new());
    }

    // L0: First sentence or first 80 chars, whichever is shorter
    let l0 = {
        let first_sentence_end = trimmed
            .find(|c: char| c == '.' || c == '!' || c == '?')
            .map(|i| i + 1)
            .unwrap_or(trimmed.len());
        let end = first_sentence_end.min(80).min(trimmed.len());
        let mut s = trimmed[..end].to_string();
        if end < trimmed.len() && !s.ends_with('.') && !s.ends_with('!') && !s.ends_with('?') {
            s.push('…');
        }
        s
    };

    // L1: First 3 sentences or first 500 chars with key terms
    let l1 = {
        let mut sentences = Vec::new();
        let mut start = 0;
        let chars: Vec<char> = trimmed.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            if (c == '.' || c == '!' || c == '?')
                && i + 1 < chars.len()
                && chars[i + 1].is_whitespace()
            {
                let sentence: String = chars[start..=i].iter().collect();
                sentences.push(sentence);
                start = i + 1;
                if sentences.len() >= 3 {
                    break;
                }
            }
        }
        // If no sentence breaks found, take up to 500 chars
        if sentences.is_empty() {
            let end = trimmed.len().min(500);
            let mut s = trimmed[..end].to_string();
            if end < trimmed.len() {
                s.push('…');
            }
            s
        } else {
            let mut overview = sentences.join(" ").trim().to_string();
            if overview.len() > 500 {
                overview.truncate(500);
                overview.push('…');
            }
            overview
        }
    };

    (l0, l1)
}

/// Estimate token count (rough: ~4 chars per token for English)
fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

// ── Hybrid Memory Store ───────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HybridMemory {
    entries: Vec<MemoryEntry>,
    entity_graph: HashMap<String, Vec<usize>>, // entity -> memory ids
    tag_index: HashMap<String, Vec<usize>>,    // tag -> memory ids
    #[serde(skip)]
    vector_index: InMemoryEmbeddingIndex,
    next_id: usize,
    pub stats: MemoryStats,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_entries: usize,
    pub world_facts: usize,
    pub experiences: usize,
    pub entity_summaries: usize,
    pub beliefs: usize,
    pub total_recalls: u64,
    pub avg_recall_strategies: f64,
}

impl HybridMemory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            entity_graph: HashMap::new(),
            tag_index: HashMap::new(),
            vector_index: InMemoryEmbeddingIndex::new(EMBEDDING_DIM),
            next_id: 0,
            stats: MemoryStats::default(),
        }
    }

    // ── RETAIN: Store new memory ──────────────────────────────────────

    pub fn retain(
        &mut self,
        content: &str,
        network: MemoryNetwork,
        entities: Vec<String>,
        tags: Vec<String>,
        tick: u64,
        embedding: Vec<f32>,
    ) -> usize {
        let id = self.next_id;
        self.next_id += 1;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let importance = Self::calculate_importance(content, &network);

        let (l0, l1) = derive_hierarchy(content);
        let entry = MemoryEntry {
            id,
            content: content.to_string(),
            abstract_l0: Some(l0),
            overview_l1: Some(l1),
            network: network.clone(),
            entities: entities.clone(),
            tags: tags.clone(),
            timestamp: now,
            tick,
            access_count: 0,
            last_accessed: now,
            importance,
            embedding: Some(embedding.clone()),
        };

        // Index in entity graph
        for entity in &entities {
            self.entity_graph
                .entry(entity.clone())
                .or_default()
                .push(id);
        }

        // Index in tag index
        for tag in &tags {
            self.tag_index.entry(tag.clone()).or_default().push(id);
        }

        // Index in vector store
        self.vector_index.insert(id, embedding);

        // Update stats
        match network {
            MemoryNetwork::WorldFact => self.stats.world_facts += 1,
            MemoryNetwork::Experience => self.stats.experiences += 1,
            MemoryNetwork::EntitySummary => self.stats.entity_summaries += 1,
            MemoryNetwork::Belief => self.stats.beliefs += 1,
        }
        self.stats.total_entries += 1;

        self.entries.push(entry);
        id
    }

    /// Remove one entry by id (rebuilds secondary indexes). Returns false if missing.
    pub fn remove_entry_by_id(&mut self, id: usize) -> bool {
        let Some(pos) = self.entries.iter().position(|e| e.id == id) else {
            return false;
        };
        let entry = self.entries.remove(pos);
        self.vector_index.remove(id);

        for ent in &entry.entities {
            if let Some(ids) = self.entity_graph.get_mut(ent) {
                ids.retain(|&x| x != id);
                if ids.is_empty() {
                    self.entity_graph.remove(ent);
                }
            }
        }
        for tag in &entry.tags {
            if let Some(ids) = self.tag_index.get_mut(tag) {
                ids.retain(|&x| x != id);
                if ids.is_empty() {
                    self.tag_index.remove(tag);
                }
            }
        }

        match entry.network {
            MemoryNetwork::WorldFact => {
                self.stats.world_facts = self.stats.world_facts.saturating_sub(1);
            }
            MemoryNetwork::Experience => {
                self.stats.experiences = self.stats.experiences.saturating_sub(1);
            }
            MemoryNetwork::EntitySummary => {
                self.stats.entity_summaries = self.stats.entity_summaries.saturating_sub(1);
            }
            MemoryNetwork::Belief => {
                self.stats.beliefs = self.stats.beliefs.saturating_sub(1);
            }
        }
        self.stats.total_entries = self.stats.total_entries.saturating_sub(1);
        true
    }

    /// Lightweight listing for HTTP review UIs (newest last; capped).
    pub fn list_entries_preview(&self, limit: usize) -> Vec<MemoryEntryPreview> {
        let n = self.entries.len();
        let skip = n.saturating_sub(limit);
        self.entries
            .iter()
            .skip(skip)
            .map(MemoryEntryPreview::from_entry)
            .collect()
    }

    fn calculate_importance(content: &str, network: &MemoryNetwork) -> f64 {
        let base = match network {
            MemoryNetwork::WorldFact => 0.6,
            MemoryNetwork::Experience => 0.7,
            MemoryNetwork::EntitySummary => 0.5,
            MemoryNetwork::Belief => 0.8,
        };
        // Longer, more detailed content is more important
        let length_bonus = (content.len() as f64 / 500.0).min(0.2);
        (base + length_bonus).min(1.0)
    }

    // ── RECALL: Multi-strategy retrieval with RRF ─────────────────────

    pub fn recall(
        &mut self,
        query: &str,
        query_embedding: &[f32],
        k: usize,
        current_tick: u64,
    ) -> Vec<RecallResult> {
        if self.entries.is_empty() {
            return Vec::new();
        }

        self.stats.total_recalls += 1;

        // Strategy 1: Semantic (vector similarity)
        let semantic_results = self.recall_semantic(query_embedding, k * 3);

        // Strategy 2: Keyword (BM25-style)
        let keyword_results = self.recall_keyword(query, k * 3);

        // Strategy 3: Graph (entity traversal)
        let entities = extract_entities(query);
        let graph_results = self.recall_graph(&entities, k * 3);

        // Strategy 4: Temporal (recency-weighted)
        let temporal_results = self.recall_temporal(current_tick, k * 3);

        // Strategy 5: Typed claims — intent × MemoryNetwork + entity overlap (not raw TF-IDF alone)
        let claim_results = self.recall_claim_typed(query, k * 3);

        // Reciprocal Rank Fusion
        let fused = self.reciprocal_rank_fusion(
            &semantic_results,
            &keyword_results,
            &graph_results,
            &temporal_results,
            &claim_results,
            k,
        );

        // Update access counts
        for result in &fused {
            if let Some(entry) = self.entries.iter_mut().find(|e| e.id == result.entry.id) {
                entry.access_count += 1;
                entry.last_accessed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
            }
        }

        fused
    }

    /// Progressive recall: token-budget-aware retrieval using L0/L1/L2 tiers.
    ///
    /// Inspired by OpenViking's hierarchical retriever:
    ///   1. Broad sweep at k_initial using all 4 strategies + RRF
    ///   2. Rerank top k_final
    ///   3. Render each result at the requested ContextLevel
    ///   4. If budget exceeded, fall back to lower tiers (L2→L1→L0)
    pub fn recall_progressive(
        &mut self,
        query: &str,
        query_embedding: &[f32],
        config: &RecallConfig,
        current_tick: u64,
    ) -> Vec<ProgressiveRecallResult> {
        // Step 1: Broad recall
        let candidates = self.recall(query, query_embedding, config.k_initial, current_tick);

        // Step 2: Take top k_final
        let top_k: Vec<RecallResult> = candidates.into_iter().take(config.k_final).collect();

        // Step 3: Progressive rendering with budget
        let mut results = Vec::new();
        let mut budget_remaining = config.token_budget;

        for result in top_k {
            // Try rendering at requested level, fall back if over budget
            let (rendered, level) =
                Self::render_at_level(&result.entry, &config.level, budget_remaining);
            let tokens = estimate_tokens(&rendered);

            if tokens > budget_remaining && !results.is_empty() {
                // Budget exhausted — stop adding results
                break;
            }

            budget_remaining = budget_remaining.saturating_sub(tokens);

            results.push(ProgressiveRecallResult {
                entry: result.entry,
                score: result.score,
                strategy_scores: result.strategy_scores,
                rendered_content: rendered,
                level,
            });
        }

        results
    }

    /// Render a memory entry at the given context level, falling back to
    /// lower tiers if the content exceeds the remaining budget.
    fn render_at_level(
        entry: &MemoryEntry,
        preferred: &ContextLevel,
        budget_tokens: usize,
    ) -> (String, ContextLevel) {
        match preferred {
            ContextLevel::L2 => {
                let tokens = estimate_tokens(&entry.content);
                if tokens <= budget_tokens {
                    return (entry.content.clone(), ContextLevel::L2);
                }
                // Fall back to L1
                if let Some(ref l1) = entry.overview_l1 {
                    if estimate_tokens(l1) <= budget_tokens {
                        return (l1.clone(), ContextLevel::L1);
                    }
                }
                // Fall back to L0
                if let Some(ref l0) = entry.abstract_l0 {
                    return (l0.clone(), ContextLevel::L0);
                }
                // Last resort: truncate
                let end = (budget_tokens * 4).min(entry.content.len());
                (entry.content[..end].to_string(), ContextLevel::L2)
            }
            ContextLevel::L1 => {
                if let Some(ref l1) = entry.overview_l1 {
                    let tokens = estimate_tokens(l1);
                    if tokens <= budget_tokens {
                        return (l1.clone(), ContextLevel::L1);
                    }
                }
                // Fall back to L0
                if let Some(ref l0) = entry.abstract_l0 {
                    return (l0.clone(), ContextLevel::L0);
                }
                // Fallback: truncate content
                let end = (budget_tokens * 4).min(entry.content.len());
                (entry.content[..end].to_string(), ContextLevel::L1)
            }
            ContextLevel::L0 => {
                if let Some(ref l0) = entry.abstract_l0 {
                    return (l0.clone(), ContextLevel::L0);
                }
                // Generate on-the-fly
                let (l0, _) = derive_hierarchy(&entry.content);
                (l0, ContextLevel::L0)
            }
        }
    }

    /// Semantic recall using vector similarity
    fn recall_semantic(&self, query_embedding: &[f32], k: usize) -> Vec<(usize, f64)> {
        self.vector_index
            .search(query_embedding, k)
            .into_iter()
            .map(|(id, sim)| (id, sim as f64))
            .collect()
    }

    /// Keyword recall using BM25-inspired term frequency scoring
    fn recall_keyword(&self, query: &str, k: usize) -> Vec<(usize, f64)> {
        let query_terms: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 2)
            .map(|w| w.to_string())
            .collect();

        if query_terms.is_empty() {
            return Vec::new();
        }

        let n = self.entries.len() as f64;
        let avg_len: f64 = self
            .entries
            .iter()
            .map(|e| e.content.len() as f64)
            .sum::<f64>()
            / n.max(1.0);

        let mut scores: Vec<(usize, f64)> = self
            .entries
            .iter()
            .map(|entry| {
                let doc_lower = entry.content.to_lowercase();
                let doc_len = doc_lower.len() as f64;
                let k1 = 1.2;
                let b = 0.75;

                let mut score = 0.0;
                for term in &query_terms {
                    // Term frequency in document
                    let tf = doc_lower.matches(term.as_str()).count() as f64;
                    if tf == 0.0 {
                        continue;
                    }

                    // Inverse document frequency (approximate)
                    let df = self
                        .entries
                        .iter()
                        .filter(|e| e.content.to_lowercase().contains(term.as_str()))
                        .count() as f64;
                    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();

                    // BM25 formula
                    let tf_norm = (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * doc_len / avg_len));
                    score += idf * tf_norm;
                }

                // Bonus for entity/tag matches
                for term in &query_terms {
                    if entry
                        .entities
                        .iter()
                        .any(|e| e.to_lowercase().contains(term.as_str()))
                    {
                        score += 1.0;
                    }
                    if entry
                        .tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(term.as_str()))
                    {
                        score += 0.5;
                    }
                }

                (entry.id, score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores.into_iter().take(k).collect()
    }

    /// Graph recall — traverse entity connections
    fn recall_graph(&self, entities: &[String], k: usize) -> Vec<(usize, f64)> {
        let mut scores: HashMap<usize, f64> = HashMap::new();

        for entity in entities {
            // Direct entity matches (high score)
            if let Some(ids) = self.entity_graph.get(entity) {
                for &id in ids {
                    *scores.entry(id).or_insert(0.0) += 2.0;
                }
            }

            // Tag matches (lower score)
            if let Some(ids) = self.tag_index.get(entity) {
                for &id in ids {
                    *scores.entry(id).or_insert(0.0) += 1.0;
                }
            }

            // Second-hop: entities that co-occur with matched entities
            if let Some(ids) = self.entity_graph.get(entity) {
                for &id in ids {
                    if let Some(entry) = self.entries.iter().find(|e| e.id == id) {
                        for other_entity in &entry.entities {
                            if other_entity != entity {
                                if let Some(hop2_ids) = self.entity_graph.get(other_entity) {
                                    for &hop2_id in hop2_ids {
                                        if hop2_id != id {
                                            *scores.entry(hop2_id).or_insert(0.0) += 0.5;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut results: Vec<(usize, f64)> = scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        results.into_iter().take(k).collect()
    }

    /// Temporal recall — recent memories weighted higher
    fn recall_temporal(&self, current_tick: u64, k: usize) -> Vec<(usize, f64)> {
        let mut scored: Vec<(usize, f64)> = self
            .entries
            .iter()
            .map(|entry| {
                let age = current_tick.saturating_sub(entry.tick) as f64;
                let recency = 1.0 / (1.0 + age / 100.0);
                let access_bonus = (entry.access_count as f64).ln_1p() * 0.1;
                let importance_weight = entry.importance;
                (entry.id, recency * importance_weight + access_bonus)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored.into_iter().take(k).collect()
    }

    /// Rank entries by typed claim fit: [`MemoryRetrievalIntent`] × [`MemoryNetwork`] × entity overlap.
    fn recall_claim_typed(&self, query: &str, k: usize) -> Vec<(usize, f64)> {
        let intent = classify_query_intent(query);
        let q_entities = extract_entities(query);
        let mut scored: Vec<(usize, f64)> = self
            .entries
            .iter()
            .map(|entry| {
                let base = network_claim_match(intent, &entry.network);
                let overlap = entity_overlap_factor(&q_entities, entry);
                let s = (base * overlap).min(1.0);
                (entry.id, s)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored.into_iter().take(k).collect()
    }

    /// Reciprocal Rank Fusion: score = Σ 1/(k + rank_i) across strategies
    fn reciprocal_rank_fusion(
        &self,
        semantic: &[(usize, f64)],
        keyword: &[(usize, f64)],
        graph: &[(usize, f64)],
        temporal: &[(usize, f64)],
        claim_typed: &[(usize, f64)],
        k: usize,
    ) -> Vec<RecallResult> {
        let rrf_k = 60.0; // standard RRF constant

        let claim_w = std::env::var("HSM_TYPED_CLAIM_RRF_WEIGHT")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|&w| w >= 0.0 && w <= 2.0)
            .unwrap_or(0.65_f64);

        // semantic, keyword, graph, temporal, claim_typed
        let weights = [1.0_f64, 0.7, 0.8, 0.5, claim_w];

        let mut fused_scores: HashMap<usize, (f64, f64, f64, f64, f64, f64)> = HashMap::new();

        for (rank, (id, score)) in semantic.iter().enumerate() {
            let entry = fused_scores
                .entry(*id)
                .or_insert((0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
            entry.0 = *score;
            entry.5 += weights[0] / (rrf_k + rank as f64 + 1.0);
        }
        for (rank, (id, score)) in keyword.iter().enumerate() {
            let entry = fused_scores
                .entry(*id)
                .or_insert((0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
            entry.1 = *score;
            entry.5 += weights[1] / (rrf_k + rank as f64 + 1.0);
        }
        for (rank, (id, score)) in graph.iter().enumerate() {
            let entry = fused_scores
                .entry(*id)
                .or_insert((0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
            entry.2 = *score;
            entry.5 += weights[2] / (rrf_k + rank as f64 + 1.0);
        }
        for (rank, (id, score)) in temporal.iter().enumerate() {
            let entry = fused_scores
                .entry(*id)
                .or_insert((0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
            entry.3 = *score;
            entry.5 += weights[3] / (rrf_k + rank as f64 + 1.0);
        }
        for (rank, (id, score)) in claim_typed.iter().enumerate() {
            let entry = fused_scores
                .entry(*id)
                .or_insert((0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
            entry.4 = *score;
            entry.5 += weights[4] / (rrf_k + rank as f64 + 1.0);
        }

        let mut results: Vec<RecallResult> = fused_scores
            .into_iter()
            .filter_map(|(id, (sem, kw, gr, temp, claim, fused))| {
                self.entries
                    .iter()
                    .find(|e| e.id == id)
                    .map(|entry| RecallResult {
                        entry: entry.clone(),
                        score: fused,
                        strategy_scores: StrategyScores {
                            semantic: sem,
                            keyword: kw,
                            graph: gr,
                            temporal: temp,
                            claim_typed: claim,
                            fused,
                        },
                    })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results.into_iter().take(k).collect()
    }

    // ── Helpers ───────────────────────────────────────────────────────

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Rebuild vector index after deserialization
    pub fn rebuild_index(&mut self) {
        self.vector_index = InMemoryEmbeddingIndex::new(EMBEDDING_DIM);
        for entry in &self.entries {
            if let Some(emb) = &entry.embedding {
                self.vector_index.insert(entry.id, emb.clone());
            }
        }
    }

    /// Get entries by network type
    pub fn by_network(&self, network: MemoryNetwork) -> Vec<&MemoryEntry> {
        self.entries
            .iter()
            .filter(|e| e.network == network)
            .collect()
    }
}

/// Extract likely entities from a query string (simple heuristic)
fn extract_entities(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|w| {
            w.len() > 3 && (w.chars().next().map_or(false, |c| c.is_uppercase()) || w.contains('_'))
        })
        .map(|w| w.to_string())
        .collect()
}

// ── ZeroClaw-style trait-based Tool system ─────────────────────────────

/// Trait that any tool must implement (ZeroClaw pattern)
/// Tools are capabilities the agent can invoke
pub trait AgentTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn execute(&self, input: &str, context: &ToolContext) -> ToolResult;
}

/// Context passed to tools during execution
#[derive(Clone, Debug)]
pub struct ToolContext {
    pub tick: u64,
    pub coherence: f64,
    pub agent_count: usize,
    pub edge_count: usize,
    pub recent_beliefs: Vec<String>,
}

/// Result from tool execution
#[derive(Clone, Debug)]
pub struct ToolResult {
    pub output: String,
    pub side_effects: Vec<ToolSideEffect>,
    pub confidence: f64,
}

#[derive(Clone, Debug)]
pub enum ToolSideEffect {
    AddBelief {
        content: String,
        confidence: f64,
    },
    RecordExperience {
        description: String,
        outcome_positive: bool,
    },
    AddOntology {
        concept: String,
        instance: String,
    },
    LinkAgents {
        a: usize,
        b: usize,
        weight: f32,
    },
    LogMessage(String),
}

/// Tool registry — manages available tools (ZeroClaw pattern)
pub struct ToolRegistry {
    tools: Vec<Box<dyn AgentTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: Box<dyn AgentTool>) {
        self.tools.push(tool);
    }

    pub fn find(&self, name: &str) -> Option<&dyn AgentTool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    pub fn list(&self) -> Vec<(&str, &str)> {
        self.tools
            .iter()
            .map(|t| (t.name(), t.description()))
            .collect()
    }

    pub fn execute(&self, name: &str, input: &str, context: &ToolContext) -> Option<ToolResult> {
        self.find(name).map(|tool| tool.execute(input, context))
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

// ── Built-in tools ────────────────────────────────────────────────────

pub struct QueryCoherenceTool;
impl AgentTool for QueryCoherenceTool {
    fn name(&self) -> &str {
        "query_coherence"
    }
    fn description(&self) -> &str {
        "Query current system coherence and structure metrics"
    }
    fn execute(&self, _input: &str, ctx: &ToolContext) -> ToolResult {
        ToolResult {
            output: format!(
                "Coherence: {:.4}, Agents: {}, Edges: {}, Tick: {}",
                ctx.coherence, ctx.agent_count, ctx.edge_count, ctx.tick
            ),
            side_effects: vec![],
            confidence: 1.0,
        }
    }
}

pub struct IntrospectBeliefsTool;
impl AgentTool for IntrospectBeliefsTool {
    fn name(&self) -> &str {
        "introspect_beliefs"
    }
    fn description(&self) -> &str {
        "List current system beliefs and their confidence scores"
    }
    fn execute(&self, _input: &str, ctx: &ToolContext) -> ToolResult {
        if ctx.recent_beliefs.is_empty() {
            return ToolResult {
                output: "No beliefs formed yet.".into(),
                side_effects: vec![],
                confidence: 1.0,
            };
        }
        let belief_list = ctx
            .recent_beliefs
            .iter()
            .enumerate()
            .map(|(i, b)| format!("  {}. {}", i + 1, b))
            .collect::<Vec<_>>()
            .join("\n");
        ToolResult {
            output: format!("Current beliefs:\n{}", belief_list),
            side_effects: vec![],
            confidence: 1.0,
        }
    }
}

pub struct AnalyzePatternsTool;
impl AgentTool for AnalyzePatternsTool {
    fn name(&self) -> &str {
        "analyze_patterns"
    }
    fn description(&self) -> &str {
        "Analyze recurring patterns in system evolution"
    }
    fn execute(&self, input: &str, ctx: &ToolContext) -> ToolResult {
        let pattern_analysis = format!(
            "Pattern analysis for '{}': System at tick {} with coherence {:.4}. \
             {} agents connected by {} edges.",
            input, ctx.tick, ctx.coherence, ctx.agent_count, ctx.edge_count
        );
        ToolResult {
            output: pattern_analysis,
            side_effects: vec![ToolSideEffect::RecordExperience {
                description: format!("Analyzed patterns: {}", input),
                outcome_positive: true,
            }],
            confidence: 0.7,
        }
    }
}

pub struct ProposeImprovementTool;
impl AgentTool for ProposeImprovementTool {
    fn name(&self) -> &str {
        "propose_improvement"
    }
    fn description(&self) -> &str {
        "Propose a structural improvement to the hypergraph"
    }
    fn execute(&self, input: &str, ctx: &ToolContext) -> ToolResult {
        let proposal = format!(
            "Improvement proposal based on '{}': With coherence at {:.4}, \
             suggest strengthening weakest connections between agent clusters.",
            input, ctx.coherence
        );
        ToolResult {
            output: proposal,
            side_effects: vec![
                ToolSideEffect::AddBelief {
                    content: format!("Proposed improvement: {}", input),
                    confidence: 0.6,
                },
                ToolSideEffect::LogMessage(format!("Improvement proposed: {}", input)),
            ],
            confidence: 0.6,
        }
    }
}

pub struct PlanGraphActionTool;
impl AgentTool for PlanGraphActionTool {
    fn name(&self) -> &str {
        "plan_graph_action"
    }

    fn description(&self) -> &str {
        "Choose the best graph tool for a request: cypher-like query, columnar scan, ANN vector search, or external scan"
    }

    fn execute(&self, input: &str, _ctx: &ToolContext) -> ToolResult {
        let plan = GraphRuntime::plan(input);
        ToolResult {
            output: format!(
                "Chosen tool: {:?}\nRationale: {}\nRewritten query: {}",
                plan.tool, plan.rationale, plan.rewritten_query
            ),
            side_effects: vec![],
            confidence: 0.85,
        }
    }
}

/// Create a default tool registry with built-in tools
pub fn default_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(QueryCoherenceTool));
    registry.register(Box::new(IntrospectBeliefsTool));
    registry.register(Box::new(AnalyzePatternsTool));
    registry.register(Box::new(ProposeImprovementTool));
    registry.register(Box::new(PlanGraphActionTool));

    // Register Pi Agent coding tools for Coder agents
    for tool in create_pi_tools() {
        registry.register(tool);
    }

    registry
}

#[cfg(test)]
mod typed_claim_tests {
    use super::*;

    #[test]
    fn classify_belief_revision() {
        assert_eq!(
            classify_query_intent("We revised our belief that the API was stable"),
            MemoryRetrievalIntent::BeliefRevision
        );
    }

    #[test]
    fn classify_temporal() {
        assert_eq!(
            classify_query_intent("What happened last week before the deploy?"),
            MemoryRetrievalIntent::Temporal
        );
    }

    #[test]
    fn network_match_prefers_belief_under_revision_intent() {
        let i = MemoryRetrievalIntent::BeliefRevision;
        assert!(network_claim_match(i, &MemoryNetwork::Belief)
            > network_claim_match(i, &MemoryNetwork::Experience));
    }

    #[test]
    fn network_match_prefers_experience_under_experiential() {
        let i = MemoryRetrievalIntent::Experiential;
        assert!(network_claim_match(i, &MemoryNetwork::Experience)
            > network_claim_match(i, &MemoryNetwork::WorldFact));
    }
}
