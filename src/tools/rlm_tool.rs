//! RLM Tool - Recursive Language Model processing for long contexts
//!
//! This tool allows agents to use the RLM (Recursive Language Model) pattern
//! to process large documents, codebases, or data through chunking and
//! parallel sub-query execution.

use crate::rlm_v2::{run_rlm, Context as RlmContext, RlmConfig};
use crate::tools::{Tool, ToolOutput};
use async_trait::async_trait;
use serde_json::json;
use serde_json::Value;

/// Tool for RLM-based document/code analysis
pub struct RlmProcessTool;

impl RlmProcessTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for RlmProcessTool {
    fn name(&self) -> &str {
        "rlm_process"
    }

    fn description(&self) -> &str {
        r#"Process large documents or codebases using the Recursive Language Model (RLM) pattern.

This tool is designed for:
- Summarizing long documents
- Analyzing large codebases
- Extracting information from multiple files
- Answering questions about extensive context

Unlike direct LLM calls, RLM:
1. Chunks the context intelligently
2. Dispatches parallel sub-queries
3. Aggregates results iteratively
4. Produces more accurate answers for long contexts

Use this when the context exceeds ~4000 tokens or when dealing with multiple files."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Source to analyze: file path, directory path, glob pattern, or URL"
                },
                "query": {
                    "type": "string",
                    "description": "The question or task to perform on the source content"
                },
                "source_type": {
                    "type": "string",
                    "enum": ["file", "directory", "glob", "url", "text"],
                    "description": "Type of source (default: auto-detect)"
                },
                "max_iterations": {
                    "type": "integer",
                    "description": "Maximum RLM iterations (default: 20)",
                    "minimum": 1,
                    "maximum": 100
                },
                "max_sub_queries": {
                    "type": "integer",
                    "description": "Maximum parallel sub-queries per iteration (default: 50)",
                    "minimum": 1,
                    "maximum": 500
                }
            },
            "required": ["source", "query"]
        })
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let source = match params.get("source").and_then(|s| s.as_str()) {
            Some(s) => s,
            None => return ToolOutput::error("Missing required parameter: source"),
        };

        let query = match params.get("query").and_then(|q| q.as_str()) {
            Some(q) => q,
            None => return ToolOutput::error("Missing required parameter: query"),
        };

        let source_type = params
            .get("source_type")
            .and_then(|s| s.as_str())
            .unwrap_or("auto");
        let max_iterations = params
            .get("max_iterations")
            .and_then(|m| m.as_u64())
            .unwrap_or(20) as usize;
        let max_sub_queries = params
            .get("max_sub_queries")
            .and_then(|m| m.as_u64())
            .unwrap_or(50) as usize;

        // Load context based on source type
        let context = match self.load_context(source, source_type).await {
            Ok(ctx) => ctx,
            Err(e) => return ToolOutput::error(format!("Failed to load context: {}", e)),
        };

        // Configure RLM
        let config = RlmConfig {
            max_iterations,
            max_sub_queries,
            ..Default::default()
        };

        // Run RLM
        match run_rlm(query, &context, Some(config)).await {
            Ok(answer) => {
                let result = format!("## RLM Analysis Result\n\n{}", answer.answer);

                let metadata = json!({
                    "confidence": answer.confidence,
                    "reasoning_steps": answer.reasoning.len(),
                    "source": source,
                    "context_bytes": context.metadata.total_bytes,
                    "chunks": context.chunks.len(),
                });

                ToolOutput::success(result).with_metadata(metadata)
            }
            Err(e) => ToolOutput::error(format!("RLM execution failed: {}", e)),
        }
    }
}

