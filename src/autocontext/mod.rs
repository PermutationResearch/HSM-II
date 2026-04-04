//! AutoContext — Unified closed-loop learning system for HSM-II
//!
//! Unifies CASS (skill learning), optimize_anything (evolutionary search),
//! and ~/.hsmii/ persistence into one cohesive system.
//!
//! Architecture (inspired by greyhaven-ai/autocontext):
//!   Competitor → Analyst → Coach → Curator
//!
//! - Playbooks: Validated multi-step strategies (richer than CASS skills)
//! - Hints: Contextual guidance for future runs
//! - Validation: Staged pipeline (Unit → Integration → Staged) with rollback
//! - Distillation: Frontier/local model routing
//! - Persistence: JSON to ~/.hsmii/autocontext/

pub mod agent_loop;
pub mod distillation;
pub mod harness;
pub mod storage;
pub mod validation;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use agent_loop::{AutoContextLoop, LoopConfig, LoopResult, RetrievedContext};
pub use distillation::{DistillationRouter, FrontierConfig, ModelTier, TrainingExample};
pub use harness::{HarnessResult, PlaybookHarness, ScenarioBuilder};
pub use storage::{AutoContextStore, StorageConfig};
pub use validation::{ValidationPipeline, ValidationResult, ValidationStage};

// ── Core Types ───────────────────────────────────────────────────────────────

/// A validated, multi-step strategy distilled from successful runs.
/// Unlike CASS Skills (single behavioral patterns), Playbooks are
/// ordered sequences of Steps that solve a class of problems.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playbook {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<Step>,
    /// The scenario class this playbook addresses (keyword pattern)
    pub scenario_pattern: String,
    /// Quality score from validation pipeline (0.0-1.0)
    pub quality_score: f64,
    /// Number of successful executions
    pub success_count: u64,
    /// Number of failed executions
    pub failure_count: u64,
    /// Generation that produced this playbook
    pub origin_generation: u64,
    /// When this playbook was last validated
    pub last_validated: u64,
    /// Validation stage reached
    pub validation_stage: ValidationStage,
    /// CASS skill IDs that this playbook relates to
    pub related_skill_ids: Vec<String>,
    /// Tags for retrieval
    pub tags: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Playbook {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        scenario: impl Into<String>,
    ) -> Self {
        let now = current_timestamp();
        Self {
            id: format!("pb_{}", uuid::Uuid::new_v4()),
            name: name.into(),
            description: description.into(),
            steps: vec![],
            scenario_pattern: scenario.into(),
            quality_score: 0.0,
            success_count: 0,
            failure_count: 0,
            origin_generation: 0,
            last_validated: 0,
            validation_stage: ValidationStage::Unit,
            related_skill_ids: vec![],
            tags: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_steps(mut self, steps: Vec<Step>) -> Self {
        self.steps = steps;
        self
    }

    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            0.0
        } else {
            self.success_count as f64 / total as f64
        }
    }

    /// Check if scenario keywords match a query.
    pub fn matches_scenario(&self, query: &str) -> f64 {
        let query_lower = query.to_lowercase();
        let pattern_lower = self.scenario_pattern.to_lowercase();
        let pattern_words: Vec<&str> = pattern_lower.split_whitespace().collect();
        if pattern_words.is_empty() {
            return 0.0;
        }
        let matched = pattern_words
            .iter()
            .filter(|w| query_lower.contains(*w))
            .count();
        matched as f64 / pattern_words.len() as f64
    }
}

/// A single executable step within a playbook.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub index: usize,
    pub description: String,
    /// Tool name to execute (matches ToolRegistry names)
    pub tool_name: Option<String>,
    /// Parameters template (may contain {{placeholders}})
    pub tool_params: Option<serde_json::Value>,
    /// LLM prompt template for non-tool steps
    pub prompt_template: Option<String>,
    /// Success criteria (evaluated by Analyst)
    pub success_criteria: String,
    /// Maximum retry count for this step
    pub max_retries: u32,
    /// Estimated duration in seconds
    pub estimated_duration_secs: u64,
}

impl Step {
    pub fn tool_step(
        index: usize,
        description: impl Into<String>,
        tool_name: impl Into<String>,
        params: serde_json::Value,
        criteria: impl Into<String>,
    ) -> Self {
        Self {
            index,
            description: description.into(),
            tool_name: Some(tool_name.into()),
            tool_params: Some(params),
            prompt_template: None,
            success_criteria: criteria.into(),
            max_retries: 2,
            estimated_duration_secs: 30,
        }
    }

    pub fn llm_step(
        index: usize,
        description: impl Into<String>,
        prompt: impl Into<String>,
        criteria: impl Into<String>,
    ) -> Self {
        Self {
            index,
            description: description.into(),
            tool_name: None,
            tool_params: None,
            prompt_template: Some(prompt.into()),
            success_criteria: criteria.into(),
            max_retries: 1,
            estimated_duration_secs: 15,
        }
    }
}

