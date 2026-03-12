//! Mastra-inspired workflow engine for hyper-stigmergic morphogenesis.
//!
//! Provides composable workflow primitives:
//! - `.then()` — sequential step chaining
//! - `.parallel()` — concurrent independent steps
//! - `.branch()` — conditional routing
//! - `.do_until()` — iteration until condition met
//! - `.for_each()` — apply step to collection items
//! - `.suspend()` — human-in-the-loop pause/resume
//! - `.map()` — data transformation between steps
//!
//! Reference: Mastra Workflows (mastra.ai/docs/workflows/overview)

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

// ── World Context ────────────────────────────────────────────────────────

/// Shared mutable context passed through workflow steps.
/// Contains world state snapshots, skill retrievals, and braid results.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowContext {
    /// Key-value store for step outputs
    pub values: HashMap<String, ContextValue>,
    /// Current tick number
    pub tick: u64,
    /// Global coherence at workflow start
    pub coherence: f64,
    /// Number of agents
    pub agent_count: usize,
    /// Number of edges
    pub edge_count: usize,
    /// Accumulated step outputs keyed by step ID
    pub step_results: HashMap<String, ContextValue>,
    /// Whether a suspend was requested
    pub suspended: bool,
    /// Suspend payload if suspended
    pub suspend_payload: Option<SuspendPayload>,
    /// Resume data if resumed from suspension
    pub resume_data: Option<HashMap<String, String>>,
    /// Iteration counter for loops
    pub iteration_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ContextValue {
    Float(f64),
    Int(i64),
    Str(String),
    Bool(bool),
    List(Vec<ContextValue>),
    Map(HashMap<String, ContextValue>),
    /// Serialized action or complex data
    Json(String),
}

impl ContextValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ContextValue::Float(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ContextValue::Int(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ContextValue::Str(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ContextValue::Bool(v) => Some(*v),
            _ => None,
        }
    }
}

impl Default for WorkflowContext {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            tick: 0,
            coherence: 0.0,
            agent_count: 0,
            edge_count: 0,
            step_results: HashMap::new(),
            suspended: false,
            suspend_payload: None,
            resume_data: None,
            iteration_count: 0,
        }
    }
}

impl WorkflowContext {
    pub fn set(&mut self, key: &str, value: ContextValue) {
        self.values.insert(key.to_string(), value);
    }

    pub fn get(&self, key: &str) -> Option<&ContextValue> {
        self.values.get(key)
    }

    pub fn get_step_result(&self, step_id: &str) -> Option<&ContextValue> {
        self.step_results.get(step_id)
    }
}

// ── Suspend/Resume ───────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SuspendPayload {
    pub reason: String,
    pub step_id: String,
    pub details: HashMap<String, String>,
    pub timestamp: u64,
}

// ── Step Result ──────────────────────────────────────────────────────────

/// Discriminated union for workflow execution results (Mastra pattern)
#[derive(Clone, Debug)]
pub enum StepResult {
    Success {
        step_id: String,
        output: WorkflowContext,
    },
    Failed {
        step_id: String,
        error: String,
    },
    Suspended {
        step_id: String,
        payload: SuspendPayload,
        context: WorkflowContext,
    },
    Skipped {
        step_id: String,
        reason: String,
    },
}

impl StepResult {
    pub fn is_success(&self) -> bool {
        matches!(self, StepResult::Success { .. })
    }
    pub fn is_failed(&self) -> bool {
        matches!(self, StepResult::Failed { .. })
    }
    pub fn is_suspended(&self) -> bool {
        matches!(self, StepResult::Suspended { .. })
    }

    pub fn context(&self) -> Option<&WorkflowContext> {
        match self {
            StepResult::Success { output, .. } => Some(output),
            StepResult::Suspended { context, .. } => Some(context),
            _ => None,
        }
    }

    pub fn into_context(self) -> Option<WorkflowContext> {
        match self {
            StepResult::Success { output, .. } => Some(output),
            StepResult::Suspended { context, .. } => Some(context),
            _ => None,
        }
    }

    pub fn step_id(&self) -> &str {
        match self {
            StepResult::Success { step_id, .. } => step_id,
            StepResult::Failed { step_id, .. } => step_id,
            StepResult::Suspended { step_id, .. } => step_id,
            StepResult::Skipped { step_id, .. } => step_id,
        }
    }
}

// ── Workflow Step Trait ───────────────────────────────────────────────────

