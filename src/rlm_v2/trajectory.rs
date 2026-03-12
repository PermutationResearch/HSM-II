//! Trajectory logging and replay for RLM
//!
//! Every RLM execution is saved as a trajectory for:
//! - Debugging and inspection
//! - Performance analysis
//! - Skill learning (CASS integration)
//! - Replay and reproducibility

use super::{RlmAction, RlmStats};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A complete RLM execution trajectory
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trajectory {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub query: String,
    pub context_source: String,
    pub context_bytes: usize,
    pub iterations: Vec<IterationSnapshot>,
    pub final_answer: Option<String>,
    pub stats: RlmStats,
    pub metadata: TrajectoryMetadata,
}

/// Metadata for a trajectory
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TrajectoryMetadata {
    pub model: String,
    pub max_iterations: usize,
    pub max_sub_queries: usize,
    pub success: bool,
    pub error: Option<String>,
    pub tags: Vec<String>,
    pub user_feedback: Option<UserFeedback>,
}

/// User feedback on a trajectory
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserFeedback {
    pub rating: i32, // -1, 0, 1 or 1-5 scale
    pub comment: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Snapshot of a single RLM iteration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IterationSnapshot {
    pub iteration: usize,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub action: RlmAction,
    /// Results from tool executions
    pub tool_results: Vec<ToolResultSnapshot>,
    /// Results from sub-queries
    pub subquery_results: Vec<SubQueryResultSnapshot>,
    /// Context state at this iteration
    pub context_summary: ContextSummary,
    /// LLM prompt (truncated)
    pub prompt_preview: String,
    /// LLM response (truncated)
    pub response_preview: String,
    /// Duration of this iteration
    pub duration_ms: u64,
}

/// Tool execution result snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResultSnapshot {
    pub tool_name: String,
    pub success: bool,
    pub output_preview: String,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Sub-query result snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubQueryResultSnapshot {
    pub query_id: String,
    pub chunk_index: usize,
    pub success: bool,
    pub result_preview: String,
    pub duration_ms: u64,
}

/// Summary of context state
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextSummary {
    pub chunks_processed: Vec<usize>,
    pub accumulated_findings: Vec<String>,
    pub working_memory_size: usize,
}

impl Trajectory {
    /// Create a new trajectory
    pub fn new(query: impl Into<String>, context_source: impl Into<String>, context_bytes: usize) -> Self {
        Self {
            id: format!("traj_{}", uuid::Uuid::new_v4().to_string()[..16].to_string()),
            timestamp: chrono::Utc::now(),
            query: query.into(),
            context_source: context_source.into(),
            context_bytes,
            iterations: Vec::new(),
            final_answer: None,
            stats: RlmStats::default(),
            metadata: TrajectoryMetadata::default(),
        }
    }
    
    /// Add an iteration snapshot
    pub fn add_iteration(&mut self, iteration: IterationSnapshot) {
        self.iterations.push(iteration);
        self.stats.total_iterations = self.iterations.len();
    }
    
    /// Set final answer
    pub fn set_final_answer(&mut self, answer: impl Into<String>) {
        self.final_answer = Some(answer.into());
        self.metadata.success = true;
    }
    
    /// Mark as failed
    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.metadata.success = false;
        self.metadata.error = Some(error.into());
    }
    
    /// Update stats
    pub fn update_stats(&mut self, stats: RlmStats) {
        self.stats = stats;
    }
    
    /// Get total duration across all iterations
    pub fn total_duration_ms(&self) -> u64 {
        self.iterations.iter().map(|i| i.duration_ms).sum()
    }
    
    /// Get the final iteration count
    pub fn iteration_count(&self) -> usize {
        self.iterations.len()
    }
    
    /// Summarize for display
    pub fn summarize(&self) -> String {
        let status = if self.metadata.success { "✓" } else { "✗" };
        let iterations = self.iterations.len();
        let duration_sec = self.total_duration_ms() as f64 / 1000.0;
        let subqueries: usize = self.iterations.iter()
            .map(|i| i.subquery_results.len())
            .sum();
        let tools: usize = self.iterations.iter()
            .map(|i| i.tool_results.len())
            .sum();
        
        format!(
            "{} {} | {} iterations | {:.1}s | {} sub-queries | {} tool calls | {} bytes context",
            status,
            self.id,
            iterations,
            duration_sec,
            subqueries,
            tools,
            self.context_bytes
        )
    }
}

/// Storage for trajectories
pub struct TrajectoryStore {
    base_path: std::path::PathBuf,
    trajectories: HashMap<String, Trajectory>,
}