/// Contextual guidance for future runs — lighter than a Playbook.
/// Hints are lessons learned that don't merit a full multi-step strategy
/// but should influence future behavior.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hint {
    pub id: String,
    pub content: String,
    /// What scenario triggers this hint
    pub trigger_pattern: String,
    /// Confidence in this hint (0.0-1.0)
    pub confidence: f64,
    /// How many times this hint was applied
    pub application_count: u64,
    /// Running success rate when hint was applied
    pub success_rate: f64,
    /// Generation that produced this hint
    pub origin_generation: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Hint {
    pub fn new(content: impl Into<String>, trigger: impl Into<String>, confidence: f64) -> Self {
        let now = current_timestamp();
        Self {
            id: format!("hint_{}", uuid::Uuid::new_v4()),
            content: content.into(),
            trigger_pattern: trigger.into(),
            confidence: confidence.clamp(0.0, 1.0),
            application_count: 0,
            success_rate: 0.0,
            origin_generation: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// Check if this hint's trigger matches a query.
    pub fn matches_trigger(&self, query: &str) -> f64 {
        let query_lower = query.to_lowercase();
        let trigger_lower = self.trigger_pattern.to_lowercase();
        let trigger_words: Vec<&str> = trigger_lower.split_whitespace().collect();
        if trigger_words.is_empty() {
            return 0.0;
        }
        let matched = trigger_words
            .iter()
            .filter(|w| query_lower.contains(*w))
            .count();
        matched as f64 / trigger_words.len() as f64
    }
}

/// A proposed strategy (may become a Playbook if validated).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Strategy {
    pub id: String,
    pub description: String,
    pub steps: Vec<Step>,
    /// Where this strategy came from
    pub source: StrategySource,
    /// Parent IDs (for lineage tracking)
    pub parents: Vec<String>,
}

impl Strategy {
    pub fn new(description: impl Into<String>, steps: Vec<Step>, source: StrategySource) -> Self {
        Self {
            id: format!("strat_{}", uuid::Uuid::new_v4()),
            description: description.into(),
            steps,
            source,
            parents: vec![],
        }
    }

    pub fn with_parents(mut self, parents: Vec<String>) -> Self {
        self.parents = parents;
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StrategySource {
    /// Generated from scratch by Competitor
    Proposed,
    /// Mutated from an existing playbook
    Mutated { parent_playbook_id: String },
    /// Crossover of two strategies
    Crossover { parent_ids: Vec<String> },
    /// Refined by Coach based on analysis
    Coached { original_id: String },
}

/// A single generation in the learning loop.
/// Tracks one full cycle of: Competitor → Analyst → Coach → Curator.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generation {
    pub id: u64,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub scenario: String,
    pub run_records: Vec<RunRecord>,
    /// Best score achieved this generation
    pub best_score: f64,
    /// Number of playbooks created/updated
    pub playbooks_affected: usize,
    /// Number of hints created/updated
    pub hints_affected: usize,
    /// Whether this generation improved on the previous
    pub improved: bool,
}

impl Generation {
    pub fn new(id: u64, scenario: impl Into<String>) -> Self {
        Self {
            id,
            started_at: current_timestamp(),
            completed_at: None,
            scenario: scenario.into(),
            run_records: vec![],
            best_score: 0.0,
            playbooks_affected: 0,
            hints_affected: 0,
            improved: false,
        }
    }
}

/// Record of a single strategy execution within a generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    /// Strategy that was executed
    pub strategy: Strategy,
    /// Evaluation scores from Analyst (dimension -> score)
    pub scores: HashMap<String, f64>,
    /// Composite score
    pub composite_score: f64,
    /// Artifacts produced (tool outputs, LLM responses)
    pub artifacts: Vec<RunArtifact>,
    /// Feedback from Analyst
    pub feedback: String,
    /// Duration in milliseconds
    pub duration_ms: u64,
    pub timestamp: u64,
}

impl RunRecord {
    pub fn new(strategy: Strategy) -> Self {
        Self {
            id: format!("run_{}", uuid::Uuid::new_v4()),
            strategy,
            scores: HashMap::new(),
            composite_score: 0.0,
            artifacts: vec![],
            feedback: String::new(),
            duration_ms: 0,
            timestamp: current_timestamp(),
        }
    }
}

/// An artifact collected during a run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunArtifact {
    pub step_index: usize,
    pub artifact_type: ArtifactType,
    pub content: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ArtifactType {
    ToolOutput,
    LlmResponse,
    IntermediateResult,
    ErrorLog,
}

/// The persistent knowledge base containing all playbooks and hints.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct KnowledgeBase {
    pub playbooks: Vec<Playbook>,
    pub hints: Vec<Hint>,
    pub total_generations: u64,
    pub total_runs: u64,
    pub last_updated: u64,
}

