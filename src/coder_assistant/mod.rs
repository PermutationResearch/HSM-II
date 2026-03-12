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
pub mod tools;

pub use agent_loop::AgentLoop;
pub use renderer::{DifferentialRenderer, MarkdownRenderer, RenderUpdate};
pub use schemas::{
    ToolParameter, ToolProviderKind, ToolProviderMetadata, ToolProviderRuntime, ToolRegistry,
    ToolSchema, WasmCapability,
};
pub use session::{Session, SessionEvent, SessionManager};
pub use streaming::{StreamEvent, StreamingHandler, ThinkingBlock};
pub use tools::{
    CoderTool, ExfiltrationPolicy, NetworkBoundary, SandboxMode, SecretBoundary, ToolContext,
    ToolExecutionAudit, ToolExecutionPolicy, ToolExecutor, ToolResult,
};

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
            api_url: "http://localhost:11434".to_string(),
            model: "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL"
                .to_string(),
            api_key: None,
            supports_thinking: true,
            supports_tools: true,
            max_tokens: 4096,
            temperature: 0.7,
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