impl TrajectoryStore {
    /// Create new trajectory store
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
            trajectories: HashMap::new(),
        }
    }
    
    /// Initialize storage directory
    pub async fn initialize(&self) -> std::io::Result<()> {
        tokio::fs::create_dir_all(&self.base_path).await?;
        Ok(())
    }
    
    /// Save a trajectory
    pub async fn save(&mut self, trajectory: &Trajectory) -> Result<(), super::RlmError> {
        // Store in memory
        self.trajectories.insert(trajectory.id.clone(), trajectory.clone());
        
        // Persist to disk
        let path = self.base_path.join(format!("{}.json", trajectory.id));
        let json = serde_json::to_string_pretty(trajectory)
            .map_err(|e| super::RlmError::StorageError(e.to_string()))?;
        
        tokio::fs::write(&path, json)
            .await
            .map_err(|e| super::RlmError::StorageError(format!("Failed to write trajectory: {}", e)))?;
        
        Ok(())
    }
    
    /// Load a trajectory by ID
    pub async fn load(&self, id: &str) -> Result<Option<Trajectory>, super::RlmError> {
        // Check memory first
        if let Some(traj) = self.trajectories.get(id) {
            return Ok(Some(traj.clone()));
        }
        
        // Load from disk
        let path = self.base_path.join(format!("{}.json", id));
        if !path.exists() {
            return Ok(None);
        }
        
        let json = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| super::RlmError::StorageError(format!("Failed to read trajectory: {}", e)))?;
        
        let trajectory: Trajectory = serde_json::from_str(&json)
            .map_err(|e| super::RlmError::StorageError(format!("Failed to parse trajectory: {}", e)))?;
        
        Ok(Some(trajectory))
    }
    
    /// List all trajectories (from disk)
    pub async fn list(&self) -> Result<Vec<TrajectorySummary>, super::RlmError> {
        let mut entries = tokio::fs::read_dir(&self.base_path)
            .await
            .map_err(|e| super::RlmError::StorageError(format!("Failed to read directory: {}", e)))?;
        
        let mut summaries = Vec::new();
        
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "json") {
                if let Ok(json) = tokio::fs::read_to_string(&path).await {
                    if let Ok(traj) = serde_json::from_str::<Trajectory>(&json) {
                        summaries.push(TrajectorySummary::from(&traj));
                    }
                }
            }
        }
        
        // Sort by timestamp descending
        summaries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(summaries)
    }
    
    /// Get recent trajectories
    pub async fn recent(&self, limit: usize) -> Result<Vec<TrajectorySummary>, super::RlmError> {
        let mut all = self.list().await?;
        all.truncate(limit);
        Ok(all)
    }
    
    /// Delete a trajectory
    pub async fn delete(&mut self, id: &str) -> Result<bool, super::RlmError> {
        self.trajectories.remove(id);
        
        let path = self.base_path.join(format!("{}.json", id));
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| super::RlmError::StorageError(format!("Failed to delete: {}", e)))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Summary of a trajectory for listing
#[derive(Clone, Debug)]
pub struct TrajectorySummary {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub query_preview: String,
    pub iterations: usize,
    pub success: bool,
    pub duration_ms: u64,
}

impl From<&Trajectory> for TrajectorySummary {
    fn from(traj: &Trajectory) -> Self {
        Self {
            id: traj.id.clone(),
            timestamp: traj.timestamp,
            query_preview: traj.query.chars().take(50).collect::<String>() + "...",
            iterations: traj.iterations.len(),
            success: traj.metadata.success,
            duration_ms: traj.total_duration_ms(),
        }
    }
}

/// Trajectory viewer for displaying and inspecting trajectories
pub struct TrajectoryViewer;

impl TrajectoryViewer {

    /// Display a trajectory in a human-readable format
    pub fn display_trajectory(traj: &Trajectory) -> String {
        let mut output = format!("═══ RLM Trajectory: {} ═══\n\n", traj.id);
        output.push_str(&format!("Query: {}\n", traj.query));
        output.push_str(&format!("Context: {} ({} bytes)\n", traj.context_source, traj.context_bytes));
        output.push_str(&format!("Started: {}\n", traj.timestamp.format("%Y-%m-%d %H:%M:%S")));
        output.push_str(&format!("Status: {}\n", if traj.metadata.success { "✓ Success" } else { "✗ Failed" }));
        output.push_str(&format!("\nIterations: {}\n", traj.iterations.len()));
        
        for (i, iter) in traj.iterations.iter().enumerate() {
            output.push_str(&format!("\n─── Iteration {} ({:.1}s) ───\n", i + 1, iter.duration_ms as f64 / 1000.0));
            output.push_str(&format!("Action: {:?}\n", iter.action));
            
            if !iter.tool_results.is_empty() {
                output.push_str(&format!("  Tools: {} executed\n", iter.tool_results.len()));
            }
            if !iter.subquery_results.is_empty() {
                output.push_str(&format!("  Sub-queries: {} completed\n", iter.subquery_results.len()));
            }
        }
        
        if let Some(answer) = &traj.final_answer {
            output.push_str(&format!("\n═══ Final Answer ═══\n{}\n", answer));
        }
        
        if let Some(error) = &traj.metadata.error {
            output.push_str(&format!("\n═══ Error ═══\n{}\n", error));
        }
        
        output
    }
}
