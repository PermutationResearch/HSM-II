//! Agent Loop for Tool Execution and Validation
//!
//! Handles the core agent loop: receive message → stream response →
//! detect tool calls → execute tools → continue conversation

use super::*;
use crate::pi_ai_compat;
use regex::Regex;
use schemas::ToolRegistry;
use streaming::StreamEvent;
use tokio::sync::mpsc;
use tools::ToolExecutor;

/// Agent loop configuration
#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub max_iterations: u32,
    pub tool_timeout_ms: u64,
    pub enable_streaming: bool,
    pub max_tool_calls_per_message: u32,
    pub model: String,
    pub api_url: String,
    pub supports_thinking: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            tool_timeout_ms: 60000,
            enable_streaming: true,
            max_tool_calls_per_message: 5,
            model: "hf.co/bartowski/Qwen2.5-Coder-7B-Instruct-GGUF:Q4_K_M".to_string(),
            api_url: "http://localhost:11434".to_string(),
            supports_thinking: true,
        }
    }
}

/// Agent loop state
pub struct AgentLoop {
    config: AgentConfig,
    tool_registry: ToolRegistry,
    tool_executor: ToolExecutor,
    messages: Vec<Message>,
    session_id: String,
}

/// Event emitted by the agent loop
#[derive(Clone, Debug)]
pub enum AgentEvent {
    /// Streaming content
    Stream(StreamEvent),
    /// Tool execution started
    ToolStart {
        name: String,
        args: serde_json::Value,
    },
    /// Tool execution completed
    ToolComplete {
        name: String,
        result: ToolExecutionResult,
    },
    /// Tool execution failed
    ToolError { name: String, error: String },
    /// Message added to conversation
    MessageAdded(Message),
    /// Loop iteration complete
    IterationComplete { iteration: u32 },
    /// Agent loop finished
    Complete,
    /// Error occurred
    Error(String),
}

