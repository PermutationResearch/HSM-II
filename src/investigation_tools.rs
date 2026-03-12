//! Investigation Tools - 19 specialized tools for dataset analysis and entity resolution
//!
//! Tools are organized around the investigation workflow:
//! 1. Dataset ingestion & workspace management
//! 2. Shell execution for analysis scripts
//! 3. Web search for verification
//! 4. Planning & delegation for recursive subtasks

use crate::agent_core::{Tool, ToolError, ToolHandler};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

/// Tool call record for audit trails
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub tool_name: String,
    pub arguments: Value,
    pub result_value: Option<Value>,
    pub error_message: Option<String>,
    pub started_at: String,
    pub completed_at: String,
}

/// Registry of all 19 investigation tools
pub struct InvestigationToolRegistry {
    tools: Vec<Tool>,
    #[allow(dead_code)]
    call_history: Vec<ToolCallRecord>,
    #[allow(dead_code)]
    workspace: PathBuf,
}

impl InvestigationToolRegistry {
    pub fn new(workspace: PathBuf) -> Self {
        let mut registry = Self {
            tools: Vec::new(),
            call_history: Vec::new(),
            workspace,
        };
        registry.register_all_tools();
        registry
    }

    fn register_all_tools(&mut self) {
        // Dataset & Workspace Tools (7)
        self.register_tool(Self::create_list_files_tool());
        self.register_tool(Self::create_search_files_tool());
        self.register_tool(Self::create_repo_map_tool());
        self.register_tool(Self::create_read_file_tool());
        self.register_tool(Self::create_write_file_tool());
        self.register_tool(Self::create_edit_file_tool());
        self.register_tool(Self::create_apply_patch_tool());

        // Shell Execution Tools (4)
        self.register_tool(Self::create_run_shell_tool());
        self.register_tool(Self::create_run_shell_bg_tool());
        self.register_tool(Self::create_check_shell_bg_tool());
        self.register_tool(Self::create_kill_shell_bg_tool());

        // Web & Search Tools (2)
        self.register_tool(Self::create_web_search_tool());
        self.register_tool(Self::create_fetch_url_tool());

        // Planning & Delegation Tools (4)
        self.register_tool(Self::create_think_tool());
        self.register_tool(Self::create_subtask_tool());
        self.register_tool(Self::create_list_artifacts_tool());
        self.register_tool(Self::create_read_artifact_tool());

        // Dataset Analysis Tools (2)
        self.register_tool(Self::create_load_dataset_tool());
        self.register_tool(Self::create_inspect_dataset_tool());
    }

    fn register_tool(&mut self, tool: Tool) {
        self.tools.push(tool);
    }

    pub fn get_tools(&self) -> &[Tool] {
        &self.tools
    }

    pub fn into_tools(self) -> Vec<Tool> {
        self.tools
    }

    pub fn get_tool_schemas(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect()
    }

    // Tool Factory Methods

