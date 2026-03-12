//! Core RLM Runtime
//!
//! The main orchestration logic:
//! 1. Load context
//! 2. Generate action via LLM
//! 3. Execute (tools or sub-queries)
//! 4. Feed results back
//! 5. Repeat until FINAL() or max iterations

use super::{
    build_rlm_system_prompt, generate_query_id, Context, FinalAnswer, LlmBridge, LlmBridgeConfig,
    LlmQuery, RlmAction, RlmError, RlmStats, SandboxConfig, SubQuery,
    SubQueryResponse, Trajectory, TrajectoryStore, DEFAULT_MAX_DEPTH, DEFAULT_MAX_ITERATIONS,
    DEFAULT_MAX_SUB_QUERIES, DEFAULT_TRUNCATE_LEN,
};
use super::executor::{ExecutionResult, RlmExecutor};
use super::trajectory::{ContextSummary, IterationSnapshot, SubQueryResultSnapshot, ToolResultSnapshot};
use std::time::Instant;

/// Configuration for the RLM runtime
#[derive(Clone, Debug)]
pub struct RlmConfig {
    pub max_iterations: usize,
    pub max_depth: usize,
    pub max_sub_queries: usize,
    pub truncate_len: usize,
    pub sandbox: SandboxConfig,
    pub llm: LlmBridgeConfig,
    pub enable_trajectory_logging: bool,
    pub trajectory_store_path: String,
}

impl Default for RlmConfig {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            max_depth: DEFAULT_MAX_DEPTH,
            max_sub_queries: DEFAULT_MAX_SUB_QUERIES,
            truncate_len: DEFAULT_TRUNCATE_LEN,
            sandbox: SandboxConfig::default(),
            llm: LlmBridgeConfig::default(),
            enable_trajectory_logging: true,
            trajectory_store_path: "./rlm_trajectories".to_string(),
        }
    }
}

/// Current status of RLM execution
#[derive(Clone, Debug)]
pub enum RlmStatus {
    Initializing,
    Iterating { current: usize, max: usize },
    ExecutingSubQueries { count: usize },
    ExecutingTools { count: usize },
    Finalizing,
    Complete { iterations: usize, duration_ms: u64 },
    Error { message: String },
}

/// A single iteration of the RLM loop
#[derive(Clone, Debug)]
pub struct RlmIteration {
    pub iteration: usize,
    pub prompt: String,
    pub action: RlmAction,
    pub execution_result: ExecutionResult,
    pub accumulated_findings: Vec<String>,
    pub duration_ms: u64,
}

/// The RLM Runtime
pub struct RlmRuntime {
    config: RlmConfig,
    executor: RlmExecutor,
    llm_bridge: LlmBridge,
    trajectory_store: Option<TrajectoryStore>,
    working_memory: Vec<String>,
}

impl RlmRuntime {
    /// Create new RLM runtime
    pub async fn new(config: RlmConfig) -> Result<Self, RlmError> {
        let executor = RlmExecutor::new(config.sandbox.clone());
        let llm_bridge = LlmBridge::new(config.llm.clone());
        
        let trajectory_store = if config.enable_trajectory_logging {
            let store = TrajectoryStore::new(&config.trajectory_store_path);
            store.initialize().await.map_err(|e| {
                RlmError::StorageError(format!("Failed to init trajectory store: {}", e))
            })?;
            Some(store)
        } else {
            None
        };
        
        Ok(Self {
            config,
            executor,
            llm_bridge,
            trajectory_store,
            working_memory: Vec::new(),
        })
    }
    