impl AgentLoop {
    fn normalize_builtin_tool_name(raw: &str) -> Option<&'static str> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "read" => Some("read"),
            "write" => Some("write"),
            "edit" => Some("edit"),
            "bash" => Some("bash"),
            "grep" => Some("grep"),
            "find" | "glob" => Some("find"),
            "ls" => Some("ls"),
            _ => None,
        }
    }

    fn normalize_tool_name(&self, raw: &str) -> Option<String> {
        if let Some(name) = Self::normalize_builtin_tool_name(raw) {
            return Some(name.to_string());
        }

        let raw = raw.trim();
        self.tool_registry
            .get(raw)
            .map(|schema| schema.name.clone())
    }

    fn adapt_args_for_schema(tool_name: &str, args: serde_json::Value) -> serde_json::Value {
        let mut args = args;
        if let Some(obj) = args.as_object_mut() {
            // Claude-style schema aliases
            if let Some(v) = obj.remove("file_path") {
                obj.insert("path".to_string(), v);
            }
            if tool_name == "edit" {
                if let Some(v) = obj.remove("old_string") {
                    obj.insert("oldText".to_string(), v);
                }
                if let Some(v) = obj.remove("new_string") {
                    obj.insert("newText".to_string(), v);
                }
            }
        }
        args
    }

    fn parse_tool_calls(&self, response: &str) -> Vec<ToolCall> {
        let mut calls: Vec<ToolCall> = Vec::new();
        let mut next_id: u64 = 1;
        let mut push_call = |name: String, args: serde_json::Value| {
            calls.push(ToolCall {
                id: format!("call_{}", next_id),
                name,
                arguments: args,
            });
            next_id += 1;
        };

        // XML / tag style calls
        let patterns = [
            ("read", r"(?is)<read>\s*<path>(.*?)</path>\s*</read>"),
            (
                "write",
                r"(?is)<write>\s*<path>(.*?)</path>\s*<content>(.*?)</content>\s*</write>",
            ),
            (
                "edit",
                r"(?is)<edit>\s*<path>(.*?)</path>\s*<oldText>(.*?)</oldText>\s*<newText>(.*?)</newText>\s*</edit>",
            ),
            ("bash", r"(?is)<bash>\s*<command>(.*?)</command>\s*</bash>"),
            (
                "grep",
                r"(?is)<grep>\s*<pattern>(.*?)</pattern>\s*(?:<path>(.*?)</path>)?\s*</grep>",
            ),
            ("find", r"(?is)<find>\s*<pattern>(.*?)</pattern>\s*</find>"),
            ("find", r"(?is)<glob>\s*<pattern>(.*?)</pattern>\s*</glob>"),
            ("ls", r"(?is)<ls>\s*(?:<path>(.*?)</path>)?\s*</ls>"),
        ];

        for (tool_name, pattern) in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                for cap in re.captures_iter(response) {
                    let args = match *tool_name {
                        "read" => {
                            serde_json::json!({"path": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim()})
                        }
                        "write" => serde_json::json!({
                            "path": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim(),
                            "content": cap.get(2).map(|m| m.as_str()).unwrap_or("").trim()
                        }),
                        "edit" => serde_json::json!({
                            "path": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim(),
                            "oldText": cap.get(2).map(|m| m.as_str()).unwrap_or("").trim(),
                            "newText": cap.get(3).map(|m| m.as_str()).unwrap_or("").trim()
                        }),
                        "bash" => {
                            serde_json::json!({"command": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim()})
                        }
                        "grep" => serde_json::json!({
                            "pattern": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim(),
                            "path": cap.get(2).map(|m| m.as_str()).unwrap_or(".").trim()
                        }),
                        "find" => {
                            serde_json::json!({"pattern": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim()})
                        }
                        "ls" => {
                            serde_json::json!({"path": cap.get(1).map(|m| m.as_str()).unwrap_or(".").trim()})
                        }
                        _ => serde_json::json!({}),
                    };
                    push_call(tool_name.to_string(), args);
                }
            }
        }

        let mut json_calls: Vec<(String, serde_json::Value)> = Vec::new();
        if let Ok(root) = serde_json::from_str::<serde_json::Value>(response.trim()) {
            self.parse_json_node(&root, &mut json_calls);
        }
        if let Ok(re_json_block) = Regex::new(r"(?is)```json\s*(.*?)```") {
            for cap in re_json_block.captures_iter(response) {
                if let Some(block) = cap.get(1).map(|m| m.as_str()) {
                    if let Ok(root) = serde_json::from_str::<serde_json::Value>(block.trim()) {
                        self.parse_json_node(&root, &mut json_calls);
                    }
                }
            }
        }

        for (name, args) in json_calls {
            let args = AgentLoop::adapt_args_for_schema(&name, args);
            push_call(name, args);
        }

        calls
    }

    fn parse_json_node(
        &self,
        node: &serde_json::Value,
        out: &mut Vec<(String, serde_json::Value)>,
    ) {
        if let Some(arr) = node.as_array() {
            for item in arr {
                self.parse_json_node(item, out);
            }
            return;
        }
        if let Some(obj) = node.as_object() {
            if let Some(inner) = obj.get("tools") {
                self.parse_json_node(inner, out);
            }
            let raw_name = obj
                .get("name")
                .or_else(|| obj.get("tool"))
                .and_then(|v| v.as_str());
            if let Some(name) = raw_name {
                if let Some(normalized) = self.normalize_tool_name(name) {
                    let args = obj
                        .get("args")
                        .or_else(|| obj.get("parameters"))
                        .or_else(|| obj.get("input"))
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));
                    out.push((normalized, args));
                }
            }
        }
    }

    pub fn new(config: AgentConfig, session_id: String) -> Self {
        Self {
            config,
            tool_registry: ToolRegistry::new(),
            tool_executor: ToolExecutor::new(),
            messages: Vec::new(),
            session_id,
        }
    }

    /// Add a system message
    pub fn add_system_message(&mut self, content: String) {
        self.messages.push(Message {
            role: MessageRole::System,
            content,
            tool_calls: None,
            tool_results: None,
            timestamp: now(),
        });
    }

    /// Add a user message and start the agent loop
    pub async fn add_user_message(
        &mut self,
        content: String,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.messages.push(Message {
            role: MessageRole::User,
            content,
            tool_calls: None,
            tool_results: None,
            timestamp: now(),
        });

        event_tx
            .send(AgentEvent::MessageAdded(
                self.messages.last().unwrap().clone(),
            ))
            .await?;

        // Run the agent loop
        self.run_loop(event_tx).await
    }

    /// Main agent loop
    async fn run_loop(
        &mut self,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut iteration = 0u32;

        loop {
            if iteration >= self.config.max_iterations {
                event_tx
                    .send(AgentEvent::Error("Max iterations reached".to_string()))
                    .await?;
                break;
            }

            iteration += 1;

            // Get assistant response (with streaming)
            let (assistant_message, tool_calls) =
                self.get_assistant_response(event_tx.clone()).await?;

            self.messages.push(assistant_message.clone());
            event_tx
                .send(AgentEvent::MessageAdded(assistant_message))
                .await?;

            // If no tool calls, we're done
            if tool_calls.is_empty() {
                break;
            }

            // Execute tool calls
            let mut tool_results = Vec::new();
            for tool_call in tool_calls
                .iter()
                .take(self.config.max_tool_calls_per_message as usize)
            {
                // Validate tool call
                if let Err(e) = self
                    .tool_registry
                    .validate(&tool_call.name, &tool_call.arguments)
                {
                    event_tx
                        .send(AgentEvent::ToolError {
                            name: tool_call.name.clone(),
                            error: e.to_string(),
                        })
                        .await?;

                    tool_results.push(ToolExecutionResult {
                        tool_call_id: tool_call.id.clone(),
                        name: tool_call.name.clone(),
                        output: format!("Validation error: {}", e),
                        success: false,
                        execution_time_ms: 0,
                    });
                    continue;
                }

                // Execute tool
                event_tx
                    .send(AgentEvent::ToolStart {
                        name: tool_call.name.clone(),
                        args: tool_call.arguments.clone(),
                    })
                    .await?;

                let start_time = std::time::Instant::now();

                let provider = self.tool_registry.provider_for(&tool_call.name).cloned();
                let result = self
                    .tool_executor
                    .execute_with_provider(provider.as_ref(), &tool_call.name, &tool_call.arguments)
                    .await;

                let execution_time_ms = start_time.elapsed().as_millis() as u64;

                match result {
                    Ok(output) => {
                        let tool_result = ToolExecutionResult {
                            tool_call_id: tool_call.id.clone(),
                            name: tool_call.name.clone(),
                            output: output.clone(),
                            success: true,
                            execution_time_ms,
                        };

                        event_tx
                            .send(AgentEvent::ToolComplete {
                                name: tool_call.name.clone(),
                                result: tool_result.clone(),
                            })
                            .await?;

                        tool_results.push(tool_result);
                    }
                    Err(e) => {
                        let tool_result = ToolExecutionResult {
                            tool_call_id: tool_call.id.clone(),
                            name: tool_call.name.clone(),
                            output: format!("Error: {}", e),
                            success: false,
                            execution_time_ms,
                        };

                        event_tx
                            .send(AgentEvent::ToolError {
                                name: tool_call.name.clone(),
                                error: e.to_string(),
                            })
                            .await?;

                        tool_results.push(tool_result);
                    }
                }
            }

            // Add tool results as messages
            for result in &tool_results {
                let tool_msg = Message {
                    role: MessageRole::Tool,
                    content: result.output.clone(),
                    tool_calls: None,
                    tool_results: None,
                    timestamp: now(),
                };
                self.messages.push(tool_msg);
            }

            event_tx
                .send(AgentEvent::IterationComplete { iteration })
                .await?;
        }

        event_tx.send(AgentEvent::Complete).await?;
        Ok(())
    }

    /// Get assistant response with streaming
    async fn get_assistant_response(
        &mut self,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<(Message, Vec<ToolCall>), Box<dyn std::error::Error>> {
        let model = pi_ai_compat::Model {
            provider: "ollama".to_string(),
            name: self.config.model.clone(),
            api_url: self.config.api_url.clone(),
            supports_thinking: self.config.supports_thinking,
            max_tokens: 4096,
            temperature: 0.7,
        };

        let mut context = pi_ai_compat::Context::new();
        for message in &self.messages {
            match message.role {
                MessageRole::System => {
                    context.messages.push(pi_ai_compat::Message {
                        role: pi_ai_compat::Role::System,
                        content: message.content.clone(),
                        thinking: None,
                    });
                }
                MessageRole::User => {
                    context.messages.push(pi_ai_compat::Message {
                        role: pi_ai_compat::Role::User,
                        content: message.content.clone(),
                        thinking: None,
                    });
                }
                MessageRole::Assistant => {
                    context.messages.push(pi_ai_compat::Message {
                        role: pi_ai_compat::Role::Assistant,
                        content: message.content.clone(),
                        thinking: None,
                    });
                }
                MessageRole::Tool => {
                    context.messages.push(pi_ai_compat::Message {
                        role: pi_ai_compat::Role::Tool,
                        content: message.content.clone(),
                        thinking: None,
                    });
                }
            }
        }

        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
        let event_tx_clone = event_tx.clone();
        let forward_task = tokio::spawn(async move {
            while let Some(ev) = stream_rx.recv().await {
                let _ = event_tx_clone.send(AgentEvent::Stream(ev)).await;
            }
        });

        let options = pi_ai_compat::CompleteOptions::default()
            .with_streaming()
            .with_thinking();

        let response =
            pi_ai_compat::complete_streaming(&model, &context, options, |token, is_thinking| {
                let ev = if is_thinking {
                    StreamEvent::Thinking {
                        text: token.to_string(),
                    }
                } else {
                    StreamEvent::Content {
                        text: token.to_string(),
                    }
                };
                let _ = stream_tx.send(ev);
            })
            .await?;

        let _ = stream_tx.send(StreamEvent::Done);
        drop(stream_tx);
        let _ = forward_task.await;

        let content = response.content;
        let tool_calls = self.parse_tool_calls(&content);

        // Create assistant message
        let message = Message {
            role: MessageRole::Assistant,
            content,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls.clone())
            },
            tool_results: None,
            timestamp: now(),
        };

        Ok((message, tool_calls))
    }

    /// Get conversation history
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Get mutable conversation history
    pub fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.messages
    }

    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    pub fn tool_registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tool_registry
    }

    /// Clear conversation (keep system message)
    pub fn clear_conversation(&mut self) {
        let system_msg = self
            .messages
            .iter()
            .find(|m| m.role == MessageRole::System)
            .cloned();

        self.messages.clear();

        if let Some(sys) = system_msg {
            self.messages.push(sys);
        }
    }

    /// Get session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

