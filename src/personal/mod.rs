//! Personal Agent System - Grounded HSM-II
//!
//! Transforms HSM-II from research simulation into practical personal AI assistant.
//! Inspired by Hermes Agent's MEMORY.md + USER.md + SOUL.md architecture.

// use std::collections::HashMap;  // TODO: Use when needed
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::ollama_client::{OllamaClient, OllamaConfig};
use crate::tools::ToolRegistry;
use tokio::sync::{mpsc, oneshot};

pub mod enhanced_agent;
pub mod gateway;
pub mod heartbeat;
pub mod hypergraph_client;
pub mod memory;
pub mod persona;

pub use enhanced_agent::{
    EnhancedPersonalAgent, EnhancedAgentConfig, AgentResponse, WorldStats,
    JouleWorkRecord, ContributionType, AgentMetrics, MessageContext,
};
pub use heartbeat::{CronJob, Heartbeat, Routine, RoutineAction, RoutineTrigger};
pub use hypergraph_client::HypergraphClient;
pub use memory::{MemoryFact, MemoryMd, PersonalMemory, Project, UserMd};
pub use persona::{Capability, Persona, Voice};

/// HSM-II Personal Agent - The unified interface
pub struct PersonalAgent {
    /// Storage path (e.g., ~/.hsmii/)
    base_path: PathBuf,
    /// Agent personality
    pub persona: Persona,
    /// Persistent memory
    pub memory: PersonalMemory,
    /// Scheduled tasks
    pub heartbeat: Heartbeat,
    /// Gateway for external communication
    pub gateway: Option<gateway::Gateway>,
    /// LLM client for generating responses
    llm: OllamaClient,
    /// Tool registry for executing tasks
    tool_registry: ToolRegistry,
    /// Recent chat history for conversation continuity (user, assistant) pairs
    chat_history: Vec<(String, String)>,
}

impl PersonalAgent {
    /// Initialize or load existing agent
    pub async fn initialize(base_path: impl AsRef<Path>) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();

        // Ensure directory structure exists
        Self::ensure_structure(&base_path).await?;

        // Load components
        let persona = Persona::load(&base_path).await?;
        let memory = PersonalMemory::load(&base_path).await?;
        let heartbeat = Heartbeat::load(&base_path).await?;

        // Initialize LLM client
        let llm_config = OllamaConfig::default();
        let llm = OllamaClient::new(llm_config);

        // Check if Ollama is available (non-blocking)
        let ollama_ready = llm.check_available().await;
        if !ollama_ready {
            println!("\n⚠️  Ollama LLM not detected. Responses will be limited.");
            println!("   To enable full AI responses, run: ollama serve\n");
        }

        tracing::info!("PersonalAgent initialized: {}", persona.name);

        // Initialize tool registry with default tools
        let tool_registry = ToolRegistry::with_default_tools();
        tracing::info!("Loaded {} tools", tool_registry.list_tools().len());

