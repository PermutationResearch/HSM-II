//! Ralph Loop Council - Fresh context iteration with worker-reviewer split
//!
//! Based on the "Ralph Wiggum Technique" from Geoffrey Huntley:
//! - Each iteration starts with FRESH context (no accumulated conversation history)
//! - State persists through FILES ONLY
//! - Worker (Model A) does the work
//! - Reviewer (Model B) reviews and decides SHIP or REVISE
//! - Cross-model review catches blind spots
//!
//! ## File Structure (`.hsm/ralph/{council_id}/`)
//! ```
//! task.md              - The original task/goal
//! iteration.txt        - Current iteration number
//! work-summary.txt     - What the worker did this iteration
//! work-complete.txt    - Created when worker claims done
//! review-result.txt    - SHIP or REVISE
//! review-feedback.txt  - Feedback for next iteration
//! .ralph-complete      - Created on successful completion
//! RALPH-BLOCKED.md     - Created if worker is stuck
//! ```

use super::{CouncilDecision, CouncilId};
use crate::ollama_client::OllamaClient;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Result of a Ralph Loop iteration
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum RalphVerdict {
    /// Work is approved, task complete
    Ship,
    /// Work needs revision, continue loop
    Revise { feedback: String },
    /// Maximum iterations reached without SHIP
    MaxIterationsReached,
    /// Worker is blocked/stuck
    Blocked { reason: String },
}

/// Configuration for Ralph Council
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RalphConfig {
    /// Maximum iterations before giving up
    pub max_iterations: usize,
    /// Worker model configuration
    pub worker: AgentConfig,
    /// Reviewer model configuration (can be different from worker)
    pub reviewer: AgentConfig,
    /// Directory for state files
    pub state_dir: PathBuf,
    /// Timeout for each phase (seconds)
    pub phase_timeout_secs: u64,
    /// Whether to enable cross-model review (different models)
    pub cross_model_review: bool,
}

impl Default for RalphConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            worker: AgentConfig::default_worker(),
            reviewer: AgentConfig::default_reviewer(),
            state_dir: PathBuf::from(".hsm/ralph"),
            phase_timeout_secs: 600,
            cross_model_review: true,
        }
    }
}

/// Agent configuration for worker or reviewer
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    pub model: String,
    pub provider: String, // "ollama", "openai", "anthropic"
    pub temperature: f64,
    pub system_prompt: String,
}

impl AgentConfig {
    /// Resolve model from `OLLAMA_MODEL` env var, falling back to `qwen2.5:14b`.
    /// When set to `"auto"`, defer to auto-detection (caller should run `OllamaConfig::detect_model`).
    fn resolve_model() -> String {
        match std::env::var("OLLAMA_MODEL") {
            Ok(m) if !m.is_empty() && m != "auto" => m,
            _ => "qwen2.5:14b".to_string(),
        }
    }

    pub fn default_worker() -> Self {
        Self {
            model: Self::resolve_model(),
            provider: "ollama".to_string(),
            temperature: 0.7,
            system_prompt: WORKER_SYSTEM_PROMPT.to_string(),
        }
    }

    pub fn default_reviewer() -> Self {
        Self {
            model: Self::resolve_model(),
            provider: "ollama".to_string(),
            temperature: 0.3, // Lower temperature for consistent review
            system_prompt: REVIEWER_SYSTEM_PROMPT.to_string(),
        }
    }
}

/// State of a Ralph Loop session
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RalphState {
    pub iteration: usize,
    pub task: String,
    pub work_summary: Option<String>,
    pub work_complete: bool,
    pub review_result: Option<String>,
    pub review_feedback: Option<String>,
    pub blocked_reason: Option<String>,
}

/// The Ralph Council - implements the Ralph Loop pattern
pub struct RalphCouncil {
    council_id: CouncilId,
    config: RalphConfig,
    state_dir: PathBuf,
    _ollama: OllamaClient,
    history: Vec<RalphIteration>,
}

/// Record of a single Ralph iteration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RalphIteration {
    pub iteration: usize,
    pub work_summary: String,
    pub review_result: String,
    pub review_feedback: Option<String>,
    pub duration_ms: u64,
}

