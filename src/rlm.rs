use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::agent::Role;
use crate::hyper_stigmergy::{BeliefSource, HyperStigmergicMorphogenesis};

/// Configuration for the bidding system
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BidConfig {
    pub architect_bias: f64,
    pub catalyst_bias: f64,
    pub chronicler_bias: f64,
    pub exploration_temperature: f64,
}

impl Default for BidConfig {
    fn default() -> Self {
        Self {
            architect_bias: 1.0,
            catalyst_bias: 1.0,
            chronicler_bias: 1.0,
            exploration_temperature: 0.1,
        }
    }
}

/// RLM State for persistence
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RLMState {
    pub bid_config: BidConfig,
    pub conversation_history: Vec<RlmMessage>,
    pub sub_agents: Vec<SubAgent>,
    pub living_prompt: LivingPrompt,
    pub embedding_cache_hits: u64,
    pub embedding_cache_misses: u64,
}

pub struct RLM {
    pub ollama: OllamaHandle,
    pub world: HyperStigmergicMorphogenesis,
    pub bid_config: BidConfig,
    pub embedding_cache: EmbeddingCache,
    pub sub_agents: Vec<SubAgent>,
    pub living_prompt: LivingPrompt,
}

pub struct OllamaHandle {
    pub model: String,
    pub endpoint: String,
}

pub struct EmbeddingCache {
    cache: HashMap<String, Vec<f32>>,
    max_size: usize,
}