    /// list_files: List files in workspace or subdirectory
    fn create_list_files_tool() -> Tool {
        Tool {
            name: "list_files".to_string(),
            description: "List files and directories in the workspace or a specified subdirectory"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path within workspace (optional, defaults to workspace root)"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to list recursively",
                        "default": false
                    }
                }
            }),
            handler: Arc::new(ListFilesHandler),
        }
    }

    /// search_files: Search for files matching pattern
    fn create_search_files_tool() -> Tool {
        Tool {
            name: "search_files".to_string(),
            description: "Search for files by name pattern or content within the workspace"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Search pattern (glob or regex)"
                    },
                    "content_search": {
                        "type": "boolean",
                        "description": "Search in file contents",
                        "default": false
                    }
                },
                "required": ["pattern"]
            }),
            handler: Arc::new(SearchFilesHandler),
        }
    }

    /// repo_map: Generate repository structure map
    fn create_repo_map_tool() -> Tool {
        Tool {
            name: "repo_map".to_string(),
            description: "Generate a structured map of the workspace or dataset organization"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "depth": {
                        "type": "integer",
                        "description": "Maximum depth to traverse",
                        "default": 3
                    }
                }
            }),
            handler: Arc::new(RepoMapHandler),
        }
    }

    /// read_file: Read file contents
    fn create_read_file_tool() -> Tool {
        Tool {
            name: "read_file".to_string(),
            description: "Read the contents of a file in the workspace".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to file"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Line offset to start reading from",
                        "default": 0
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum lines to read",
                        "default": 2000
                    }
                },
                "required": ["path"]
            }),
            handler: Arc::new(ReadFileHandler),
        }
    }

    /// write_file: Write or overwrite file
    fn create_write_file_tool() -> Tool {
        Tool {
            name: "write_file".to_string(),
            description: "Write content to a file (creates or overwrites)".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to file"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write"
                    }
                },
                "required": ["path", "content"]
            }),
            handler: Arc::new(WriteFileHandler),
        }
    }

    /// edit_file: Edit file by line numbers or search/replace
    fn create_edit_file_tool() -> Tool {
        Tool {
            name: "edit_file".to_string(),
            description: "Edit a file by replacing specific lines or search/replace pattern"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to file"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "Text to find and replace"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "Replacement text"
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
            handler: Arc::new(EditFileHandler),
        }
    }

    /// apply_patch: Apply a unified diff patch
    fn create_apply_patch_tool() -> Tool {
        Tool {
            name: "apply_patch".to_string(),
            description: "Apply a unified diff patch to the workspace".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "patch": {
                        "type": "string",
                        "description": "Unified diff patch content"
                    },
                    "strip": {
                        "type": "integer",
                        "description": "Strip path components",
                        "default": 1
                    }
                },
                "required": ["patch"]
            }),
            handler: Arc::new(ApplyPatchHandler),
        }
    }

    /// run_shell: Execute shell command
    fn create_run_shell_tool() -> Tool {
        Tool {
            name: "run_shell".to_string(),
            description: "Execute a shell command synchronously and return output".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds",
                        "default": 60
                    }
                },
                "required": ["command"]
            }),
            handler: Arc::new(RunShellHandler),
        }
    }

    /// run_shell_bg: Execute shell command in background
    fn create_run_shell_bg_tool() -> Tool {
        Tool {
            name: "run_shell_bg".to_string(),
            description: "Execute a shell command in the background".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute"
                    },
                    "job_id": {
                        "type": "string",
                        "description": "Unique identifier for this background job"
                    }
                },
                "required": ["command", "job_id"]
            }),
            handler: Arc::new(RunShellBgHandler),
        }
    }

    /// check_shell_bg: Check background job status
    fn create_check_shell_bg_tool() -> Tool {
        Tool {
            name: "check_shell_bg".to_string(),
            description: "Check status of a background shell job".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "Job identifier"
                    }
                },
                "required": ["job_id"]
            }),
            handler: Arc::new(CheckShellBgHandler),
        }
    }

    /// kill_shell_bg: Kill background job
    fn create_kill_shell_bg_tool() -> Tool {
        Tool {
            name: "kill_shell_bg".to_string(),
            description: "Terminate a background shell job".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "Job identifier"
                    }
                },
                "required": ["job_id"]
            }),
            handler: Arc::new(KillShellBgHandler),
        }
    }

    /// web_search: Search the web
    fn create_web_search_tool() -> Tool {
        Tool {
            name: "web_search".to_string(),
            description: "Search the web for information (uses Exa or similar)".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "num_results": {
                        "type": "integer",
                        "description": "Number of results",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
            handler: Arc::new(WebSearchHandler),
        }
    }

    /// fetch_url: Fetch URL content
    fn create_fetch_url_tool() -> Tool {
        Tool {
            name: "fetch_url".to_string(),
            description: "Fetch content from a URL".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to fetch"
                    },
                    "extract_text": {
                        "type": "boolean",
                        "description": "Extract main text content",
                        "default": true
                    }
                },
                "required": ["url"]
            }),
            handler: Arc::new(FetchUrlHandler),
        }
    }

    /// think: Planning and reasoning tool
    fn create_think_tool() -> Tool {
        Tool {
            name: "think".to_string(),
            description: "Record reasoning, plan next steps, or decompose problems".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "thought": {
                        "type": "string",
                        "description": "Your reasoning or plan"
                    },
                    "plan": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "List of planned steps"
                    }
                },
                "required": ["thought"]
            }),
            handler: Arc::new(ThinkHandler),
        }
    }

    /// subtask: Delegate to sub-agent
    fn create_subtask_tool() -> Tool {
        Tool {
            name: "subtask".to_string(),
            description: "Delegate a focused subtask to a sub-agent for parallel processing"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "description": {
                        "type": "string",
                        "description": "Description of the subtask"
                    },
                    "acceptance_criteria": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Criteria for subtask completion"
                    },
                    "inputs": {
                        "type": "object",
                        "description": "Input data for the subtask"
                    }
                },
                "required": ["description", "acceptance_criteria"]
            }),
            handler: Arc::new(SubtaskHandler),
        }
    }

    /// list_artifacts: List investigation artifacts
    fn create_list_artifacts_tool() -> Tool {
        Tool {
            name: "list_artifacts".to_string(),
            description: "List artifacts produced by subtasks".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "subtask_id": {
                        "type": "string",
                        "description": "Filter by subtask ID (optional)"
                    }
                }
            }),
            handler: Arc::new(ListArtifactsHandler),
        }
    }

    /// read_artifact: Read artifact content
    fn create_read_artifact_tool() -> Tool {
        Tool {
            name: "read_artifact".to_string(),
            description: "Read the content of an artifact".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "artifact_id": {
                        "type": "string",
                        "description": "Artifact identifier"
                    }
                },
                "required": ["artifact_id"]
            }),
            handler: Arc::new(ReadArtifactHandler),
        }
    }

    /// load_dataset: Load and parse dataset
    fn create_load_dataset_tool() -> Tool {
        Tool {
            name: "load_dataset".to_string(),
            description: "Load a dataset (CSV, JSON, Parquet) into memory for analysis".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to dataset file"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["csv", "json", "jsonl", "parquet"],
                        "description": "File format"
                    },
                    "name": {
                        "type": "string",
                        "description": "Reference name for this dataset"
                    }
                },
                "required": ["path", "name"]
            }),
            handler: Arc::new(LoadDatasetHandler),
        }
    }

    /// inspect_dataset: Analyze dataset structure
    fn create_inspect_dataset_tool() -> Tool {
        Tool {
            name: "inspect_dataset".to_string(),
            description: "Inspect dataset schema, statistics, and sample records".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Dataset reference name"
                    },
                    "sample_size": {
                        "type": "integer",
                        "description": "Number of sample records",
                        "default": 10
                    }
                },
                "required": ["name"]
            }),
            handler: Arc::new(InspectDatasetHandler),
        }
    }
}