/// Result from the worker phase
#[derive(Clone, Debug)]
pub struct WorkResult {
    pub summary: String,
    pub files_modified: Vec<String>,
    pub claimed_complete: bool,
    pub raw_response: String,
}

/// Result from the reviewer phase
#[derive(Clone, Debug)]
pub struct ReviewResult {
    pub verdict: RalphVerdict,
    pub feedback: Option<String>,
    pub raw_response: String,
}

impl RalphCouncil {
    /// Create new Ralph Council
    pub fn new(council_id: CouncilId, config: RalphConfig) -> Self {
        let state_dir = config.state_dir.join(council_id.to_string());
        let _ollama = OllamaClient::new(crate::ollama_client::OllamaConfig::default());

        Self {
            council_id,
            config,
            state_dir,
            _ollama,
            history: Vec::new(),
        }
    }
    
    /// Generate text using Ollama with a specific model
    async fn generate_with_model(&self, model: &str, prompt: &str) -> anyhow::Result<String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        
        let body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
        });
        
        let response = client
            .post("http://localhost:11434/api/generate")
            .json(&body)
            .send()
            .await?;
        
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Ollama returned status {}", response.status()));
        }
        
        let json: serde_json::Value = response.json().await?;
        let text = json["response"]
            .as_str()
            .unwrap_or("")
            .to_string();
        
        Ok(text)
    }

    /// Initialize state directory and write task
    pub async fn initialize(&self, task: &str) -> anyhow::Result<()> {
        // Create state directory
        fs::create_dir_all(&self.state_dir).await?;

        // Write task file
        let task_path = self.state_dir.join("task.md");
        fs::write(&task_path, task).await?;

        // Initialize iteration counter
        let iter_path = self.state_dir.join("iteration.txt");
        fs::write(&iter_path, "1").await?;

        // Create empty state files
        for file in &["work-summary.txt", "review-feedback.txt"] {
            let path = self.state_dir.join(file);
            if !path.exists() {
                fs::write(&path, "").await?;
            }
        }

        Ok(())
    }

    /// Run the full Ralph Loop
    pub async fn execute(&mut self, task: &str) -> anyhow::Result<(RalphVerdict, CouncilDecision)> {
        // Initialize
        self.initialize(task).await?;

        let start_time = std::time::Instant::now();

        // Main Ralph Loop
        for iteration in 1..=self.config.max_iterations {
            println!("[Ralph] Starting iteration {}/{}", iteration, self.config.max_iterations);

            // Update iteration counter
            let iter_path = self.state_dir.join("iteration.txt");
            fs::write(&iter_path, iteration.to_string()).await?;

            // === WORK PHASE ===
            let work_result = self.worker_phase(iteration).await?;

            // Check if worker claims completion
            if !work_result.claimed_complete {
                // Worker didn't claim complete, continue to next iteration
                self.record_iteration(iteration, &work_result.summary, "CONTINUE", None, 0).await?;
                continue;
            }

            // === REVIEW PHASE ===
            let review_result = self.reviewer_phase(iteration).await?;

            // Record this iteration
            let feedback = match &review_result.verdict {
                RalphVerdict::Revise { feedback } => Some(feedback.clone()),
                _ => None,
            };

            self.record_iteration(
                iteration,
                &work_result.summary,
                &review_result.raw_response,
                feedback.clone(),
                0,
            )
            .await?;

            match &review_result.verdict {
                RalphVerdict::Ship => {
                    println!("[Ralph] SHIP received after {} iterations", iteration);

                    // Mark as complete
                    let complete_path = self.state_dir.join(".ralph-complete");
                    fs::write(&complete_path, format!("Completed after {} iterations\n", iteration)).await?;

                    // Write review result
                    let result_path = self.state_dir.join("review-result.txt");
                    fs::write(&result_path, "SHIP").await?;

                    let decision = self.create_ship_decision(iteration, start_time.elapsed().as_millis() as u64);
                    return Ok((RalphVerdict::Ship, decision));
                }
                RalphVerdict::Revise { feedback } => {
                    println!("[Ralph] REVISE received: {}", feedback.chars().take(100).collect::<String>());

                    // Write review feedback for next iteration
                    let feedback_path = self.state_dir.join("review-feedback.txt");
                    fs::write(&feedback_path, feedback).await?;

                    let result_path = self.state_dir.join("review-result.txt");
                    fs::write(&result_path, "REVISE").await?;

                    // Clear work-complete to force re-work
                    let complete_path = self.state_dir.join("work-complete.txt");
                    if complete_path.exists() {
                        fs::remove_file(&complete_path).await.ok();
                    }
                }
                RalphVerdict::Blocked { reason } => {
                    println!("[Ralph] BLOCKED: {}", reason);

                    let blocked_path = self.state_dir.join("RALPH-BLOCKED.md");
                    fs::write(&blocked_path, format!("# Blocked\n\n{}", reason)).await?;

                    let decision = self.create_blocked_decision(iteration, reason.clone());
                    return Ok((RalphVerdict::Blocked { reason: reason.clone() }, decision));
                }
                _ => {}
            }
        }

        // Max iterations reached
        println!("[Ralph] Max iterations ({}) reached", self.config.max_iterations);

        let decision = self.create_max_iter_decision(self.config.max_iterations);
        Ok((RalphVerdict::MaxIterationsReached, decision))
    }

    /// Worker phase - fresh context, does the work
    async fn worker_phase(&self, _iteration: usize) -> anyhow::Result<WorkResult> {
        // Build fresh prompt for worker
        let prompt = self.build_worker_prompt();

        // Call LLM with worker model
        let response = match self.generate_with_model(&self.config.worker.model, &prompt).await {
            Ok(resp) => resp,
            Err(e) => return Err(anyhow::anyhow!("Worker LLM error: {}", e)),
        };

        // Parse response for signals
        let claimed_complete = response.contains("WORK_COMPLETE")
            || response.contains("work-complete.txt")
            || self.state_dir.join("work-complete.txt").exists();

        // Extract summary
        let summary = if let Some(start) = response.find("SUMMARY:") {
            let summary_start = start + 8;
            let summary_end = response[summary_start..].find('\n').unwrap_or(response.len() - summary_start);
            response[summary_start..summary_start + summary_end].trim().to_string()
        } else {
            response.chars().take(200).collect::<String>()
        };

        // Write work summary
        let summary_path = self.state_dir.join("work-summary.txt");
        fs::write(&summary_path, &summary).await?;

        Ok(WorkResult {
            summary,
            files_modified: Vec::new(), // Would extract from response
            claimed_complete,
            raw_response: response,
        })
    }

    /// Reviewer phase - reviews the work
    async fn reviewer_phase(&self, _iteration: usize) -> anyhow::Result<ReviewResult> {
        // Build fresh prompt for reviewer
        let prompt = self.build_reviewer_prompt();

        // Call LLM with reviewer model (potentially different from worker)
        let response = match self.generate_with_model(&self.config.reviewer.model, &prompt).await {
            Ok(resp) => resp,
            Err(e) => return Err(anyhow::anyhow!("Reviewer LLM error: {}", e)),
        };

        // Parse verdict
        let verdict = if response.contains("SHIP") || response.contains("Ship") {
            RalphVerdict::Ship
        } else if response.contains("BLOCKED") || response.contains("blocked") {
            let reason = response
                .lines()
                .find(|l| l.contains("REASON:"))
                .map(|l| l.trim_start_matches("REASON:").trim().to_string())
                .unwrap_or_else(|| "Worker unable to proceed".to_string());
            RalphVerdict::Blocked { reason }
        } else {
            // Default to REVISE
            let feedback = response
                .lines()
                .find(|l| l.contains("FEEDBACK:") || l.contains("Feedback:"))
                .map(|l| {
                    l.trim_start_matches("FEEDBACK:")
                        .trim_start_matches("Feedback:")
                        .trim()
                        .to_string()
                })
                .unwrap_or_else(|| response.clone());
            RalphVerdict::Revise { feedback }
        };

        Ok(ReviewResult {
            verdict,
            feedback: None, // Extracted above
            raw_response: response,
        })
    }

    /// Build worker prompt with fresh context
    fn build_worker_prompt(&self) -> String {
        let task_path = self.state_dir.join("task.md");
        let feedback_path = self.state_dir.join("review-feedback.txt");
        let iteration_path = self.state_dir.join("iteration.txt");

        let mut prompt = WORKER_PROMPT_TEMPLATE
            .replace("{TASK_PATH}", &task_path.to_string_lossy())
            .replace("{FEEDBACK_PATH}", &feedback_path.to_string_lossy())
            .replace("{ITERATION_PATH}", &iteration_path.to_string_lossy())
            .replace("{STATE_DIR}", &self.state_dir.to_string_lossy());

        // Add actual task content
        if let Ok(task) = std::fs::read_to_string(&task_path) {
            prompt = prompt.replace("{TASK_CONTENT}", &task);
        }

        // Add feedback if exists
        if let Ok(feedback) = std::fs::read_to_string(&feedback_path) {
            if !feedback.trim().is_empty() {
                prompt.push_str("\n\n## PREVIOUS REVIEW FEEDBACK\n");
                prompt.push_str(&feedback);
                prompt.push_str("\n\nAddress this feedback in your work.");
            }
        }

        prompt
    }

    /// Build reviewer prompt with fresh context
    fn build_reviewer_prompt(&self) -> String {
        let task_path = self.state_dir.join("task.md");
        let work_summary_path = self.state_dir.join("work-summary.txt");
        let work_complete_path = self.state_dir.join("work-complete.txt");

        let mut prompt = REVIEWER_PROMPT_TEMPLATE
            .replace("{TASK_PATH}", &task_path.to_string_lossy())
            .replace("{WORK_SUMMARY_PATH}", &work_summary_path.to_string_lossy())
            .replace("{STATE_DIR}", &self.state_dir.to_string_lossy());

        // Add actual content
        if let Ok(task) = std::fs::read_to_string(&task_path) {
            prompt = prompt.replace("{TASK_CONTENT}", &task);
        }
        if let Ok(summary) = std::fs::read_to_string(&work_summary_path) {
            prompt = prompt.replace("{WORK_SUMMARY}", &summary);
        }

        // Check if worker claimed complete
        let work_complete = work_complete_path.exists();
        prompt = prompt.replace("{WORK_COMPLETE}", if work_complete { "YES" } else { "NO" });

        prompt
    }

    /// Record iteration in history
    async fn record_iteration(
        &mut self,
        iteration: usize,
        work_summary: &str,
        review_result: &str,
        review_feedback: Option<String>,
        duration_ms: u64,
    ) -> anyhow::Result<()> {
        let iter = RalphIteration {
            iteration,
            work_summary: work_summary.to_string(),
            review_result: review_result.to_string(),
            review_feedback,
            duration_ms,
        };
        self.history.push(iter);
        Ok(())
    }

    /// Create SHIP decision
    fn create_ship_decision(&self, iterations: usize, duration_ms: u64) -> CouncilDecision {
        use super::{CouncilDecisionMetadata, ExecutionPlan, ExecutionStep};
        
        CouncilDecision {
            council_id: self.council_id,
            proposal_id: format!("ralph_{}", self.council_id),
            decision: super::Decision::Approve,
            confidence: 0.95,
            participating_agents: vec![1, 2], // Worker=1, Reviewer=2
            execution_plan: Some(ExecutionPlan {
                steps: vec![ExecutionStep {
                    sequence: 1,
                    description: format!(
                        "Ralph Loop completed after {} iterations ({}ms)",
                        iterations, duration_ms
                    ),
                    assigned_agent: Some(1), // Worker=1
                    dependencies: vec![],
                }],
                estimated_duration_ms: duration_ms,
                rollback_strategy: None,
            }),
            decided_at: current_timestamp(),
            mode_used: super::CouncilMode::Ralph,
            metadata: CouncilDecisionMetadata::default(),
        }
    }

    /// Create blocked decision
    fn create_blocked_decision(&self, _iterations: usize, _reason: String) -> CouncilDecision {
        use super::CouncilDecisionMetadata;
        
        CouncilDecision {
            council_id: self.council_id,
            proposal_id: format!("ralph_{}", self.council_id),
            decision: super::Decision::Reject,
            confidence: 0.5,
            participating_agents: vec![1], // Worker=1
            execution_plan: None,
            decided_at: current_timestamp(),
            mode_used: super::CouncilMode::Ralph,
            metadata: CouncilDecisionMetadata::default(),
        }
    }

    /// Create max iterations decision
    fn create_max_iter_decision(&self, _max_iterations: usize) -> CouncilDecision {
        use super::CouncilDecisionMetadata;
        
        CouncilDecision {
            council_id: self.council_id,
            proposal_id: format!("ralph_{}", self.council_id),
            decision: super::Decision::Reject,
            confidence: 0.3,
            participating_agents: vec![1, 2], // Worker=1, Reviewer=2
            execution_plan: None,
            decided_at: current_timestamp(),
            mode_used: super::CouncilMode::Ralph,
            metadata: CouncilDecisionMetadata::default(),
        }
    }

    /// Get iteration history
    pub fn history(&self) -> &[RalphIteration] {
        &self.history
    }

    /// Get state directory
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }
}