/// Core trait for executable workflow steps.
/// Each step receives context, transforms it, and returns a result.
pub trait WorkflowStepFn: Send + Sync {
    fn id(&self) -> &str;
    fn execute<'a>(
        &'a self,
        ctx: WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = StepResult> + Send + 'a>>;
}

/// Condition function for branching and loops
pub trait ConditionFn: Send + Sync {
    fn evaluate(&self, ctx: &WorkflowContext) -> bool;
}

/// Map/transform function between steps
pub trait MapFn: Send + Sync {
    fn transform(&self, ctx: WorkflowContext) -> WorkflowContext;
}

// ── Concrete Step Wrapper ────────────────────────────────────────────────

/// A named step wrapping an async function
pub struct FnStep<F>
where
    F: Fn(WorkflowContext) -> Pin<Box<dyn Future<Output = StepResult> + Send>> + Send + Sync,
{
    pub step_id: String,
    pub func: F,
}

impl<F> WorkflowStepFn for FnStep<F>
where
    F: Fn(WorkflowContext) -> Pin<Box<dyn Future<Output = StepResult> + Send>> + Send + Sync,
{
    fn id(&self) -> &str {
        &self.step_id
    }
    fn execute<'a>(
        &'a self,
        ctx: WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = StepResult> + Send + 'a>> {
        (self.func)(ctx)
    }
}

/// Closure-based condition
pub struct FnCondition<F: Fn(&WorkflowContext) -> bool + Send + Sync>(pub F);

impl<F: Fn(&WorkflowContext) -> bool + Send + Sync> ConditionFn for FnCondition<F> {
    fn evaluate(&self, ctx: &WorkflowContext) -> bool {
        (self.0)(ctx)
    }
}

/// Closure-based map
pub struct FnMap<F: Fn(WorkflowContext) -> WorkflowContext + Send + Sync>(pub F);

impl<F: Fn(WorkflowContext) -> WorkflowContext + Send + Sync> MapFn for FnMap<F> {
    fn transform(&self, ctx: WorkflowContext) -> WorkflowContext {
        (self.0)(ctx)
    }
}

// ── Workflow Node (AST) ──────────────────────────────────────────────────

/// Workflow definition as an AST of composable nodes.
/// This is the "compiled" form before execution.
pub enum WorkflowNode {
    /// Execute a single step
    Step(Arc<dyn WorkflowStepFn>),

    /// Sequential: execute nodes in order, threading context through
    Then(Vec<WorkflowNode>),

    /// Parallel: execute all nodes concurrently, merge outputs
    Parallel(Vec<WorkflowNode>),

    /// Branch: evaluate conditions, execute first matching branch
    Branch(Vec<(Arc<dyn ConditionFn>, WorkflowNode)>),

    /// DoUntil: repeat node until condition returns true
    DoUntil {
        body: Box<WorkflowNode>,
        condition: Arc<dyn ConditionFn>,
        max_iterations: usize,
    },

    /// DoWhile: repeat node while condition returns true
    DoWhile {
        body: Box<WorkflowNode>,
        condition: Arc<dyn ConditionFn>,
        max_iterations: usize,
    },

    /// Map: transform context between steps
    Map(Arc<dyn MapFn>),

    /// Suspend: pause workflow for human-in-the-loop
    Suspend { reason: String, step_id: String },
}

// ── Workflow Builder ─────────────────────────────────────────────────────

/// Fluent builder for constructing workflow definitions (Mastra pattern).
///
/// ```ignore
/// let workflow = WorkflowBuilder::new("tick_workflow")
///     .then(observe_step)
///     .parallel(vec![bidding_step, decay_step, drift_step])
///     .then(select_winner_step)
///     .branch(vec![
///         (is_architect, architect_workflow),
///         (is_catalyst, catalyst_workflow),
///         (is_chronicler, chronicler_workflow),
///     ])
///     .then(apply_action_step)
///     .do_until(refine_step, coherence_improved, 5)
///     .then(record_experience_step)
///     .build();
/// ```
pub struct WorkflowBuilder {
    pub id: String,
    nodes: Vec<WorkflowNode>,
}

