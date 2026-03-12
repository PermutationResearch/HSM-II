//! Agent Core - pi-agent-core style architecture
//!
//! Provides:
//! - AgentLoop: Core orchestration for tool execution and LLM interaction
//! - Agent: High-level wrapper with state management, queuing, and events
//! - Event-driven architecture for reactive UIs

pub mod agent;
pub mod attachments;
pub mod events;
pub mod loop_core;
pub mod queue;
pub mod transport;

pub use agent::{Agent, AgentBuilder};
pub use attachments::{Attachment, AttachmentType};
pub use events::{AgentEvent, EventBus, EventHandler};
pub use loop_core::{AgentError, AgentLoop};
pub use queue::{MessageQueue, QueueMode};
pub use transport::{
    DirectTransport, ProxyTransport, Transport, TransportError, TransportResponse,
};

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Agent state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentState {
    pub conversation: Vec<Message>,
    pub pending_tool_calls: Vec<ToolCall>,
    pub is_processing: bool,
    pub turn_count: u32,
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

impl AgentState {
    pub fn new() -> Self {
        Self {
            conversation: Vec::new(),
            pending_tool_calls: Vec::new(),
            is_processing: false,
            turn_count: 0,
            metadata: std::collections::HashMap::new(),
        }
    }

    pub fn add_message(&mut self, msg: Message) {
        self.conversation.push(msg);
    }

    pub fn last_message(&self) -> Option<&Message> {
        self.conversation.last()
    }

    pub fn clear(&mut self) {
        let system_msgs: Vec<Message> = self
            .conversation
            .iter()
            .filter(|m| matches!(m.role, Role::System))
            .cloned()
            .collect();
        self.conversation = system_msgs;
        self.pending_tool_calls.clear();
        self.turn_count = 0;
    }
}

impl Default for AgentState {
    fn default() -> Self {
        Self::new()
    }
}

/// Message in conversation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub attachments: Vec<Attachment>,
    pub timestamp: u64,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            attachments: Vec::new(),
            timestamp: now(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            attachments: Vec::new(),
            timestamp: now(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            attachments: Vec::new(),
            timestamp: now(),
        }
    }

    pub fn tool(content: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            attachments: Vec::new(),
            timestamp: now(),
        }
    }

    pub fn with_attachments(mut self, attachments: Vec<Attachment>) -> Self {
        self.attachments = attachments;
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Tool call from model
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Tool definition
#[derive(Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub handler: Arc<dyn ToolHandler>,
}

impl std::fmt::Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tool")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("parameters", &self.parameters)
            .field("handler", &"<ToolHandler>")
            .finish()
    }
}

/// Tool handler trait
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync + std::fmt::Debug {
    async fn execute(&self, args: &serde_json::Value) -> Result<String, ToolError>;
}

/// Tool error
#[derive(Debug, Clone)]
pub enum ToolError {
    ExecutionError(String),
    InvalidArguments(String),
    NotFound(String),
    Timeout,
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::ExecutionError(msg) => write!(f, "Execution error: {}", msg),
            ToolError::InvalidArguments(msg) => write!(f, "Invalid arguments: {}", msg),
            ToolError::NotFound(name) => write!(f, "Tool not found: {}", name),
            ToolError::Timeout => write!(f, "Tool execution timed out"),
        }
    }
}

impl std::error::Error for ToolError {}

/// Model configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub temperature: f32,
    pub max_tokens: u32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: "ollama".to_string(),
            model: "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL"
                .to_string(),
            api_url: "http://localhost:11434".to_string(),
            api_key: None,
            temperature: 0.7,
            max_tokens: 4096,
        }
    }
}

/// Get current timestamp
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Shared agent state
pub type SharedState = Arc<RwLock<AgentState>>;

/// Event sender type
pub type EventSender = mpsc::UnboundedSender<AgentEvent>;

/// Message queue callback
pub type QueueCallback = Box<dyn Fn() -> Vec<Message> + Send + Sync>;