    /// Execute RLM on a query with context
    pub async fn execute(
        &mut self,
        query: impl Into<String>,
        context: &Context,
    ) -> Result<FinalAnswer, RlmError> {
        let query = query.into();
        let start_time = Instant::now();
        
        // Initialize trajectory
        let mut trajectory = Trajectory::new(
            &query,
            &context.metadata.source,
            context.metadata.total_bytes,
        );
        trajectory.metadata.model = self.config.llm.model.clone();
        trajectory.metadata.max_iterations = self.config.max_iterations;
        trajectory.metadata.max_sub_queries = self.config.max_sub_queries;
        
        let mut stats = RlmStats::default();
        let system_prompt = build_rlm_system_prompt(&self.config);
        
        // Main RLM loop
        for iteration in 0..self.config.max_iterations {
            let iter_start = Instant::now();
            
            // Build prompt for this iteration
            let prompt = self.build_iteration_prompt(
                &query,
                context,
                iteration,
                &system_prompt,
            );
            
            // Get action from LLM
            let action = self.query_llm_for_action(&prompt).await?;
            
            // Execute action
            let (execution_result, subquery_responses) = match &action {
                RlmAction::ExecuteTools { .. } => {
                    let result = self.executor.execute_action(&action).await?;
                    (result, Vec::new())
                }
                RlmAction::SubQueries { queries } => {
                    let result = self.executor.execute_action(&action).await?;
                    let responses = self.execute_sub_queries(queries, context).await?;
                    
                    // Store responses in working memory
                    for resp in &responses {
                        self.working_memory.push(format!(
                            "[Query {}]: {}",
                            resp.query_id, resp.result
                        ));
                    }
                    
                    stats.total_sub_queries += queries.len();
                    (result, responses)
                }
                RlmAction::Final { answer } => {
                    // Store final answer in trajectory
                    trajectory.set_final_answer(&answer.answer);
                    stats.total_iterations = iteration + 1;
                    stats.total_duration_ms = start_time.elapsed().as_millis() as u64;
                    trajectory.update_stats(stats);
                    
                    // Persist trajectory
                    if let Some(store) = &mut self.trajectory_store {
                        let _ = store.save(&trajectory).await;
                    }
                    
                    return Ok(answer.clone());
                }
                RlmAction::RequestContext { request } => {
                    self.working_memory.push(format!(
                        "[Context Request]: {}",
                        request
                    ));
                    let result = self.executor.execute_action(&action).await?;
                    (result, Vec::new())
                }
            };
            
            // Record iteration in trajectory
            let snapshot = self.create_iteration_snapshot(
                iteration + 1,
                &action,
                &execution_result,
                &subquery_responses,
                &prompt,
                iter_start.elapsed().as_millis() as u64,
            );
            trajectory.add_iteration(snapshot);
            
            stats.total_iterations = iteration + 1;
            stats.total_tool_calls += execution_result.tool_results.len();
        }
        
        // Max iterations exceeded
        let error_msg = format!("Max iterations ({}) exceeded", self.config.max_iterations);
        trajectory.mark_failed(&error_msg);
        
        if let Some(store) = &mut self.trajectory_store {
            let _ = store.save(&trajectory).await;
        }
        
        Err(RlmError::MaxIterationsExceeded(self.config.max_iterations))
    }
    
    /// Build the prompt for an iteration
    fn build_iteration_prompt(
        &self,
        query: &str,
        context: &Context,
        iteration: usize,
        system_prompt: &str,
    ) -> String {
        let mut prompt = String::new();
        
        // System instruction
        prompt.push_str(system_prompt);
        prompt.push_str("\n\n");
        
        // Context metadata
        prompt.push_str(&context.to_llm_metadata());
        prompt.push_str("\n\n");
        
        // Main query
        prompt.push_str(&format!("## QUERY\n{}\n\n", query));
        
        // Iteration info
        prompt.push_str(&format!("## ITERATION {}\n", iteration + 1));
        prompt.push_str(&format!("Chunks available: {}\n", context.chunks.len()));
        
        // Working memory (accumulated findings)
        if !self.working_memory.is_empty() {
            prompt.push_str("\n## ACCUMULATED FINDINGS\n");
            for (i, finding) in self.working_memory.iter().rev().take(10).enumerate() {
                prompt.push_str(&format!("{}. {}\n", i + 1, finding));
            }
        }
        
        // Available chunks for sub-queries
        if context.chunks.len() > 1 {
            prompt.push_str("\n## AVAILABLE CHUNKS\n");
            for chunk in &context.chunks {
                let preview: String = chunk.content.chars().take(80).collect();
                prompt.push_str(&format!(
                    "Chunk {} (lines {}-{}): {}...\n",
                    chunk.index,
                    chunk.start_line,
                    chunk.end_line,
                    preview
                ));
            }
        }
        
        prompt.push_str("\n## YOUR TURN\n");
        prompt.push_str("Respond with your action in the specified JSON format.\n");
        
        prompt
    }
    
    /// Query LLM for next action
    async fn query_llm_for_action(&mut self, prompt: &str) -> Result<RlmAction, RlmError> {
        let response = self.llm_bridge
            .generate_direct(prompt)
            .await
            .map_err(|e| RlmError::LlmQueryFailed(format!("LLM error: {}", e)))?;
        
        // Try to parse JSON from response
        let action = self.parse_action_from_response(&response)?;
        Ok(action)
    }
    
