use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::Role;
use crate::hyper_stigmergy::{BeliefSource, HyperStigmergicMorphogenesis};
use crate::tools::{ToolOutput, ToolRegistry};

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
    /// Tool registry for real tool execution in the learning loop
    pub tool_registry: Arc<ToolRegistry>,
    /// History of tool executions for reflection and learning
    pub tool_execution_log: Vec<ToolExecutionRecord>,
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
    /// Trigger one world `execute_self_improvement_cycle` (supervision loop: propose → review → record).
    SelfImprove { intent: String },
    SpawnSubAgent { role: Role, specialty: String },
}

/// Record of a tool execution for learning feedback
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolExecutionRecord {
    pub tool_name: String,
    pub parameters: Value,
    pub success: bool,
    pub output_preview: String,
    pub duration_ms: u64,
    pub timestamp: u64,
}

#[derive(Clone, Debug)]
pub enum Context {
    Text(String),
    ActionExecuted(String),
    Prediction(f32),
    NoveltyScore(f32),
    ImprovementResult(crate::hyper_stigmergy::ImprovementResult),
    Reflection(ReflectionResult),
    /// Result from actual tool execution
    ToolResult {
        tool_name: String,
        success: bool,
        output: String,
    },
}

/// Runs multiple bounded supervision iterations against the same intent (recursive conditioning via `history`).
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
    // Initialize the real tool registry with all 60+ tools
    let mut registry = ToolRegistry::new();
    crate::tools::register_all_tools(&mut registry);

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
        tool_registry: Arc::new(registry),
        tool_execution_log: Vec::new(),
    }
}

impl RLM {
    /// Execute a tool by name through the registry
    async fn run_tool(&mut self, tool_name: &str, params: Value) -> Context {
        let start = std::time::Instant::now();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let output = if let Some(tool) = self.tool_registry.get(tool_name) {
            tool.execute(params.clone()).await
        } else {
            ToolOutput::error(format!("Tool '{}' not found in registry", tool_name))
        };

        let duration_ms = start.elapsed().as_millis() as u64;
        let preview: String = output.result.chars().take(500).collect();

        // Record for learning feedback
        self.tool_execution_log.push(ToolExecutionRecord {
            tool_name: tool_name.to_string(),
            parameters: params,
            success: output.success,
            output_preview: preview.clone(),
            duration_ms,
            timestamp: now,
        });

        // Trim log to last 50 entries
        if self.tool_execution_log.len() > 50 {
            self.tool_execution_log.drain(..self.tool_execution_log.len() - 50);
        }

        Context::ToolResult {
            tool_name: tool_name.to_string(),
            success: output.success,
            output: if output.success {
                output.result
            } else {
                output.error.unwrap_or_else(|| "unknown error".to_string())
            },
        }
    }

    /// Ask the LLM to select tools for an intent, then execute them
    async fn llm_driven_tool_dispatch(&mut self, intent: &str, role: Role) -> anyhow::Result<Vec<Context>> {
        // Build a tool-selection prompt
        let available_tools: Vec<_> = self.tool_registry.list_tools()
            .into_iter()
            .map(|(name, desc)| format!("  - {}: {}", name, desc))
            .collect();

        let prompt = format!(
            "You are an agent with role {:?} in a hyper-stigmergic morphogenesis system.\n\
             Intent: {}\n\n\
             Available tools:\n{}\n\n\
             Select 1-3 tools to execute for this intent. Respond in JSON:\n\
             {{\"tool_calls\": [{{\"tool\": \"<name>\", \"params\": {{...}}}}]}}\n\
             If no tools are needed, respond: {{\"tool_calls\": []}}\n\
             Be concise. Only call tools that directly address the intent.",
            role,
            intent,
            available_tools.join("\n")
        );

        // Try LLM-driven selection
        match self.call_ollama(&prompt).await {
            Ok(response) => {
                let tool_calls = Self::parse_tool_calls(&response);
                if tool_calls.is_empty() {
                    return Ok(vec![Context::Text(format!(
                        "{:?} analyzed intent (no tools needed): {}", role, intent
                    ))]);
                }

                let mut results = Vec::new();
                for (tool_name, params) in tool_calls {
                    let result = self.run_tool(&tool_name, params).await;
                    results.push(result);
                }
                Ok(results)
            }
            Err(_) => {
                // Fallback to heuristic dispatch
                self.heuristic_tool_dispatch(intent, role).await
            }
        }
    }

