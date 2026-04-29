//! Rust-Native Tool System for HSM-II
//!
//! Provides 60+ production-ready tools, competing with Hermes/OpenClaw:
//!
//! ## Web & Browser (9 tools)
//! - web_search - Search with multiple backends
//! - firecrawl_scrape - Firecrawl API (markdown/html; needs FIRECRAWL_API_KEY)
//! - browser_navigate, browser_wait, browser_click, browser_type, browser_screenshot
//! - browser_get_text, browser_close
//! - browser_use_run (Browser Use provider bridge)
//!
//! ## File Operations (10 tools)
//! - read_file, write_file, edit_file, file_info
//! - list_directory, search_files
//! - archive_extract, archive_create
//! - file_view (deprecated alias for read_file)
//!
//! ## Shell & System (10 tools)
//! - bash - Execute shell commands
//! - system_info, env, process_list, disk_usage
//! - grep, find (file search)
//!
//! ## Git (11 tools)
//! - git_status, git_log, git_diff, git_add, git_commit
//! - git_push, git_pull, git_branch, git_checkout, git_clone
//! - git_fetch, git_merge, git_stash, git_reset, git_remote
//!
//! ## API & Data (14 tools)
//! - http_request, webhook_send
//! - json_parse, json_validate
//! - base64, url, markdown
//! - csv_parse, csv_generate
//!
//! ## Calculations (7 tools)
//! - calculator, convert, random
//! - hash, uuid, datetime
//!
//! ## Text Processing (10 tools)
//! - text_replace, text_split, text_join, text_case
//! - text_truncate, word_count, text_diff
//! - regex_extract, template
//!
//! All tools integrate with:
//! - CASS for skill learning
//! - Memory for experience recording
//! - Council for complex decisions

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod bash_policy;
pub mod connector_runtime;
pub mod file_tools;
pub mod harness_gate;
pub mod integrated_executor;
pub mod registry;
pub mod scored_tool_router;
pub mod secret_scanner;
pub mod security;
pub mod subprocess_env;
pub mod shell_tools;
pub mod tool_permissions;
pub use harness_gate::HarnessPolicyGate;
pub mod bundle;
pub use bundle::ToolBundle;
pub mod firecrawl_tool;
pub mod web_ingest;
pub mod web_search;

// New comprehensive tool modules
pub mod api_tools;
pub mod browser_tools;
pub mod browser_use_tool;
pub mod calculation_tools;
pub mod email_tools;
pub mod git_tools;
pub mod system_tools;
pub mod text_tools;

pub use file_tools::{EditTool, ReadTool, WriteTool};
pub use firecrawl_tool::FirecrawlScrapeTool;
pub use integrated_executor::IntegratedToolExecutor;
pub use registry::ToolRegistry;
pub use scored_tool_router::{
    pick_tool_for_prompt, rank_tools_for_prompt, route_prompt_execute, ScoredRouteConfig,
    ScoredRouteError, ScoredRouteFailReason, ScoredTool,
};
pub use shell_tools::{BashTool, FindTool, GrepTool};
pub use tool_permissions::ToolPermissionContext;
pub use web_search::WebSearchTool;

// Browser tools
pub use browser_tools::{
    BrowserClickTool, BrowserCloseTool, BrowserGetTextTool, BrowserNavigateTool,
    BrowserScreenshotTool, BrowserTypeTool, BrowserWaitTool,
};
pub use browser_use_tool::BrowserUseRunTool;

// Git tools
pub use git_tools::{
    GitAddTool, GitBranchTool, GitCheckoutTool, GitCloneTool, GitCommitTool, GitDiffTool,
    GitLogTool, GitPullTool, GitPushTool, GitStatusTool,
};

// API tools
pub use api_tools::{
    Base64Tool, CsvGenerateTool, CsvParseTool, HttpRequestTool, JsonParseTool, JsonValidateTool,
    MarkdownTool, UrlTool, WebhookSendTool,
};

// Calculation tools
pub use calculation_tools::{
    CalculatorTool, DateTimeTool, HashTool, RandomTool, UnitConversionTool, UuidTool,
};

// System tools
pub use system_tools::{
    ArchiveCreateTool, ArchiveExtractTool, DiskUsageTool, EnvironmentTool, FileInfoTool,
    ListDirectoryTool, ProcessListTool, ReadFileEnhancedTool, SearchFilesTool, SystemInfoTool,
};