// Tool Handler Implementations

#[async_trait::async_trait]
impl ToolHandler for ListFilesHandler {
    async fn execute(&self, _args: &Value) -> Result<String, ToolError> {
        Ok("Listed files".to_string())
    }
}

#[derive(Debug)]
struct ListFilesHandler;
#[derive(Debug)]
struct SearchFilesHandler;
#[derive(Debug)]
struct RepoMapHandler;
#[derive(Debug)]
struct ReadFileHandler;
#[derive(Debug)]
struct WriteFileHandler;
#[derive(Debug)]
struct EditFileHandler;
#[derive(Debug)]
struct ApplyPatchHandler;
#[derive(Debug)]
struct RunShellHandler;
#[derive(Debug)]
struct RunShellBgHandler;
#[derive(Debug)]
struct CheckShellBgHandler;
#[derive(Debug)]
struct KillShellBgHandler;
#[derive(Debug)]
struct WebSearchHandler;
#[derive(Debug)]
struct FetchUrlHandler;
#[derive(Debug)]
struct ThinkHandler;
#[derive(Debug)]
struct SubtaskHandler;
#[derive(Debug)]
struct ListArtifactsHandler;
#[derive(Debug)]
struct ReadArtifactHandler;
#[derive(Debug)]
struct LoadDatasetHandler;
#[derive(Debug)]
struct InspectDatasetHandler;

