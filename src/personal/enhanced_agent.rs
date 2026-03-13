//! Enhanced Personal Agent - Full HSM-II Integration
//!
//! This transforms the PersonalAgent into a complete HSM-II system with:
//! - HyperStigmergicMorphogenesis (multi-agent world)
//! - LadybugDB persistence (vector + graph storage)
//! - CASS (skill learning)
//! - Council (deliberation for complex decisions)
//! - Ralph Loop (iterative coding with worker-reviewer)
//! - RLM (Recursive Language Model for large documents)
//! - DKS (distributed knowledge)
//! - JouleWork (thermodynamic compensation)
//! - Tool Execution (60+ tools)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};
use async_trait::async_trait;

use crate::{
    CASS, CouncilMember, Proposal, RalphCouncil, RalphConfig,
    DKSSystem, DKSConfig, DKSTickResult, HyperStigmergicMorphogenesis,
    EmbeddedGraphStore, AgentId, Role,
    cass::{ContextSnapshot, embedding::EmbeddingEngine},
    council::{CouncilEvidence, CouncilEvidenceKind, CouncilFactory, ModeConfig,
              StigmergicCouncilContext, Decision, RalphVerdict},
    personal::gateway::{Message, Platform},
    tools::{Tool, ToolRegistry, ToolCall as ToolCallEntry, RlmProcessTool},
    rlm::LivingPrompt,
    ollama_client::{OllamaClient, OllamaConfig},
    social_memory::DataSensitivity,
};
use crate::hyper_stigmergy::{HyperEdge, Belief, BeliefSource, Experience, ExperienceOutcome};

/// Configuration for the enhanced agent
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnhancedAgentConfig {
    /// Number of agents in the world
    pub agent_count: usize,
    /// Enable council for complex decisions
    pub enable_council: bool,
    /// Enable CASS skill learning
    pub enable_cass: bool,
    /// Enable DKS knowledge evolution
    pub enable_dks: bool,
    /// Minimum confidence threshold for council
    pub council_threshold: f64,
    /// Auto-save interval in seconds
    pub save_interval_secs: u64,
    /// Enable JouleWork compensation tracking
    pub track_joulework: bool,
}

impl Default for EnhancedAgentConfig {
    fn default() -> Self {
        Self {
            agent_count: 5,
            enable_council: true,
            enable_cass: true,
            enable_dks: true,
            council_threshold: 0.7,
            save_interval_secs: 60,
            track_joulework: true,
        }
    }
}

/// Runtime services for the enhanced agent
pub struct RuntimeServices {
    pub dks: DKSSystem,
    pub cass: CASS,
    pub embedding_engine: EmbeddingEngine,
    pub last_dks_tick: Option<DKSTickResult>,
    pub cass_initialized: bool,
}

/// Message processing context - tracks agent contributions
#[derive(Clone, Debug)]
pub struct MessageContext {
    pub message_id: String,
    pub user_id: String,
    pub content: String,
    pub platform: Platform,
    pub assigned_agents: Vec<AgentId>,
    pub council_used: bool,
    pub skills_accessed: Vec<String>,
    pub start_time: Instant,
    pub joulework_contributions: HashMap<AgentId, f64>,
}

/// Enhanced Personal Agent with full HSM-II capabilities
pub struct EnhancedPersonalAgent {
    /// Storage path
    pub base_path: PathBuf,
    /// Configuration
    pub config: EnhancedAgentConfig,
    /// The HSM-II world - contains all agents, beliefs, edges
    pub world: HyperStigmergicMorphogenesis,
    /// Runtime services (DKS, CASS, embeddings)
    pub services: RuntimeServices,
    /// LLM client
    pub llm: OllamaClient,
    /// Tool registry
    pub tool_registry: ToolRegistry,
    /// Council factory for automatic mode selection
    pub council_factory: CouncilFactory,
    /// RLM Living Prompt for self-evolving system prompt enrichment
    pub living_prompt: LivingPrompt,
    /// Recent message history
    pub chat_history: Vec<(String, String)>,
    /// Active message contexts
    pub active_contexts: HashMap<String, MessageContext>,
    /// Agent performance metrics for JouleWork
    pub agent_metrics: AgentMetrics,
    /// Last save timestamp
    pub last_save: Instant,
    /// Messages since last RLM reflection
    pub messages_since_reflection: u64,
    /// Gateway message channel
    pub gateway_tx: Option<mpsc::Sender<(Message, oneshot::Sender<String>)>>,
    /// Gateway instance (kept alive to keep bots running)
    pub gateway: Option<crate::personal::gateway::Gateway>,
}

/// Tracks agent contributions for JouleWork compensation
#[derive(Clone, Debug, Default)]
pub struct AgentMetrics {
    pub total_messages_processed: u64,
    pub council_invocations: u64,
    pub skills_distilled: u64,
    pub joulework_history: Vec<JouleWorkRecord>,
}

