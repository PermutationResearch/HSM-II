//! AutoContext Agent Loop — Competitor → Analyst → Coach → Curator
//!
//! Orchestrates the closed-loop learning cycle:
//!   1. **Competitor**: Generates or mutates strategies for a scenario
//!   2. **Analyst**: Evaluates strategies, scores runs, produces feedback
//!   3. **Coach**: Refines top strategies based on analysis
//!   4. **Curator**: Persists validated playbooks & hints, prunes the weak
//!
//! Each `tick()` advances one generation for a given scenario.

use std::collections::HashMap;
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::ollama_client::OllamaClient;
use crate::tools::ToolRegistry;

use super::distillation::DistillationRouter;
use super::harness::PlaybookHarness;
use super::storage::AutoContextStore;
use super::validation::{ValidationPipeline, ValidationResult};
use super::{
    current_timestamp, Generation, Hint, KnowledgeBase, Playbook, RunRecord, Step,
    Strategy, StrategySource,
};

// ── Configuration ───────────────────────────────────────────────────────────

/// Configuration for the autocontext loop.
#[derive(Clone, Debug)]
pub struct LoopConfig {
    /// Maximum strategies generated per generation
    pub strategies_per_gen: usize,
    /// Minimum composite score to consider a run successful
    pub success_threshold: f64,
    /// Number of top strategies to refine (Coach phase)
    pub top_k_for_coaching: usize,
    /// Enable LLM-powered strategy generation (vs template-only)
    pub llm_strategy_gen: bool,
    /// Enable distillation routing (frontier for hard, local for easy)
    pub enable_distillation: bool,
    /// Hint confidence threshold for promotion
    pub hint_confidence_threshold: f64,
    /// Max playbooks to keep per scenario pattern
    pub max_playbooks_per_scenario: usize,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            strategies_per_gen: 4,
            success_threshold: 0.65,
            top_k_for_coaching: 2,
            llm_strategy_gen: true,
            enable_distillation: false,
            hint_confidence_threshold: 0.5,
            max_playbooks_per_scenario: 5,
        }
    }
}

// ── Context retrieved for a scenario ────────────────────────────────────────

/// Context retrieved from the knowledge base for a scenario.
#[derive(Clone, Debug, Default)]
pub struct RetrievedContext {
    pub playbooks: Vec<Playbook>,
    pub hints: Vec<Hint>,
    pub scenario: String,
}

// ── Loop result ─────────────────────────────────────────────────────────────

/// Result of one generation tick.
#[derive(Clone, Debug)]
pub struct LoopResult {
    pub generation_id: u64,
    pub scenario: String,
    pub strategies_generated: usize,
    pub strategies_evaluated: usize,
    pub best_score: f64,
    pub playbooks_created: usize,
    pub playbooks_updated: usize,
    pub hints_created: usize,
    pub validated: bool,
    pub duration_ms: u64,
}

// ── The Loop ────────────────────────────────────────────────────────────────

/// Orchestrates the Competitor → Analyst → Coach → Curator cycle.
pub struct AutoContextLoop {
    pub config: LoopConfig,
    pub knowledge_base: KnowledgeBase,
    pub store: AutoContextStore,
    pub harness: PlaybookHarness,
    pub validation: ValidationPipeline,
    pub distillation: Option<DistillationRouter>,
    pub generation_counter: u64,
}

impl AutoContextLoop {
    pub fn new(store: AutoContextStore, config: LoopConfig) -> Self {
        let validation = if config.success_threshold > 0.7 {
            ValidationPipeline::full_pipeline()
        } else {
            ValidationPipeline::default_pipeline()
        };

        Self {
            config,
            knowledge_base: KnowledgeBase::new(),
            store,
            harness: PlaybookHarness::new(),
            validation,
            distillation: None,
            generation_counter: 0,
        }
    }

    pub fn with_distillation(mut self, router: DistillationRouter) -> Self {
        self.distillation = Some(router);
        self
    }