// Text tools
pub use text_tools::{
    RegexExtractTool, TemplateTool, TextCaseTool, TextDiffTool, TextJoinTool, TextReplaceTool,
    TextSplitTool, TextTruncateTool, WordCountTool,
};

// Feature flag tools
pub mod flags_tools;
pub use flags_tools::{
    get_flag_tools, CheckFlagTool, CreateFlagTool, EmergencyRollbackTool, FlagStatsTool,
    UpdateRolloutTool,
};

// RLM tools
pub mod rlm_tool;
pub use rlm_tool::{RlmProcessTool, RlmTrajectoryTool};

/// HTTP MCP tools on the personal agent (plugin manifests + optional `tools/list`).
pub mod mcp_bridge;

/// Enterprise ops YAML: `read_operations`, `list_tickets` (personal agent home).
pub mod ops_tools;
pub use ops_tools::register_personal_ops_tools;

/// Company OS human inbox + memory pool HTTP tools.
pub mod company_os_tools;
pub use company_os_tools::{
    CompanyAgentRunFeedbackTool, CompanyCreateTaskTool, CompanyListTasksTool,
    CompanyMemoryAppendTool, CompanyMemorySearchTool, CompanyPromoteFeedbackToTaskTool,
    CompanyTaskRequiresHumanTool, CompanyToolCallTool, CompanyToolDescribeTool,
    CompanyToolDiscoverTool, CompanyUpdateTaskTool,
};

/// On-disk SKILL.md: `skills_list`, `skill_md_read` (shared catalog from personal agent).
pub mod skill_md_tools;
pub use skill_md_tools::register_skill_md_tools;

// MiroFish-inspired prediction tool
pub mod prediction_tool;
pub use email_tools::{MaildirListTool, MaildirReadTool, ReadEmlTool};
pub use prediction_tool::PredictionTool;

/// Tool trait - all tools implement this
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (used by LLM to call it)
    fn name(&self) -> &str;

    /// Tool description (shown to LLM)
    fn description(&self) -> &str;

    /// JSON schema for tool parameters
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with given parameters
    async fn execute(&self, params: Value) -> ToolOutput;
}

/// Output from a tool execution
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolOutput {
    pub success: bool,
    pub result: String,
    pub error: Option<String>,
    pub metadata: Option<Value>,
}

impl ToolOutput {
    pub fn success(result: impl Into<String>) -> Self {
        Self {
            success: true,
            result: result.into(),
            error: None,
            metadata: None,
        }
    }