impl WorkflowBuilder {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            nodes: Vec::new(),
        }
    }

    /// Sequential step: `.then(step)`
    pub fn then(mut self, step: Arc<dyn WorkflowStepFn>) -> Self {
        self.nodes.push(WorkflowNode::Step(step));
        self
    }

    /// Parallel execution: `.parallel([step1, step2, step3])`
    pub fn parallel(mut self, steps: Vec<Arc<dyn WorkflowStepFn>>) -> Self {
        let nodes = steps.into_iter().map(|s| WorkflowNode::Step(s)).collect();
        self.nodes.push(WorkflowNode::Parallel(nodes));
        self
    }

    /// Conditional branching: `.branch([(cond, step), ...])`
    pub fn branch(
        mut self,
        branches: Vec<(Arc<dyn ConditionFn>, Arc<dyn WorkflowStepFn>)>,
    ) -> Self {
        let branch_nodes = branches
            .into_iter()
            .map(|(cond, step)| (cond, WorkflowNode::Step(step)))
            .collect();
        self.nodes.push(WorkflowNode::Branch(branch_nodes));
        self
    }

    /// Branch with sub-workflows instead of single steps
    pub fn branch_workflows(mut self, branches: Vec<(Arc<dyn ConditionFn>, WorkflowNode)>) -> Self {
        self.nodes.push(WorkflowNode::Branch(branches));
        self
    }

    /// Loop until condition is true: `.do_until(step, condition, max_iter)`
    pub fn do_until(
        mut self,
        step: Arc<dyn WorkflowStepFn>,
        condition: Arc<dyn ConditionFn>,
        max_iterations: usize,
    ) -> Self {
        self.nodes.push(WorkflowNode::DoUntil {
            body: Box::new(WorkflowNode::Step(step)),
            condition,
            max_iterations,
        });
        self
    }

    /// Loop while condition is true: `.do_while(step, condition, max_iter)`
    pub fn do_while(
        mut self,
        step: Arc<dyn WorkflowStepFn>,
        condition: Arc<dyn ConditionFn>,
        max_iterations: usize,
    ) -> Self {
        self.nodes.push(WorkflowNode::DoWhile {
            body: Box::new(WorkflowNode::Step(step)),
            condition,
            max_iterations,
        });
        self
    }

    /// Transform context: `.map(transform_fn)`
    pub fn map(mut self, transform: Arc<dyn MapFn>) -> Self {
        self.nodes.push(WorkflowNode::Map(transform));
        self
    }

    /// Suspend for human input: `.suspend(reason)`
    pub fn suspend(mut self, step_id: &str, reason: &str) -> Self {
        self.nodes.push(WorkflowNode::Suspend {
            reason: reason.to_string(),
            step_id: step_id.to_string(),
        });
        self
    }

    /// Compile the workflow into an executable node tree
    pub fn build(self) -> Workflow {
        let root = if self.nodes.len() == 1 {
            self.nodes.into_iter().next().unwrap()
        } else {
            WorkflowNode::Then(self.nodes)
        };

        Workflow { id: self.id, root }
    }
}

// ── Workflow ──────────────────────────────────────────────────────────────

/// A compiled workflow ready for execution
pub struct Workflow {
    pub id: String,
    pub root: WorkflowNode,
}

impl Workflow {
    /// Execute the workflow with the given initial context
    pub async fn execute(&self, ctx: WorkflowContext) -> StepResult {
        Self::execute_node(&self.root, ctx).await
    }

    /// Resume a suspended workflow with resume data
    pub async fn resume(
        &self,
        mut ctx: WorkflowContext,
        resume_data: HashMap<String, String>,
    ) -> StepResult {
        ctx.suspended = false;
        ctx.suspend_payload = None;
        ctx.resume_data = Some(resume_data);
        self.execute(ctx).await
    }

