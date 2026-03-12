//! LCM: Lossless Context Management
//!
//! A deterministic architecture for LLM memory management based on:
//! - Hierarchical Summary DAG (Directed Acyclic Graph)
//! - Dual-state memory: Immutable Store + Active Context
//! - Lossless retrievability via provenance pointers
//! - Operator-level recursion (llm_map, agentic_map)
//! - Two-tier compaction (soft/hard thresholds)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod operators;
pub mod storage;

pub use operators::{AgenticMap, LlmMap};
pub use storage::{LcmStorage, SqliteStorage};

/// Unique identifier for nodes in the DAG
pub type NodeId = String;

/// Types of nodes in the context DAG
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    /// Original message (user, assistant, or tool)
    Message {
        role: String,
        content: String,
        tokens: usize,
    },
    /// Summary of message spans (leaf or condensed)
    Summary {
        kind: SummaryKind,
        summary_text: String,
        tokens: usize,
        /// IDs of source nodes this summary covers
        source_ids: Vec<NodeId>,
    },
    /// Large file reference with exploration summary
    LargeFile {
        path: String,
        file_id: String,
        mime_type: String,
        token_count: usize,
        exploration_summary: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SummaryKind {
    /// Direct summary of a span of messages
    Leaf,
    /// Higher-order summary of other summaries
    Condensed,
}

/// A node in the hierarchical DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    pub id: NodeId,
    pub node_type: NodeType,
    pub created_at: DateTime<Utc>,
    /// Parent nodes (for summaries, this is what they summarize)
    pub parents: Vec<NodeId>,
    /// Child nodes (for navigation)
    pub children: Vec<NodeId>,
    /// Whether this node is in the active context window
    pub in_active_context: bool,
    /// Depth in the DAG (0 for root messages)
    pub depth: u32,
}

/// The hierarchical DAG structure
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextDag {
    /// All nodes indexed by ID
    pub nodes: HashMap<NodeId, DagNode>,
    /// Root nodes (original messages not covered by any summary)
    pub roots: Vec<NodeId>,
    /// Current leaf nodes (most recent)
    pub leaves: Vec<NodeId>,
    /// Total estimated tokens in the DAG
    pub total_tokens: usize,
}

/// Configuration for LCM thresholds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LcmConfig {
    /// Soft threshold: trigger async compaction (% of context_limit)
    pub soft_threshold_percent: f32,
    /// Hard threshold: block and force compaction (% of context_limit)
    pub hard_threshold_percent: f32,
    /// Context window limit (e.g., 4096, 8192, 32768, 262144)
    pub context_limit: usize,
    /// Large file threshold in tokens (> this gets exploration summary)
    pub large_file_threshold: usize,
    /// Number of recent messages to keep uncompressed
    pub keep_recent_count: usize,
    /// Max depth for summary condensation
    pub max_summary_depth: u32,
}

impl Default for LcmConfig {
    fn default() -> Self {
        Self {
            soft_threshold_percent: 0.75,
            hard_threshold_percent: 0.85,
            context_limit: 262_144, // 262K tokens
            large_file_threshold: 25_000,
            keep_recent_count: 6,
            max_summary_depth: 3,
        }
    }
}

/// Statistics for context usage
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextStats {
    pub message_count: usize,
    pub summary_count: usize,
    pub large_file_count: usize,
    pub estimated_tokens: usize,
    pub limit_tokens: usize,
    pub percent_used: f32,
    pub regular_tokens: usize,
    pub cache_read_tokens: usize,
    pub cache_write_tokens: usize,
    pub has_summary: bool,
}

/// LCM Context Manager
pub struct LcmContext {
    /// The hierarchical DAG
    pub dag: ContextDag,
    /// Configuration
    pub config: LcmConfig,
    /// Storage backend
    pub storage: Box<dyn LcmStorage + Send + Sync>,
    /// Current active context window (node IDs in order)
    pub active_window: Vec<NodeId>,
    /// Recursion depth for sub-agents
    pub recursion_depth: u32,
    /// Maximum allowed recursion
    pub max_recursion_depth: u32,
}

impl LcmContext {
    pub async fn new(storage: Box<dyn LcmStorage + Send + Sync>) -> anyhow::Result<Self> {
        let dag = storage
            .load_dag()
            .await
            .unwrap_or_default()
            .unwrap_or_default();

        Ok(Self {
            dag,
            config: LcmConfig::default(),
            storage,
            active_window: Vec::new(),
            recursion_depth: 0,
            max_recursion_depth: 10,
        })
    }