impl KnowledgeBase {
    pub fn new() -> Self {
        Self::default()
    }

    /// Find playbooks matching a scenario query, sorted by relevance.
    pub fn find_playbooks(&self, query: &str, top_k: usize) -> Vec<&Playbook> {
        let mut scored: Vec<(&Playbook, f64)> = self
            .playbooks
            .iter()
            .map(|pb| {
                let relevance = pb.matches_scenario(query);
                let quality_boost = pb.quality_score * 0.3;
                let success_boost = pb.success_rate() * 0.2;
                (pb, relevance + quality_boost + success_boost)
            })
            .filter(|(_, score)| *score > 0.1)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(top_k).map(|(pb, _)| pb).collect()
    }

    /// Find hints matching a query, sorted by relevance.
    pub fn find_hints(&self, query: &str, top_k: usize) -> Vec<&Hint> {
        let mut scored: Vec<(&Hint, f64)> = self
            .hints
            .iter()
            .map(|h| {
                let relevance = h.matches_trigger(query);
                let confidence_boost = h.confidence * 0.3;
                (h, relevance + confidence_boost)
            })
            .filter(|(_, score)| *score > 0.1)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(top_k).map(|(h, _)| h).collect()
    }

    /// Add or update a playbook.
    pub fn upsert_playbook(&mut self, playbook: Playbook) {
        if let Some(existing) = self.playbooks.iter_mut().find(|p| p.id == playbook.id) {
            *existing = playbook;
        } else {
            self.playbooks.push(playbook);
        }
        self.last_updated = current_timestamp();
    }

    /// Add or update a hint.
    pub fn upsert_hint(&mut self, hint: Hint) {
        if let Some(existing) = self.hints.iter_mut().find(|h| h.id == hint.id) {
            *existing = hint;
        } else {
            self.hints.push(hint);
        }
        self.last_updated = current_timestamp();
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

pub fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playbook_creation() {
        let pb = Playbook::new("Search Strategy", "Search and summarize", "web search news");
        assert!(pb.id.starts_with("pb_"));
        assert_eq!(pb.steps.len(), 0);
        assert_eq!(pb.success_rate(), 0.0);
    }

    #[test]
    fn test_playbook_scenario_matching() {
        let pb = Playbook::new("Search", "desc", "web search news");
        assert!(pb.matches_scenario("search for web news") > 0.5);
        assert!(pb.matches_scenario("completely unrelated query") < 0.1);
    }

    #[test]
    fn test_hint_creation() {
        let h = Hint::new("Use grep before read", "code search file", 0.8);
        assert!(h.id.starts_with("hint_"));
        assert_eq!(h.confidence, 0.8);
    }

    #[test]
    fn test_hint_trigger_matching() {
        let h = Hint::new("content", "code search file", 0.8);
        assert!(h.matches_trigger("search for code in file") > 0.5);
        assert!(h.matches_trigger("weather forecast") < 0.1);
    }

    #[test]
    fn test_knowledge_base_find() {
        let mut kb = KnowledgeBase::new();
        kb.upsert_playbook(Playbook::new("Search", "desc", "web search"));
        kb.upsert_playbook(Playbook::new("Code", "desc", "code refactor rust"));

        let results = kb.find_playbooks("search the web", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Search");
    }

    #[test]
    fn test_step_constructors() {
        let tool_step = Step::tool_step(
            0,
            "Search web",
            "web_search",
            serde_json::json!({"query": "test"}),
            "results found",
        );
        assert_eq!(tool_step.tool_name, Some("web_search".to_string()));

        let llm_step = Step::llm_step(1, "Summarize", "Summarize: {{input}}", "summary produced");
        assert!(llm_step.prompt_template.is_some());
        assert!(llm_step.tool_name.is_none());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let pb = Playbook::new("Test", "desc", "pattern").with_steps(vec![Step::tool_step(
            0,
            "step1",
            "grep",
            serde_json::json!({}),
            "ok",
        )]);

        let json = serde_json::to_string(&pb).unwrap();
        let deserialized: Playbook = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "Test");
        assert_eq!(deserialized.steps.len(), 1);
    }

    #[test]
    fn test_knowledge_base_upsert() {
        let mut kb = KnowledgeBase::new();
        let mut pb = Playbook::new("Test", "desc", "pattern");
        let id = pb.id.clone();
        kb.upsert_playbook(pb.clone());
        assert_eq!(kb.playbooks.len(), 1);

        pb.quality_score = 0.9;
        kb.upsert_playbook(pb);
        assert_eq!(kb.playbooks.len(), 1);
        assert_eq!(kb.playbooks[0].quality_score, 0.9);
    }
}