impl EmbeddingCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::new(),
            max_size,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Vec<f32>> {
        self.cache.get(key)
    }

    pub fn insert(&mut self, key: String, embedding: Vec<f32>) {
        if self.cache.len() >= self.max_size {
            if let Some(first_key) = self.cache.keys().next().cloned() {
                self.cache.remove(&first_key);
            }
        }
        self.cache.insert(key, embedding);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubAgent {
    pub id: String,
    pub role: Role,
    pub specialty: String,
    pub activation_threshold: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LivingPrompt {
    pub base_prompt: String,
    pub accumulated_insights: Vec<String>,
    pub context_window: Vec<RlmMessage>,
    pub max_context: usize,
    /// GEPA: History of prompt mutations for evolution tracking
    pub evolution_history: Vec<PromptMutation>,
    pub mutation_count: u32,
    /// Mistakes/failures to avoid (GEPA: negative instructions > positive)
    pub avoid_patterns: Vec<String>,
}

/// Tracks a single prompt mutation event (GEPA pattern)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptMutation {
    pub description: String,
    pub coherence_before: f64,
    pub coherence_after: f64,
    pub kept: bool,
    pub timestamp: u64,
}

/// Result from a reflect() call
#[derive(Clone, Debug)]
pub struct ReflectionResult {
    pub insights: Vec<String>,
    pub beliefs_generated: usize,
    pub prompt_evolved: bool,
    pub summary: String,
}

impl LivingPrompt {
    pub fn new(base: &str) -> Self {
        Self {
            base_prompt: base.to_string(),
            accumulated_insights: Vec::new(),
            context_window: Vec::new(),
            max_context: 10,
            evolution_history: Vec::new(),
            mutation_count: 0,
            avoid_patterns: Vec::new(),
        }
    }

    pub fn add_message(&mut self, msg: RlmMessage) {
        self.context_window.push(msg);
        if self.context_window.len() > self.max_context {
            self.context_window.remove(0);
        }
    }

    pub fn add_insight(&mut self, insight: String) {
        self.accumulated_insights.push(insight);
        if self.accumulated_insights.len() > 100 {
            self.accumulated_insights.remove(0);
        }
    }

    pub fn render(&self) -> String {
        let mut prompt = self.base_prompt.clone();

        if !self.accumulated_insights.is_empty() {
            prompt.push_str("\n\n### Accumulated Insights\n");
            for insight in &self.accumulated_insights {
                prompt.push_str(&format!("- {}\n", insight));
            }
        }

        // GEPA: Negative instructions are more effective than positive
        if !self.avoid_patterns.is_empty() {
            prompt.push_str("\n\n### AVOID These Patterns\n");
            for pattern in &self.avoid_patterns {
                prompt.push_str(&format!("- DO NOT: {}\n", pattern));
            }
        }

        if !self.context_window.is_empty() {
            prompt.push_str("\n\n### Recent Context\n");
            for msg in &self.context_window {
                prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
            }
        }

        prompt
    }

    /// Add a failure pattern to avoid (GEPA: mistake prevention > positive instructions)
    pub fn add_avoid_pattern(&mut self, pattern: String) {
        if !self.avoid_patterns.contains(&pattern) {
            self.avoid_patterns.push(pattern);
            if self.avoid_patterns.len() > 20 {
                self.avoid_patterns.remove(0);
            }
        }
    }

    /// GEPA-style prompt evolution: mutate base_prompt based on execution trace analysis
    pub fn evolve(&mut self, analysis: &str, coherence_before: f64, coherence_after: f64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let improved = coherence_after > coherence_before;

        if improved {
            // Append successful insight to base prompt
            let addition = format!(
                "\n[Evolved insight #{}]: {}",
                self.mutation_count + 1,
                analysis
            );
            self.base_prompt.push_str(&addition);

            // Keep base_prompt from growing unbounded
            if self.base_prompt.len() > 2000 {
                // Trim oldest evolved insights, keep core prompt
                if let Some(pos) = self.base_prompt.find("[Evolved insight #1]") {
                    self.base_prompt = self.base_prompt[..pos].to_string()
                        + &self.base_prompt[self.base_prompt.len().saturating_sub(800)..];
                }
            }
        }

        self.evolution_history.push(PromptMutation {
            description: analysis.to_string(),
            coherence_before,
            coherence_after,
            kept: improved,
            timestamp: now,
        });

        if self.evolution_history.len() > 50 {
            self.evolution_history.remove(0);
        }

        self.mutation_count += 1;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RlmMessage {
    pub role: String,
    pub content: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug)]
pub enum RlmAction {
    ExecuteAction(crate::action::Action),
    QueryEnvironment(String),
    PredictCoherence { context: String },
    ComputeNovelty { proposal: String },
    SelfImprove { intent: String },
    SpawnSubAgent { role: Role, specialty: String },
}

#[derive(Clone, Debug)]
pub enum Context {
    Text(String),
    ActionExecuted(String),
    Prediction(f32),
    NoveltyScore(f32),
    ImprovementResult(crate::hyper_stigmergy::ImprovementResult),
    Reflection(ReflectionResult),
}

/// Self-improvement cycle controller
pub struct SelfImprovementCycle {
    pub intent: String,
    pub iterations: usize,
    pub current_iteration: usize,
    pub best_coherence: f64,
    pub history: Vec<crate::hyper_stigmergy::ImprovementEvent>,
}

impl SelfImprovementCycle {
    pub fn new(intent: &str, iterations: usize) -> Self {
        Self {
            intent: intent.to_string(),
            iterations,
            current_iteration: 0,
            best_coherence: 0.0,
            history: Vec::new(),
        }
    }

    pub fn should_continue(&self) -> bool {
        self.current_iteration < self.iterations
    }

    pub fn record_iteration(&mut self, event: crate::hyper_stigmergy::ImprovementEvent) {
        if event.coherence_after > self.best_coherence {
            self.best_coherence = event.coherence_after;
        }
        self.history.push(event);
        self.current_iteration += 1;
    }
}

/// Create RLM from world state
pub async fn rlm_from_world(world: HyperStigmergicMorphogenesis, model: &str) -> RLM {
    RLM {
        ollama: OllamaHandle {
            model: model.to_string(),
            endpoint: "http://localhost:11434".to_string(),
        },
        world,
        bid_config: BidConfig::default(),
        embedding_cache: EmbeddingCache::new(1000),
        sub_agents: vec![
            SubAgent {
                id: "architect_1".to_string(),
                role: Role::Architect,
                specialty: "structure".to_string(),
                activation_threshold: 0.7,
            },
            SubAgent {
                id: "catalyst_1".to_string(),
                role: Role::Catalyst,
                specialty: "innovation".to_string(),
                activation_threshold: 0.6,
            },
            SubAgent {
                id: "chronicler_1".to_string(),
                role: Role::Chronicler,
                specialty: "documentation".to_string(),
                activation_threshold: 0.5,
            },
        ],
        living_prompt: LivingPrompt::new(
            "You are the Architect-Conductor, a hyper-stigmergic morphogenesis system.",
        ),
    }
}

impl RLM {
    pub async fn execute(&mut self, intent: &str) -> anyhow::Result<Vec<Context>> {
        let selected_role = self.world.select_role_via_bidding(&self.bid_config);

        println!("  Role selected via bidding: {:?}", selected_role);

        let results = match selected_role {
            Role::Architect => self.architect_handler(intent).await,
            Role::Catalyst => self.catalyst_handler(intent).await,
            Role::Chronicler => self.chronicler_handler(intent).await,
            // Critic reuses architect logic (structural focus with skepticism)
            Role::Critic => self.architect_handler(intent).await,
            // Explorer reuses catalyst logic (novelty focus with exploration)
            Role::Explorer => self.catalyst_handler(intent).await,
            // Coder has specialized code-focused handler
            Role::Coder => self.coder_handler(intent).await,
        };

        self.living_prompt.add_message(RlmMessage {
            role: "system".to_string(),
            content: format!("Executed intent with {:?}: {}", selected_role, intent),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });

        results
    }

    async fn architect_handler(&mut self, intent: &str) -> anyhow::Result<Vec<Context>> {
        let prediction = self.world.predict_coherence(intent);
        Ok(vec![
            Context::Text(format!("Architect analyzing: {}", intent)),
            Context::Prediction(prediction),
        ])
    }

    async fn catalyst_handler(&mut self, intent: &str) -> anyhow::Result<Vec<Context>> {
        let novelty = self.world.compute_novelty(intent);
        if novelty > 0.7 {
            let result = self.world.execute_self_improvement_cycle(intent);
            Ok(vec![
                Context::Text(format!("Catalyst innovating: {}", intent)),
                Context::NoveltyScore(novelty),
                Context::ImprovementResult(result),
            ])
        } else {
            Ok(vec![
                Context::Text(format!("Catalyst exploring: {}", intent)),
                Context::NoveltyScore(novelty),
            ])
        }
    }

    async fn chronicler_handler(&mut self, intent: &str) -> anyhow::Result<Vec<Context>> {
        let history_summary = format!(
            "Recorded {} improvement events",
            self.world.improvement_history.len()
        );
        Ok(vec![
            Context::Text(format!("Chronicler documenting: {}", intent)),
            Context::Text(history_summary),
        ])
    }

    async fn coder_handler(&mut self, intent: &str) -> anyhow::Result<Vec<Context>> {
        // Coder agents analyze code-related intents
        let _tool_context = crate::memory::ToolContext {
            coherence: self.world.global_coherence(),
            agent_count: self.world.agents.len(),
            edge_count: self
                .world
                .adjacency
                .values()
                .map(|v| v.len())
                .sum::<usize>()
                / 2,
            tick: self.world.tick_count,
            recent_beliefs: self
                .world
                .beliefs
                .iter()
                .rev()
                .take(3)
                .map(|b| b.content.clone())
                .collect(),
        };

        // Check if intent mentions specific tools
        let mut tool_outputs = Vec::new();

        if intent.contains("read") || intent.contains("file") {
            tool_outputs.push(Context::Text(
                "Coder: Use pi_read <filepath> to explore files".to_string(),
            ));
        }
        if intent.contains("search") || intent.contains("find") {
            tool_outputs.push(Context::Text(
                "Coder: Use pi_grep <pattern> or pi_find <pattern> to search".to_string(),
            ));
        }
        if intent.contains("run") || intent.contains("execute") || intent.contains("test") {
            tool_outputs.push(Context::Text(
                "Coder: Use pi_bash <command> to execute commands".to_string(),
            ));
        }
        if intent.contains("edit") || intent.contains("modify") || intent.contains("fix") {
            tool_outputs.push(Context::Text(
                "Coder: Use pi_edit or pi_write to modify files".to_string(),
            ));
        }

        if tool_outputs.is_empty() {
            tool_outputs.push(Context::Text(format!(
                "Coder analyzing code task: {}",
                intent
            )));
            tool_outputs.push(Context::Text(
                "Available tools: pi_read, pi_bash, pi_grep, pi_find, pi_ls, pi_edit, pi_write"
                    .to_string(),
            ));
        }

        Ok(tool_outputs)
    }

    pub fn save_state(&self) -> RLMState {
        RLMState {
            bid_config: self.bid_config.clone(),
            conversation_history: self.living_prompt.context_window.clone(),
            sub_agents: self.sub_agents.clone(),
            living_prompt: self.living_prompt.clone(),
            embedding_cache_hits: 0,
            embedding_cache_misses: 0,
        }
    }

    pub fn load_state(&mut self, state: RLMState) {
        self.bid_config = state.bid_config;
        self.living_prompt = state.living_prompt;
        self.sub_agents = state.sub_agents;
    }

    /// Hindsight CARA-inspired reflection: analyze history to generate insights and beliefs.
    /// This builds a prompt from recent improvement history + current beliefs, sends it to
    /// Ollama, and parses the response into new insights, beliefs, and avoid-patterns.
    /// If Ollama is unavailable, falls back to heuristic reflection.
    pub async fn reflect(&mut self) -> ReflectionResult {
        let coherence_before = self.world.global_coherence();

        // Build reflection prompt from execution traces
        let reflection_prompt = self.build_reflection_prompt();

        // Try Ollama, fall back to heuristic
        let analysis = match self.call_ollama(&reflection_prompt).await {
            Ok(response) => response,
            Err(_) => self.heuristic_reflect(),
        };

        // Parse insights from the analysis
        let insights = Self::parse_insights(&analysis);
        let avoid_patterns = Self::parse_avoid_patterns(&analysis);

        // Add insights to living prompt
        for insight in &insights {
            self.living_prompt.add_insight(insight.clone());
        }

        // Add avoid patterns (GEPA: mistakes > positive instructions)
        for pattern in &avoid_patterns {
            self.living_prompt.add_avoid_pattern(pattern.clone());
        }

        // Generate beliefs from insights
        let mut beliefs_generated = 0;
        for insight in &insights {
            self.world
                .add_belief(insight, 0.6, BeliefSource::Reflection);
            beliefs_generated += 1;
        }

        // Generate beliefs from improvement history patterns
        self.world.generate_beliefs_from_history();

        // Decay old beliefs
        self.world.decay_beliefs();

        // GEPA: Evolve the living prompt based on this reflection
        let coherence_after = self.world.global_coherence();
        self.living_prompt
            .evolve(&analysis, coherence_before, coherence_after);

        // GRPO: Update agent bid biases based on group-relative performance
        let coherence_delta = coherence_after - coherence_before;
        if self.world.agents.len() > 1 {
            // Simulate per-agent rewards based on role alignment with outcome
            let rewards: Vec<f64> = self
                .world
                .agents
                .iter()
                .map(|agent| {
                    let role_bonus = match agent.role {
                        Role::Architect => {
                            if coherence_delta > 0.0 {
                                0.3
                            } else {
                                -0.1
                            }
                        }
                        Role::Catalyst => {
                            if insights.len() > 2 {
                                0.3
                            } else {
                                -0.1
                            }
                        }
                        Role::Chronicler => {
                            if beliefs_generated > 1 {
                                0.2
                            } else {
                                0.0
                            }
                        }
                        Role::Critic => {
                            if coherence_delta < 0.0 {
                                0.3
                            } else {
                                -0.05
                            }
                        } // rewarded for catching issues
                        Role::Explorer => {
                            if insights.len() > 3 {
                                0.35
                            } else {
                                0.0
                            }
                        } // rewarded for breadth
                        Role::Coder => {
                            if insights.len() > 1 {
                                0.25
                            } else {
                                0.0
                            }
                        } // rewarded for actionable insights
                    };
                    coherence_delta + role_bonus + agent.drives.growth * 0.1
                })
                .collect();

            for (i, agent) in self.world.agents.iter_mut().enumerate() {
                agent.grpo_update_bid(&rewards, rewards[i], agent.learning_rate);
            }
        }

        // Track reflection
        self.world.reflection_count += 1;
        self.world.last_reflection_tick = self.world.tick_count;

        let summary = format!(
            "Reflection #{}: {} insights, {} beliefs, {} avoid-patterns | coherence {:.4} → {:.4}",
            self.world.reflection_count,
            insights.len(),
            beliefs_generated,
            avoid_patterns.len(),
            coherence_before,
            coherence_after,
        );

        ReflectionResult {
            insights,
            beliefs_generated,
            prompt_evolved: coherence_after > coherence_before,
            summary,
        }
    }

    fn build_reflection_prompt(&self) -> String {
        let mut prompt = String::new();
        prompt.push_str("You are analyzing a hyper-stigmergic morphogenesis system.\n");
        prompt.push_str("Based on the execution trace below, generate:\n");
        prompt.push_str(
            "1. INSIGHTS: Key observations about system behavior (prefix with 'INSIGHT:')\n",
        );
        prompt.push_str("2. AVOID: Patterns that led to degradation (prefix with 'AVOID:')\n\n");

        // Add recent improvement history
        prompt.push_str("## Recent Improvement History\n");
        for event in self.world.improvement_history.iter().rev().take(10) {
            let delta = event.coherence_after - event.coherence_before;
            prompt.push_str(&format!(
                "- {:?}: delta={:+.4}, novelty={:.2}, applied={}\n",
                event.mutation_type, delta, event.novelty_score, event.applied
            ));
        }

        // Add current beliefs
        let top_beliefs = self.world.top_beliefs(5);
        if !top_beliefs.is_empty() {
            prompt.push_str("\n## Current Beliefs\n");
            for b in &top_beliefs {
                prompt.push_str(&format!("- [{:.0}%] {}\n", b.confidence * 100.0, b.content));
            }
        }

        // Add system state
        prompt.push_str(&format!(
            "\n## System State\nCoherence: {:.4}\nAgents: {}\nEdges: {}\nTick: {}\n",
            self.world.global_coherence(),
            self.world.agents.len(),
            self.world.edges.len(),
            self.world.tick_count,
        ));

        // Add active skills context (SkillRL integration)
        let skill_count = self.world.skill_bank.all_skills().len();
        if skill_count > 0 {
            prompt.push_str("\n## Active Skill Bank\n");
            prompt.push_str(&format!("{} skills in bank. Top skills:\n", skill_count));
            let all_skills = self.world.skill_bank.all_skills();
            let mut top_skills: Vec<_> = all_skills
                .iter()
                .filter(|s| s.confidence > 0.3)
                .copied()
                .collect();
            top_skills.sort_by(|a, b| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for skill in top_skills.iter().take(5) {
                prompt.push_str(&format!(
                    "- [{}] {}: {} (conf={:.0}%, used={})\n",
                    match &skill.level {
                        crate::skill::SkillLevel::General => "G",
                        crate::skill::SkillLevel::RoleSpecific(_) => "R",
                        crate::skill::SkillLevel::TaskSpecific(_) => "T",
                    },
                    skill.title,
                    skill.principle,
                    skill.confidence * 100.0,
                    skill.usage_count
                ));
            }
        }

        prompt.push_str("\nProvide 2-5 INSIGHT: lines and 1-3 AVOID: lines. Be concise.\n");
        prompt
    }

    /// Call Ollama for reflection (non-streaming, just get the full response)
    async fn call_ollama(&self, prompt: &str) -> Result<String, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("HTTP client error: {}", e))?;

        let body = serde_json::json!({
            "model": self.ollama.model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": 0.7,
                "num_predict": 300,
            }
        });

        let resp = client
            .post(format!("{}/api/generate", self.ollama.endpoint))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Ollama connection error: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Ollama returned status {}", resp.status()));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("JSON parse error: {}", e))?;

        json["response"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "No response field in Ollama output".to_string())
    }

    /// Heuristic fallback when Ollama is unavailable
    fn heuristic_reflect(&self) -> String {
        let mut analysis = String::new();
        let coherence = self.world.global_coherence();

        // Analyze improvement trends
        let recent: Vec<f64> = self
            .world
            .improvement_history
            .iter()
            .rev()
            .take(5)
            .map(|e| e.coherence_after - e.coherence_before)
            .collect();

        let avg_delta = if recent.is_empty() {
            0.0
        } else {
            recent.iter().sum::<f64>() / recent.len() as f64
        };

        if avg_delta > 0.001 {
            analysis.push_str("INSIGHT: System is on an upward trajectory — recent mutations are improving coherence\n");
        } else if avg_delta < -0.001 {
            analysis.push_str(
                "INSIGHT: System coherence is declining — recent mutations may be too disruptive\n",
            );
            analysis.push_str("AVOID: Applying high-novelty mutations during coherence decline\n");
        } else {
            analysis.push_str("INSIGHT: System has reached a plateau — consider increasing exploration temperature\n");
        }

        if coherence > 0.8 {
            analysis.push_str(
                "INSIGHT: High coherence suggests over-optimization — introduce diversity\n",
            );
            analysis.push_str("AVOID: Further optimization when coherence exceeds 0.8\n");
        } else if coherence < 0.3 {
            analysis.push_str("INSIGHT: Low coherence indicates fragmentation — focus on strengthening connections\n");
            analysis.push_str("AVOID: Removing edges when coherence is below 0.3\n");
        }

        let edge_ratio = self.world.edges.len() as f64 / self.world.agents.len().max(1) as f64;
        if edge_ratio < 1.5 {
            analysis.push_str("INSIGHT: Sparse connectivity — agent network needs more links\n");
        } else if edge_ratio > 5.0 {
            analysis.push_str(
                "INSIGHT: Dense network may be creating noise — prune weak connections\n",
            );
        }

        analysis
    }

    fn parse_insights(analysis: &str) -> Vec<String> {
        analysis
            .lines()
            .filter(|line| line.trim_start().starts_with("INSIGHT:"))
            .map(|line| {
                line.trim_start()
                    .trim_start_matches("INSIGHT:")
                    .trim()
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn parse_avoid_patterns(analysis: &str) -> Vec<String> {
        analysis
            .lines()
            .filter(|line| line.trim_start().starts_with("AVOID:"))
            .map(|line| {
                line.trim_start()
                    .trim_start_matches("AVOID:")
                    .trim()
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect()
    }
}