        Ok(Self {
            base_path,
            persona,
            memory,
            heartbeat,
            gateway: None,
            llm,
            tool_registry,
            chat_history: Vec::new(),
        })
    }

    /// Bootstrap new agent with user onboarding
    pub async fn bootstrap(base_path: impl AsRef<Path>) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();

        // Ensure directory structure exists first
        Self::ensure_structure(&base_path).await?;

        println!("🌱 Welcome to HSM-II Personal Agent");
        println!("Let's set up your AI companion...\n");

        // Interactive onboarding
        let persona = Persona::bootstrap(&base_path).await?;
        let memory = PersonalMemory::bootstrap(&base_path).await?;
        let heartbeat = Heartbeat::default();

        // Initialize LLM client
        let llm_config = OllamaConfig::default();
        let llm = OllamaClient::new(llm_config);

        tracing::info!("New PersonalAgent created: {}", persona.name);

        // Initialize tool registry with default tools
        let tool_registry = ToolRegistry::with_default_tools();

        Ok(Self {
            base_path,
            persona,
            memory,
            heartbeat,
            gateway: None,
            llm,
            tool_registry,
            chat_history: Vec::new(),
        })
    }

    /// Process incoming message from any gateway
    pub async fn handle_message(&mut self, msg: gateway::Message) -> Result<String> {
        // 1. Get relevant context from memory (facts, projects, preferences)
        let context = self.memory.get_context(&msg.content).await?;

        // 2. Load today's conversation history from disk
        self.chat_history = self.memory.load_chat_history().await?;

        // 3. Build system prompt from persona + context
        let system_prompt = self.build_system_prompt(&context);

        // 4. Generate response with full conversation history
        let response = self.generate_response(&system_prompt, &msg).await?;

        // 5. Record interaction to disk (appends to today's log)
        self.memory.record_interaction(&msg, &response).await?;

        // 6. Persist changes
        self.save().await?;

        Ok(response)
    }

    /// Execute a task using available tools
    pub async fn execute_task(&mut self, task: &str) -> Result<TaskResult> {
        tracing::info!("Executing task: {}", task);

        // 1. Check CASS for relevant skills
        // TODO: Integrate with CASS

        // 2. If skill found, execute it
        // TODO: Skill execution

        // 3. If no skill, use LLM + tools
        let result = self.execute_with_tools(task).await?;

        // 4. Record for potential skill distillation
        // TODO: After N similar tasks, distill into skill

        Ok(result)
    }

    /// Run heartbeat check
    pub async fn heartbeat(&mut self) -> Result<Vec<HeartbeatResult>> {
        self.heartbeat.tick(&self.base_path).await
    }

    /// Start gateway for external communication
    /// Returns a channel receiver for processing messages
    pub async fn start_gateway(&mut self, config: gateway::Config) -> Result<mpsc::Receiver<(gateway::Message, oneshot::Sender<String>)>> {
        let mut gateway = gateway::Gateway::new(config);
        
        // Create channel for message passing (avoids circular reference)
        let (tx, rx) = mpsc::channel::<(gateway::Message, oneshot::Sender<String>)>(100);
        
        // Create handler that uses the channel
        let handler = ChannelMessageHandler { sender: tx };
        gateway.on_message(handler);
        gateway.start().await?;
        self.gateway = Some(gateway);
        
        Ok(rx)
    }

    /// Get the current LLM model name
    pub fn current_model(&self) -> String {
        self.llm.model().to_string()
    }

    /// Build system prompt for LLM
    fn build_system_prompt(&self, context: &str) -> String {
        let name = &self.persona.name;
        format!(
            "You are {name}. You MUST stay in character as {name} at all times. Never say you are a large language model or AI assistant. You are {name}.\n\n\
            {persona}\n\n\
            {capabilities}\n\n\
            ## Relevant Context\n{context}\n\n\
            ## Rules\n\
            - Always introduce yourself as {name} if asked who you are\n\
            - Stay in character based on your Identity and Voice above\n\
            - Be helpful and proactive\n\
            - Ask clarifying questions when needed\n\
            - Reference previous conversations when relevant\n\
            - Keep responses concise and natural",
            name = name,
            persona = self.persona.to_system_prompt(),
            capabilities = self.format_capabilities(),
            context = context,
        )
    }

    /// Format available capabilities
    fn format_capabilities(&self) -> String {
        "## Your Capabilities\n- Web search and research\n- File operations\n- Terminal commands (sandboxed)\n- Task scheduling\n- Multi-agent coordination\n- Memory and context awareness".to_string()
    }

    /// Strip leaked chat template tokens from LLM output
    fn clean_response(text: &str) -> String {
        let mut cleaned = text.to_string();
        // Strip Llama 3 chat template tokens
        for token in &[
            "<|end_header_id|>", "<|start_header_id|>", "<|eot_id|>",
            "<|begin_of_text|>", "<|end_of_text|>", "<|finetune_right_pad_id|>",
        ] {
            cleaned = cleaned.replace(token, "");
        }
        // Also strip any remaining <|...|> patterns
        while let Some(start) = cleaned.find("<|") {
            if let Some(end) = cleaned[start..].find("|>") {
                cleaned.replace_range(start..start + end + 2, "");
            } else {
                break;
            }
        }
        cleaned.trim().to_string()
    }

    /// Generate response using LLM
    async fn generate_response(
        &self,
        system_prompt: &str,
        msg: &gateway::Message,
    ) -> Result<String> {
        // Use chat endpoint with conversation history
        let result = self.llm.chat(system_prompt, &msg.content, &self.chat_history).await;

        if result.text.is_empty() || result.timed_out {
            // Fallback if LLM fails - provide helpful message
            Ok(format!("I'm {name}, your AI assistant. I'd respond to '{}' if Ollama was running.\n\nTo enable full responses:\n1. Install Ollama: https://ollama.com\n2. Run: ollama serve\n3. Pull a model: ollama pull llama3.2\n\nFor now, I can still help with file operations and memory.",
                msg.content,
                name = self.persona.name
            ))
        } else {
            Ok(Self::clean_response(&result.text))
        }
    }

    /// Execute task with tool calling
    async fn execute_with_tools(&mut self, task: &str) -> Result<TaskResult> {
        tracing::info!("Executing task with tools: {}", task);
        
        // Build system prompt with available tools
        let tools_description = self.tool_registry.list_tools()
            .iter()
            .map(|(name, desc)| format!("- {}: {}", name, desc))
            .collect::<Vec<_>>()
            .join("\n");
        
        let _system_prompt = format!(
            "You are an AI assistant that can use tools to complete tasks.\n\n\
             Available tools:\n{}\n\n\
             To use a tool, respond with JSON in this format:\n\
             {{\"tool\": \"tool_name\", \"parameters\": {{\"param1\": \"value1\"}}}}\n\n\
             If no tool is needed, respond normally.",
            tools_description
        );
        
        // Generate response from LLM
        let llm_result = self.llm.generate(task).await;
        
        let mut tool_calls = vec![];
        let mut output = String::new();
        
        // Try to parse tool call from response
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&llm_result.text) {
            if let Some(tool_name) = json.get("tool").and_then(|v| v.as_str()) {
                if let Some(params) = json.get("parameters") {
                    // Execute the tool
                    let call = crate::tools::ToolCall {
                        name: tool_name.to_string(),
                        parameters: params.clone(),
                        call_id: uuid::Uuid::new_v4().to_string(),
                    };
                    
                    tracing::info!("Executing tool: {} with params: {:?}", tool_name, params);
                    let result = self.tool_registry.execute(call).await;
                    
                    let tool_result = if result.output.success {
                        result.output.result.clone()
                    } else {
                        result.output.error.clone().unwrap_or_else(|| "Unknown error".to_string())
                    };
                    
                    tool_calls.push(crate::personal::ToolCall {
                        name: tool_name.to_string(),
                        arguments: params.clone(),
                        result: tool_result.clone(),
                        timestamp: chrono::Utc::now(),
                    });
                    
                    output = tool_result;
                }
            } else {
                // No tool call, use LLM response directly
                output = llm_result.text;
            }
        } else {
            // Not valid JSON, use LLM response
            output = llm_result.text;
        }
        
        // If no output and no tool calls, provide a default message
        if output.is_empty() && tool_calls.is_empty() {
            output = format!("I understand you want to: {}. However, I couldn't determine which tool to use. Please be more specific or check that the required tools are available.", task);
        }
        
        Ok(TaskResult {
            success: !output.starts_with("Tool error:"),
            output,
            tool_calls,
        })
    }

    /// Save all state to disk
    pub async fn save(&self) -> Result<()> {
        self.memory.save(&self.base_path).await?;
        self.heartbeat.save(&self.base_path).await?;
        // Persona rarely changes, only save if modified
        Ok(())
    }

    /// Ensure directory structure exists
    async fn ensure_structure(base_path: &Path) -> Result<()> {
        tokio::fs::create_dir_all(base_path).await?;
        tokio::fs::create_dir_all(base_path.join("memory")).await?;
        tokio::fs::create_dir_all(base_path.join("skills")).await?;
        tokio::fs::create_dir_all(base_path.join("todo")).await?;
        tokio::fs::create_dir_all(base_path.join("federation")).await?;
        tokio::fs::create_dir_all(base_path.join("cache")).await?;
        Ok(())
    }
}

/// Result of task execution
#[derive(Clone, Debug)]
pub struct TaskResult {
    pub success: bool,
    pub output: String,
    pub tool_calls: Vec<ToolCall>,
}

/// A tool call record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: String,
    pub timestamp: DateTime<Utc>,
}

/// Result of heartbeat action
#[derive(Clone, Debug)]
pub struct HeartbeatResult {
    pub action: String,
    pub success: bool,
    pub message: String,
}

/// Message handler that uses channels to communicate with the agent
/// This avoids circular references between gateway and agent
pub struct ChannelMessageHandler {
    sender: mpsc::Sender<(gateway::Message, oneshot::Sender<String>)>,
}

#[async_trait::async_trait]
impl gateway::MessageHandler for ChannelMessageHandler {
    async fn handle(&self, msg: gateway::Message) -> anyhow::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.sender.send((msg, tx)).await?;
        let response = rx.await?;
        Ok(response)
    }
}

/// Get default HSM-II home directory
pub fn hsmii_home() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join(".hsmii")
}