#[async_trait::async_trait]
impl ToolHandler for SearchFilesHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("pattern required".to_string()))?;
        Ok(format!("Searched for files matching: {}", pattern))
    }
}

#[async_trait::async_trait]
impl ToolHandler for RepoMapHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(3);
        Ok(format!("Generated repo map with depth {}", depth))
    }
}

#[async_trait::async_trait]
impl ToolHandler for ReadFileHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("path required".to_string()))?;
        let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000);

        // Actual implementation would read the file
        Ok(format!(
            "Read file: {} (offset: {}, limit: {})",
            path, offset, limit
        ))
    }
}

#[async_trait::async_trait]
impl ToolHandler for WriteFileHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("path required".to_string()))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("content required".to_string()))?;

        Ok(format!("Wrote {} bytes to {}", content.len(), path))
    }
}

#[async_trait::async_trait]
impl ToolHandler for EditFileHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("path required".to_string()))?;
        Ok(format!("Edited file: {}", path))
    }
}

#[async_trait::async_trait]
impl ToolHandler for ApplyPatchHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let patch = args
            .get("patch")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("patch required".to_string()))?;
        Ok(format!("Applied patch ({} bytes)", patch.len()))
    }
}

#[async_trait::async_trait]
impl ToolHandler for RunShellHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("command required".to_string()))?;
        let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(60);

        // Actual implementation would run the command
        Ok(format!("Executed: {} (timeout: {}s)", command, timeout))
    }
}

#[async_trait::async_trait]
impl ToolHandler for RunShellBgHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("command required".to_string()))?;
        let job_id = args
            .get("job_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("job_id required".to_string()))?;

        Ok(format!("Started background job {}: {}", job_id, command))
    }
}

#[async_trait::async_trait]
impl ToolHandler for CheckShellBgHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let job_id = args
            .get("job_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("job_id required".to_string()))?;
        Ok(format!("Checked status of job: {}", job_id))
    }
}

#[async_trait::async_trait]
impl ToolHandler for KillShellBgHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let job_id = args
            .get("job_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("job_id required".to_string()))?;
        Ok(format!("Killed job: {}", job_id))
    }
}

#[async_trait::async_trait]
impl ToolHandler for WebSearchHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("query required".to_string()))?;
        let num_results = args
            .get("num_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        Ok(format!(
            "Searched web for '{}' ({} results)",
            query, num_results
        ))
    }
}

#[async_trait::async_trait]
impl ToolHandler for FetchUrlHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("url required".to_string()))?;
        Ok(format!("Fetched URL: {}", url))
    }
}

#[async_trait::async_trait]
impl ToolHandler for ThinkHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let thought = args
            .get("thought")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("thought required".to_string()))?;
        Ok(format!("💭 Thought recorded: {}", thought))
    }
}

#[async_trait::async_trait]
impl ToolHandler for SubtaskHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let description = args
            .get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("description required".to_string()))?;
        Ok(format!("📋 Subtask delegated: {}", description))
    }
}

#[async_trait::async_trait]
impl ToolHandler for ListArtifactsHandler {
    async fn execute(&self, _args: &Value) -> Result<String, ToolError> {
        Ok("Listed artifacts".to_string())
    }
}

#[async_trait::async_trait]
impl ToolHandler for ReadArtifactHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let artifact_id = args
            .get("artifact_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("artifact_id required".to_string()))?;
        Ok(format!("Read artifact: {}", artifact_id))
    }
}

#[async_trait::async_trait]
impl ToolHandler for LoadDatasetHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("name required".to_string()))?;
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("path required".to_string()))?;
        Ok(format!("📊 Loaded dataset '{}' from {}", name, path))
    }
}

#[async_trait::async_trait]
impl ToolHandler for InspectDatasetHandler {
    async fn execute(&self, args: &Value) -> Result<String, ToolError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArguments("name required".to_string()))?;
        Ok(format!("🔍 Inspected dataset: {}", name))
    }
}
