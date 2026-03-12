//! Enhanced Personal Agent - Full HSM-II Integration
//!
//! This transforms the PersonalAgent into a complete HSM-II system with:
//! - HyperStigmergicMorphogenesis (multi-agent world)
//! - LadybugDB persistence (vector + graph storage)
//! - CASS (skill learning)
//! - Council (deliberation for complex decisions)
//! - DKS (distributed knowledge)
//! - JouleWork (thermodynamic compensation)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};
use async_trait::async_trait;

use crate::{
    CASS, Council, CouncilMember, CouncilMode, Proposal,
    DKSSystem, DKSConfig, DKSTickResult, HyperStigmergicMorphogenesis,
    EmbeddedGraphStore, AgentId, Role,
    cass::{ContextSnapshot, embedding::EmbeddingEngine},
    council::{CouncilEvidence, CouncilEvidenceKind, StigmergicCouncilContext, Decision},
    personal::gateway::{Message, Platform},
    tools::ToolRegistry,
    ollama_client::{OllamaClient, OllamaConfig},
};
use crate::hyper_stigmergy::{HyperEdge, Belief, BeliefSource};

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
    /// Recent message history
    pub chat_history: Vec<(String, String)>,
    /// Active message contexts
    pub active_contexts: HashMap<String, MessageContext>,
    /// Agent performance metrics for JouleWork
    pub agent_metrics: AgentMetrics,
    /// Last save timestamp
    pub last_save: Instant,
    /// Gateway message channel
    pub gateway_tx: Option<mpsc::Sender<(Message, oneshot::Sender<String>)>>,
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
        
        // Initialize LLM
        let llm = OllamaClient::new(OllamaConfig::default());
        
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
            chat_history: Vec::new(),
            active_contexts: HashMap::new(),
            agent_metrics,
            last_save: Instant::now(),
            gateway_tx: None,
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
        
        // Step 1: Determine if this needs council deliberation
        let needs_council = self.should_use_council(&msg.content);
        
        let response = if needs_council && self.config.enable_council {
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
    
    /// Process message using council deliberation
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
        
        // Create proposal
        let mut proposal = Proposal::new(
            &format!("msg-{}", msg.id),
            "User Query",
            &msg.content,
            0, // System agent
        );
        
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
        
        // Create and run council
        let mut council = Council::new(CouncilMode::Debate, proposal, members);
        
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
        
        // Generate response based on decision
        let decision_text = match decision.decision {
            Decision::Approve => "Approved".to_string(),
            Decision::Reject => "Rejected".to_string(),
            Decision::Amend { .. } => "Amended".to_string(),
            Decision::Defer { reason } => format!("Deferred: {}", reason),
        };
        
        let content = format!(
            "[Council {} - Confidence: {:.2}]\nAgents: {:?}",
            decision_text,
            decision.confidence,
            decision.participating_agents
        );
        
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
    
    /// Process message using CASS skills and single agent
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
        let _beliefs = self.get_relevant_beliefs(&msg.content).await?;
        
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
        
        // Generate response
        let start = Instant::now();
        let content = if !skill_matches.is_empty() {
            let best_match = &skill_matches[0];
            context.skills_accessed.push(best_match.skill.id.clone());
            format!(
                "[Skill: {} - {:.2}]\n{}",
                best_match.skill.title,
                best_match.semantic_score,
                self.llm.generate(&msg.content).await.text
            )
        } else {
            // Fall back to LLM
            self.llm.generate(&msg.content).await.text
        };
        
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
            content,
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