    /// Parse action from LLM response
    fn parse_action_from_response(&self, response: &str) -> Result<RlmAction, RlmError> {
        // First, try to find JSON in the response
        let json_str = if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                &response[start..=end]
            } else {
                response
            }
        } else {
            response
        };
        
        // Try to parse as JSON
        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(json) => {
                // Check for "action" field
                if let Some(action_type) = json.get("action").and_then(|a| a.as_str()) {
                    match action_type {
                        "execute_tools" | "ExecuteTools" => {
                            let tool_calls = json.get("tool_calls")
                                .and_then(|t| t.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| serde_json::from_value(v.clone()).ok())
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            Ok(RlmAction::ExecuteTools { tool_calls })
                        }
                        "sub_queries" | "SubQueries" => {
                            let queries = json.get("queries")
                                .and_then(|q| q.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .enumerate()
                                        .filter_map(|(_i, v)| {
                                            let mut query: SubQuery = serde_json::from_value(v.clone()).ok()?;
                                            if query.id.is_empty() {
                                                query.id = generate_query_id();
                                            }
                                            Some(query)
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            Ok(RlmAction::SubQueries { queries })
                        }
                        "final" | "Final" | "FINAL" => {
                            if let Some(answer) = json.get("answer") {
                                let final_answer: FinalAnswer = serde_json::from_value(answer.clone())
                                    .unwrap_or_else(|_| FinalAnswer {
                                        answer: answer.as_str().unwrap_or("").to_string(),
                                        confidence: 0.8,
                                        reasoning: vec![],
                                    });
                                Ok(RlmAction::Final { answer: final_answer })
                            } else {
                                Err(RlmError::InvalidAction("Missing 'answer' in FINAL action".to_string()))
                            }
                        }
                        "request_context" | "RequestContext" => {
                            let request = json.get("request")
                                .and_then(|r| r.as_str())
                                .unwrap_or("")
                                .to_string();
                            Ok(RlmAction::RequestContext { request })
                        }
                        _ => Err(RlmError::InvalidAction(format!("Unknown action type: {}", action_type))),
                    }
                } else {
                    // Try to infer action from response content
                    if response.contains("FINAL") || response.contains("final answer") {
                        Ok(RlmAction::Final {
                            answer: FinalAnswer {
                                answer: response.to_string(),
                                confidence: 0.7,
                                reasoning: vec!["Inferred from unstructured response".to_string()],
                            }
                        })
                    } else {
                        Err(RlmError::InvalidAction("No 'action' field found in response".to_string()))
                    }
                }
            }
            Err(e) => {
                // Fallback: treat as text response, try to infer FINAL
                if response.len() < 500 && !response.contains("{") {
                    Ok(RlmAction::Final {
                        answer: FinalAnswer {
                            answer: response.trim().to_string(),
                            confidence: 0.6,
                            reasoning: vec!["Parsed from unstructured text response".to_string()],
                        }
                    })
                } else {
                    Err(RlmError::InvalidAction(format!("Failed to parse JSON: {}", e)))
                }
            }
        }
    }
    
    /// Execute sub-queries in parallel
    async fn execute_sub_queries(
        &mut self,
        queries: &[SubQuery],
        context: &Context,
    ) -> Result<Vec<SubQueryResponse>, RlmError> {
        // Convert to LlmQuery objects
        let llm_queries: Vec<LlmQuery> = queries
            .iter()
            .filter_map(|sq| {
                context.get_chunk(sq.chunk_index).map(|chunk| {
                    LlmQuery {
                        id: sq.id.clone(),
                        chunk_content: chunk.content.clone(),
                        instruction: sq.instruction.clone(),
                        context_preview: sq.context_preview.clone(),
                    }
                })
            })
            .collect();
        
        // Execute in parallel
        let results = self.llm_bridge.query_parallel(llm_queries).await;
        
        // Collect successful results
        let responses: Vec<SubQueryResponse> = results
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();
        
        Ok(responses)
    }
    
    /// Create iteration snapshot for trajectory
    fn create_iteration_snapshot(
        &self,
        iteration: usize,
        action: &RlmAction,
        execution: &ExecutionResult,
        subquery_responses: &[SubQueryResponse],
        prompt: &str,
        duration_ms: u64,
    ) -> IterationSnapshot {
        let tool_results = execution.tool_results.iter().map(|r| {
            ToolResultSnapshot {
                tool_name: "tool".to_string(), // Would need to map from call_id
                success: r.success,
                output_preview: r.output.chars().take(100).collect(),
                error: r.error.clone(),
                duration_ms: duration_ms / execution.tool_results.len().max(1) as u64,
            }
        }).collect();
        
        let subquery_snapshots = subquery_responses.iter().map(|r| {
            SubQueryResultSnapshot {
                query_id: r.query_id.clone(),
                chunk_index: 0, // Would need to track this
                success: true,
                result_preview: r.result.chars().take(100).collect(),
                duration_ms: r.duration_ms,
            }
        }).collect();
        
        IterationSnapshot {
            iteration,
            timestamp: chrono::Utc::now(),
            action: action.clone(),
            tool_results,
            subquery_results: subquery_snapshots,
            context_summary: ContextSummary {
                chunks_processed: vec![],
                accumulated_findings: self.working_memory.clone(),
                working_memory_size: self.working_memory.len(),
            },
            prompt_preview: prompt.chars().take(200).collect(),
            response_preview: format!("{:?}", action),
            duration_ms,
        }
    }
    
    /// Get trajectory store
    pub fn trajectory_store(&self) -> Option<&TrajectoryStore> {
        self.trajectory_store.as_ref()
    }
    
    /// Get mutable trajectory store
    pub fn trajectory_store_mut(&mut self) -> Option<&mut TrajectoryStore> {
        self.trajectory_store.as_mut()
    }
    
    /// Clear working memory
    pub fn clear_memory(&mut self) {
        self.working_memory.clear();
    }
    
    /// Get working memory
    pub fn working_memory(&self) -> &[String] {
        &self.working_memory
    }
}

/// Convenience function for one-shot RLM execution
pub async fn run_rlm(
    query: impl Into<String>,
    context: &Context,
    config: Option<RlmConfig>,
) -> Result<FinalAnswer, RlmError> {
    let config = config.unwrap_or_default();
    let mut runtime = RlmRuntime::new(config).await?;
    runtime.execute(query, context).await
}
