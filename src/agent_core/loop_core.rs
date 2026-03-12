//! Agent Loop Core - The orchestration engine
//!
//! The loop handles:
//! 1. User message processing
//! 2. Tool call execution
//! 3. Results fed back to LLM
//! 4. Repeats until model produces response without tool calls
//! 5. Message queuing via callback after each turn

use super::*;
use events::AgentEvent;

/// The core agent loop
pub struct AgentLoop {
    state: SharedState,
    transport: Box<dyn Transport>,
    tools: Vec<Tool>,
    event_sender: Option<EventSender>,
    queue_callback: Option<QueueCallback>,
}

impl AgentLoop {
    pub fn new(state: SharedState) -> Self {
        Self {
            state,
            transport: Box::new(DirectTransport::new()),
            tools: Vec::new(),
            event_sender: None,
            queue_callback: None,
        }
    }

    pub fn with_transport(mut self, transport: Box<dyn Transport>) -> Self {
        self.transport = transport;
        self
    }

    pub fn with_tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = tools;
        self
    }

    /// Add a single tool
    pub fn add_tool(&mut self, tool: Tool) {
        self.tools.push(tool);
    }

    pub fn with_event_sender(mut self, sender: EventSender) -> Self {
        self.event_sender = Some(sender);
        self
    }

    pub fn with_queue_callback(mut self, callback: QueueCallback) -> Self {
        self.queue_callback = Some(callback);
        self
    }

    /// Emit event if sender is configured
    fn emit(&self, event: AgentEvent) {
        if let Some(sender) = &self.event_sender {
            let _ = sender.send(event);
        }
    }

    /// Run the agent loop with a user message
    pub async fn run(&self, user_message: Message, config: &ModelConfig) -> Result<(), AgentError> {
        // Add user message to state
        {
            let mut state = self.state.write().await;
            state.add_message(user_message);
            state.is_processing = true;
        }

        self.emit(AgentEvent::MessageStart);

        // Main loop - continues until no tool calls
        loop {
            let turn = {
                let mut state = self.state.write().await;
                state.turn_count += 1;
                state.turn_count
            };

            self.emit(AgentEvent::TurnStart { turn });

            // Get messages for LLM
            let messages = {
                let state = self.state.read().await;
                state.conversation.clone()
            };

            // Call LLM
            let response = self.call_llm(&messages, config).await?;

            // Add assistant message to state
            let has_tool_calls = response.tool_calls.is_some();
            {
                let mut state = self.state.write().await;
                state.add_message(Message {
                    role: Role::Assistant,
                    content: response.content.clone(),
                    tool_calls: response.tool_calls.clone(),
                    tool_call_id: None,
                    attachments: Vec::new(),
                    timestamp: now(),
                });
            }

            self.emit(AgentEvent::MessageComplete(Message {
                role: Role::Assistant,
                content: response.content,
                tool_calls: response.tool_calls.clone(),
                tool_call_id: None,
                attachments: Vec::new(),
                timestamp: now(),
            }));

            // If no tool calls, we're done
            if !has_tool_calls {
                self.emit(AgentEvent::TurnComplete { turn });
                break;
            }

            // Execute tool calls
            if let Some(tool_calls) = response.tool_calls {
                for tool_call in tool_calls {
                    self.emit(AgentEvent::ToolStart {
                        tool_call: tool_call.clone(),
                    });

                    let result = self.execute_tool(&tool_call).await;

                    match result {
                        Ok(output) => {
                            self.emit(AgentEvent::ToolComplete {
                                tool_call_id: tool_call.id.clone(),
                                result: output.clone(),
                            });

                            // Add tool result to state
                            let mut state = self.state.write().await;
                            state.add_message(Message::tool(output, &tool_call.id));
                        }
                        Err(e) => {
                            self.emit(AgentEvent::ToolError {
                                tool_call_id: tool_call.id.clone(),
                                error: e.to_string(),
                            });

                            // Add error as tool result
                            let mut state = self.state.write().await;
                            state
                                .add_message(Message::tool(format!("Error: {}", e), &tool_call.id));
                        }
                    }
                }
            }

            self.emit(AgentEvent::TurnComplete { turn });

            // Check for queued messages after each turn
            if let Some(callback) = &self.queue_callback {
                let queued = callback();
                if !queued.is_empty() {
                    let count = queued.len();
                    {
                        let mut state = self.state.write().await;
                        for msg in queued {
                            state.add_message(msg);
                        }
                    }
                    self.emit(AgentEvent::QueuedMessagesInjected { count });
                }
            }
        }

        {
            let mut state = self.state.write().await;
            state.is_processing = false;
        }

        self.emit(AgentEvent::Done);
        Ok(())
    }

    /// Call LLM via transport
    async fn call_llm(
        &self,
        messages: &[Message],
        config: &ModelConfig,
    ) -> Result<TransportResponse, AgentError> {
        self.transport
            .complete(messages, config)
            .await
            .map_err(|e| AgentError::TransportError(e.to_string()))
    }

    /// Execute a tool
    async fn execute_tool(&self, tool_call: &ToolCall) -> Result<String, ToolError> {
        // Find tool
        let tool = self
            .tools
            .iter()
            .find(|t| t.name == tool_call.name)
            .ok_or_else(|| ToolError::NotFound(tool_call.name.clone()))?;

        // Execute
        self.emit(AgentEvent::ToolExecuting {
            name: tool_call.name.clone(),
            args: tool_call.arguments.clone(),
        });

        tool.handler.execute(&tool_call.arguments).await
    }

    /// Get current state
    pub async fn get_state(&self) -> AgentState {
        self.state.read().await.clone()
    }

    /// Clear conversation
    pub async fn clear(&self) {
        let mut state = self.state.write().await;
        state.clear();
        self.emit(AgentEvent::ConversationCleared);
    }
}

/// Agent error
#[derive(Debug, Clone)]
pub enum AgentError {
    TransportError(String),
    ToolError(String),
    StateError(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::TransportError(msg) => write!(f, "Transport error: {}", msg),
            AgentError::ToolError(msg) => write!(f, "Tool error: {}", msg),
            AgentError::StateError(msg) => write!(f, "State error: {}", msg),
        }
    }
}

impl std::error::Error for AgentError {}
