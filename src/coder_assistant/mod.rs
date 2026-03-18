//! Advanced Coder Assistant Module
//!
//! Features:
//! - Streaming responses with thinking/reasoning support
//! - TypeBox schema-based tool definitions
//! - Agent loop for tool execution and validation
//! - Cross-provider context handoffs
//! - Session management
//! - Differential rendering support

pub mod agent_loop;
pub mod renderer;
pub mod schemas;
pub mod session;
pub mod streaming;

// Tool system — split from the original monolithic tools.rs
pub mod builtin_tools;
pub mod external_providers;
pub mod sandbox;
pub mod security_policy;
pub mod tool_executor;
pub mod tools;

pub use agent_loop::AgentLoop;
pub use renderer::{DifferentialRenderer, MarkdownRenderer, RenderUpdate};
pub use schemas::{
    ToolParameter, ToolProviderKind, ToolProviderMetadata, ToolProviderRuntime, ToolRegistry,
    ToolSchema, WasmCapability,
};
pub use session::{Session, SessionEvent, SessionManager};
pub use streaming::{StreamEvent, StreamingHandler, ThinkingBlock};
pub use tool_executor::{CoderTool, ToolContext, ToolError as CoderToolError, ToolExecutor, ToolResult};
pub use security_policy::{
    AuditEntry, ExfiltrationPolicy, NetworkBoundary, SandboxMode, SecretBoundary,
    ToolExecutionAudit,
};
pub use tools::ToolExecutionPolicy;

use serde::{Deserialize, Serialize};

/// Provider configuration for cross-provider support
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub api_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub supports_thinking: bool,
    pub supports_tools: bool,
    pub max_tokens: u32,
    pub temperature: f32,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            name: "ollama".to_string(),
            api_url: crate::config::network::DEFAULT_OLLAMA_URL.to_string(),
            model: crate::ollama_client::resolve_model_from_env(
                crate::config::models::DEFAULT_SCORER_MODEL,
            ),
            api_key: None,
            supports_thinking: true,
            supports_tools: true,
            max_tokens: crate::config::algorithm::DEFAULT_MAX_TOKENS as u32,
            temperature: crate::config::thresholds::DEFAULT_TEMPERATURE as f32,
        }
    }
}

/// Message in the conversation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_results: Option<Vec<ToolResult>>,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Tool call from the model
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Tool execution result
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    pub tool_call_id: String,
    pub name: String,
    pub output: String,
    pub success: bool,
    pub execution_time_ms: u64,
}

/// Get current timestamp
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