// === PROMPTS ===

const WORKER_SYSTEM_PROMPT: &str = r#"You are the WORKER in a Ralph Loop.

CRITICAL RULES:
1. Your context is FRESH each iteration - you will NOT remember previous iterations
2. ALL state persists through FILES ONLY in {STATE_DIR}
3. Read the task from task.md FIRST
4. Check review-feedback.txt for feedback from previous iterations
5. Make meaningful progress on the task
6. When complete, create work-complete.txt with "done"
7. Write a summary of what you did to work-summary.txt

Your work persists. You are building on previous iterations even though you don't remember them.
"#;

const REVIEWER_SYSTEM_PROMPT: &str = r#"You are the REVIEWER in a Ralph Loop.

You are a DIFFERENT model than the worker. Your fresh perspective catches mistakes.

REVIEW CRITERIA:
1. Does the work actually accomplish the task?
2. Does it run without errors?
3. Is it reasonably complete?
4. Are there obvious bugs or issues?

BE STRICT but FAIR:
- Don't nitpick style if functionality is correct
- DO reject incomplete work
- DO reject code that doesn't run
- DO reject if tests fail

OUTPUT FORMAT:
- If approved: SHIP
- If needs work: REVISE\nFEEDBACK: [specific issues]
- If blocked: BLOCKED\nREASON: [why worker can't proceed]
"#;

const WORKER_PROMPT_TEMPLATE: &str = r#"## RALPH LOOP - WORK PHASE

You are in iteration of a work loop with FRESH context.

### YOUR TASK (from {TASK_PATH})
{TASK_CONTENT}

### STATE FILES (in {STATE_DIR})
- task.md = The task (READ THIS)
- iteration.txt = Current iteration number
- review-feedback.txt = Feedback from last review (check this!)
- work-complete.txt = Create when done
- work-summary.txt = Write what you did

### INSTRUCTIONS
1. Read the task: cat {TASK_PATH}
2. Check for feedback: cat {FEEDBACK_PATH}
3. List files: ls -la {STATE_DIR}
4. Make progress on the task
5. Address any feedback from previous iterations
6. When complete: echo "done" > {STATE_DIR}/work-complete.txt
7. Write summary: echo "what I did" > {STATE_DIR}/work-summary.txt

If review-feedback.txt has content, ADDRESS THAT FEEDBACK FIRST.

SUMMARY: [Write a brief summary of your work here]
"#;

const REVIEWER_PROMPT_TEMPLATE: &str = r#"## RALPH LOOP - REVIEW PHASE

You are reviewing work done by another AI agent.

### THE TASK (from {TASK_PATH})
{TASK_CONTENT}

### WORK SUMMARY (from {WORK_SUMMARY_PATH})
{WORK_SUMMARY}

### WORKER CLAIMS COMPLETE: {WORK_COMPLETE}

### REVIEW CRITERIA
1. Does the work accomplish the task?
2. Does it run without errors?
3. Is it reasonably complete?
4. Are there obvious bugs?

### YOUR DECISION
Respond with ONE of:

SHIP
- Work is complete and correct

REVISE
FEEDBACK: [Specific issues to fix]

BLOCKED
REASON: [Why the worker cannot proceed]
"#;

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ralph_verdict_serialization() {
        let verdict = RalphVerdict::Revise {
            feedback: "Fix the bug".to_string(),
        };
        let json = serde_json::to_string(&verdict).unwrap();
        assert!(json.contains("Revise"));
    }

    #[test]
    fn test_default_config() {
        let config = RalphConfig::default();
        assert_eq!(config.max_iterations, 10);
        assert!(config.cross_model_review);
    }
}