    /// Heuristic tool dispatch when LLM is unavailable
    async fn heuristic_tool_dispatch(&mut self, intent: &str, role: Role) -> anyhow::Result<Vec<Context>> {
        let intent_lower = intent.to_lowercase();
        let mut results = Vec::new();

        match role {
            Role::Coder => {
                if intent_lower.contains("read") || intent_lower.contains("file") {
                    // Try to extract a file path from intent
                    let path = Self::extract_path_from_intent(intent).unwrap_or_else(|| ".".to_string());
                    results.push(self.run_tool("list_directory", serde_json::json!({"path": path})).await);
                }
                if intent_lower.contains("search") || intent_lower.contains("find") || intent_lower.contains("grep") {
                    let pattern = Self::extract_pattern_from_intent(intent);
                    results.push(self.run_tool("grep", serde_json::json!({"pattern": pattern, "path": "."})).await);
                }
                if intent_lower.contains("run") || intent_lower.contains("execute") || intent_lower.contains("test") {
                    let cmd = Self::extract_command_from_intent(intent).unwrap_or_else(|| "echo 'no command specified'".to_string());
                    results.push(self.run_tool("bash", serde_json::json!({"command": cmd})).await);
                }
                if intent_lower.contains("status") || intent_lower.contains("git") {
                    results.push(self.run_tool("git_status", serde_json::json!({})).await);
                }
            }
            Role::Architect => {
                // Structural analysis: read project structure
                results.push(self.run_tool("list_directory", serde_json::json!({"path": ".", "recursive": false})).await);
                if intent_lower.contains("topology") || intent_lower.contains("structure") {
                    results.push(self.run_tool("grep", serde_json::json!({"pattern": "pub struct|pub fn|pub mod", "path": "src/"})).await);
                }
            }
            Role::Catalyst => {
                // Exploration: search for novelty
                if intent_lower.contains("web") || intent_lower.contains("search") || intent_lower.contains("research") {
                    let query = Self::extract_pattern_from_intent(intent);
                    results.push(self.run_tool("web_search", serde_json::json!({"query": query})).await);
                } else {
                    // Search codebase for innovation opportunities
                    results.push(self.run_tool("grep", serde_json::json!({"pattern": "TODO|FIXME|HACK|OPTIMIZE", "path": "."})).await);
                }
            }
            Role::Chronicler => {
                // Documentation: gather system state
                results.push(self.run_tool("git_log", serde_json::json!({"count": 10})).await);
                results.push(self.run_tool("system_info", serde_json::json!({})).await);
            }
            Role::Critic => {
                // Skeptical analysis: look for problems
                results.push(self.run_tool("grep", serde_json::json!({"pattern": "unwrap\\(\\)|panic!|unsafe", "path": "src/"})).await);
            }
            Role::Explorer => {
                // Broad exploration: find files and patterns
                results.push(self.run_tool("find", serde_json::json!({"pattern": "*.rs", "path": "src/"})).await);
            }
        }

        // Fallback if no tools matched
        if results.is_empty() {
            results.push(Context::Text(format!(
                "{:?} processed intent (heuristic, no tool match): {}", role, intent
            )));
        }

        Ok(results)
    }

