//! Pi-AI Compatible API
//!
//! Provides a JavaScript-style API for the Rust Ollama integration:
//! - getModel(provider, model) -> Model
//! - complete(model, context, options) -> Response
//! - Context for conversation management
//! - Cross-model handoffs with thinking preservation

use ollama_rs::{
    generation::chat::request::ChatMessageRequest,
    generation::chat::{ChatMessage, MessageRole as OllamaRole},
    Ollama,
};
use serde::{Deserialize, Serialize};

/// Model configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub provider: String,
    pub name: String,
    pub api_url: String,
    pub supports_thinking: bool,
    pub max_tokens: u32,
    pub temperature: f32,
}

impl Model {
    /// Create Ollama model config (our default)
    pub fn ollama(name: impl Into<String>) -> Self {
        Self {
            provider: "ollama".to_string(),
            name: name.into(),
            api_url: crate::config::network::DEFAULT_OLLAMA_URL.to_string(),
            supports_thinking: true,
            max_tokens: 4096,
            temperature: 0.7,
        }
    }

    /// Create with DeepSeek-R1 abliterated default (respects OLLAMA_MODEL env var)
    pub fn deepseek_abliterated() -> Self {
        Self::ollama(crate::ollama_client::resolve_model_from_env(
            "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL",
        ))
    }
}

/// Context for conversation management
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Context {
    pub messages: Vec<Message>,
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add system message
    pub fn with_system(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message {
            role: Role::System,
            content: content.into(),
            thinking: None,
        });
        self
    }

    /// Add user message
    pub fn user(&mut self, content: impl Into<String>) -> &mut Self {
        self.messages.push(Message {
            role: Role::User,
            content: content.into(),
            thinking: None,
        });
        self
    }

    /// Add assistant message (from previous completion)
    pub fn assistant(&mut self, response: &Response) -> &mut Self {
        self.messages.push(Message {
            role: Role::Assistant,
            content: response.content.clone(),
            thinking: response.thinking.clone(),
        });
        self
    }

    /// Get last message
    pub fn last(&self) -> Option<&Message> {
        self.messages.last()
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Clear conversation (keep system message)
    pub fn clear(&mut self) {
        let system = self
            .messages
            .iter()
            .find(|m| m.role == Role::System)
            .cloned();
        self.messages.clear();
        if let Some(sys) = system {
            self.messages.push(sys);
        }
    }

    /// Get conversation as formatted string (for debugging)
    pub fn format_conversation(&self) -> String {
        self.messages
            .iter()
            .map(|m| format!("[{:?}] {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

/// Message role
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Message in conversation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

/// Completion response
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    pub usage: Usage,
    pub finish_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Completion options
#[derive(Clone, Debug, Default)]
pub struct CompleteOptions {
    pub thinking_enabled: bool,
    pub stream: bool,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub tools: Option<Vec<ToolDef>>,
}

/// Tool definition
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl CompleteOptions {
    pub fn with_thinking(mut self) -> Self {
        self.thinking_enabled = true;
        self
    }

    pub fn with_streaming(mut self) -> Self {
        self.stream = true;
        self
    }

    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDef>) -> Self {
        self.tools = Some(tools);
        self
    }
}

/// Get model by provider and name
///
/// # Examples
/// ```
/// use hyper_stigmergy::pi_ai_compat::{getModel, Model};
///
/// let deepseek = getModel("ollama", "hf.co/tensorblock/DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q3_K_M");
/// let qwen = getModel("ollama", "qwen2.5:7b");
/// ```
#[allow(non_snake_case)]
pub fn getModel(provider: impl AsRef<str>, name: impl Into<String>) -> Model {
    let provider = provider.as_ref();
    let name = name.into();

    match provider {
        "ollama" => Model::ollama(name),
        _ => Model {
            provider: provider.to_string(),
            name,
            api_url: crate::config::network::DEFAULT_OLLAMA_URL.to_string(),
            supports_thinking: true,
            max_tokens: 4096,
            temperature: 0.7,
        },
    }
}

/// Complete a conversation with a model
///
/// # Examples
/// ```
/// use hyper_stigmergy::pi_ai_compat::{getModel, complete, Context, CompleteOptions};
///
/// async fn example() {
///     let model = getModel("ollama", "hf.co/tensorblock/DeepSeek-R1-Distill-Llama-8B-abliterated-GGUF:Q3_K_M");
///     let mut ctx = Context::new();
///     ctx.user("What is 25 * 18?");
///     
///     let response = complete(&model, &ctx, CompleteOptions::default()).await.unwrap();
///     println!("{}", response.content);
/// }
/// ```
pub async fn complete(
    model: &Model,
    context: &Context,
    options: CompleteOptions,
) -> Result<Response, PiAiError> {
    let ollama = Ollama::new(model.api_url.clone(), 11434);

    // Build messages for Ollama
    let messages: Vec<ChatMessage> = context
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::System => OllamaRole::System,
                Role::User => OllamaRole::User,
                Role::Assistant => OllamaRole::Assistant,
                Role::Tool => OllamaRole::Tool,
            };

            // Include thinking as context for other models
            let content = if let Some(thinking) = &m.thinking {
                format!("{}\n\n<thinking>{}</thinking>", m.content, thinking)
            } else {
                m.content.clone()
            };

            ChatMessage::new(role, content)
        })
        .collect();

    let request = ChatMessageRequest::new(model.name.clone(), messages);

    // Non-streaming completion
    match ollama.send_chat_messages(request).await {
        Ok(response) => {
            let content = response.message.content;

            // Parse thinking blocks
            let (content, thinking) = if model.supports_thinking && options.thinking_enabled {
                parse_thinking(&content)
            } else {
                (content, None)
            };

            Ok(Response {
                content,
                thinking,
                usage: Usage {
                    prompt_tokens: 0, // Ollama doesn't return these
                    completion_tokens: 0,
                    total_tokens: 0,
                },
                finish_reason: Some("stop".to_string()),
            })
        }
        Err(e) => Err(PiAiError::ModelError(e.to_string())),
    }
}

