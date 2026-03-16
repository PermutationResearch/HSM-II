//! Agent - High-level wrapper with state management and event handling

use super::*;
use events::EventBus;
use queue::{MessageQueue, QueueMode};

/// High-level Agent class
pub struct Agent {
    loop_core: AgentLoop,
    state: SharedState,
    event_bus: EventBus,
    message_queue: std::sync::Mutex<MessageQueue>,
    queue_mode: QueueMode,
    model_config: ModelConfig,
}

impl Agent {
    /// Create new agent
    pub fn new() -> Self {
        let state = Arc::new(RwLock::new(AgentState::new()));
        let loop_core = AgentLoop::new(state.clone());

        Self {
            loop_core,
            state: state.clone(),
            event_bus: EventBus::new(),
            message_queue: std::sync::Mutex::new(MessageQueue::new()),
            queue_mode: QueueMode::OneAtATime,
            model_config: ModelConfig::default(),
        }
    }

    /// Builder pattern
    pub fn builder() -> AgentBuilder {
        AgentBuilder::new()
    }

    /// Set system message
    pub async fn set_system(&self, content: impl Into<String>) {
        let msg = Message::system(content);
        let mut state = self.state.write().await;
        // Remove existing system messages
        state
            .conversation
            .retain(|m| !matches!(m.role, Role::System));
        state.conversation.insert(0, msg);
    }

    /// Send user message (queue for processing)
    pub fn send(&self, content: impl Into<String>) {
        let msg = Message::user(content);
        let mut queue = self.message_queue.lock().unwrap();
        queue.push(msg);
    }

    /// Send message with attachments
    pub fn send_with_attachments(&self, content: impl Into<String>, attachments: Vec<Attachment>) {
        let msg = Message::user(content).with_attachments(attachments);
        let mut queue = self.message_queue.lock().unwrap();
        queue.push(msg);
    }

    /// Run the agent (processes queued messages)
    pub async fn run(&self) -> Result<(), AgentError> {
        // Setup queue callback
        let queue = self.message_queue.lock().unwrap();
        let _queue_clone = MessageQueue::new(); // Clone for callback
        drop(queue);

        // Process messages
        loop {
            let messages = {
                let mut queue = self.message_queue.lock().unwrap();
                queue.set_mode(self.queue_mode);
                queue.drain()
            };

            if messages.is_empty() {
                break;
            }

            for msg in messages {
                self.loop_core.run(msg, &self.model_config).await?;
            }
        }

        Ok(())
    }

    /// Run with single message
    pub async fn run_once(&self, content: impl Into<String>) -> Result<(), AgentError> {
        self.send(content);
        self.run().await
    }

    /// Subscribe to events
    pub fn on_event<H>(&self, handler: H)
    where
        H: EventHandler + 'static,
    {
        self.event_bus.subscribe(handler);
    }

    /// Get conversation history
    pub async fn conversation(&self) -> Vec<Message> {
        self.state.read().await.conversation.clone()
    }

    /// Get last message
    pub async fn last_message(&self) -> Option<Message> {
        self.state.read().await.last_message().cloned()
    }

    /// Clear conversation
    pub async fn clear(&self) {
        self.loop_core.clear().await;
    }

    /// Set queue mode
    pub fn set_queue_mode(&mut self, mode: QueueMode) {
        self.queue_mode = mode;
    }

    /// Add tool
    pub fn add_tool(&mut self, tool: Tool) {
        self.loop_core.add_tool(tool);
    }

    /// Set model
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model_config.model = model.into();
    }

    /// Export conversation to JSON
    pub async fn export(&self) -> Result<String, serde_json::Error> {
        let state = self.state.read().await;
        serde_json::to_string_pretty(&state.conversation)
    }

    /// Import conversation from JSON
    pub async fn import(&self, json: &str) -> Result<(), serde_json::Error> {
        let messages: Vec<Message> = serde_json::from_str(json)?;
        let mut state = self.state.write().await;
        state.conversation = messages;
        Ok(())
    }
}

impl Default for Agent {
    fn default() -> Self {
        Self::new()
    }
}

/// Agent builder
pub struct AgentBuilder {
    system_message: Option<String>,
    tools: Vec<Tool>,
    queue_mode: QueueMode,
    model_config: ModelConfig,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            system_message: None,
            tools: Vec::new(),
            queue_mode: QueueMode::OneAtATime,
            model_config: ModelConfig::default(),
        }
    }

    pub fn system(mut self, message: impl Into<String>) -> Self {
        self.system_message = Some(message.into());
        self
    }

    pub fn tool(mut self, tool: Tool) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn queue_mode(mut self, mode: QueueMode) -> Self {
        self.queue_mode = mode;
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model_config.model = model.into();
        self
    }

    pub fn build(self) -> Agent {
        let state = Arc::new(RwLock::new(AgentState::new()));

        // Add system message if provided
        if let Some(sys) = self.system_message {
            let sys_msg = Message::system(sys);
            state.try_write().unwrap().add_message(sys_msg);
        }

        let loop_core = AgentLoop::new(state.clone()).with_tools(self.tools);

        Agent {
            loop_core,
            state,
            event_bus: EventBus::new(),
            message_queue: std::sync::Mutex::new(MessageQueue::new()),
            queue_mode: self.queue_mode,
            model_config: self.model_config,
        }
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_builder() {
        let model = crate::ollama_client::resolve_model_from_env(
            "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL",
        );
        let _agent = Agent::builder()
            .system("You are a helpful assistant")
            .queue_mode(QueueMode::AllAtOnce)
            .model(&model)
            .build();

        // Just verify it builds
        assert!(true);
    }
}