/// A single JouleWork compensation record
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JouleWorkRecord {
    pub timestamp: u64,
    pub message_id: String,
    pub agent_id: AgentId,
    pub agent_role: String,
    pub contribution_type: ContributionType,
    pub jw_score: f64,
    pub coherence_delta: f64,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ContributionType {
    ResponseGeneration,
    CouncilDeliberation,
    SkillApplication,
    KnowledgeRetrieval,
    ToolExecution,
}

/// Agent response with metadata
#[derive(Clone, Debug)]
pub struct AgentResponse {
    pub content: String,
    pub primary_agent: AgentId,
    pub council_used: bool,
    pub confidence: f64,
    pub skills_used: Vec<String>,
    pub joulework_contributions: HashMap<AgentId, f64>,
    pub processing_time_ms: u64,
}

impl EnhancedPersonalAgent {
    /// Initialize or load existing enhanced agent
    pub async fn initialize(base_path: impl AsRef<Path>) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        
        // Ensure directory structure
        Self::ensure_structure(&base_path).await?;
        
        // Try to load from LadybugDB first
        let (world, config) = if EmbeddedGraphStore::exists() {
            info!("Loading world from LadybugDB...");
            let (world, _rlm) = EmbeddedGraphStore::load_world()?;
            let config = Self::load_config(&base_path).await?;
            (world, config)
        } else {
            info!("Creating new HSM-II world...");
            let config = EnhancedAgentConfig::default();
            let world = Self::create_new_world(&config).await?;
            (world, config)
        };
        
        // Initialize LLM with auto-detected model
        let mut llm_config = OllamaConfig::default();
        if llm_config.model == "auto" {
            llm_config.model = OllamaConfig::detect_model(&llm_config.host, llm_config.port).await;
        }
        let llm = OllamaClient::new(llm_config);
        
        // Check Ollama availability
        let ollama_ready = llm.check_available().await;
        if !ollama_ready {
            warn!("Ollama LLM not detected. Council and CASS will be limited.");
        }
        
        // Initialize runtime services
        let services = Self::initialize_services(&world).await?;
        
        // Initialize tool registry
        let tool_registry = ToolRegistry::with_default_tools();
        info!("Loaded {} tools", tool_registry.list_tools().len());

        // Initialize council factory with automatic mode selection
        let council_factory = CouncilFactory::new(ModeConfig::default());
        info!("Council factory initialized with automatic mode selection (Debate/Orchestrate/Simple/LLM)");

        // Initialize RLM LivingPrompt for self-evolving prompt enrichment
        let living_prompt = LivingPrompt::new(
            "You are an HSM-II multi-agent system. Use your tools when the user asks you to perform actions like searching, reading files, running commands, or calculations. Respond with a JSON tool call when appropriate.",
        );
        info!("RLM LivingPrompt initialized for prompt evolution");

        // Load metrics
        let agent_metrics = Self::load_metrics(&base_path).await.unwrap_or_default();

        info!("EnhancedPersonalAgent initialized with {} agents", world.agents.len());

        Ok(Self {
            base_path,
            config,
            world,
            services,
            llm,
            tool_registry,
            council_factory,
            living_prompt,
            chat_history: Vec::new(),
            active_contexts: HashMap::new(),
            agent_metrics,
            last_save: Instant::now(),
            messages_since_reflection: 0,
            gateway_tx: None,
            gateway: None,
        })
    }
    
    /// Create a new world with configured agents
    async fn create_new_world(config: &EnhancedAgentConfig) -> Result<HyperStigmergicMorphogenesis> {
        let mut world = HyperStigmergicMorphogenesis::new(config.agent_count);
        
        // Assign specific roles to agents
        let roles = vec![
            Role::Architect,   // Structure and coherence
            Role::Catalyst,    // Innovation and novelty
            Role::Chronicler,  // Memory and documentation
            Role::Critic,      // Risk assessment
            Role::Explorer,    // Diversity and exploration
        ];
        
        for (i, agent) in world.agents.iter_mut().enumerate() {
            if i < roles.len() {
                agent.role = roles[i].clone();
                agent.description = format!("{:?} specializing in personal assistance", roles[i]);
            }
        }
        
        // Initialize with some default beliefs
        world.beliefs.push(Belief {
            id: 0,
            content: "The user is a human seeking assistance from a multi-agent system".to_string(),
            confidence: 0.9,
            source: BeliefSource::UserProvided,
            supporting_evidence: vec!["Initial setup".to_string()],
            contradicting_evidence: vec![],
            created_at: current_timestamp(),
            updated_at: current_timestamp(),
            update_count: 0,
        });
        
        world.beliefs.push(Belief {
            id: 1,
            content: "Complex decisions benefit from multi-agent council deliberation".to_string(),
            confidence: 0.85,
            source: BeliefSource::Inference,
            supporting_evidence: vec!["System design".to_string()],
            contradicting_evidence: vec![],
            created_at: current_timestamp(),
            updated_at: current_timestamp(),
            update_count: 0,
        });
        
        // Create initial hyperedges for connectivity
        for i in 0..config.agent_count.min(5) {
            let next = (i + 1) % config.agent_count;
            world.edges.push(HyperEdge {
                participants: vec![i as u64, next as u64],
                weight: 1.0,
                emergent: false,
                age: 0,
                tags: HashMap::from([("type".to_string(), "initial".to_string())]),
                created_at: current_timestamp(),
                embedding: None,
                scope: None,
                provenance: None,
                trust_tags: None,
                origin_system: None,
                knowledge_layer: None,
            });
        }
        
        Ok(world)
    }
    
    /// Initialize runtime services
    async fn initialize_services(
        world: &HyperStigmergicMorphogenesis,
    ) -> Result<RuntimeServices> {
        // Initialize DKS
        let dks = DKSSystem::new(DKSConfig::default());
        
        // Initialize CASS with skill bank from world
        let cass = CASS::new(world.skill_bank.clone());
        
        // Initialize embedding engine
        let embedding_engine = EmbeddingEngine::new();
        
        Ok(RuntimeServices {
            dks,
            cass,
            embedding_engine,
            last_dks_tick: None,
            cass_initialized: false,
        })
    }
    
    /// Process incoming message with full HSM-II pipeline
    pub async fn handle_message(&mut self, msg: Message) -> Result<String> {
        let start_time = Instant::now();
        let message_id = msg.id.clone();
        
        // Create message context
        let mut context = MessageContext {
            message_id: message_id.clone(),
            user_id: msg.user_id.clone(),
            content: msg.content.clone(),
            platform: msg.platform,
            assigned_agents: Vec::new(),
            council_used: false,
            skills_accessed: Vec::new(),
            start_time,
            joulework_contributions: HashMap::new(),
        };
        
        // Check for special commands first
        let response = if msg.content.starts_with("/ralph") {
            // Explicit Ralph Loop command
            let task = msg.content.trim_start_matches("/ralph").trim();
            if task.is_empty() {
                AgentResponse {
                    content: "Usage: /ralph <coding task description>".to_string(),
                    primary_agent: 0,
                    council_used: false,
                    confidence: 1.0,
                    skills_used: vec![],
                    joulework_contributions: HashMap::new(),
                    processing_time_ms: 0,
                }
            } else {
                self.process_with_ralph(&msg, &mut context, task).await?
            }
        } else if msg.content.starts_with("/rlm") {
            // Explicit RLM command for large documents
            self.process_with_rlm_command(&msg, &mut context).await?
        } else if msg.content.starts_with("/tool") {
            // Tool execution command
            self.process_tool_command(&msg, &mut context).await?
        } else if self.should_use_ralph(&msg.content) {
            // Auto-detect coding tasks for Ralph Loop
            info!("Auto-detected coding task, using Ralph Loop");
            self.process_with_ralph(&msg, &mut context, &msg.content).await?
        } else if self.should_use_rlm(&msg.content) {
            // Auto-detect large document processing
            info!("Auto-detected document processing, using RLM");
            self.process_with_rlm(&msg, &mut context).await?
        } else if self.should_use_council(&msg.content) && self.config.enable_council {
            // Complex decision - use council
            self.process_with_council(&msg, &mut context).await?
        } else {
            // Simple query - use single agent with CASS skills
            self.process_with_skills(&msg, &mut context).await?
        };
        
        // Track JouleWork contributions
        self.track_contributions(&context, &response).await?;

        // Update world state
        self.world.tick();

        // Run DKS tick if enabled
        if self.config.enable_dks {
            let dks_tick = self.services.dks.tick();
            self.services.last_dks_tick = Some(dks_tick);
        }

        // Update chat history for conversation context (keep last 20 exchanges)
        self.chat_history.push(("user".to_string(), msg.content.clone()));
        self.chat_history.push(("assistant".to_string(), response.content.clone()));
        if self.chat_history.len() > 40 {
            // Trim oldest messages, keep last 40 entries (20 exchanges)
            let drain_count = self.chat_history.len() - 40;
            self.chat_history.drain(..drain_count);
        }

        // Social memory: record promise if agent committed to something
        {
            let now_ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            let lower_resp = response.content.to_lowercase();
            let made_commitment = lower_resp.contains("i'll ") || lower_resp.contains("i will ")
                || lower_resp.contains("let me ") || lower_resp.contains("here's what i'll do");
            if made_commitment {
                let agent_id = response.primary_agent;
                let promise_id = self.world.social_memory.record_promise(
                    agent_id,
                    None, // beneficiary = user (external)
                    &msg.content[..msg.content.len().min(80)],
                    &response.content[..response.content.len().min(120)],
                    DataSensitivity::Public,
                    now_ts,
                    None,
                );
                info!("Social memory: recorded promise {} from agent {}", promise_id, agent_id);
            }

            // Resolve previous promises if response indicates completion
            let completed = lower_resp.contains("done") || lower_resp.contains("completed")
                || lower_resp.contains("here's the result") || lower_resp.contains("finished");
            if completed {
                // Find and resolve pending promises from this agent
                let pending: Vec<String> = self.world.social_memory.promises.iter()
                    .filter(|(_, p)| p.status == crate::social_memory::PromiseStatus::Pending)
                    .map(|(id, _)| id.clone())
                    .collect();
                for pid in pending.iter().take(1) {
                    self.world.social_memory.resolve_promise(
                        pid,
                        crate::social_memory::PromiseStatus::Kept,
                        Some(response.primary_agent),
                        now_ts,
                        Some(response.confidence),
                        Some(true),
                        Some(true),
                        &[],
                    );
                    info!("Social memory: resolved promise {}", pid);
                }
            }
        }

        // CASS: Record experience for skill distillation
        {
            let _coherence = self.world.global_coherence();
            let outcome = if response.confidence > 0.7 {
                ExperienceOutcome::Positive { coherence_delta: response.confidence - 0.5 }
            } else {
                ExperienceOutcome::Negative { coherence_delta: response.confidence - 0.5 }
            };
            let exp = Experience {
                id: self.world.experiences.len(),
                description: msg.content[..msg.content.len().min(200)].to_string(),
                context: format!("council={} skills={:?} confidence={:.2}",
                    response.council_used, response.skills_used, response.confidence),
                outcome,
                timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
                tick: self.world.tick_count,
                embedding: None,
            };
            self.world.experiences.push(exp);

            // Attempt distillation every 10 experiences
            if self.world.experiences.len() % 10 == 0 && self.world.experiences.len() >= 3 {
                let result = self.world.skill_bank.distill_from_experiences(
                    &self.world.experiences,
                    &self.world.improvement_history,
                );
                if result.new_skills > 0 {
                    info!("CASS: Distilled {} new skills from {} experiences",
                        result.new_skills, self.world.experiences.len());
                    self.agent_metrics.skills_distilled += result.new_skills as u64;
                }
            }
        }

        // DKS: Trigger evolution on meaningful interactions (council or high-confidence)
        if self.config.enable_dks && (response.council_used || response.confidence > 0.8) {
            let dks_tick = self.services.dks.tick();
            self.services.last_dks_tick = Some(dks_tick);
            info!("DKS: Event-driven evolution tick (council={}, confidence={:.2})",
                response.council_used, response.confidence);
        }

        // RLM: Feed message into living prompt for context tracking
        self.living_prompt.add_message(crate::rlm::RlmMessage {
            role: "user".to_string(),
            content: msg.content.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        self.living_prompt.add_message(crate::rlm::RlmMessage {
            role: "assistant".to_string(),
            content: response.content.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });

        // RLM: Evolve living prompt based on coherence changes every 5 messages
        self.messages_since_reflection += 1;
        if self.messages_since_reflection >= 5 {
            let coherence_before = self.world.global_coherence();
            // Generate beliefs from recent improvement history
            self.world.generate_beliefs_from_history();
            self.world.decay_beliefs();
            let coherence_after = self.world.global_coherence();
            let summary = format!(
                "Processed {} messages. Confidence: {:.2}. Skills used: {:?}",
                self.messages_since_reflection, response.confidence, response.skills_used
            );
            self.living_prompt.evolve(&summary, coherence_before, coherence_after);
            self.messages_since_reflection = 0;
            info!("RLM: Living prompt evolved (coherence {:.4} → {:.4})", coherence_before, coherence_after);
        }

        // Save if needed
        self.maybe_save().await?;

        // Store context
        self.active_contexts.insert(message_id, context);

        Ok(response.content)
    }
    
    /// Determine if message requires council deliberation
    fn should_use_council(&self, content: &str) -> bool {
        let lower = content.to_lowercase();
        
        // Keywords that suggest complexity
        let complex_keywords = [
            "should i", "decide", "choose between", "compare", "analyze",
            "strategy", "plan", "design", "architecture", "important",
            "consequences", "risk", "complex", "multi-step", "coordinate",
        ];
        
        // Check for complex indicators
        let has_complex_keyword = complex_keywords.iter().any(|kw| lower.contains(kw));
        let is_long = content.len() > 200;
        let has_multiple_questions = content.matches('?').count() > 1;
        
        has_complex_keyword || (is_long && has_multiple_questions)
    }
    
    /// Determine if message should use Ralph Loop (coding tasks)
    fn should_use_ralph(&self, content: &str) -> bool {
        let lower = content.to_lowercase();

        // Keywords that suggest coding/development tasks
        let coding_keywords = [
            "code", "program", "function", "implement", "refactor",
            "bug", "fix", "debug", "error", "compile", "build",
            "write a script", "create a tool", "develop", "class",
            "rust", "python", "javascript", "typescript", "java",
            "file processing", "parse", "extract", "transform",
        ];
        
        // Check for coding indicators
        let has_coding_keyword = coding_keywords.iter().any(|kw| lower.contains(kw));
        let mentions_file_with_code = lower.contains("file") && (lower.contains("code") || lower.contains("script"));
        let asks_for_implementation = lower.contains("implement") || lower.contains("write") || lower.contains("create");
        
        // Ralph is best for implementation tasks that may need iteration
        (has_coding_keyword && asks_for_implementation) || mentions_file_with_code
    }
    
    /// Determine if message should use RLM (large document processing)
    fn should_use_rlm(&self, content: &str) -> bool {
        let lower = content.to_lowercase();
        
        // Keywords that suggest large document processing
        let document_keywords = [
            "summarize", "analyze document", "process file",
            "read file", "extract from", "scan document",
            "large text", "long document", "multiple files",
            "directory", "folder", "codebase", "repository",
        ];
        
        // File extensions that suggest large documents
        let document_extensions = [".txt", ".md", ".pdf", ".doc", ".csv", ".json", ".xml"];
        
        let has_doc_keyword = document_keywords.iter().any(|kw| lower.contains(kw));
        let mentions_file = document_extensions.iter().any(|ext| lower.contains(ext));
        let mentions_directory = lower.contains("/") || lower.contains("\\") || lower.contains("directory");
        
        has_doc_keyword || mentions_file || mentions_directory
    }
    
    /// Process message using Ralph Loop (worker-reviewer pattern)
    async fn process_with_ralph(
        &mut self,
        msg: &Message,
        context: &mut MessageContext,
        task: &str,
    ) -> Result<AgentResponse> {
        info!("Using Ralph Loop for message: {}", msg.id);
        
        let start = Instant::now();
        
        // Create Ralph Council
        let council_id = uuid::Uuid::new_v4();
        let config = RalphConfig {
            max_iterations: 10,
            state_dir: self.base_path.join("ralph"),
            ..Default::default()
        };
        
        let mut ralph = RalphCouncil::new(council_id, config);
        
        // Run Ralph Loop
        match ralph.execute(task).await {
            Ok((verdict, decision)) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                
                context.council_used = true;
                
                let content = match verdict {
                    RalphVerdict::Ship => {
                        format!(
                            "✅ **Ralph Loop Complete**\n\nTask completed successfully after deliberation.\n\nConfidence: {:.2}",
                            decision.confidence
                        )
                    }
                    RalphVerdict::MaxIterationsReached => {
                        format!(
                            "⚠️ **Ralph Loop Incomplete**\n\nMaximum iterations reached without achieving consensus.\n\nThe task may need refinement or manual intervention."
                        )
                    }
                    RalphVerdict::Blocked { reason } => {
                        format!(
                            "🚫 **Ralph Loop Blocked**\n\nReason: {}\n\nThe worker was unable to proceed. Please provide more specific instructions.",
                            reason
                        )
                    }
                    RalphVerdict::Revise { .. } => {
                        // This shouldn't happen as execute() returns on Ship/MaxIter/Blocked
                        format!(
                            "📝 **Ralph Loop Iterating**\n\nThe task is being refined through iterations.\n\nCheck {} for progress.",
                            ralph.state_dir().display()
                        )
                    }
                };
                
                Ok(AgentResponse {
                    content,
                    primary_agent: 0,
                    council_used: true,
                    confidence: decision.confidence,
                    skills_used: vec!["ralph_loop".to_string()],
                    joulework_contributions: context.joulework_contributions.clone(),
                    processing_time_ms: duration_ms,
                })
            }
            Err(e) => {
                warn!("Ralph Loop failed: {}", e);
                Ok(AgentResponse {
                    content: format!("❌ Ralph Loop failed: {}", e),
                    primary_agent: 0,
                    council_used: false,
                    confidence: 0.0,
                    skills_used: vec![],
                    joulework_contributions: HashMap::new(),
                    processing_time_ms: start.elapsed().as_millis() as u64,
                })
            }
        }
    }
    
    /// Process message using RLM for large documents
    async fn process_with_rlm(
        &mut self,
        msg: &Message,
        _context: &mut MessageContext,
    ) -> Result<AgentResponse> {
        info!("Using RLM for message: {}", msg.id);
        
        let start = Instant::now();
        
        // Extract file path from message
        let file_path = self.extract_file_path(&msg.content);
        
        if file_path.is_none() {
            return Ok(AgentResponse {
                content: "📄 **RLM Document Processing**\n\nPlease specify a file path.\n\nExample: `Process this file: ./documents/report.txt`".to_string(),
                primary_agent: 0,
                council_used: false,
                confidence: 0.0,
                skills_used: vec![],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            });
        }
        
        let file_path = file_path.unwrap();
        let query = self.extract_query(&msg.content);
        
        // Use RLM tool
        let tool = RlmProcessTool::new();
        let params = json!({
            "source": file_path,
            "query": query,
            "source_type": "file"
        });
        
        let output = tool.execute(params).await;
        
        let content = if output.success {
            format!(
                "📊 **RLM Analysis Complete**\n\n{}",
                output.result
            )
        } else {
            format!(
                "❌ **RLM Processing Failed**\n\nError: {}",
                output.error.unwrap_or_else(|| "Unknown error".to_string())
            )
        };
        
        Ok(AgentResponse {
            content,
            primary_agent: 0,
            council_used: false,
            confidence: if output.success { 0.8 } else { 0.0 },
            skills_used: vec!["rlm_process".to_string()],
            joulework_contributions: HashMap::new(),
            processing_time_ms: start.elapsed().as_millis() as u64,
        })
    }
    
    /// Process RLM command with explicit arguments
    async fn process_with_rlm_command(
        &mut self,
        msg: &Message,
        _context: &mut MessageContext,
    ) -> Result<AgentResponse> {
        let args = msg.content.trim_start_matches("/rlm").trim();
        
        if args.is_empty() {
            return Ok(AgentResponse {
                content: "Usage: /rlm <file_path> [query]\n\nExample: `/rlm ./document.txt summarize this`".to_string(),
                primary_agent: 0,
                council_used: false,
                confidence: 1.0,
                skills_used: vec![],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            });
        }
        
        // Parse args: first token is path, rest is query
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        let file_path = parts[0];
        let query = parts.get(1).map(|s| s.to_string()).unwrap_or_else(|| "Analyze this document".to_string());
        
        let start = Instant::now();
        
        let tool = RlmProcessTool::new();
        let params = json!({
            "source": file_path,
            "query": query,
            "source_type": "file"
        });
        
        let output = tool.execute(params).await;
        
        let content = if output.success {
            output.result
        } else {
            format!("Error: {}", output.error.unwrap_or_else(|| "Unknown error".to_string()))
        };
        
        Ok(AgentResponse {
            content,
            primary_agent: 0,
            council_used: false,
            confidence: if output.success { 0.8 } else { 0.0 },
            skills_used: vec!["rlm_process".to_string()],
            joulework_contributions: HashMap::new(),
            processing_time_ms: start.elapsed().as_millis() as u64,
        })
    }
    
    /// Process tool command
    async fn process_tool_command(
        &mut self,
        msg: &Message,
        _context: &mut MessageContext,
    ) -> Result<AgentResponse> {
        let args = msg.content.trim_start_matches("/tool").trim();
        
        if args.is_empty() {
            // List available tools
            let tools = self.tool_registry.list_tools();
            let tool_list: Vec<String> = tools.iter()
                .map(|(name, desc)| format!("- `{}`: {}", name, desc))
                .collect();
            
            return Ok(AgentResponse {
                content: format!(
                    "🔧 **Available Tools** ({} total)\n\n{}\n\nUsage: `/tool <tool_name> <parameters>`",
                    tools.len(),
                    tool_list.join("\n")
                ),
                primary_agent: 0,
                council_used: false,
                confidence: 1.0,
                skills_used: vec![],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            });
        }
        
        // Parse tool name and parameters
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        let tool_name = parts[0];
        let params_str = parts.get(1).unwrap_or(&"{}");
        
        // Try to parse parameters as JSON
        let params = serde_json::from_str(params_str).unwrap_or_else(|_| {
            // If not valid JSON, use as single parameter
            json!({ "input": params_str })
        });
        
        let start = Instant::now();
        
        // Execute tool
        let output = if let Some(tool) = self.tool_registry.get(tool_name) {
            tool.execute(params).await
        } else {
            return Ok(AgentResponse {
                content: format!("❌ Unknown tool: `{}`\n\nUse `/tool` to list available tools.", tool_name),
                primary_agent: 0,
                council_used: false,
                confidence: 0.0,
                skills_used: vec![],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            });
        };
        
        let content = if output.success {
            format!("✅ **Tool Result: {}**\n\n{}", tool_name, output.result)
        } else {
            format!("❌ **Tool Error: {}**\n\n{}", tool_name, output.error.unwrap_or_else(|| "Unknown error".to_string()))
        };
        
        Ok(AgentResponse {
            content,
            primary_agent: 0,
            council_used: false,
            confidence: if output.success { 0.9 } else { 0.0 },
            skills_used: vec![tool_name.to_string()],
            joulework_contributions: HashMap::new(),
            processing_time_ms: start.elapsed().as_millis() as u64,
        })
    }
    
    /// Extract file path from message content
    fn extract_file_path(&self, content: &str) -> Option<String> {
        // Look for common file path patterns
        let patterns = [
            r"(?:file|path|document)[\s:]+([\w./\\~]+\.\w+)",
            r"(?:from|in|at)[\s:]+([\w./\\~]+\.\w+)",
        ];
        
        for pattern in &patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if let Some(caps) = re.captures(content) {
                    if let Some(path) = caps.get(1) {
                        let path_str = path.as_str();
                        // Verify it looks like a file path
                        if path_str.contains('.') || path_str.starts_with('/') || path_str.starts_with("./") {
                            return Some(path_str.to_string());
                        }
                    }
                }
            }
        }
        
        // Fallback: look for words that look like paths
        for word in content.split_whitespace() {
            if (word.contains('/') || word.contains("\\")) && word.contains('.') {
                return Some(word.to_string());
            }
        }
        
        None
    }
    
    /// Extract query from message content
    fn extract_query(&self, content: &str) -> String {
        // Remove file paths and common filler words
        let without_paths: String = content
            .split_whitespace()
            .filter(|w| !(w.contains('/') || w.contains("\\")))
            .collect::<Vec<_>>()
            .join(" ");
        
        // Extract action words
        let actions = ["summarize", "analyze", "extract", "find", "process", "read"];
        let lower = without_paths.to_lowercase();
        
        for action in &actions {
            if lower.contains(action) {
                // Return from the action word onwards
                if let Some(pos) = lower.find(action) {
                    return without_paths[pos..].to_string();
                }
            }
        }
        
        // Default query
        "Analyze this document".to_string()
    }
    
    /// Process message using council deliberation with automatic mode selection
    async fn process_with_council(
        &mut self,
        msg: &Message,
        context: &mut MessageContext,
    ) -> Result<AgentResponse> {
        info!("Using council deliberation for message: {}", msg.id);

        // Create members from world agents
        let members: Vec<CouncilMember> = self.world.agents.iter()
            .take(5)
            .map(|a| CouncilMember {
                agent_id: a.id,
                role: a.role.clone(),
                expertise_score: a.calculate_jw(self.world.global_coherence(), 3),
                participation_weight: 1.0,
            })
            .collect();

        // Create proposal with complexity estimation
        let mut proposal = Proposal::new(
            &format!("msg-{}", msg.id),
            "User Query",
            &msg.content,
            0, // System agent
        );
        proposal.estimate_complexity(); // Auto-estimate complexity from content

        // Add stigmergic context
        let stig_ctx = StigmergicCouncilContext {
            preferred_agent: None,
            preferred_tool: None,
            confidence: 0.7,
            require_council_review: true,
            rationale: "Complex query requiring multi-agent deliberation".to_string(),
            evidence: vec![CouncilEvidence {
                id: format!("ev-{}", msg.id),
                kind: CouncilEvidenceKind::Trace,
                summary: "User query".to_string(),
            }],
            graph_snapshot_bullets: vec![],
            graph_queries: vec![],
        };
        proposal.stigmergic_context = Some(stig_ctx);

        // Use CouncilFactory for automatic mode selection (Debate/Orchestrate/Simple/LLM)
        // instead of hardcoding CouncilMode::Debate
        let mut council = self.council_factory.create_council(&proposal, members)?;
        info!("Council mode selected: {:?} (complexity={:.2}, urgency={:.2})",
              council.mode, proposal.complexity, proposal.urgency);

        // Run evaluation
        let start = Instant::now();
        let decision = council.evaluate().await?;
        let deliberation_time = start.elapsed().as_millis() as u64;

        // Record council usage
        context.council_used = true;
        self.agent_metrics.council_invocations += 1;

        // Track agent contributions
        for member in &council.members {
            let jw = self.world.agents.get(member.agent_id as usize)
                .map(|a| a.calculate_jw(self.world.global_coherence(), 3))
                .unwrap_or(0.5);
            context.joulework_contributions.insert(member.agent_id, jw);
        }

        // Generate response: use LLM to synthesize council decision into human-readable answer
        let decision_text = match &decision.decision {
            Decision::Approve => "Approved".to_string(),
            Decision::Reject => "Rejected".to_string(),
            Decision::Amend { .. } => "Amended".to_string(),
            Decision::Defer { reason } => format!("Deferred: {}", reason),
        };

        // Build enriched prompt with RLM LivingPrompt context
        let enriched_prompt = self.living_prompt.render();
        let council_prompt = format!(
            "{}\n\n## Council Decision\nMode: {:?}\nOutcome: {}\nConfidence: {:.2}\nAgents: {:?}\n\nBased on the council's deliberation above, provide a helpful response to the user's query:\n{}",
            enriched_prompt,
            decision.mode_used,
            decision_text,
            decision.confidence,
            decision.participating_agents,
            msg.content
        );

        // Use LLM to generate a natural response based on council decision
        let llm_result = self.llm.generate(&council_prompt).await;
        let content = if llm_result.timed_out || llm_result.text.is_empty() {
            // Fallback to structured council output
            format!(
                "[Council {:?} → {} | Confidence: {:.2}]\nAgents: {:?}",
                decision.mode_used,
                decision_text,
                decision.confidence,
                decision.participating_agents
            )
        } else {
            Self::clean_response(&llm_result.text)
        };

        // Get primary agent from participating agents
        let primary_agent = decision.participating_agents.first().copied().unwrap_or(0);

        Ok(AgentResponse {
            content,
            primary_agent,
            council_used: true,
            confidence: decision.confidence,
            skills_used: vec![],
            joulework_contributions: context.joulework_contributions.clone(),
            processing_time_ms: deliberation_time,
        })
    }
    
    /// Process message using CASS skills, tool execution, and single agent
    async fn process_with_skills(
        &mut self,
        msg: &Message,
        context: &mut MessageContext,
    ) -> Result<AgentResponse> {
        // Initialize CASS if needed
        if !self.services.cass_initialized {
            match self.services.cass.initialize().await {
                Ok(_) => {
                    self.services.cass_initialized = true;
                    info!("CASS initialized");
                }
                Err(e) => {
                    warn!("CASS initialization failed: {}", e);
                }
            }
        }

        // Get relevant beliefs from world
        let beliefs = self.get_relevant_beliefs(&msg.content).await?;

        // Create context snapshot
        let context_snapshot = ContextSnapshot {
            timestamp: current_timestamp(),
            active_agents: self.world.agents.iter().map(|a| a.id).collect(),
            dominant_roles: self.world.agents.iter().take(3).map(|a| a.role.clone()).collect(),
            current_goals: vec![msg.content.clone()],
            recent_skills_used: vec![],
            system_load: 0.5,
            error_rate: 0.0,
            coherence_score: self.world.global_coherence(),
        };

        self.services.cass.update_context(context_snapshot.clone());

        // Search for skills
        let skill_matches = self.services.cass.search(&msg.content, Some(context_snapshot), 3).await;

        // Select agent based on message type
        let selected_agent = self.select_agent_for_message(&msg.content);
        context.assigned_agents.push(selected_agent);

        // Build enriched system prompt with RLM LivingPrompt + tool schemas
        let enriched_prompt = self.living_prompt.render();
        let tools_description = self.tool_registry.list_tools()
            .iter()
            .map(|(name, desc)| format!("- {}: {}", name, desc))
            .collect::<Vec<_>>()
            .join("\n");

        let belief_context = if !beliefs.is_empty() {
            let belief_strs: Vec<String> = beliefs.iter()
                .take(3)
                .map(|b| format!("- [{:.0}%] {}", b.confidence * 100.0, b.content))
                .collect();
            format!("\n\n## Relevant Beliefs\n{}", belief_strs.join("\n"))
        } else {
            String::new()
        };

        let skill_context = if !skill_matches.is_empty() {
            let best = &skill_matches[0];
            context.skills_accessed.push(best.skill.id.clone());
            format!("\n\n## Matched Skill: {} (score: {:.2})\n{}", best.skill.title, best.semantic_score, best.skill.principle)
        } else {
            String::new()
        };

        let system_prompt = format!(
            "{enriched_prompt}{belief_context}{skill_context}\n\n\
             ## Available Tools\n{tools_description}\n\n\
             ## Tool Usage\n\
             When the user asks you to perform an action (search, read files, run commands, calculate, etc.), \
             respond with a JSON tool call:\n\
             {{\"tool\": \"tool_name\", \"parameters\": {{\"param\": \"value\"}}}}\n\n\
             If no tool is needed, respond normally with helpful text."
        );

        // Generate response with tool-aware prompt
        let start = Instant::now();
        let llm_result = self.llm.chat(&system_prompt, &msg.content, &self.chat_history).await;

        let mut final_content = if llm_result.timed_out || llm_result.text.is_empty() {
            // LLM unavailable fallback
            format!("I'd like to help with '{}', but the LLM is currently unavailable.", msg.content)
        } else {
            Self::clean_response(&llm_result.text)
        };

        // Tool execution loop: parse tool calls from LLM response and execute them
        let mut tool_used = false;
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&final_content) {
            if let Some(tool_name) = json.get("tool").and_then(|v| v.as_str()) {
                if let Some(params) = json.get("parameters") {
                    if self.tool_registry.has(tool_name) {
                        info!("Tool call detected: {} with params: {:?}", tool_name, params);
                        let call = ToolCallEntry {
                            name: tool_name.to_string(),
                            parameters: params.clone(),
                            call_id: uuid::Uuid::new_v4().to_string(),
                        };

                        let result = self.tool_registry.execute(call).await;
                        tool_used = true;

                        let tool_output = if result.output.success {
                            result.output.result.clone()
                        } else {
                            result.output.error.clone().unwrap_or_else(|| "Tool execution failed".to_string())
                        };

                        info!("Tool {} executed: success={}", tool_name, result.output.success);

                        // Feed tool result back to LLM for synthesis
                        let synthesis_prompt = format!(
                            "You executed the tool '{}' and got this result:\n\n{}\n\n\
                             Now provide a helpful, concise response to the user based on this result.",
                            tool_name, tool_output
                        );
                        let synthesis = self.llm.chat(&system_prompt, &synthesis_prompt, &self.chat_history).await;
                        final_content = if synthesis.timed_out || synthesis.text.is_empty() {
                            // Fallback: return raw tool output
                            format!("[Tool: {}]\n{}", tool_name, tool_output)
                        } else {
                            Self::clean_response(&synthesis.text)
                        };
                    } else {
                        warn!("LLM requested unknown tool: {}", tool_name);
                    }
                }
            }
        }

        // Track tool execution contribution type
        if tool_used {
            if let Some(agent) = self.world.agents.get(selected_agent as usize) {
                let jw = agent.calculate_jw(self.world.global_coherence(), 3);
                context.joulework_contributions.insert(selected_agent, jw);
            }
        }

        let processing_time = start.elapsed().as_millis() as u64;

        // Calculate JouleWork for this agent
        let _jw = if let Some(agent) = self.world.agents.get(selected_agent as usize) {
            let jw = agent.calculate_jw(self.world.global_coherence(), 3);
            context.joulework_contributions.insert(selected_agent, jw);
            jw
        } else {
            0.5
        };

        Ok(AgentResponse {
            content: final_content,
            primary_agent: selected_agent,
            council_used: false,
            confidence: if !skill_matches.is_empty() { skill_matches[0].semantic_score } else { 0.6 },
            skills_used: skill_matches.into_iter().take(1).map(|s| s.skill.id).collect(),
            joulework_contributions: context.joulework_contributions.clone(),
            processing_time_ms: processing_time,
        })
    }
    
    /// Select best agent for a message based on content
    fn select_agent_for_message(&self, content: &str) -> AgentId {
        let lower = content.to_lowercase();
        
        // Map content to roles
        let target_role = if lower.contains("code") || lower.contains("program") {
            Role::Coder
        } else if lower.contains("document") || lower.contains("remember") {
            Role::Chronicler
        } else if lower.contains("risk") || lower.contains("safe") || lower.contains("check") {
            Role::Critic
        } else if lower.contains("explore") || lower.contains("discover") || lower.contains("find") {
            Role::Explorer
        } else if lower.contains("structure") || lower.contains("organize") || lower.contains("plan") {
            Role::Architect
        } else {
            Role::Catalyst // Default - handles general queries
        };
        
        // Find agent with matching role and highest JW
        self.world.agents.iter()
            .filter(|a| a.role == target_role)
            .max_by(|a, b| {
                let jwa = a.calculate_jw(self.world.global_coherence(), 3);
                let jwb = b.calculate_jw(self.world.global_coherence(), 3);
                jwa.partial_cmp(&jwb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|a| a.id)
            .unwrap_or(0)
    }
    
    /// Strip leaked chat template tokens from LLM output
    fn clean_response(text: &str) -> String {
        let mut cleaned = text.to_string();
        for token in &[
            "<|end_header_id|>", "<|start_header_id|>", "<|eot_id|>",
            "<|begin_of_text|>", "<|end_of_text|>", "<|finetune_right_pad_id|>",
        ] {
            cleaned = cleaned.replace(token, "");
        }
        while let Some(start) = cleaned.find("<|") {
            if let Some(end) = cleaned[start..].find("|>") {
                cleaned.replace_range(start..start + end + 2, "");
            } else {
                break;
            }
        }
        cleaned.trim().to_string()
    }

    /// Get relevant beliefs from world based on content (public for CLI access)
    pub async fn get_relevant_beliefs(&self, content: &str) -> Result<Vec<crate::hyper_stigmergy::Belief>> {
        // Simple keyword matching - could be enhanced with embeddings
        let content_lower = content.to_lowercase();
        let words: Vec<&str> = content_lower.split_whitespace().collect();
        
        let relevant: Vec<_> = self.world.beliefs.iter()
            .filter(|b| {
                let belief_lower = b.content.to_lowercase();
                let belief_words: Vec<&str> = belief_lower.split_whitespace().collect();
                words.iter().any(|w| belief_words.contains(w))
            })
            .cloned()
            .take(5)
            .collect();
        
        Ok(relevant)
    }
    
    /// Track JouleWork contributions
    async fn track_contributions(
        &mut self,
        context: &MessageContext,
        response: &AgentResponse,
    ) -> Result<()> {
        if !self.config.track_joulework {
            return Ok(());
        }
        
        for (agent_id, jw_score) in &response.joulework_contributions {
            if let Some(agent) = self.world.agents.get(*agent_id as usize) {
                let record = JouleWorkRecord {
                    timestamp: current_timestamp(),
                    message_id: context.message_id.clone(),
                    agent_id: *agent_id,
                    agent_role: format!("{:?}", agent.role),
                    contribution_type: if context.council_used {
                        ContributionType::CouncilDeliberation
                    } else if !response.skills_used.is_empty() {
                        ContributionType::SkillApplication
                    } else if response.content.contains("[Tool:") {
                        ContributionType::ToolExecution
                    } else {
                        ContributionType::ResponseGeneration
                    },
                    jw_score: *jw_score,
                    coherence_delta: 0.0, // Would track actual coherence change
                    description: format!("Processed message: {}", 
                        context.content.chars().take(50).collect::<String>()),
                };
                
                self.agent_metrics.joulework_history.push(record);
            }
        }
        
        self.agent_metrics.total_messages_processed += 1;
        
        Ok(())
    }
    
    /// Save state to LadybugDB if interval elapsed
    async fn maybe_save(&mut self) -> Result<()> {
        let elapsed = self.last_save.elapsed().as_secs();
        if elapsed >= self.config.save_interval_secs {
            self.save().await?;
            self.last_save = Instant::now();
        }
        Ok(())
    }
    
    /// Start gateway for external communication
    /// Returns a channel receiver for processing messages
    pub async fn start_gateway(&mut self, config: crate::personal::gateway::Config) -> Result<mpsc::Receiver<(Message, oneshot::Sender<String>)>> {
        use crate::personal::gateway::Gateway;
        
        let mut gateway = Gateway::new(config);
        
        // Create channel for message passing (avoids circular reference)
        let (tx, rx) = mpsc::channel::<(Message, oneshot::Sender<String>)>(100);
        
        // Create handler that uses the channel
        let handler = ChannelMessageHandler { sender: tx };
        gateway.on_message(handler);
        
        gateway.start().await?;
        
        // Store gateway to keep it alive (critical fix!)
        self.gateway = Some(gateway);
        
        Ok(rx)
    }

    /// Save full state to LadybugDB
    pub async fn save(&mut self) -> Result<()> {
        info!("Saving world state to LadybugDB...");
        
        // Save world state
        EmbeddedGraphStore::save_world(&self.world, None)?;
        
        // Save metrics
        self.save_metrics().await?;
        
        // Save config
        self.save_config().await?;
        
        info!("Save complete");
        Ok(())
    }
    
    /// Get current world statistics
    pub fn get_stats(&self) -> WorldStats {
        WorldStats {
            agent_count: self.world.agents.len(),
            edge_count: self.world.edges.len(),
            belief_count: self.world.beliefs.len(),
            coherence: self.world.global_coherence(),
            tick_count: self.world.tick_count,
            total_messages: self.agent_metrics.total_messages_processed,
            council_invocations: self.agent_metrics.council_invocations,
        }
    }
    
    /// Ensure directory structure exists
    async fn ensure_structure(base_path: &Path) -> Result<()> {
        tokio::fs::create_dir_all(base_path).await?;
        tokio::fs::create_dir_all(base_path.join("memory")).await?;
        tokio::fs::create_dir_all(base_path.join("metrics")).await?;
        Ok(())
    }
    
    /// Load configuration
    async fn load_config(base_path: &Path) -> Result<EnhancedAgentConfig> {
        let config_path = base_path.join("config.json");
        if config_path.exists() {
            let content = tokio::fs::read_to_string(config_path).await?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(EnhancedAgentConfig::default())
        }
    }
    
    /// Save configuration
    async fn save_config(&self) -> Result<()> {
        let config_path = self.base_path.join("config.json");
        let content = serde_json::to_string_pretty(&self.config)?;
        tokio::fs::write(config_path, content).await?;
        Ok(())
    }
    
    /// Load metrics
    async fn load_metrics(base_path: &Path) -> Result<AgentMetrics> {
        let metrics_path = base_path.join("metrics").join("joulework.json");
        if metrics_path.exists() {
            let content = tokio::fs::read_to_string(metrics_path).await?;
            let history: Vec<JouleWorkRecord> = serde_json::from_str(&content)?;
            Ok(AgentMetrics {
                total_messages_processed: history.len() as u64,
                council_invocations: 0,
                skills_distilled: 0,
                joulework_history: history,
            })
        } else {
            Ok(AgentMetrics::default())
        }
    }
    
    /// Save metrics
    async fn save_metrics(&self) -> Result<()> {
        let metrics_path = self.base_path.join("metrics").join("joulework.json");
        let content = serde_json::to_string_pretty(&self.agent_metrics.joulework_history)?;
        tokio::fs::write(metrics_path, content).await?;
        Ok(())
    }
}

/// World statistics for monitoring
#[derive(Clone, Debug, Serialize)]
pub struct WorldStats {
    pub agent_count: usize,
    pub edge_count: usize,
    pub belief_count: usize,
    pub coherence: f64,
    pub tick_count: u64,
    pub total_messages: u64,
    pub council_invocations: u64,
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Message handler that uses channels to communicate with the agent
/// This avoids circular references between gateway and agent
pub struct ChannelMessageHandler {
    sender: mpsc::Sender<(Message, oneshot::Sender<String>)>,
}

#[async_trait]
impl crate::personal::gateway::MessageHandler for ChannelMessageHandler {
    async fn handle(&self, msg: Message) -> anyhow::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.sender.send((msg, tx)).await?;
        let response = rx.await?;
        Ok(response)
    }
}