impl RlmProcessTool {
    async fn load_context(&self, source: &str, source_type: &str) -> anyhow::Result<RlmContext> {
        // Auto-detect source type if not specified
        let source_type = if source_type == "auto" {
            if source.starts_with("http://") || source.starts_with("https://") {
                "url"
            } else if source.contains('*') || source.contains('?') {
                "glob"
            } else if std::path::Path::new(source).is_dir() {
                "directory"
            } else {
                "file"
            }
        } else {
            source_type
        };

        match source_type {
            "file" => RlmContext::from_file(source)
                .await
                .map_err(|e| anyhow::anyhow!(e)),
            "directory" => RlmContext::from_directory(source)
                .await
                .map_err(|e| anyhow::anyhow!(e)),
            "glob" => RlmContext::from_glob(source)
                .await
                .map_err(|e| anyhow::anyhow!(e)),
            "url" => RlmContext::from_url(source)
                .await
                .map_err(|e| anyhow::anyhow!(e)),
            "text" => Ok(RlmContext::from_text(source, "direct_text")),
            _ => Err(anyhow::anyhow!("Unknown source type: {}", source_type)),
        }
    }
}

/// Tool for viewing RLM trajectories
pub struct RlmTrajectoryTool;

impl RlmTrajectoryTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for RlmTrajectoryTool {
    fn name(&self) -> &str {
        "rlm_trajectory"
    }

    fn description(&self) -> &str {
        r#"View or list RLM execution trajectories.

Trajectories record the full execution history of RLM runs, including:
- All iterations and actions taken
- Tool calls and sub-queries
- LLM prompts and responses
- Timing and statistics

Use this to debug RLM behavior or analyze performance."#
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "view", "recent"],
                    "description": "Action to perform"
                },
                "trajectory_id": {
                    "type": "string",
                    "description": "ID of trajectory to view (required for 'view' action)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Number of trajectories to list (for 'list' or 'recent')",
                    "default": 10
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        use crate::rlm_v2::TrajectoryStore;

        let action = match params.get("action").and_then(|a| a.as_str()) {
            Some(a) => a,
            None => return ToolOutput::error("Missing required parameter: action"),
        };

        let store = TrajectoryStore::new("./rlm_trajectories");
        if let Err(e) = store.initialize().await {
            return ToolOutput::error(format!("Failed to init trajectory store: {}", e));
        }

        match action {
            "list" => {
                let limit = params.get("limit").and_then(|l| l.as_u64()).unwrap_or(10) as usize;
                match store.list().await {
                    Ok(trajs) => {
                        let limited: Vec<_> = trajs.into_iter().take(limit).collect();
                        let output = limited
                            .iter()
                            .map(|t| {
                                format!(
                                    "{} | {} | {} iters | {} | {:.1}s",
                                    t.id,
                                    t.timestamp.format("%Y-%m-%d %H:%M"),
                                    t.iterations,
                                    if t.success { "✓" } else { "✗" },
                                    t.duration_ms as f64 / 1000.0
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        ToolOutput::success(output)
                    }
                    Err(e) => ToolOutput::error(format!("Failed to list trajectories: {}", e)),
                }
            }
            "recent" => {
                let limit = params.get("limit").and_then(|l| l.as_u64()).unwrap_or(5) as usize;
                match store.recent(limit).await {
                    Ok(trajs) => {
                        let output = trajs
                            .iter()
                            .map(|t| {
                                format!(
                                    "{} | {} | {} iters | {}",
                                    t.id,
                                    t.timestamp.format("%Y-%m-%d %H:%M"),
                                    t.iterations,
                                    t.query_preview
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        ToolOutput::success(output)
                    }
                    Err(e) => ToolOutput::error(format!("Failed to get recent: {}", e)),
                }
            }
            "view" => {
                let id = match params.get("trajectory_id").and_then(|i| i.as_str()) {
                    Some(i) => i,
                    None => return ToolOutput::error("Missing trajectory_id for 'view' action"),
                };

                match store.load(id).await {
                    Ok(Some(traj)) => {
                        let output = crate::rlm_v2::TrajectoryViewer::display_trajectory(&traj);
                        ToolOutput::success(output)
                    }
                    Ok(None) => ToolOutput::error(format!("Trajectory '{}' not found", id)),
                    Err(e) => ToolOutput::error(format!("Failed to load trajectory: {}", e)),
                }
            }
            _ => ToolOutput::error(format!("Unknown action: {}", action)),
        }
    }
}