    pub fn error(error: impl Into<String>) -> Self {
        Self {
            success: false,
            result: String::new(),
            error: Some(error.into()),
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// A tool call from the LLM
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub parameters: Value,
    pub call_id: String,
    /// Optional long-horizon harness envelope (gateway / lead–subagent contract).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_run: Option<crate::harness::HarnessRunEnvelope>,
    /// Optional idempotency key for dedupe / audit (gap 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

impl Default for ToolCall {
    fn default() -> Self {
        Self {
            name: String::new(),
            parameters: Value::Null,
            call_id: String::new(),
            harness_run: None,
            idempotency_key: None,
        }
    }
}

/// Result of a tool execution with call info
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub call: ToolCall,
    pub output: ToolOutput,
    pub duration_ms: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Tool execution context passed to all tools
#[derive(Clone, Debug)]
pub struct ToolContext {
    pub working_dir: std::path::PathBuf,
    pub agent_name: String,
    pub coherence: f64,
    pub session_id: String,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            agent_name: "HSM-II".to_string(),
            coherence: 1.0,
            session_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// Helper to create JSON schema for a tool
pub fn object_schema(properties: Vec<(&str, &str, bool)>) -> Value {
    let mut props = serde_json::Map::new();
    let mut required = Vec::new();

    for (name, description, is_required) in properties {
        props.insert(
            name.to_string(),
            serde_json::json!({
                "type": "string",
                "description": description
            }),
        );
        if is_required {
            required.push(name.to_string());
        }
    }

    serde_json::json!({
        "type": "object",
        "properties": props,
        "required": required
    })
}

/// Register all 60+ tools in a registry
pub fn register_all_tools(registry: &mut ToolRegistry) {
    use std::sync::Arc;

    // Core tools
    registry.register(Arc::new(WebSearchTool::new()));
    registry.register(Arc::new(FirecrawlScrapeTool::new()));
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(EditTool));
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(GrepTool));
    registry.register(Arc::new(FindTool));

    // Browser tools
    registry.register(Arc::new(BrowserNavigateTool::new()));
    registry.register(Arc::new(BrowserWaitTool::new()));
    registry.register(Arc::new(BrowserClickTool::new()));
    registry.register(Arc::new(BrowserTypeTool::new()));
    registry.register(Arc::new(BrowserScreenshotTool::new()));
    registry.register(Arc::new(BrowserGetTextTool::new()));
    registry.register(Arc::new(BrowserCloseTool::new()));
    registry.register(Arc::new(BrowserUseRunTool::new()));

    // Git tools
    registry.register(Arc::new(GitStatusTool::new()));
    registry.register(Arc::new(GitLogTool::new()));
    registry.register(Arc::new(GitDiffTool::new()));
    registry.register(Arc::new(GitAddTool::new()));
    registry.register(Arc::new(GitCommitTool::new()));
    registry.register(Arc::new(GitPushTool::new()));
    registry.register(Arc::new(GitPullTool::new()));
    registry.register(Arc::new(GitBranchTool::new()));
    registry.register(Arc::new(GitCheckoutTool::new()));
    registry.register(Arc::new(GitCloneTool::new()));

    // API tools
    registry.register(Arc::new(HttpRequestTool::new()));
    registry.register(Arc::new(CompanyTaskRequiresHumanTool::new()));
    registry.register(Arc::new(CompanyToolDiscoverTool::new()));
    registry.register(Arc::new(CompanyToolDescribeTool::new()));
    registry.register(Arc::new(CompanyToolCallTool::new()));
    registry.register(Arc::new(CompanyMemorySearchTool::new()));
    registry.register(Arc::new(CompanyMemoryAppendTool::new()));
    registry.register(Arc::new(CompanyAgentRunFeedbackTool::new()));
    registry.register(Arc::new(CompanyPromoteFeedbackToTaskTool::new()));
    registry.register(Arc::new(CompanyCreateTaskTool::new()));
    registry.register(Arc::new(CompanyUpdateTaskTool::new()));
    registry.register(Arc::new(CompanyListTasksTool::new()));
    registry.register(Arc::new(WebhookSendTool::new()));
    registry.register(Arc::new(JsonParseTool::new()));
    registry.register(Arc::new(JsonValidateTool::new()));
    registry.register(Arc::new(Base64Tool::new()));
    registry.register(Arc::new(UrlTool::new()));
    registry.register(Arc::new(MarkdownTool::new()));
    registry.register(Arc::new(CsvParseTool::new()));
    registry.register(Arc::new(CsvGenerateTool::new()));

    // Calculation tools
    registry.register(Arc::new(CalculatorTool::new()));
    registry.register(Arc::new(UnitConversionTool::new()));
    registry.register(Arc::new(RandomTool::new()));
    registry.register(Arc::new(HashTool::new()));
    registry.register(Arc::new(UuidTool::new()));
    registry.register(Arc::new(DateTimeTool::new()));

    // System tools
    registry.register(Arc::new(SystemInfoTool::new()));
    registry.register(Arc::new(EnvironmentTool::new()));
    registry.register(Arc::new(ProcessListTool::new()));
    registry.register(Arc::new(DiskUsageTool::new()));
    registry.register(Arc::new(FileInfoTool::new()));
    registry.register(Arc::new(ListDirectoryTool::new()));
    registry.register(Arc::new(ReadFileEnhancedTool::new()));
    registry.register(Arc::new(SearchFilesTool::new()));
    registry.register(Arc::new(ArchiveExtractTool::new()));
    registry.register(Arc::new(ArchiveCreateTool::new()));

    // Text tools
    registry.register(Arc::new(TextReplaceTool::new()));
    registry.register(Arc::new(TextSplitTool::new()));
    registry.register(Arc::new(TextJoinTool::new()));
    registry.register(Arc::new(TextCaseTool::new()));
    registry.register(Arc::new(TextTruncateTool::new()));
    registry.register(Arc::new(WordCountTool::new()));
    registry.register(Arc::new(TextDiffTool::new()));
    registry.register(Arc::new(RegexExtractTool::new()));
    registry.register(Arc::new(TemplateTool::new()));

    // RLM tools
    registry.register(Arc::new(RlmProcessTool::new()));
    registry.register(Arc::new(RlmTrajectoryTool::new()));

    // MiroFish-inspired prediction tool
    registry.register(Arc::new(PredictionTool::new()));

    // Email: .eml + Maildir (attachments / paperclip inventory)
    registry.register(Arc::new(ReadEmlTool::new()));
    registry.register(Arc::new(MaildirListTool::new()));
    registry.register(Arc::new(MaildirReadTool::new()));
}