/// Parse thinking blocks from content
fn parse_thinking(content: &str) -> (String, Option<String>) {
    if !content.contains("<think>") {
        return (content.to_string(), None);
    }

    let mut main_content = String::new();
    let mut thinking = String::new();
    let mut in_thinking = false;

    for line in content.lines() {
        if line.contains("<think>") {
            in_thinking = true;
            let after = line.split("<think>").nth(1).unwrap_or("");
            if !after.is_empty() {
                thinking.push_str(after);
                thinking.push('\n');
            }
        } else if line.contains("</think>") {
            in_thinking = false;
            let before = line.split("</think>").next().unwrap_or("");
            thinking.push_str(before);

            let after = line.split("</think>").nth(1).unwrap_or("");
            if !after.is_empty() {
                main_content.push_str(after);
                main_content.push('\n');
            }
        } else if in_thinking {
            thinking.push_str(line);
            thinking.push('\n');
        } else {
            main_content.push_str(line);
            main_content.push('\n');
        }
    }

    let thinking = if thinking.is_empty() {
        None
    } else {
        Some(thinking)
    };
    (main_content.trim().to_string(), thinking)
}

/// Streaming completion
pub async fn complete_streaming<F>(
    model: &Model,
    context: &Context,
    options: CompleteOptions,
    mut on_token: F,
) -> Result<Response, PiAiError>
where
    F: FnMut(&str, bool), // (token, is_thinking)
{
    let ollama = Ollama::new(model.api_url.clone(), 11434);

    let messages: Vec<ChatMessage> = context
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::System => OllamaRole::System,
                Role::User => OllamaRole::User,
                Role::Assistant => OllamaRole::Assistant,
                Role::Tool => OllamaRole::Tool,
            };

            let content = if let Some(thinking) = &m.thinking {
                format!("{}\n\n<thinking>{}</thinking>", m.content, thinking)
            } else {
                m.content.clone()
            };

            ChatMessage::new(role, content)
        })
        .collect();

    let request = ChatMessageRequest::new(model.name.clone(), messages);

    use tokio_stream::StreamExt;

    let mut full_content = String::new();
    let mut in_thinking = false;

    match ollama.send_chat_messages_stream(request).await {
        Ok(mut stream) => {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(chunk) => {
                        let token = chunk.message.content;

                        // Track thinking state
                        if token.contains("<think>") {
                            in_thinking = true;
                            let after = token.split("<think>").nth(1).unwrap_or("");
                            if !after.is_empty() {
                                on_token(after, true);
                                full_content.push_str(after);
                            }
                        } else if token.contains("</think>") {
                            in_thinking = false;
                            let before = token.split("</think>").next().unwrap_or("");
                            if !before.is_empty() {
                                on_token(before, true);
                                full_content.push_str(before);
                            }
                            let after = token.split("</think>").nth(1).unwrap_or("");
                            if !after.is_empty() {
                                on_token(after, false);
                                full_content.push_str(after);
                            }
                        } else {
                            on_token(&token, in_thinking);
                            full_content.push_str(&token);
                        }
                    }
                    Err(_) => break,
                }
            }

            let (content, thinking) = if model.supports_thinking && options.thinking_enabled {
                parse_thinking(&full_content)
            } else {
                (full_content, None)
            };

            Ok(Response {
                content,
                thinking,
                usage: Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
                finish_reason: Some("stop".to_string()),
            })
        }
        Err(e) => Err(PiAiError::ModelError(e.to_string())),
    }
}

/// Error types
#[derive(Debug, Clone)]
pub enum PiAiError {
    ModelError(String),
    SerializationError(String),
    ContextError(String),
}

impl std::fmt::Display for PiAiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PiAiError::ModelError(msg) => write!(f, "Model error: {}", msg),
            PiAiError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            PiAiError::ContextError(msg) => write!(f, "Context error: {}", msg),
        }
    }
}

impl std::error::Error for PiAiError {}

impl From<serde_json::Error> for PiAiError {
    fn from(e: serde_json::Error) -> Self {
        PiAiError::SerializationError(e.to_string())
    }
}

/// Convenience re-exports
pub mod prelude {
    pub use super::{
        complete, complete_streaming, getModel, CompleteOptions, Context, Message, Model,
        PiAiError, Response, Role, ToolDef, Usage,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_thinking() {
        let input = "Hello\n<think>\nThis is thinking\n</think>\nWorld";
        let (content, thinking) = parse_thinking(input);

        assert_eq!(content, "Hello\nWorld");
        assert_eq!(thinking, Some("This is thinking\n".to_string()));
    }

    #[test]
    fn test_context_serialization() {
        let mut ctx = Context::new();
        ctx.user("Hello");

        let json = ctx.to_json().unwrap();
        let restored = Context::from_json(&json).unwrap();

        assert_eq!(restored.messages.len(), 1);
    }
}
