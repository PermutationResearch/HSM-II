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
use crate::tools::scored_tool_router::rank_tools_for_prompt;
use crate::tools::ToolRegistry;
use tokio::sync::{mpsc, oneshot};

pub mod agent_memory_pipeline;
pub mod autodream;
pub mod business_pack;
pub mod enhanced_agent;
pub mod gateway;
pub mod heartbeat;
pub mod hsm_cron;
pub mod hypergraph_client;
pub mod integrated_agent;
pub mod kairos;
pub mod kb_manifest;
pub mod memory;
pub mod ops_config;
pub mod outbound;
pub mod pairing_store;
pub mod path_attachments;
pub mod persona;
pub mod prompt_assembly;
pub mod prompt_defaults;
pub mod task_trail;

pub use enhanced_agent::{
    AgentMetrics, AgentResponse, ContributionType, EnhancedAgentConfig, EnhancedPersonalAgent,
    JouleWorkRecord, MessageContext, WorldStats,
};
pub use integrated_agent::{
    integrated_home, AgentComponents, ComponentStatus, IntegratedAgentConfig,
    IntegratedPersonalAgent,
};
pub use kb_manifest::{load_kb_manifest_report, KbManifestReport};

/// Full-stack agent: same as [`IntegratedPersonalAgent`] (shared `EnhancedPersonalAgent` core + integration layer).
pub type UnifiedPersonalAgent = IntegratedPersonalAgent;
pub use business_pack::{
    validate_pack_yaml_file, BusinessPack, BusinessPersona, CompanyProfile, PackValidationReport,
    MAX_KNOWLEDGE_FILE_BYTES, MAX_POLICY_FILE_BYTES, MAX_TOTAL_INJECTED_BYTES,
    SUPPORTED_SCHEMA_VERSION,
};
pub use heartbeat::{CronJob, Heartbeat, Routine, RoutineAction, RoutineTrigger};
pub use hypergraph_client::HypergraphClient;
pub use memory::{MemoryFact, MemoryMd, PersonalMemory, Project, UserMd};
pub use ops_config::{
    load_ops_config, resolve_ops_config_path, OperationsConfig, OPS_SCHEMA_VERSION,
};
pub use pairing_store::PairingStore;
pub use persona::{Capability, Persona, Voice};
pub use task_trail::TaskTrail;

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

        // Initialize LLM client with auto-detected model
        let mut llm_config = OllamaConfig::default();
        if llm_config.model == "auto" {
            llm_config.model = OllamaConfig::detect_model(&llm_config.host, llm_config.port).await;
        }
        let llm = OllamaClient::new(llm_config);

        // Check if Ollama is available (non-blocking)
        let ollama_ready = llm.check_available().await;
        if !ollama_ready {
            println!("\n⚠️  Ollama LLM not detected. Responses will be limited.");
            println!("   To enable full AI responses, run: ollama serve\n");
        }

        tracing::info!("PersonalAgent initialized: {}", persona.name);

        // Full HSM-II tool surface (web, browser, git, API, …) — same as enhanced agent
        let mut tool_registry = ToolRegistry::new();
        crate::tools::register_all_tools(&mut tool_registry);
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

        // Initialize LLM client with auto-detected model
        let mut llm_config = OllamaConfig::default();
        if llm_config.model == "auto" {
            llm_config.model = OllamaConfig::detect_model(&llm_config.host, llm_config.port).await;
        }
        let llm = OllamaClient::new(llm_config);

        tracing::info!("New PersonalAgent created: {}", persona.name);

        let mut tool_registry = ToolRegistry::new();
        crate::tools::register_all_tools(&mut tool_registry);

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
    pub async fn start_gateway(
        &mut self,
        config: gateway::Config,
    ) -> Result<mpsc::Receiver<(gateway::Message, oneshot::Sender<String>)>> {
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
            "<|end_header_id|>",
            "<|start_header_id|>",
            "<|eot_id|>",
            "<|begin_of_text|>",
            "<|end_of_text|>",
            "<|finetune_right_pad_id|>",
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
        let result = self
            .llm
            .chat(system_prompt, &msg.content, &self.chat_history)
            .await;

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
        let rank_cap = std::env::var("HSM_TOOL_PROMPT_CAP")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|&n| n > 0)
            .unwrap_or(24usize);
        let ranked_tools = rank_tools_for_prompt(&self.tool_registry, task, rank_cap);
        let tool_catalog = self.tool_registry.list_tools();
        let tool_descs: std::collections::HashMap<&str, &str> =
            tool_catalog.iter().copied().collect();
        let mut tool_lines = ranked_tools
            .iter()
            .filter_map(|t| {
                tool_descs
                    .get(t.name.as_str())
                    .map(|desc| format!("- {}: {} [score={:.1}]", t.name, desc, t.score))
            })
            .collect::<Vec<_>>();
        if tool_lines.is_empty() {
            tool_lines = tool_catalog
                .iter()
                .take(rank_cap)
                .map(|(name, desc)| format!("- {}: {}", name, desc))
                .collect();
        }
        let hidden_count = tool_catalog.len().saturating_sub(tool_lines.len());
        if hidden_count > 0 {
            tool_lines.push(format!(
                "- ... {} additional tools hidden by prompt budget cap",
                hidden_count
            ));
        }
        let tools_description = tool_lines.join("\n");

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
                        harness_run: None,
                        idempotency_key: None,
                    };

                    tracing::info!("Executing tool: {} with params: {:?}", tool_name, params);
                    let result = self.tool_registry.execute(call).await;

                    let tool_result = if result.output.success {
                        result.output.result.clone()
                    } else {
                        result
                            .output
                            .error
                            .clone()
                            .unwrap_or_else(|| "Unknown error".to_string())
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

/// Resolve HSM-II home directory (Hermes-style multi-instance / profiles).
///
/// Precedence: explicit `cli_config` if set, else environment `HSMII_HOME`, else
/// `~/.hsmii`, optionally scoped to `~/.hsmii/profiles/<profile>/` when `profile` or
/// `HSMII_PROFILE` is set.
pub fn resolve_hsmii_home(cli_config: Option<PathBuf>, profile: Option<&str>) -> PathBuf {
    if let Some(p) = cli_config {
        return p;
    }
    if let Ok(h) = std::env::var("HSMII_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    let base = dirs::home_dir()
        .expect("Could not find home directory")
        .join(".hsmii");
    let profile_name = profile
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::var("HSMII_PROFILE")
                .ok()
                .filter(|s| !s.trim().is_empty())
        });
    if let Some(name) = profile_name {
        return base.join("profiles").join(sanitize_profile_dir_name(&name));
    }
    base
}

fn sanitize_profile_dir_name(name: &str) -> String {
    let t = name.trim();
    if t.is_empty() || t == "." || t == ".." {
        return "default".to_string();
    }
    let out: String = t
        .chars()
        .map(|c| match c {
            '/' | '\\' => '_',
            c if c.is_alphanumeric() || c == '-' || c == '_' => c,
            ' ' => '_',
            _ => '_',
        })
        .collect();
    if out.is_empty() {
        "default".into()
    } else {
        out
    }
}

/// Default HSM-II home (`resolve_hsmii_home` with no CLI override; respects `HSMII_HOME` / `HSMII_PROFILE`).
pub fn hsmii_home() -> PathBuf {
    resolve_hsmii_home(None, None)
}
