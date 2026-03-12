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
///
/// Results fused via Reciprocal Rank Fusion (RRF)

// ── Memory Entry ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: usize,
    pub content: String,
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
    pub fused: f64,
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

        let entry = MemoryEntry {
            id,
            content: content.to_string(),
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

        // Reciprocal Rank Fusion
        let fused = self.reciprocal_rank_fusion(
            &semantic_results,
            &keyword_results,
            &graph_results,
            &temporal_results,
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

    /// Reciprocal Rank Fusion: score = Σ 1/(k + rank_i) across strategies
    fn reciprocal_rank_fusion(
        &self,
        semantic: &[(usize, f64)],
        keyword: &[(usize, f64)],
        graph: &[(usize, f64)],
        temporal: &[(usize, f64)],
        k: usize,
    ) -> Vec<RecallResult> {
        let rrf_k = 60.0; // standard RRF constant

        let mut fused_scores: HashMap<usize, (f64, f64, f64, f64, f64)> = HashMap::new();

        // Weight each strategy
        let weights = [1.0, 0.7, 0.8, 0.5]; // semantic, keyword, graph, temporal

        for (rank, (id, score)) in semantic.iter().enumerate() {
            let entry = fused_scores.entry(*id).or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
            entry.0 = *score;
            entry.4 += weights[0] / (rrf_k + rank as f64 + 1.0);
        }
        for (rank, (id, score)) in keyword.iter().enumerate() {
            let entry = fused_scores.entry(*id).or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
            entry.1 = *score;
            entry.4 += weights[1] / (rrf_k + rank as f64 + 1.0);
        }
        for (rank, (id, score)) in graph.iter().enumerate() {
            let entry = fused_scores.entry(*id).or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
            entry.2 = *score;
            entry.4 += weights[2] / (rrf_k + rank as f64 + 1.0);
        }
        for (rank, (id, score)) in temporal.iter().enumerate() {
            let entry = fused_scores.entry(*id).or_insert((0.0, 0.0, 0.0, 0.0, 0.0));
            entry.3 = *score;
            entry.4 += weights[3] / (rrf_k + rank as f64 + 1.0);
        }

        let mut results: Vec<RecallResult> = fused_scores
            .into_iter()
            .filter_map(|(id, (sem, kw, gr, temp, fused))| {
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