/// Agent loop builder
pub struct AgentLoopBuilder {
    config: AgentConfig,
    system_message: Option<String>,
    custom_tools: Vec<super::schemas::ToolSchema>,
}

impl AgentLoopBuilder {
    pub fn new() -> Self {
        Self {
            config: AgentConfig::default(),
            system_message: None,
            custom_tools: Vec::new(),
        }
    }

    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_system_message(mut self, message: String) -> Self {
        self.system_message = Some(message);
        self
    }

    pub fn with_tool(mut self, tool: super::schemas::ToolSchema) -> Self {
        self.custom_tools.push(tool);
        self
    }

    pub fn build(self, session_id: String) -> AgentLoop {
        let mut agent = AgentLoop::new(self.config, session_id);

        if let Some(sys_msg) = self.system_message {
            agent.add_system_message(sys_msg);
        }

        for tool in self.custom_tools {
            agent.tool_registry.register(tool);
        }

        agent
    }
}

impl Default for AgentLoopBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coder_assistant::schemas::{ObjectSchema, ToolSchema};
    use std::collections::HashMap;

    #[test]
    fn parses_registered_external_tool_calls_from_json() {
        let tool = ToolSchema {
            name: "mail_search".to_string(),
            description: "Search mailbox".to_string(),
            parameters: ObjectSchema {
                schema_type: "object".to_string(),
                properties: HashMap::new(),
            },
            required: vec![],
        };
        let agent = AgentLoopBuilder::new()
            .with_tool(tool)
            .build("session-1".to_string());

        let calls =
            agent.parse_tool_calls(r#"{"tool":"mail_search","args":{"query":"latest status"}}"#);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "mail_search");
        assert_eq!(calls[0].arguments["query"], "latest status");
    }
}