    /// Add a message to the context
    pub async fn add_message(&mut self, role: &str, content: &str) -> anyhow::Result<NodeId> {
        let tokens = Self::estimate_tokens(content);
        let node_id = format!(
            "msg_{}_{}",
            Utc::now().timestamp_millis(),
            self.dag.nodes.len()
        );

        let node = DagNode {
            id: node_id.clone(),
            node_type: NodeType::Message {
                role: role.to_string(),
                content: content.to_string(),
                tokens,
            },
            created_at: Utc::now(),
            parents: Vec::new(),
            children: Vec::new(),
            in_active_context: true,
            depth: 0,
        };

        self.dag.nodes.insert(node_id.clone(), node);
        self.dag.leaves.push(node_id.clone());
        self.dag.total_tokens += tokens;
        self.active_window.push(node_id.clone());

        // Persist to storage
        self.storage
            .save_node(&node_id, &self.dag.nodes[&node_id])
            .await?;

        Ok(node_id)
    }

    /// Add a large file with exploration summary
    pub async fn add_large_file(
        &mut self,
        path: &str,
        content: &str,
        mime_type: &str,
    ) -> anyhow::Result<NodeId> {
        let tokens = Self::estimate_tokens(content);
        let file_id = format!(
            "file_{}_{}",
            Utc::now().timestamp_millis(),
            self.dag.nodes.len()
        );

        // Generate exploration summary based on file type
        let exploration_summary = self
            .generate_exploration_summary(path, content, mime_type)
            .await;

        let node = DagNode {
            id: file_id.clone(),
            node_type: NodeType::LargeFile {
                path: path.to_string(),
                file_id: file_id.clone(),
                mime_type: mime_type.to_string(),
                token_count: tokens,
                exploration_summary,
            },
            created_at: Utc::now(),
            parents: Vec::new(),
            children: Vec::new(),
            in_active_context: true,
            depth: 0,
        };

        self.dag.nodes.insert(file_id.clone(), node);
        self.dag.leaves.push(file_id.clone());
        // Large files don't count toward active context tokens (they're external)

        self.storage
            .save_node(&file_id, &self.dag.nodes[&file_id])
            .await?;

        Ok(file_id)
    }

    /// Generate type-aware exploration summary for large files
    async fn generate_exploration_summary(
        &self,
        path: &str,
        content: &str,
        mime_type: &str,
    ) -> String {
        match mime_type {
            "application/json" | "text/json" => Self::summarize_json(content),
            "text/csv" => Self::summarize_csv(content),
            "application/sql" | "text/sql" => Self::summarize_sql(content),
            "text/x-python" | "text/x-rust" | "text/javascript" => {
                Self::summarize_code(path, content)
            }
            _ => Self::summarize_text(content),
        }
    }

    fn summarize_json(content: &str) -> String {
        // Extract schema and shape
        let mut keys = std::collections::HashSet::new();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(content) {
            Self::extract_json_keys(&value, &mut keys, "");
        }
        format!(
            "[JSON] Schema keys: {}. Size: {} chars",
            keys.into_iter().take(10).collect::<Vec<_>>().join(", "),
            content.len()
        )
    }