    fn execute_node(
        node: &WorkflowNode,
        ctx: WorkflowContext,
    ) -> Pin<Box<dyn Future<Output = StepResult> + Send + '_>> {
        Box::pin(async move {
            match node {
                WorkflowNode::Step(step) => step.execute(ctx).await,

                WorkflowNode::Then(nodes) => {
                    let mut current_ctx = ctx;
                    let mut last_step_id = String::new();

                    for node in nodes {
                        let result = Self::execute_node(node, current_ctx).await;
                        last_step_id = result.step_id().to_string();

                        match result {
                            StepResult::Success { output, .. } => {
                                current_ctx = output;
                            }
                            // Propagate failures and suspensions
                            other => return other,
                        }
                    }

                    StepResult::Success {
                        step_id: last_step_id,
                        output: current_ctx,
                    }
                }

                WorkflowNode::Parallel(nodes) => {
                    // Execute all nodes concurrently via tokio::spawn
                    let mut handles = Vec::new();
                    let ctx_arc = Arc::new(ctx.clone());

                    for node in nodes {
                        // Clone context for each parallel branch
                        let branch_ctx = (*ctx_arc).clone();
                        // We need to execute sequentially here since nodes aren't Send+'static
                        // In production, each branch would be a spawned task
                        let result = Self::execute_node(node, branch_ctx).await;

                        match &result {
                            StepResult::Failed { .. } => return result,
                            StepResult::Suspended { .. } => return result,
                            _ => {}
                        }
                        handles.push(result);
                    }

                    // Merge all successful results into context
                    let mut merged = (*ctx_arc).clone();
                    for result in handles {
                        if let StepResult::Success { step_id: _, output } = result {
                            // Merge step_results from each parallel branch
                            for (k, v) in output.step_results {
                                merged.step_results.insert(k, v);
                            }
                            for (k, v) in output.values {
                                merged.values.insert(k, v);
                            }
                        }
                    }

                    StepResult::Success {
                        step_id: "parallel".to_string(),
                        output: merged,
                    }
                }

                WorkflowNode::Branch(branches) => {
                    for (condition, node) in branches {
                        if condition.evaluate(&ctx) {
                            return Self::execute_node(node, ctx).await;
                        }
                    }

                    // No branch matched
                    StepResult::Skipped {
                        step_id: "branch".to_string(),
                        reason: "No branch condition matched".to_string(),
                    }
                }

                WorkflowNode::DoUntil {
                    body,
                    condition,
                    max_iterations,
                } => {
                    let mut current_ctx = ctx;
                    let mut iterations = 0;

                    loop {
                        current_ctx.iteration_count = iterations as u64;
                        let result = Self::execute_node(body, current_ctx).await;

                        match result {
                            StepResult::Success { output, step_id } => {
                                current_ctx = output;
                                iterations += 1;

                                if condition.evaluate(&current_ctx) || iterations >= *max_iterations
                                {
                                    return StepResult::Success {
                                        step_id,
                                        output: current_ctx,
                                    };
                                }
                            }
                            other => return other,
                        }
                    }
                }

                WorkflowNode::DoWhile {
                    body,
                    condition,
                    max_iterations,
                } => {
                    let mut current_ctx = ctx;
                    let mut iterations = 0;

                    while condition.evaluate(&current_ctx) && iterations < *max_iterations {
                        current_ctx.iteration_count = iterations as u64;
                        let result = Self::execute_node(body, current_ctx).await;

                        match result {
                            StepResult::Success { output, .. } => {
                                current_ctx = output;
                                iterations += 1;
                            }
                            other => return other,
                        }
                    }

                    StepResult::Success {
                        step_id: "do_while".to_string(),
                        output: current_ctx,
                    }
                }

                WorkflowNode::Map(transform) => {
                    let transformed = transform.transform(ctx);
                    StepResult::Success {
                        step_id: "map".to_string(),
                        output: transformed,
                    }
                }

                WorkflowNode::Suspend { reason, step_id } => {
                    let mut suspended_ctx = ctx;
                    // Check if we have resume data (being resumed)
                    if suspended_ctx.resume_data.is_some() {
                        // Already resumed, continue
                        StepResult::Success {
                            step_id: step_id.clone(),
                            output: suspended_ctx,
                        }
                    } else {
                        let payload = SuspendPayload {
                            reason: reason.clone(),
                            step_id: step_id.clone(),
                            details: HashMap::new(),
                            timestamp: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                        };
                        suspended_ctx.suspended = true;
                        suspended_ctx.suspend_payload = Some(payload.clone());

                        StepResult::Suspended {
                            step_id: step_id.clone(),
                            payload,
                            context: suspended_ctx,
                        }
                    }
                }
            }
        })
    }
}

impl fmt::Debug for Workflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Workflow").field("id", &self.id).finish()
    }
}

// ── Workflow Registry ────────────────────────────────────────────────────

/// Registry for named workflows, enabling runtime workflow selection
pub struct WorkflowRegistry {
    workflows: HashMap<String, Workflow>,
}

impl WorkflowRegistry {
    pub fn new() -> Self {
        Self {
            workflows: HashMap::new(),
        }
    }

    pub fn register(&mut self, workflow: Workflow) {
        self.workflows.insert(workflow.id.clone(), workflow);
    }

    pub fn get(&self, id: &str) -> Option<&Workflow> {
        self.workflows.get(id)
    }

    pub fn list(&self) -> Vec<&str> {
        self.workflows.keys().map(|k| k.as_str()).collect()
    }
}

impl Default for WorkflowRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Retry Configuration (Mastra error handling) ──────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub delay_ms: u64,
    pub backoff_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            delay_ms: 1000,
            backoff_factor: 2.0,
        }
    }
}