    /// Parse tool calls from LLM JSON response
    fn parse_tool_calls(response: &str) -> Vec<(String, Value)> {
        // Find JSON in response
        let json_str = if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                &response[start..=end]
            } else {
                return Vec::new();
            }
        } else {
            return Vec::new();
        };

        let parsed: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        parsed.get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|call| {
                        let tool = call.get("tool")?.as_str()?.to_string();
                        let params = call.get("params").cloned().unwrap_or(serde_json::json!({}));
                        Some((tool, params))
                    })
                    .take(3) // Max 3 tool calls per dispatch
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Extract a file path from an intent string
    fn extract_path_from_intent(intent: &str) -> Option<String> {
        // Look for common path patterns
        for word in intent.split_whitespace() {
            if word.contains('/') || word.contains('.') && !word.starts_with('.') {
                // Looks like a path
                let cleaned = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-');
                if !cleaned.is_empty() {
                    return Some(cleaned.to_string());
                }
            }
        }
        None
    }

    /// Extract a search pattern from an intent string
    fn extract_pattern_from_intent(intent: &str) -> String {
        // Remove common prefixes and use the rest as pattern
        let cleaned = intent
            .replace("search for", "")
            .replace("find", "")
            .replace("grep", "")
            .replace("look for", "")
            .trim()
            .to_string();
        if cleaned.is_empty() { intent.to_string() } else { cleaned }
    }

    /// Extract a shell command from an intent string
    fn extract_command_from_intent(intent: &str) -> Option<String> {
        // Look for backtick-wrapped commands or common prefixes
        if let Some(start) = intent.find('`') {
            if let Some(end) = intent[start + 1..].find('`') {
                return Some(intent[start + 1..start + 1 + end].to_string());
            }
        }
        // Look for "run X" or "execute X"
        for prefix in &["run ", "execute ", "test "] {
            if let Some(pos) = intent.to_lowercase().find(prefix) {
                let cmd = &intent[pos + prefix.len()..];
                if !cmd.is_empty() {
                    return Some(cmd.trim().to_string());
                }
            }
        }
        None
    }

    pub async fn execute(&mut self, intent: &str) -> anyhow::Result<Vec<Context>> {
        let selected_role = self.world.select_role_via_bidding(&self.bid_config);

        println!("  Role selected via bidding: {:?}", selected_role);

        let results = match selected_role {
            Role::Architect => self.architect_handler(intent).await,
            Role::Catalyst => self.catalyst_handler(intent).await,
            Role::Chronicler => self.chronicler_handler(intent).await,
            Role::Critic => self.architect_handler(intent).await,
            Role::Explorer => self.catalyst_handler(intent).await,
            Role::Coder => self.coder_handler(intent).await,
        };

        // Log tool execution summary to living prompt
        let tool_results_in_batch: Vec<_> = match &results {
            Ok(ctxs) => ctxs.iter().filter_map(|c| match c {
                Context::ToolResult { tool_name, success, output } => {
                    let preview: String = output.chars().take(100).collect();
                    Some(format!("{}:{} → {}", tool_name, if *success { "ok" } else { "FAIL" }, preview))
                }
                _ => None,
            }).collect(),
            Err(_) => Vec::new(),
        };

        if !tool_results_in_batch.is_empty() {
            self.living_prompt.add_insight(format!(
                "[Tools] {} executed: {}",
                tool_results_in_batch.len(),
                tool_results_in_batch.join(" | ")
            ));
        }

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
        let mut results = vec![
            Context::Prediction(prediction),
        ];

        // Execute real tools for structural analysis
        let tool_results = self.llm_driven_tool_dispatch(intent, Role::Architect).await?;
        results.extend(tool_results);

        Ok(results)
    }

    async fn catalyst_handler(&mut self, intent: &str) -> anyhow::Result<Vec<Context>> {
        let novelty = self.world.compute_novelty(intent);
        let mut results = vec![Context::NoveltyScore(novelty)];

        if novelty > 0.7 {
            let result = self.world.execute_self_improvement_cycle(intent);
            results.push(Context::ImprovementResult(result));
        }

        // Execute real tools for exploration
        let tool_results = self.llm_driven_tool_dispatch(intent, Role::Catalyst).await?;
        results.extend(tool_results);

        Ok(results)
    }

    async fn chronicler_handler(&mut self, intent: &str) -> anyhow::Result<Vec<Context>> {
        let mut results = vec![
            Context::Text(format!(
                "Recorded {} improvement events",
                self.world.improvement_history.len()
            )),
        ];

        // Execute real tools for documentation/logging
        let tool_results = self.llm_driven_tool_dispatch(intent, Role::Chronicler).await?;
        results.extend(tool_results);

        Ok(results)
    }

    async fn coder_handler(&mut self, intent: &str) -> anyhow::Result<Vec<Context>> {
        // Execute real tools based on LLM analysis of intent
        self.llm_driven_tool_dispatch(intent, Role::Coder).await
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

        // Add recent tool execution results for learning
        if !self.tool_execution_log.is_empty() {
            prompt.push_str("\n## Recent Tool Executions\n");
            for record in self.tool_execution_log.iter().rev().take(10) {
                prompt.push_str(&format!(
                    "- {} [{}] ({}ms): {}\n",
                    record.tool_name,
                    if record.success { "OK" } else { "FAIL" },
                    record.duration_ms,
                    record.output_preview.chars().take(80).collect::<String>()
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

        // Analyze tool execution patterns
        if !self.tool_execution_log.is_empty() {
            let total = self.tool_execution_log.len();
            let failures = self.tool_execution_log.iter().filter(|r| !r.success).count();
            let failure_rate = failures as f64 / total as f64;

            if failure_rate > 0.5 {
                analysis.push_str(&format!(
                    "INSIGHT: High tool failure rate ({:.0}%) — review tool selection strategy\n",
                    failure_rate * 100.0
                ));
                analysis.push_str("AVOID: Calling tools without validating parameters first\n");
            } else if total > 5 {
                analysis.push_str(&format!(
                    "INSIGHT: Tool execution healthy ({} calls, {:.0}% success rate)\n",
                    total,
                    (1.0 - failure_rate) * 100.0
                ));
            }

            // Check for slow tools
            let avg_ms: f64 = self.tool_execution_log.iter().map(|r| r.duration_ms as f64).sum::<f64>() / total as f64;
            if avg_ms > 5000.0 {
                analysis.push_str("INSIGHT: Tool execution is slow — consider caching or lighter tools\n");
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_rlm() -> RLM {
        let world = HyperStigmergicMorphogenesis::new(4);
        let mut registry = ToolRegistry::new();
        crate::tools::register_all_tools(&mut registry);

        RLM {
            ollama: OllamaHandle {
                model: "test".to_string(),
                endpoint: "http://localhost:11434".to_string(),
            },
            world,
            bid_config: BidConfig::default(),
            embedding_cache: EmbeddingCache::new(100),
            sub_agents: Vec::new(),
            living_prompt: LivingPrompt::new("Test prompt"),
            tool_registry: Arc::new(registry),
            tool_execution_log: Vec::new(),
        }
    }

    #[tokio::test]
    async fn test_run_tool_read_file() {
        let mut rlm = make_test_rlm();

        let result = rlm.run_tool("read_file", serde_json::json!({"path": "Cargo.toml"})).await;

        match result {
            Context::ToolResult { tool_name, success, output } => {
                assert_eq!(tool_name, "read_file");
                assert!(success, "read_file failed: {}", output);
                assert!(output.contains("[package]"), "Expected Cargo.toml contents");
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }

        // Should have recorded in execution log
        assert_eq!(rlm.tool_execution_log.len(), 1);
        assert_eq!(rlm.tool_execution_log[0].tool_name, "read_file");
        assert!(rlm.tool_execution_log[0].success);
    }

    #[tokio::test]
    async fn test_run_tool_calculator() {
        let mut rlm = make_test_rlm();

        let result = rlm.run_tool("calculator", serde_json::json!({"expression": "3 * 7"})).await;

        match result {
            Context::ToolResult { tool_name, success, output } => {
                assert_eq!(tool_name, "calculator");
                assert!(success, "calculator failed: {}", output);
                assert!(output.contains("21"), "3*7 should be 21, got: {}", output);
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_run_tool_nonexistent() {
        let mut rlm = make_test_rlm();

        let result = rlm.run_tool("nonexistent_tool", serde_json::json!({})).await;

        match result {
            Context::ToolResult { success, output, .. } => {
                assert!(!success, "Should have failed for nonexistent tool");
                assert!(output.contains("not found"), "Error should mention 'not found': {}", output);
            }
            other => panic!("Expected ToolResult, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_heuristic_dispatch_coder_git() {
        let mut rlm = make_test_rlm();

        let results = rlm.heuristic_tool_dispatch("check git status", Role::Coder).await.unwrap();

        // Should have called git_status
        let has_tool_result = results.iter().any(|r| matches!(r, Context::ToolResult { tool_name, .. } if tool_name == "git_status"));
        assert!(has_tool_result, "Expected git_status tool result, got: {:?}", results);
    }

    #[tokio::test]
    async fn test_heuristic_dispatch_architect() {
        let mut rlm = make_test_rlm();

        let results = rlm.heuristic_tool_dispatch("analyze project structure", Role::Architect).await.unwrap();

        // Should have at least one tool result (list_directory)
        let has_tool_result = results.iter().any(|r| matches!(r, Context::ToolResult { .. }));
        assert!(has_tool_result, "Architect should execute at least one tool, got: {:?}", results);
    }

    #[tokio::test]
    async fn test_heuristic_dispatch_critic() {
        let mut rlm = make_test_rlm();

        let results = rlm.heuristic_tool_dispatch("review code safety", Role::Critic).await.unwrap();

        // Should have called grep for unwrap/panic/unsafe
        let has_tool_result = results.iter().any(|r| matches!(r, Context::ToolResult { tool_name, .. } if tool_name == "grep"));
        assert!(has_tool_result, "Critic should grep for unsafe patterns, got: {:?}", results);
    }

    #[tokio::test]
    async fn test_tool_execution_log_growth() {
        let mut rlm = make_test_rlm();

        // Execute many tools
        for i in 0..60 {
            rlm.run_tool("calculator", serde_json::json!({"expression": format!("{} + 1", i)})).await;
        }

        // Log should be capped at 50
        assert!(rlm.tool_execution_log.len() <= 50,
            "Log should be capped at 50, got {}", rlm.tool_execution_log.len());
    }

    #[test]
    fn test_parse_tool_calls() {
        let response = r#"{"tool_calls": [{"tool": "grep", "params": {"pattern": "fn main"}}, {"tool": "read_file", "params": {"path": "src/main.rs"}}]}"#;
        let calls = RLM::parse_tool_calls(response);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "grep");
        assert_eq!(calls[1].0, "read_file");
    }

    #[test]
    fn test_parse_tool_calls_empty() {
        let response = r#"{"tool_calls": []}"#;
        let calls = RLM::parse_tool_calls(response);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_tool_calls_max_3() {
        let response = r#"{"tool_calls": [{"tool": "a", "params": {}}, {"tool": "b", "params": {}}, {"tool": "c", "params": {}}, {"tool": "d", "params": {}}]}"#;
        let calls = RLM::parse_tool_calls(response);
        assert_eq!(calls.len(), 3, "Should cap at 3 tool calls");
    }

    #[test]
    fn test_parse_tool_calls_invalid_json() {
        let response = "This is not JSON at all";
        let calls = RLM::parse_tool_calls(response);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_extract_path_from_intent() {
        assert_eq!(RLM::extract_path_from_intent("read src/main.rs"), Some("src/main.rs".to_string()));
        assert_eq!(RLM::extract_path_from_intent("no path here"), None);
    }

    #[test]
    fn test_extract_command_from_intent() {
        assert_eq!(RLM::extract_command_from_intent("run `cargo test`"), Some("cargo test".to_string()));
        assert_eq!(RLM::extract_command_from_intent("execute cargo build"), Some("cargo build".to_string()));
        assert_eq!(RLM::extract_command_from_intent("just some text"), None);
    }
}