    fn extract_json_keys(
        value: &serde_json::Value,
        keys: &mut std::collections::HashSet<String>,
        prefix: &str,
    ) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    let full_key = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{}.{}", prefix, k)
                    };
                    keys.insert(full_key.clone());
                    Self::extract_json_keys(v, keys, &full_key);
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, v) in arr.iter().take(3).enumerate() {
                    Self::extract_json_keys(v, keys, &format!("{}[{}]", prefix, i));
                }
            }
            _ => {}
        }
    }

    fn summarize_csv(content: &str) -> String {
        let lines: Vec<&str> = content.lines().take(5).collect();
        let columns = lines.first().map(|l| l.split(',').count()).unwrap_or(0);
        let rows = content.lines().count();
        format!(
            "[CSV] {} columns, ~{} rows. Headers: {}",
            columns,
            rows,
            lines
                .first()
                .unwrap_or(&"N/A")
                .chars()
                .take(100)
                .collect::<String>()
        )
    }

    fn summarize_sql(content: &str) -> String {
        let tables: Vec<&str> = content
            .lines()
            .filter(|l| l.to_uppercase().contains("CREATE TABLE"))
            .take(5)
            .collect();
        format!(
            "[SQL] {} tables defined. Preview: {}",
            tables.len(),
            tables.join("; ").chars().take(150).collect::<String>()
        )
    }

    fn summarize_code(path: &str, content: &str) -> String {
        let functions: Vec<&str> = content
            .lines()
            .filter(|l| {
                l.contains("fn ")
                    || l.contains("function ")
                    || l.contains("def ")
                    || l.contains("class ")
                    || l.contains("struct ")
                    || l.contains("impl ")
            })
            .take(10)
            .map(|l| l.trim())
            .collect();
        format!(
            "[Code: {}] {} functions/types. Signatures: {}",
            path,
            functions.len(),
            functions.join("; ").chars().take(200).collect::<String>()
        )
    }

    fn summarize_text(content: &str) -> String {
        let words: Vec<&str> = content.split_whitespace().take(50).collect();
        format!(
            "[Text] {} chars. Preview: {}...",
            content.len(),
            words.join(" ")
        )
    }

    /// Check if compaction is needed and perform if so
    pub async fn maybe_compact(&mut self) -> CompactionResult {
        let soft_limit =
            (self.config.context_limit as f32 * self.config.soft_threshold_percent) as usize;
        let hard_limit =
            (self.config.context_limit as f32 * self.config.hard_threshold_percent) as usize;

        let active_tokens: usize = self
            .active_window
            .iter()
            .filter_map(|id| self.dag.nodes.get(id))
            .map(|node| match &node.node_type {
                NodeType::Message { tokens, .. } => *tokens,
                NodeType::LargeFile { .. } => 0, // External storage
                NodeType::Summary { tokens, .. } => *tokens,
            })
            .sum();

        if active_tokens < soft_limit {
            return CompactionResult::NotNeeded;
        }

        if active_tokens >= hard_limit {
            // Hard limit: block and force compaction
            self.perform_compaction(true).await
        } else {
            // Soft limit: trigger async compaction
            CompactionResult::AsyncTriggered
        }
    }

    /// Perform compaction of older messages into summaries
    async fn perform_compaction(&mut self, _blocking: bool) -> CompactionResult {
        let keep_count = self.config.keep_recent_count;
        if self.active_window.len() <= keep_count {
            return CompactionResult::NotEnoughMessages;
        }

        // Calculate current active tokens
        let active_tokens: usize = self
            .active_window
            .iter()
            .filter_map(|id| self.dag.nodes.get(id))
            .map(|node| match &node.node_type {
                NodeType::Message { tokens, .. } => *tokens,
                NodeType::LargeFile { .. } => 0,
                NodeType::Summary { tokens, .. } => *tokens,
            })
            .sum();

        // Messages to summarize (oldest ones)
        let to_summarize: Vec<NodeId> = self
            .active_window
            .iter()
            .take(self.active_window.len() - keep_count)
            .cloned()
            .collect();

        if to_summarize.is_empty() {
            return CompactionResult::NotEnoughMessages;
        }

        // Create summary text from messages
        let summary_text = self.create_summary(&to_summarize).await;
        let summary_tokens = Self::estimate_tokens(&summary_text);

        // Create summary node
        let summary_id = format!(
            "sum_{}_{}",
            Utc::now().timestamp_millis(),
            self.dag.nodes.len()
        );
        let summary_node = DagNode {
            id: summary_id.clone(),
            node_type: NodeType::Summary {
                kind: SummaryKind::Leaf,
                summary_text,
                tokens: summary_tokens,
                source_ids: to_summarize.clone(),
            },
            created_at: Utc::now(),
            parents: to_summarize.clone(),
            children: Vec::new(),
            in_active_context: true,
            depth: 1,
        };

        // Update parent nodes to point to this summary
        for parent_id in &to_summarize {
            if let Some(parent) = self.dag.nodes.get_mut(parent_id) {
                parent.children.push(summary_id.clone());
                parent.in_active_context = false;
            }
        }

        // Add summary to DAG
        self.dag.nodes.insert(summary_id.clone(), summary_node);
        self.dag.total_tokens = self.dag.total_tokens.saturating_sub(
            to_summarize
                .iter()
                .filter_map(|id| self.dag.nodes.get(id))
                .map(|n| match &n.node_type {
                    NodeType::Message { tokens, .. } => *tokens,
                    _ => 0,
                })
                .sum::<usize>(),
        ) + summary_tokens;

        // Update active window: replace old messages with summary
        self.active_window = self
            .active_window
            .iter()
            .skip(to_summarize.len())
            .cloned()
            .collect();
        self.active_window.insert(0, summary_id.clone());

        // Persist
        self.storage
            .save_node(&summary_id, &self.dag.nodes[&summary_id])
            .await
            .ok();
        self.storage.save_dag(&self.dag).await.ok();

        // Try to condense if we have multiple summaries at same level
        self.condense_summaries().await;

        CompactionResult::Compacted {
            summarized_count: to_summarize.len(),
            summary_id,
            saved_tokens: active_tokens - summary_tokens,
        }
    }

    /// Create a summary text from a collection of nodes
    async fn create_summary(&self, node_ids: &[NodeId]) -> String {
        let mut text = String::new();
        text.push_str("[Summary of previous conversation]\n");

        for node_id in node_ids {
            if let Some(node) = self.dag.nodes.get(node_id) {
                match &node.node_type {
                    NodeType::Message { role, content, .. } => {
                        text.push_str(&format!(
                            "{}: {}\n",
                            role,
                            content.chars().take(200).collect::<String>()
                        ));
                    }
                    NodeType::Summary { summary_text, .. } => {
                        text.push_str(summary_text);
                        text.push('\n');
                    }
                    _ => {}
                }
            }
        }

        text.push_str("[End summary]");
        text
    }

    /// Condense multiple summaries into higher-order summaries
    async fn condense_summaries(&mut self) {
        // Find summaries at the same depth that can be condensed
        let summaries_at_depth: Vec<(u32, Vec<NodeId>)> = self.find_summaries_by_depth();

        for (depth, summary_ids) in summaries_at_depth {
            if depth >= self.config.max_summary_depth {
                continue;
            }

            if summary_ids.len() >= 3 {
                // Create condensed summary
                let condensed_text = format!(
                    "[Condensed summary of {} prior summaries]",
                    summary_ids.len()
                );
                let condensed_tokens = Self::estimate_tokens(&condensed_text);

                let condensed_id = format!("cond_{}_{}", Utc::now().timestamp_millis(), depth);
                let condensed_node = DagNode {
                    id: condensed_id.clone(),
                    node_type: NodeType::Summary {
                        kind: SummaryKind::Condensed,
                        summary_text: condensed_text,
                        tokens: condensed_tokens,
                        source_ids: summary_ids.clone(),
                    },
                    created_at: Utc::now(),
                    parents: summary_ids.clone(),
                    children: Vec::new(),
                    in_active_context: true,
                    depth: depth + 1,
                };

                for sid in &summary_ids {
                    if let Some(node) = self.dag.nodes.get_mut(sid) {
                        node.children.push(condensed_id.clone());
                        node.in_active_context = false;
                    }
                }

                self.dag.nodes.insert(condensed_id.clone(), condensed_node);

                // Update active window
                self.active_window.retain(|id| !summary_ids.contains(id));
                self.active_window.insert(0, condensed_id.clone());

                self.storage
                    .save_node(&condensed_id, &self.dag.nodes[&condensed_id])
                    .await
                    .ok();
            }
        }
    }

    fn find_summaries_by_depth(&self) -> Vec<(u32, Vec<NodeId>)> {
        let mut by_depth: HashMap<u32, Vec<NodeId>> = HashMap::new();

        for (id, node) in &self.dag.nodes {
            if let NodeType::Summary { .. } = &node.node_type {
                if node.in_active_context {
                    by_depth.entry(node.depth).or_default().push(id.clone());
                }
            }
        }

        by_depth.into_iter().collect()
    }

    /// Expand a summary back into its constituent messages
    pub fn expand_summary(&self, summary_id: &NodeId) -> Option<Vec<(String, String)>> {
        let node = self.dag.nodes.get(summary_id)?;

        match &node.node_type {
            NodeType::Summary { source_ids, .. } => {
                let mut messages = Vec::new();
                for source_id in source_ids {
                    if let Some(source_node) = self.dag.nodes.get(source_id) {
                        match &source_node.node_type {
                            NodeType::Message { role, content, .. } => {
                                messages.push((role.clone(), content.clone()));
                            }
                            NodeType::Summary { .. } => {
                                // Recursively expand
                                if let Some(sub_messages) = self.expand_summary(source_id) {
                                    messages.extend(sub_messages);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some(messages)
            }
            _ => None,
        }
    }

    /// Search the immutable store with regex
    pub fn grep(&self, pattern: &str) -> Option<Vec<(NodeId, String)>> {
        let regex = regex::Regex::new(pattern).ok()?;
        let mut results = Vec::new();

        for (id, node) in &self.dag.nodes {
            match &node.node_type {
                NodeType::Message { content, .. } => {
                    if regex.is_match(content) {
                        results.push((id.clone(), content.clone()));
                    }
                }
                NodeType::Summary { summary_text, .. } => {
                    if regex.is_match(summary_text) {
                        results.push((id.clone(), summary_text.clone()));
                    }
                }
                NodeType::LargeFile {
                    exploration_summary,
                    ..
                } => {
                    if regex.is_match(exploration_summary) {
                        results.push((id.clone(), exploration_summary.clone()));
                    }
                }
            }
        }

        Some(results)
    }

    /// Build the active context for LLM consumption
    pub fn build_active_context(&self) -> String {
        let mut context = String::new();

        for node_id in &self.active_window {
            if let Some(node) = self.dag.nodes.get(node_id) {
                match &node.node_type {
                    NodeType::Message { role, content, .. } => {
                        context.push_str(&format!("{}: {}\n", role, content));
                    }
                    NodeType::Summary { summary_text, .. } => {
                        context.push_str(summary_text);
                        context.push('\n');
                    }
                    NodeType::LargeFile {
                        path,
                        exploration_summary,
                        ..
                    } => {
                        context.push_str(&format!("[File: {}] {}\n", path, exploration_summary));
                    }
                }
            }
        }

        context
    }

    /// Get statistics about context usage
    pub fn get_stats(&self) -> ContextStats {
        let mut message_count = 0;
        let mut summary_count = 0;
        let mut large_file_count = 0;
        let mut regular_tokens = 0;
        let mut cache_read_tokens = 0;
        let mut cache_write_tokens = 0;

        for node_id in &self.active_window {
            if let Some(node) = self.dag.nodes.get(node_id) {
                match &node.node_type {
                    NodeType::Message { tokens, .. } => {
                        message_count += 1;
                        regular_tokens += tokens;
                    }
                    NodeType::Summary { .. } => {
                        summary_count += 1;
                    }
                    NodeType::LargeFile { .. } => {
                        large_file_count += 1;
                    }
                }
            }
        }

        let estimated_tokens = self
            .active_window
            .iter()
            .filter_map(|id| self.dag.nodes.get(id))
            .map(|node| match &node.node_type {
                NodeType::Message { tokens, .. } => *tokens,
                NodeType::Summary { tokens, .. } => *tokens,
                _ => 0,
            })
            .sum();

        // Cache read/write approximation grounded in the real DAG:
        // - cache_read_tokens: summary tokens currently in active context window.
        // - cache_write_tokens: summary tokens materialized in DAG storage.
        for node_id in &self.active_window {
            if let Some(node) = self.dag.nodes.get(node_id) {
                if let NodeType::Summary { tokens, .. } = &node.node_type {
                    cache_read_tokens += *tokens;
                }
            }
        }
        for node in self.dag.nodes.values() {
            if let NodeType::Summary { tokens, .. } = &node.node_type {
                cache_write_tokens += *tokens;
            }
        }

        ContextStats {
            message_count,
            summary_count,
            large_file_count,
            estimated_tokens,
            limit_tokens: self.config.context_limit,
            percent_used: (estimated_tokens as f32 / self.config.context_limit as f32 * 100.0)
                .min(100.0),
            regular_tokens,
            cache_read_tokens,
            cache_write_tokens,
            has_summary: summary_count > 0,
        }
    }

    /// Estimate token count (rough approximation: ~4 chars per token)
    pub fn estimate_tokens(text: &str) -> usize {
        text.len() / 4
    }

    /// Check if we can spawn a sub-agent (recursion guard)
    pub fn can_delegate(&self) -> bool {
        self.recursion_depth < self.max_recursion_depth
    }

    /// Increment recursion depth when spawning sub-agent
    pub fn enter_sub_agent(&mut self) {
        self.recursion_depth += 1;
    }

    /// Decrement recursion depth when sub-agent completes
    pub fn exit_sub_agent(&mut self) {
        self.recursion_depth = self.recursion_depth.saturating_sub(1);
    }
}

/// Result of compaction attempt
#[derive(Debug)]
pub enum CompactionResult {
    NotNeeded,
    AsyncTriggered,
    Compacted {
        summarized_count: usize,
        summary_id: NodeId,
        saved_tokens: usize,
    },
    NotEnoughMessages,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_estimation() {
        assert_eq!(LcmContext::estimate_tokens("hello world"), 2); // 11 chars / 4 = 2
        assert_eq!(LcmContext::estimate_tokens(""), 0);
    }
}