    /// Load knowledge base from disk.
    pub async fn load(&mut self) -> anyhow::Result<()> {
        self.knowledge_base = self.store.load().await?;
        let generations = self.store.load_generations().await?;
        self.generation_counter = generations.last().map(|g| g.id + 1).unwrap_or(0);
        info!(
            "AutoContext loaded: {} playbooks, {} hints, gen #{}",
            self.knowledge_base.playbooks.len(),
            self.knowledge_base.hints.len(),
            self.generation_counter
        );
        Ok(())
    }

    /// Save knowledge base to disk.
    pub async fn save(&self) -> anyhow::Result<()> {
        self.store.save(&self.knowledge_base).await
    }

    /// Retrieve context for a scenario (playbooks + hints).
    pub fn retrieve_context(&self, scenario: &str) -> RetrievedContext {
        RetrievedContext {
            playbooks: self
                .knowledge_base
                .find_playbooks(scenario, 5)
                .into_iter()
                .cloned()
                .collect(),
            hints: self
                .knowledge_base
                .find_hints(scenario, 10)
                .into_iter()
                .cloned()
                .collect(),
            scenario: scenario.to_string(),
        }
    }

    /// Run one full generation: Competitor → Analyst → Coach → Curator.
    pub async fn tick(
        &mut self,
        scenario: &str,
        tool_registry: &mut ToolRegistry,
        llm: &OllamaClient,
    ) -> anyhow::Result<LoopResult> {
        let start = Instant::now();
        let gen_id = self.generation_counter;
        self.generation_counter += 1;
        let mut generation = Generation::new(gen_id, scenario);

        info!("AutoContext gen #{}: scenario='{}'", gen_id, scenario);

        // ── Phase 1: COMPETITOR — generate strategies ───────────────────
        let context = self.retrieve_context(scenario);
        let strategies = self
            .competitor_phase(scenario, &context, llm)
            .await;
        let strat_count = strategies.len();
        info!("  Competitor: {} strategies generated", strat_count);

        // ── Phase 2: ANALYST — evaluate each strategy ───────────────────
        let mut run_records = Vec::new();
        for strategy in &strategies {
            let record = self
                .analyst_phase(strategy, scenario, tool_registry, llm)
                .await;
            run_records.push(record);
        }

        // Sort by composite score (best first)
        run_records.sort_by(|a, b| {
            b.composite_score
                .partial_cmp(&a.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let best_score = run_records.first().map(|r| r.composite_score).unwrap_or(0.0);
        let evaluated_count = run_records.len();
        info!(
            "  Analyst: {} runs evaluated, best={:.3}",
            evaluated_count, best_score
        );

        // ── Phase 3: COACH — refine top strategies ──────────────────────
        let top_runs: Vec<&RunRecord> = run_records
            .iter()
            .take(self.config.top_k_for_coaching)
            .collect();

        let coached_strategies = self.coach_phase(&top_runs, scenario, llm).await;
        info!("  Coach: {} refined strategies", coached_strategies.len());

        // Evaluate coached strategies too
        for coached in &coached_strategies {
            let record = self
                .analyst_phase(coached, scenario, tool_registry, llm)
                .await;
            run_records.push(record);
        }

        // Re-sort after adding coached results
        run_records.sort_by(|a, b| {
            b.composite_score
                .partial_cmp(&a.composite_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let best_score = run_records.first().map(|r| r.composite_score).unwrap_or(0.0);
        generation.best_score = best_score;
        generation.run_records = run_records;

        // ── Phase 4: CURATOR — persist validated playbooks & hints ──────
        let (playbooks_created, playbooks_updated, hints_created, validated) = self
            .curator_phase(&mut generation, tool_registry, llm)
            .await;

        generation.playbooks_affected = playbooks_created + playbooks_updated;
        generation.hints_affected = hints_created;
        generation.completed_at = Some(current_timestamp());
        generation.improved = best_score > self.config.success_threshold;

        // Save generation record
        self.store.save_generation(&generation).await?;
        self.save().await?;

        let duration_ms = start.elapsed().as_millis() as u64;
        info!(
            "  AutoContext gen #{} complete: best={:.3}, pb_new={}, pb_upd={}, hints={}, took={}ms",
            gen_id, best_score, playbooks_created, playbooks_updated, hints_created, duration_ms
        );

        Ok(LoopResult {
            generation_id: gen_id,
            scenario: scenario.to_string(),
            strategies_generated: strat_count + coached_strategies.len(),
            strategies_evaluated: generation.run_records.len(),
            best_score,
            playbooks_created,
            playbooks_updated,
            hints_created,
            validated,
            duration_ms,
        })
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Phase Implementations
    // ═════════════════════════════════════════════════════════════════════════

    /// COMPETITOR: Generate candidate strategies for a scenario.
    async fn competitor_phase(
        &self,
        scenario: &str,
        context: &RetrievedContext,
        llm: &OllamaClient,
    ) -> Vec<Strategy> {
        let mut strategies = Vec::new();

        // Strategy 1: Mutate existing playbooks
        for pb in context.playbooks.iter().take(2) {
            strategies.push(self.mutate_playbook(pb));
        }

        // Strategy 2: LLM-generated strategies
        if self.config.llm_strategy_gen {
            if let Some(llm_strat) = self.generate_strategy_via_llm(scenario, context, llm).await {
                strategies.push(llm_strat);
            }
        }

        // Strategy 3: Crossover of top two playbooks
        if context.playbooks.len() >= 2 {
            strategies.push(self.crossover(&context.playbooks[0], &context.playbooks[1]));
        }

        // Strategy 4: Fresh proposal (simple template)
        strategies.push(self.fresh_proposal(scenario));

        // Ensure we have at least the configured count
        while strategies.len() < self.config.strategies_per_gen {
            strategies.push(self.fresh_proposal(scenario));
        }

        strategies.truncate(self.config.strategies_per_gen);
        strategies
    }

    /// ANALYST: Execute and evaluate a single strategy.
    async fn analyst_phase(
        &self,
        strategy: &Strategy,
        scenario: &str,
        tool_registry: &mut ToolRegistry,
        llm: &OllamaClient,
    ) -> RunRecord {
        let start = Instant::now();
        let mut record = RunRecord::new(strategy.clone());

        // Build a temporary playbook to execute via harness
        let temp_pb = Playbook::new(
            &strategy.description,
            &strategy.description,
            scenario,
        )
        .with_steps(strategy.steps.clone());

        // Execute through harness
        let mut context = HashMap::new();
        context.insert("query".to_string(), scenario.to_string());

        let result = self.harness.execute(&temp_pb, tool_registry, llm, &context).await;
        record.artifacts = result.artifacts;
        record.duration_ms = start.elapsed().as_millis() as u64;

        // Score dimensions
        let completion_score = result.steps_completed as f64 / result.steps_total.max(1) as f64;
        let success_score = if result.success { 1.0 } else { 0.0 };
        let speed_score = (1.0 - (record.duration_ms as f64 / 30_000.0).min(1.0)).max(0.0);

        record.scores.insert("completion".to_string(), completion_score);
        record.scores.insert("success".to_string(), success_score);
        record.scores.insert("speed".to_string(), speed_score);

        // Composite: weighted average
        record.composite_score = completion_score * 0.4 + success_score * 0.4 + speed_score * 0.2;

        // Generate feedback via LLM
        record.feedback = self.generate_feedback(&record, llm).await;

        debug!(
            "  Analyst: '{}' → score={:.3} (completion={:.2}, success={:.2}, speed={:.2})",
            strategy.description, record.composite_score, completion_score, success_score, speed_score
        );

        record
    }

    /// COACH: Refine the top strategies based on analyst feedback.
    async fn coach_phase(
        &self,
        top_runs: &[&RunRecord],
        scenario: &str,
        llm: &OllamaClient,
    ) -> Vec<Strategy> {
        let mut coached = Vec::new();

        for run in top_runs {
            if run.composite_score < 0.3 {
                continue; // Not worth coaching
            }

            let prompt = format!(
                "You are an expert strategy coach. Refine this approach based on the feedback.\n\n\
                 Scenario: {}\n\
                 Strategy: {}\n\
                 Score: {:.2}\n\
                 Feedback: {}\n\n\
                 Provide ONLY an improved step-by-step plan. Each line should be one step.\n\
                 Format: STEP_NUMBER. DESCRIPTION (tool:TOOL_NAME if applicable)\n\
                 Keep it to 2-5 steps.",
                scenario,
                run.strategy.description,
                run.composite_score,
                run.feedback,
            );

            let result = llm.generate(&prompt).await;
            if result.timed_out || result.text.trim().is_empty() {
                continue;
            }

            let steps = self.parse_steps_from_llm(&result.text);
            if !steps.is_empty() {
                coached.push(
                    Strategy::new(
                        format!("Coached: {}", run.strategy.description),
                        steps,
                        StrategySource::Coached {
                            original_id: run.strategy.id.clone(),
                        },
                    )
                    .with_parents(vec![run.strategy.id.clone()]),
                );
            }
        }

        coached
    }

    /// CURATOR: Validate and persist top strategies as playbooks + hints.
    async fn curator_phase(
        &mut self,
        generation: &mut Generation,
        tool_registry: &mut ToolRegistry,
        llm: &OllamaClient,
    ) -> (usize, usize, usize, bool) {
        let mut playbooks_created = 0usize;
        let mut playbooks_updated = 0usize;
        let mut hints_created = 0usize;
        let mut any_validated = false;

        let scenario = generation.scenario.clone();
        let gen_id = generation.id;

        // Identify winning runs
        let winners: Vec<RunRecord> = generation
            .run_records
            .iter()
            .filter(|r| r.composite_score >= self.config.success_threshold)
            .cloned()
            .collect();

        for winner in &winners {
            // Build a playbook candidate
            let mut pb = Playbook::new(
                &winner.strategy.description,
                format!("Auto-generated from gen #{} score {:.3}", gen_id, winner.composite_score),
                &scenario,
            )
            .with_steps(winner.strategy.steps.clone());
            pb.quality_score = winner.composite_score;
            pb.origin_generation = gen_id;

            // Validate through pipeline
            let vr: ValidationResult = self.validation.validate(&pb, tool_registry, llm).await;
            if vr.passed {
                pb.validation_stage = vr.stage_reached;
                pb.last_validated = current_timestamp();

                // Check if this updates an existing playbook or creates a new one
                let existing = self
                    .knowledge_base
                    .playbooks
                    .iter()
                    .find(|p| {
                        p.scenario_pattern == scenario && p.name == pb.name
                    })
                    .map(|p| p.id.clone());

                if let Some(existing_id) = existing {
                    pb.id = existing_id;
                    pb.success_count += 1;
                    self.knowledge_base.upsert_playbook(pb);
                    playbooks_updated += 1;
                } else {
                    pb.success_count = 1;
                    self.knowledge_base.upsert_playbook(pb);
                    playbooks_created += 1;
                }
                any_validated = true;
            } else if vr.rollback_needed {
                warn!(
                    "Playbook '{}' failed staged validation, rolling back",
                    winner.strategy.description
                );
                // Mark as failed in generation
                // (nothing else needed — playbook was never persisted)
            }
        }

        // Extract hints from ALL runs (even failures have lessons)
        for run in &generation.run_records {
            if let Some(hint) = self.extract_hint(run, &scenario) {
                if hint.confidence >= self.config.hint_confidence_threshold {
                    self.knowledge_base.upsert_hint(hint);
                    hints_created += 1;
                }
            }
        }

        // Prune: keep only top N playbooks per scenario
        self.prune_playbooks(&scenario);

        // Update KB stats
        self.knowledge_base.total_generations = gen_id + 1;
        self.knowledge_base.total_runs += generation.run_records.len() as u64;
        self.knowledge_base.last_updated = current_timestamp();

        (playbooks_created, playbooks_updated, hints_created, any_validated)
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Strategy Generation Helpers
    // ═════════════════════════════════════════════════════════════════════════

    /// Mutate an existing playbook into a new strategy.
    fn mutate_playbook(&self, playbook: &Playbook) -> Strategy {
        let mut steps = playbook.steps.clone();

        // Simple mutations: swap two steps, or modify parameters
        if steps.len() >= 2 {
            let i = steps.len() / 2;
            steps.swap(0, i);
        }

        Strategy::new(
            format!("Mutated: {}", playbook.name),
            steps,
            StrategySource::Mutated {
                parent_playbook_id: playbook.id.clone(),
            },
        )
        .with_parents(vec![playbook.id.clone()])
    }

    /// Crossover two playbooks by interleaving steps.
    fn crossover(&self, a: &Playbook, b: &Playbook) -> Strategy {
        let mut steps = Vec::new();
        let max_len = a.steps.len().max(b.steps.len());

        for i in 0..max_len {
            if i % 2 == 0 {
                if let Some(step) = a.steps.get(i) {
                    let mut s = step.clone();
                    s.index = steps.len();
                    steps.push(s);
                }
            } else if let Some(step) = b.steps.get(i) {
                let mut s = step.clone();
                s.index = steps.len();
                steps.push(s);
            }
        }

        Strategy::new(
            format!("Crossover: {} × {}", a.name, b.name),
            steps,
            StrategySource::Crossover {
                parent_ids: vec![a.id.clone(), b.id.clone()],
            },
        )
        .with_parents(vec![a.id.clone(), b.id.clone()])
    }

    /// Generate a strategy from scratch via LLM.
    async fn generate_strategy_via_llm(
        &self,
        scenario: &str,
        context: &RetrievedContext,
        llm: &OllamaClient,
    ) -> Option<Strategy> {
        let hint_text = context
            .hints
            .iter()
            .take(3)
            .map(|h| format!("- {}", h.content))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Generate a step-by-step strategy to accomplish this task.\n\n\
             Scenario: {}\n\
             {}\n\
             Available tools: web_search, grep, read_file, write_file, bash, list_directory\n\n\
             Provide ONLY steps, one per line.\n\
             Format: STEP_NUMBER. DESCRIPTION (tool:TOOL_NAME if applicable)\n\
             Keep it to 2-5 steps.",
            scenario,
            if hint_text.is_empty() {
                String::new()
            } else {
                format!("Hints from past experience:\n{}\n", hint_text)
            }
        );

        let result = llm.generate(&prompt).await;
        if result.timed_out || result.text.trim().is_empty() {
            return None;
        }

        let steps = self.parse_steps_from_llm(&result.text);
        if steps.is_empty() {
            return None;
        }

        Some(Strategy::new(
            format!("LLM-generated for: {}", scenario),
            steps,
            StrategySource::Proposed,
        ))
    }

    /// Create a simple template-based strategy.
    fn fresh_proposal(&self, scenario: &str) -> Strategy {
        let steps = vec![
            Step::llm_step(
                0,
                "Analyze the scenario",
                format!("Analyze this scenario and break it into sub-tasks: {}", scenario),
                "analysis produced",
            ),
            Step::llm_step(
                1,
                "Generate solution",
                format!(
                    "Based on the analysis, provide a solution for: {}",
                    scenario
                ),
                "solution produced",
            ),
        ];

        Strategy::new(
            format!("Fresh proposal: {}", scenario),
            steps,
            StrategySource::Proposed,
        )
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Analysis & Feedback Helpers
    // ═════════════════════════════════════════════════════════════════════════

    /// Generate analyst feedback for a run.
    async fn generate_feedback(&self, record: &RunRecord, llm: &OllamaClient) -> String {
        let artifact_summary: String = record
            .artifacts
            .iter()
            .take(3)
            .map(|a| {
                let preview = &a.content[..a.content.len().min(100)];
                format!("[{}] {}", a.step_index, preview)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Briefly evaluate this strategy execution (1-2 sentences).\n\
             Strategy: {}\n\
             Score: {:.2}\n\
             Steps completed: {}\n\
             Artifacts:\n{}\n\
             What worked? What could improve?",
            record.strategy.description,
            record.composite_score,
            record.artifacts.len(),
            artifact_summary,
        );

        let result = llm.generate(&prompt).await;
        if result.timed_out {
            format!(
                "Score: {:.2}. {} artifacts collected.",
                record.composite_score,
                record.artifacts.len()
            )
        } else {
            result.text.lines().take(3).collect::<Vec<_>>().join(" ")
        }
    }

    /// Extract a hint from a run record.
    fn extract_hint(&self, run: &RunRecord, scenario: &str) -> Option<Hint> {
        // Only extract hints from runs with meaningful feedback
        if run.feedback.is_empty() || run.feedback.len() < 10 {
            return None;
        }

        let confidence = if run.composite_score > self.config.success_threshold {
            0.7 + (run.composite_score - self.config.success_threshold) * 0.5
        } else {
            // Failure hints are less confident but still useful
            0.3 + run.composite_score * 0.3
        };

        Some(Hint::new(
            &run.feedback,
            scenario,
            confidence.min(1.0),
        ))
    }

    /// Parse step descriptions from LLM output.
    fn parse_steps_from_llm(&self, text: &str) -> Vec<Step> {
        let mut steps = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to match "N. Description (tool:TOOL_NAME)"
            let (desc, tool_name) = if let Some(paren_idx) = trimmed.rfind("(tool:") {
                let desc = trimmed[..paren_idx].trim();
                let tool_part = &trimmed[paren_idx + 6..];
                let tool = tool_part.trim_end_matches(')').trim();
                (desc.to_string(), Some(tool.to_string()))
            } else {
                (trimmed.to_string(), None)
            };

            // Strip leading number + period
            let desc = desc
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ')
                .to_string();

            if desc.is_empty() {
                continue;
            }

            let idx = steps.len();
            if let Some(tool) = tool_name {
                steps.push(Step::tool_step(
                    idx,
                    &desc,
                    &tool,
                    serde_json::json!({"query": desc}),
                    "output produced",
                ));
            } else {
                steps.push(Step::llm_step(
                    idx,
                    &desc,
                    format!("Execute: {}", desc),
                    "output produced",
                ));
            }
        }

        steps
    }

    /// Prune playbooks keeping only top N per scenario.
    fn prune_playbooks(&mut self, scenario: &str) {
        let max = self.config.max_playbooks_per_scenario;

        // Separate matching and non-matching
        let (mut matching, other): (Vec<Playbook>, Vec<Playbook>) = self
            .knowledge_base
            .playbooks
            .drain(..)
            .partition(|p| p.scenario_pattern == *scenario);

        // Sort matching by quality (best first)
        matching.sort_by(|a, b| {
            b.quality_score
                .partial_cmp(&a.quality_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Keep top N
        matching.truncate(max);

        // Recombine
        self.knowledge_base.playbooks = other;
        self.knowledge_base.playbooks.extend(matching);
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_config_defaults() {
        let cfg = LoopConfig::default();
        assert_eq!(cfg.strategies_per_gen, 4);
        assert_eq!(cfg.top_k_for_coaching, 2);
        assert!(cfg.llm_strategy_gen);
    }

    #[test]
    fn test_retrieve_context_empty() {
        let store = AutoContextStore::new("/tmp/test_ac_loop");
        let acl = AutoContextLoop::new(store, LoopConfig::default());
        let ctx = acl.retrieve_context("anything");
        assert!(ctx.playbooks.is_empty());
        assert!(ctx.hints.is_empty());
    }

    #[test]
    fn test_retrieve_context_with_data() {
        let store = AutoContextStore::new("/tmp/test_ac_loop2");
        let mut acl = AutoContextLoop::new(store, LoopConfig::default());

        acl.knowledge_base
            .upsert_playbook(Playbook::new("Search", "desc", "web search"));
        acl.knowledge_base
            .upsert_hint(Hint::new("Use grep first", "code search", 0.8));

        let ctx = acl.retrieve_context("search the web");
        assert_eq!(ctx.playbooks.len(), 1);
    }

    #[test]
    fn test_fresh_proposal() {
        let store = AutoContextStore::new("/tmp/test_ac_loop3");
        let acl = AutoContextLoop::new(store, LoopConfig::default());
        let strat = acl.fresh_proposal("find bugs in code");
        assert_eq!(strat.steps.len(), 2);
        assert!(strat.description.contains("find bugs"));
    }

    #[test]
    fn test_mutate_playbook() {
        let store = AutoContextStore::new("/tmp/test_ac_loop4");
        let acl = AutoContextLoop::new(store, LoopConfig::default());
        let pb = Playbook::new("Test", "desc", "pattern").with_steps(vec![
            Step::llm_step(0, "step A", "prompt A", "ok"),
            Step::llm_step(1, "step B", "prompt B", "ok"),
        ]);
        let strat = acl.mutate_playbook(&pb);
        assert_eq!(strat.steps.len(), 2);
        assert!(matches!(strat.source, StrategySource::Mutated { .. }));
    }

    #[test]
    fn test_crossover() {
        let store = AutoContextStore::new("/tmp/test_ac_loop5");
        let acl = AutoContextLoop::new(store, LoopConfig::default());
        let a = Playbook::new("A", "desc", "pattern").with_steps(vec![
            Step::llm_step(0, "A0", "prompt", "ok"),
            Step::llm_step(1, "A1", "prompt", "ok"),
        ]);
        let b = Playbook::new("B", "desc", "pattern").with_steps(vec![
            Step::llm_step(0, "B0", "prompt", "ok"),
            Step::llm_step(1, "B1", "prompt", "ok"),
        ]);
        let strat = acl.crossover(&a, &b);
        assert_eq!(strat.steps.len(), 2); // interleaved: A0, B1
        assert!(matches!(strat.source, StrategySource::Crossover { .. }));
    }

    #[test]
    fn test_parse_steps_from_llm() {
        let store = AutoContextStore::new("/tmp/test_ac_loop6");
        let acl = AutoContextLoop::new(store, LoopConfig::default());

        let text = "1. Search the web for Rust news (tool:web_search)\n\
                     2. Summarize the findings\n\
                     3. Save results to file (tool:write_file)";

        let steps = acl.parse_steps_from_llm(text);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].tool_name, Some("web_search".to_string()));
        assert!(steps[1].tool_name.is_none());
        assert_eq!(steps[2].tool_name, Some("write_file".to_string()));
    }

    #[test]
    fn test_extract_hint() {
        let store = AutoContextStore::new("/tmp/test_ac_loop7");
        let acl = AutoContextLoop::new(store, LoopConfig::default());

        let mut run = RunRecord::new(Strategy::new("test", vec![], StrategySource::Proposed));
        run.composite_score = 0.8;
        run.feedback = "Search worked well but summarization was too verbose".to_string();

        let hint = acl.extract_hint(&run, "web search");
        assert!(hint.is_some());
        let hint = hint.unwrap();
        assert!(hint.confidence > 0.5);
        assert!(hint.content.contains("summarization"));
    }

    #[test]
    fn test_prune_playbooks() {
        let store = AutoContextStore::new("/tmp/test_ac_loop8");
        let mut acl = AutoContextLoop::new(store, LoopConfig::default());
        acl.config.max_playbooks_per_scenario = 2;

        for i in 0..5 {
            let mut pb = Playbook::new(format!("PB{}", i), "desc", "pattern");
            pb.quality_score = i as f64 / 10.0;
            acl.knowledge_base.upsert_playbook(pb);
        }

        acl.prune_playbooks("pattern");
        let matching: Vec<_> = acl
            .knowledge_base
            .playbooks
            .iter()
            .filter(|p| p.scenario_pattern == "pattern")
            .collect();
        assert_eq!(matching.len(), 2);
        // Best two should remain
        assert!(matching[0].quality_score >= matching[1].quality_score);
    }
}
