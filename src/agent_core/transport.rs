//! Transport abstraction - Direct or Proxy

use super::{Message, ModelConfig};
use async_trait::async_trait;
use serde_json::json;

fn normalize_tool_name(raw: &str) -> Option<String> {
    let lowered = raw.trim().to_ascii_lowercase();
    let normalized = match lowered.as_str() {
        "read" => "read",
        "write" => "write",
        "edit" => "edit",
        "bash" => "bash",
        "grep" => "grep",
        "find" | "glob" => "find",
        "ls" => "ls",
        other if !other.is_empty() => other,
        _ => return None,
    };
    Some(normalized.to_string())
}

fn parse_tool_calls_from_content(content: &str) -> Vec<super::ToolCall> {
    let mut out = Vec::new();
    let mut next_id: u64 = 1;

    fn parse_node(node: &serde_json::Value, out: &mut Vec<(String, serde_json::Value)>) {
        if let Some(arr) = node.as_array() {
            for item in arr {
                parse_node(item, out);
            }
            return;
        }
        if let Some(obj) = node.as_object() {
            if let Some(inner) = obj.get("tools") {
                parse_node(inner, out);
            }
            let raw_name = obj
                .get("name")
                .or_else(|| obj.get("tool"))
                .and_then(|v| v.as_str());
            if let Some(name) = raw_name.and_then(normalize_tool_name) {
                let args = obj
                    .get("args")
                    .or_else(|| obj.get("parameters"))
                    .or_else(|| obj.get("input"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                out.push((name, args));
            }
        }
    }

    let mut parsed = Vec::new();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(content.trim()) {
        parse_node(&value, &mut parsed);
    }
    if let Ok(re) = regex::Regex::new(r"(?is)```json\s*(.*?)```") {
        for cap in re.captures_iter(content) {
            if let Some(block) = cap.get(1).map(|m| m.as_str()) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(block.trim()) {
                    parse_node(&value, &mut parsed);
                }
            }
        }
    }

    for (name, arguments) in parsed {
        out.push(super::ToolCall {
            id: format!("tool_{}", next_id),
            name,
            arguments,
        });
        next_id += 1;
    }
    out
}

/// Transport trait for LLM communication
#[async_trait]
pub trait Transport: Send + Sync {
    async fn complete(
        &self,
        messages: &[Message],
        config: &ModelConfig,
    ) -> Result<TransportResponse, TransportError>;
}

/// Transport response
#[derive(Clone, Debug)]
pub struct TransportResponse {
    pub content: String,
    pub tool_calls: Option<Vec<super::ToolCall>>,
    pub usage: Usage,
}

#[derive(Clone, Debug)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Transport error
#[derive(Debug, Clone)]
pub enum TransportError {
    ConnectionError(String),
    ModelError(String),
    Timeout,
    SerializationError(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            TransportError::ModelError(msg) => write!(f, "Model error: {}", msg),
            TransportError::Timeout => write!(f, "Request timed out"),
            TransportError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for TransportError {}

/// Direct transport - connects directly to LLM API
pub struct DirectTransport;

impl DirectTransport {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Transport for DirectTransport {
    async fn complete(
        &self,
        messages: &[Message],
        config: &ModelConfig,
    ) -> Result<TransportResponse, TransportError> {
        use ollama_rs::generation::chat::request::ChatMessageRequest;
        use ollama_rs::{
            generation::chat::{ChatMessage, MessageRole},
            Ollama,
        };

        let ollama = Ollama::new(config.api_url.clone(), 11434);

        let chat_messages: Vec<ChatMessage> = messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    super::Role::System => MessageRole::System,
                    super::Role::User => MessageRole::User,
                    super::Role::Assistant => MessageRole::Assistant,
                    super::Role::Tool => MessageRole::Tool,
                };
                ChatMessage::new(role, m.content.clone())
            })
            .collect();

        let request = ChatMessageRequest::new(config.model.clone(), chat_messages);

        match ollama.send_chat_messages(request).await {
            Ok(response) => {
                let tool_calls = parse_tool_calls_from_content(&response.message.content);
                Ok(TransportResponse {
                    content: response.message.content,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    usage: Usage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                })
            }
            Err(e) => Err(TransportError::ModelError(format!("{:?}", e))),
        }
    }
}

impl Default for DirectTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// Proxy transport - connects through a proxy server
#[allow(dead_code)]
pub struct ProxyTransport {
    proxy_url: String,
}

impl ProxyTransport {
    pub fn new(proxy_url: impl Into<String>) -> Self {
        Self {
            proxy_url: proxy_url.into(),
        }
    }
}

#[async_trait]
impl Transport for ProxyTransport {
    async fn complete(
        &self,
        messages: &[Message],
        config: &ModelConfig,
    ) -> Result<TransportResponse, TransportError> {
        let client = reqwest::Client::new();
        let req_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    super::Role::System => "system",
                    super::Role::User => "user",
                    super::Role::Assistant => "assistant",
                    super::Role::Tool => "tool",
                };
                json!({
                    "role": role,
                    "content": m.content,
                })
            })
            .collect();

        let body = json!({
            "model": config.model,
            "messages": req_messages,
            "temperature": config.temperature,
            "max_tokens": config.max_tokens,
            "stream": false
        });

        let mut req = client.post(&self.proxy_url).json(&body);
        if let Some(api_key) = &config.api_key {
            req = req.bearer_auth(api_key);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| TransportError::ConnectionError(e.to_string()))?;
        let status = resp.status();
        let value: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| TransportError::SerializationError(e.to_string()))?;

        if !status.is_success() {
            let msg = value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("proxy request failed");
            return Err(TransportError::ModelError(format!("{} ({})", msg, status)));
        }

        let content = value
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string();

        let usage = Usage {
            prompt_tokens: value
                .get("usage")
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: value
                .get("usage")
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: value
                .get("usage")
                .and_then(|u| u.get("total_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        };

        let tool_calls = parse_tool_calls_from_content(&content);
        Ok(TransportResponse {
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            usage,
        })
    }
}
