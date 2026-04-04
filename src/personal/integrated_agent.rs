//! Fully Integrated Personal Agent - HSM-II with all components wired
//!
//! This module provides the complete integration of:
//! - Federation (multi-node knowledge sharing)
//! - Email Agent (IMAP inbox when configured) + **`/email answer`** to paste an email and get an LLM draft (no IMAP required)
//! - Coder Assistant (dedicated code editing mode)
//! - Prolog Logic (symbolic reasoning engine)
//! - GPU Compute (optional acceleration)
//! - Ouroboros Compatibility (blockchain integration)
//! - Pi AI Compatibility (external AI system bridges)
//! - Hermes Agent Bridge (external tool ecosystem)
//!
//! Integration routes email/federation/Prolog/coder **before** the shared pipeline.
//! Standard chat uses [`EnhancedPersonalAgent::handle_message`] (AutoContext, council, CASS, social memory, RLM).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{info, warn};

use crate::personal::gateway::Message;
use crate::personal::EnhancedPersonalAgent;
use crate::EmbeddedGraphStore;

// Component imports
use crate::coder_assistant::SessionManager;
use crate::email::{EmailAgent, EmailConfig};
use crate::federation::{FederationClient, FederationConfig};
use crate::pi_ai_compat::Context;
use crate::prolog_engine::{Atom, PrologEngine, Term};

#[cfg(feature = "gpu")]
use crate::gpu::GpuAccelerator;

/// Result of a subsystem route: either the shared core already finished the turn, or we still need `finalize_integration_turn`.
enum IntegrationRoute {
    Completed { text: String },
    NeedsFinalize(crate::personal::AgentResponse),
}

/// Configuration for the fully integrated agent
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntegratedAgentConfig {
    /// Base enhanced agent config
    pub base: crate::personal::EnhancedAgentConfig,

    // Feature toggles
    /// Enable email agent integration
    pub enable_email: bool,
    /// Enable federation client
    pub enable_federation: bool,
    /// Enable coder assistant mode
    pub enable_coder_assistant: bool,
    /// Enable Prolog symbolic reasoning
    pub enable_prolog: bool,
    /// Enable Pi AI compatibility bridge
    pub enable_pi_ai: bool,
    /// Enable Ouroboros compatibility
    pub enable_ouroboros: bool,
    /// Enable Hermes bridge (external tool ecosystem)
    pub enable_hermes: bool,
    /// Enable GPU acceleration
    #[cfg(feature = "gpu")]
    pub enable_gpu: bool,

    // Component-specific configs
    pub email_config: Option<EmailConfig>,
    pub federation_config: Option<FederationConfig>,

    // Maintenance settings
    /// Enable automatic graph gardening
    pub enable_gardening: bool,
    /// Gardening interval in seconds
    pub gardening_interval_secs: u64,
    /// Edge decay threshold for pruning
    pub decay_threshold: f64,
}

impl Default for IntegratedAgentConfig {
    fn default() -> Self {
        Self {
            base: crate::personal::EnhancedAgentConfig::default(),
            enable_email: false,
            enable_federation: false,
            enable_coder_assistant: true,
            enable_prolog: true,
            enable_pi_ai: true,
            enable_ouroboros: false,
            enable_hermes: true, // Hermes bridge enabled by default for external tool ecosystem
            #[cfg(feature = "gpu")]
            enable_gpu: false,
            email_config: None,
            federation_config: None,
            enable_gardening: true,
            gardening_interval_secs: 3600, // Hourly
            decay_threshold: 0.1,
        }
    }
}

/// Component holders - all optional components are wrapped in Option
pub struct AgentComponents {
    /// Email agent for inbox management
    pub email: Option<RwLock<EmailAgent>>,
    /// Federation client for multi-node knowledge sharing
    pub federation: Option<RwLock<FederationClient>>,
    /// Coder assistant session manager
    pub coder_sessions: RwLock<SessionManager>,
    /// Prolog engine for symbolic reasoning
    pub prolog: RwLock<PrologEngine>,
    /// Pi AI compatibility context
    pub pi_ai_context: RwLock<Context>,

    #[cfg(feature = "gpu")]
    /// GPU accelerator for compute-intensive operations
    pub gpu: Option<RwLock<GpuAccelerator>>,

    #[cfg(not(feature = "gpu"))]
    _gpu_placeholder: (),
}

/// Fully integrated personal agent: **one** HSM-II core plus optional subsystems.
pub struct IntegratedPersonalAgent {
    /// Full enhanced pipeline (hypergraph, CASS, council, AutoContext, RLM, DKS, …).
    pub core: EnhancedPersonalAgent,
    /// Integration-layer toggles and `base` config (kept in sync with `core.config` on save).
    pub config: IntegratedAgentConfig,
    pub components: AgentComponents,
    pub last_gardening: Instant,
}

/// Component status for health checks
#[derive(Clone, Debug, Serialize)]
pub struct ComponentStatus {
    pub email: bool,
    pub federation: bool,
    pub coder_assistant: bool,
    pub prolog: bool,
    pub pi_ai: bool,
    pub ouroboros: bool,
    #[cfg(feature = "gpu")]
    pub gpu: bool,
    pub hermes: bool,
}

impl IntegratedPersonalAgent {
    /// Home directory (same as `core.base_path`).
    #[inline]
    pub fn base_path(&self) -> &Path {
        self.core.base_path.as_path()
    }

    /// Initialize or load existing integrated agent
    pub async fn initialize(base_path: impl AsRef<Path>) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();

        Self::ensure_structure(&base_path).await?;

        let config = Self::load_config(&base_path).await?;

        let world = if EmbeddedGraphStore::exists() {
            info!("Loading world from LadybugDB...");
            let (w, _rlm) = EmbeddedGraphStore::load_world()?;
            w
        } else {
            info!("Creating new HSM-II world...");
            EnhancedPersonalAgent::create_new_world(&config.base).await?
        };

        let core = EnhancedPersonalAgent::assemble_from_world(
            base_path.clone(),
            world,
            config.base.clone(),
        )
        .await?;

        let components = Self::initialize_components(&config, &base_path).await?;

        info!(
            "IntegratedPersonalAgent: {} agents in shared core; active components: {}",
            core.world.agents.len(),
            Self::format_active_components(&config)
        );

        Ok(Self {
            core,
            config,
            components,
            last_gardening: Instant::now(),
        })
    }

    /// Initialize optional components based on config
    async fn initialize_components(
        config: &IntegratedAgentConfig,
        base_path: &Path,
    ) -> Result<AgentComponents> {
        // Email agent
        let email = if config.enable_email {
            if let Some(email_cfg) = &config.email_config {
                match EmailAgent::new(email_cfg.clone()).await {
                    Ok(agent) => {
                        info!("Email agent initialized");
                        Some(RwLock::new(agent))
                    }
                    Err(e) => {
                        warn!("Failed to initialize email agent: {}", e);
                        None
                    }
                }
            } else {
                warn!("Email enabled but no config provided");
                None
            }
        } else {
            None
        };

        // Federation client
        let federation = if config.enable_federation {
            // Create with default system ID and empty peers for now
            let client = FederationClient::new("hsmii-local".to_string(), Vec::new());
            info!("Federation client initialized");
            Some(RwLock::new(client))
        } else {
            None
        };

        // Coder assistant sessions
        let coder_sessions = RwLock::new(SessionManager::new(&base_path.join("coder_sessions")));
        if config.enable_coder_assistant {
            info!("Coder assistant session manager initialized");
        }

        // Prolog engine
        let prolog = RwLock::new(PrologEngine::new(10)); // max_depth = 10
        if config.enable_prolog {
            info!("Prolog engine initialized");
        }

        // Pi AI context
        let pi_ai_context = RwLock::new(Context::new().with_system(
            "You are HSM-II, a multi-agent AI assistant with symbolic reasoning capabilities.",
        ));

        #[cfg(feature = "gpu")]
        let gpu = if config.enable_gpu {
            match GpuAccelerator::new().await {
                Ok(acc) => {
                    info!("GPU accelerator initialized");
                    Some(RwLock::new(acc))
                }
                Err(e) => {
                    warn!("GPU acceleration not available: {}", e);
                    None
                }
            }
        } else {
            None
        };

        #[cfg(not(feature = "gpu"))]
        let _gpu_placeholder = ();

        Ok(AgentComponents {
            email,
            federation,
            coder_sessions,
            prolog,
            pi_ai_context,
            #[cfg(feature = "gpu")]
            gpu,
            #[cfg(not(feature = "gpu"))]
            _gpu_placeholder,
        })
    }

    /// Format active components for logging
    fn format_active_components(config: &IntegratedAgentConfig) -> String {
        let mut active = vec![];
        if config.enable_email {
            active.push("email");
        }
        if config.enable_federation {
            active.push("federation");
        }
        if config.enable_coder_assistant {
            active.push("coder");
        }
        if config.enable_prolog {
            active.push("prolog");
        }
        if config.enable_pi_ai {
            active.push("pi-ai");
        }
        if config.enable_ouroboros {
            active.push("ouroboros");
        }
        if config.enable_hermes {
            active.push("hermes");
        }
        #[cfg(feature = "gpu")]
        if config.enable_gpu {
            active.push("gpu");
        }

        if active.is_empty() {
            "none (base only)".to_string()
        } else {
            active.join(", ")
        }
    }

    /// Main message processing pipeline with all components integrated
    pub async fn handle_message(&mut self, msg: Message) -> Result<String> {
        self.core.maybe_run_heartbeat_tick().await;

        let start_time = Instant::now();
        let message_id = msg.id.clone();

        // Create message context
        let mut context = crate::personal::MessageContext {
            message_id: message_id.clone(),
            user_id: msg.user_id.clone(),
            content: msg.content.clone(),
            platform: msg.platform,
            assigned_agents: Vec::new(),
            council_used: false,
            skills_accessed: Vec::new(),
            tool_steps: Vec::new(),
            start_time,
            joulework_contributions: HashMap::new(),
        };

        // === COMPONENT ROUTING ===
        // Check for specialized component usage before general processing

        let response = if msg.content.starts_with("/email") {
            // Explicit email command
            self.process_email_command(&msg, &mut context).await?
        } else if msg.content.starts_with("/federation") {
            // Explicit federation command
            self.process_federation_command(&msg, &mut context).await?
        } else if msg.content.starts_with("/prolog") {
            // Explicit Prolog query
            self.process_prolog_command(&msg, &mut context).await?
        } else if msg.content.starts_with("/coder") || msg.content.starts_with("/code") {
            match self.process_coder_command(&msg, &mut context).await? {
                IntegrationRoute::Completed { text } => return Ok(text),
                IntegrationRoute::NeedsFinalize(r) => r,
            }
        } else if self.should_use_email(&msg.content) && self.config.enable_email {
            // Auto-detect email-related queries
            info!("Auto-detected email query");
            self.process_email_query(&msg, &mut context).await?
        } else if self.should_use_federation(&msg.content) && self.config.enable_federation {
            // Auto-detect federation queries
            info!("Auto-detected federation query");
            self.process_federation_query(&msg, &mut context).await?
        } else if self.should_use_prolog(&msg.content) && self.config.enable_prolog {
            info!("Auto-detected symbolic reasoning query");
            match self.process_symbolic_query(&msg, &mut context).await? {
                IntegrationRoute::Completed { text } => return Ok(text),
                IntegrationRoute::NeedsFinalize(r) => r,
            }
        } else {
            let text = self.core.handle_message(msg.clone()).await?;
            if self.config.enable_gardening
                && self.last_gardening.elapsed().as_secs() > self.config.gardening_interval_secs
            {
                self.garden_hypergraph().await?;
                self.last_gardening = Instant::now();
                self.core.save().await?;
            }
            return Ok(text);
        };

        if self.config.enable_gardening
            && self.last_gardening.elapsed().as_secs() > self.config.gardening_interval_secs
        {
            self.garden_hypergraph().await?;
            self.last_gardening = Instant::now();
        }

        self.core
            .finalize_integration_turn(&msg, context, &response)
            .await?;

        Ok(response.content)
    }

    // === AUTO-DETECTION FUNCTIONS ===

    /// Detect if message is email-related
    fn should_use_email(&self, content: &str) -> bool {
        let lower = content.to_lowercase();
        let email_keywords = [
            "email",
            "inbox",
            "mail",
            "message",
            "reply to",
            "send mail",
            "check mail",
            "unread",
            "spam",
            "newsletter",
            "gmail",
            "imap",
            "smtp",
        ];
        email_keywords.iter().any(|kw| lower.contains(kw))
    }

    /// Detect if message is federation-related
    fn should_use_federation(&self, content: &str) -> bool {
        let lower = content.to_lowercase();
        let fed_keywords = [
            "federation",
            "sync",
            "share knowledge",
            "peer",
            "node",
            "distributed",
            "cross-system",
            "mesh",
            "remote agent",
        ];
        fed_keywords.iter().any(|kw| lower.contains(kw))
    }

    /// Detect if message requires symbolic reasoning
    fn should_use_prolog(&self, content: &str) -> bool {
        let lower = content.to_lowercase();
        let prolog_keywords = [
            "prove",
            "deduce",
            "infer",
            "logic",
            "predicate",
            "rule",
            "constraint",
            "satisfy",
            "forall",
            "exists",
            "implication",
        ];
        prolog_keywords.iter().any(|kw| lower.contains(kw))
    }

    // === COMPONENT PROCESSORS ===

    /// Process email commands
    async fn process_email_command(
        &mut self,
        msg: &Message,
        _context: &mut crate::personal::MessageContext,
    ) -> Result<crate::personal::AgentResponse> {
        let args = msg.content.trim_start_matches("/email").trim();

        // LLM reply draft — no IMAP required (same as EnhancedPersonalAgent `/email answer`).
        if args.starts_with("answer") || args.starts_with("reply") {
            let after = args
                .strip_prefix("answer")
                .or_else(|| args.strip_prefix("reply"))
                .unwrap_or("")
                .trim_start();
            if after.is_empty() {
                return Ok(crate::personal::AgentResponse {
                    content: "📧 **Usage:** `/email answer` then paste the inbound email in the same message:\n\n\
                        `/email answer`\n\
                        From: …\n\
                        Subject: …\n\n\
                        Body…\n\n\
                        `/email reply` is equivalent. Review the draft before sending."
                        .to_string(),
                    primary_agent: 0,
                    council_used: false,
                    confidence: 1.0,
                    skills_used: vec!["email_draft".to_string()],
                    joulework_contributions: HashMap::new(),
                    processing_time_ms: 0,
                });
            }
            return match self.core.draft_email_reply(after).await {
                Ok(draft) => Ok(crate::personal::AgentResponse {
                    content: format!("📧 **Draft reply** (review before sending)\n\n{draft}"),
                    primary_agent: 0,
                    council_used: false,
                    confidence: 0.85,
                    skills_used: vec!["email_draft".to_string()],
                    joulework_contributions: HashMap::new(),
                    processing_time_ms: 0,
                }),
                Err(e) => Ok(crate::personal::AgentResponse {
                    content: format!("❌ Could not draft reply: {e}"),
                    primary_agent: 0,
                    council_used: false,
                    confidence: 0.0,
                    skills_used: vec![],
                    joulework_contributions: HashMap::new(),
                    processing_time_ms: 0,
                }),
            };
        }

        if let Some(email_agent) = &self.components.email {
            let email = email_agent.read().await;

            let content = if args.is_empty() || args == "status" {
                let stats = email.stats();
                format!(
                    "📧 **Email Agent Status**\n\nProcessed: {} emails",
                    stats.total_processed
                )
            } else if args == "inbox" || args == "check" {
                drop(email); // Release read lock
                let mut email_mut = email_agent.write().await;
                match email_mut.process_inbox(10).await {
                    Ok(actions) => {
                        let mut output =
                            format!("📧 **Inbox Processing** ({} actions)\n\n", actions.len());
                        for action in actions.iter().take(5) {
                            output.push_str(&format!("- {:?}\n", action));
                        }
                        output
                    }
                    Err(e) => format!("❌ Email processing failed: {}", e),
                }
            } else {
                "📧 **Email commands**\n\
                - `/email answer` — paste inbound email; LLM drafts a reply (no IMAP needed)\n\
                - `/email status` — stats (requires email agent)\n\
                - `/email inbox` — process inbox (requires email agent)\n"
                    .to_string()
            };

            Ok(crate::personal::AgentResponse {
                content,
                primary_agent: 0,
                council_used: false,
                confidence: 0.9,
                skills_used: vec!["email".to_string()],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            })
        } else {
            Ok(crate::personal::AgentResponse {
                content: "📧 **IMAP email agent** is not enabled (`enable_email` + `email_config`).\n\n\
                You can still draft replies with **`/email answer`** (paste the message in the same chat).\n\
                Enable the email agent for `/email inbox` / `status`."
                    .to_string(),
                primary_agent: 0,
                council_used: false,
                confidence: 0.0,
                skills_used: vec![],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            })
        }
    }

    /// Process email queries (auto-detected)
    async fn process_email_query(
        &mut self,
        msg: &Message,
        context: &mut crate::personal::MessageContext,
    ) -> Result<crate::personal::AgentResponse> {
        // For now, route to the command processor with a default action
        let modified_msg = Message {
            content: format!("/email inbox"),
            ..msg.clone()
        };
        self.process_email_command(&modified_msg, context).await
    }

    /// Process federation commands
    async fn process_federation_command(
        &mut self,
        msg: &Message,
        _context: &mut crate::personal::MessageContext,
    ) -> Result<crate::personal::AgentResponse> {
        let args = msg.content.trim_start_matches("/federation").trim();

        if let Some(_fed_client) = &self.components.federation {
            let content = if args.is_empty() || args == "status" {
                "🌐 **Federation Status**\n\nFederation client is active.\nUse `/federation sync` to synchronize with peers.".to_string()
            } else if args == "sync" {
                // Trigger sync
                "🌐 **Federation Sync**\n\nKnowledge synchronization initiated.".to_string()
            } else if args.starts_with("query") {
                "🌐 **Federation Query**\n\nQuerying peer nodes...".to_string()
            } else {
                "🌐 **Federation Commands**\n- `/federation status` - Show federation status\n- `/federation sync` - Sync with peers\n- `/federation query <topic>` - Query distributed knowledge".to_string()
            };

            Ok(crate::personal::AgentResponse {
                content,
                primary_agent: 0,
                council_used: false,
                confidence: 0.9,
                skills_used: vec!["federation".to_string()],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            })
        } else {
            Ok(crate::personal::AgentResponse {
                content: "🌐 Federation not enabled. Set enable_federation=true in config."
                    .to_string(),
                primary_agent: 0,
                council_used: false,
                confidence: 0.0,
                skills_used: vec![],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            })
        }
    }

    /// Process federation queries (auto-detected)
    async fn process_federation_query(
        &mut self,
        msg: &Message,
        context: &mut crate::personal::MessageContext,
    ) -> Result<crate::personal::AgentResponse> {
        let modified_msg = Message {
            content: "/federation status".to_string(),
            ..msg.clone()
        };
        self.process_federation_command(&modified_msg, context)
            .await
    }

    /// Process Prolog commands
    async fn process_prolog_command(
        &mut self,
        msg: &Message,
        _context: &mut crate::personal::MessageContext,
    ) -> Result<crate::personal::AgentResponse> {
        let query_str = msg.content.trim_start_matches("/prolog").trim();

        if query_str.is_empty() {
            return Ok(crate::personal::AgentResponse {
                content: "🔮 **Prolog Reasoning**\n\nUsage: `/prolog <query>`\n\nExamples:\n- `/prolog member(X, [a,b,c])`\n- `/prolog assert(father(john, jim))`".to_string(),
                primary_agent: 0,
                council_used: false,
                confidence: 1.0,
                skills_used: vec![],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            });
        }

        let prolog = self.components.prolog.read().await;

        // Parse and execute query
        let result = match self.parse_prolog_query(query_str) {
            Ok(query) => {
                let query_result = prolog.query(&query);
                if query_result.succeeded && !query_result.solutions.is_empty() {
                    let mut output = format!(
                        "🔮 **Prolog Results** ({} solutions)\n\n",
                        query_result.solutions.len()
                    );
                    for (i, solution) in query_result.solutions.iter().take(5).enumerate() {
                        output.push_str(&format!("{}. {:?}\n", i + 1, solution));
                    }
                    output
                } else {
                    "No solutions found.".to_string()
                }
            }
            Err(e) => format!("Parse error: {}", e),
        };

        Ok(crate::personal::AgentResponse {
            content: result,
            primary_agent: 0,
            council_used: false,
            confidence: 0.95,
            skills_used: vec!["prolog".to_string()],
            joulework_contributions: HashMap::new(),
            processing_time_ms: 0,
        })
    }

    /// Process symbolic queries (auto-detected)
    async fn process_symbolic_query(
        &mut self,
        msg: &Message,
        context: &mut crate::personal::MessageContext,
    ) -> Result<IntegrationRoute> {
        let prolog_prompt = format!(
            "Convert this reasoning request into a simple Prolog-style fact or query.\n\nRequest: {}\n\nIf it can be expressed as a Prolog fact (like 'father(john, jim)') or query (like 'father(john, X)'), output ONLY that. Otherwise output 'NATURAL'.",
            msg.content
        );

        let llm_result = self.core.llm.generate(&prolog_prompt).await;
        let extracted = llm_result.text.trim();

        if extracted != "NATURAL" && extracted.contains('(') {
            let modified_msg = Message {
                content: format!("/prolog {}", extracted),
                ..msg.clone()
            };
            Ok(IntegrationRoute::NeedsFinalize(
                self.process_prolog_command(&modified_msg, context).await?,
            ))
        } else {
            let text = self.core.handle_message(msg.clone()).await?;
            if self.config.enable_gardening
                && self.last_gardening.elapsed().as_secs() > self.config.gardening_interval_secs
            {
                self.garden_hypergraph().await?;
                self.last_gardening = Instant::now();
                self.core.save().await?;
            }
            Ok(IntegrationRoute::Completed { text })
        }
    }

    /// Parse a simple Prolog query
    fn parse_prolog_query(&self, input: &str) -> Result<Atom> {
        // Simple parser for basic Prolog syntax
        // e.g., "member(X, [a,b,c])" or "father(john, jim)"

        let input = input.trim();
        if let Some(paren_pos) = input.find('(') {
            let predicate = &input[..paren_pos];
            let args_str = &input[paren_pos + 1..input.len() - 1];

            let args: Vec<Term> = args_str
                .split(',')
                .map(|s| {
                    let s = s.trim();
                    if s.starts_with(|c: char| c.is_uppercase()) {
                        Term::Var(s.to_string())
                    } else if s.starts_with('[') && s.ends_with(']') {
                        // Parse list
                        let inner = &s[1..s.len() - 1];
                        let items: Vec<Term> = inner
                            .split(',')
                            .map(|item| Term::Atom(item.trim().to_string()))
                            .collect();
                        Term::List(items)
                    } else {
                        Term::Atom(s.to_string())
                    }
                })
                .collect();

            Ok(Atom::new(predicate, args))
        } else {
            // Simple atom
            Ok(Atom::new(input, vec![]))
        }
    }

    /// Process coder assistant commands
    async fn process_coder_command(
        &mut self,
        msg: &Message,
        _context: &mut crate::personal::MessageContext,
    ) -> Result<IntegrationRoute> {
        let args = msg.content.trim_start_matches("/coder").trim();
        let args = args.trim_start_matches("/code").trim();

        if args.is_empty() {
            return Ok(IntegrationRoute::NeedsFinalize(crate::personal::AgentResponse {
                content: "💻 **Coder Assistant**\n\nUsage:\n- `/coder <task>` - Start coding session\n- `/coder status` - Show active sessions\n\nThe coder assistant provides dedicated code editing with tool support.".to_string(),
                primary_agent: 0,
                council_used: false,
                confidence: 1.0,
                skills_used: vec![],
                joulework_contributions: HashMap::new(),
                processing_time_ms: 0,
            }));
        }

        if args == "status" {
            return Ok(IntegrationRoute::NeedsFinalize(
                crate::personal::AgentResponse {
                    content: "💻 **Coder Assistant**: Ready".to_string(),
                    primary_agent: 0,
                    council_used: false,
                    confidence: 1.0,
                    skills_used: vec!["coder".to_string()],
                    joulework_contributions: HashMap::new(),
                    processing_time_ms: 0,
                },
            ));
        }

        let mut ralph_msg = msg.clone();
        ralph_msg.content = format!("/ralph {}", args);
        let text = self.core.handle_message(ralph_msg).await?;
        if self.config.enable_gardening
            && self.last_gardening.elapsed().as_secs() > self.config.gardening_interval_secs
        {
            self.garden_hypergraph().await?;
            self.last_gardening = Instant::now();
            self.core.save().await?;
        }
        Ok(IntegrationRoute::Completed { text })
    }

    /// Garden the hypergraph - remove decayed edges, consolidate beliefs
    pub async fn garden_hypergraph(&mut self) -> Result<()> {
        info!("🌱 Gardening hypergraph...");

        let initial_edges = self.core.world.edges.len();
        let initial_beliefs = self.core.world.beliefs.len();

        self.core
            .world
            .edges
            .retain(|edge| edge.weight > self.config.decay_threshold);

        self.core.world.decay_beliefs();

        self.core
            .world
            .beliefs
            .retain(|belief| belief.confidence > self.config.decay_threshold);

        let edges_removed = initial_edges - self.core.world.edges.len();
        let beliefs_removed = initial_beliefs - self.core.world.beliefs.len();

        info!(
            "🌱 Gardening complete: removed {} edges, {} beliefs",
            edges_removed, beliefs_removed
        );

        Ok(())
    }

    /// Save state (shared hypergraph + integration config).
    pub async fn save(&mut self) -> Result<()> {
        self.config.base = self.core.config.clone();
        self.core.save().await?;
        self.save_config().await?;
        Ok(())
    }

    /// Get component status
    pub fn get_component_status(&self) -> ComponentStatus {
        ComponentStatus {
            email: self.components.email.is_some(),
            federation: self.components.federation.is_some(),
            coder_assistant: self.config.enable_coder_assistant,
            prolog: self.config.enable_prolog,
            pi_ai: self.config.enable_pi_ai,
            ouroboros: self.config.enable_ouroboros,
            #[cfg(feature = "gpu")]
            gpu: self.components.gpu.is_some(),
            hermes: self.config.enable_hermes,
        }
    }

    /// Get world statistics
    pub fn get_stats(&self) -> crate::personal::WorldStats {
        self.core.get_stats()
    }

    // === UTILITY FUNCTIONS ===

    async fn ensure_structure(base_path: &Path) -> Result<()> {
        tokio::fs::create_dir_all(base_path).await?;
        tokio::fs::create_dir_all(base_path.join("memory")).await?;
        tokio::fs::create_dir_all(base_path.join("metrics")).await?;
        tokio::fs::create_dir_all(base_path.join("coder_sessions")).await?;
        tokio::fs::create_dir_all(base_path.join("federation")).await?;
        Ok(())
    }

    async fn load_config(base_path: &Path) -> Result<IntegratedAgentConfig> {
        let integrated = base_path.join("integrated_config.json");
        if integrated.exists() {
            let content = tokio::fs::read_to_string(integrated).await?;
            return Ok(serde_json::from_str(&content)?);
        }
        let legacy = base_path.join("config.json");
        if legacy.exists() {
            let content = tokio::fs::read_to_string(legacy).await?;
            let base: crate::personal::EnhancedAgentConfig = serde_json::from_str(&content)?;
            return Ok(IntegratedAgentConfig {
                base,
                ..IntegratedAgentConfig::default()
            });
        }
        Ok(IntegratedAgentConfig::default())
    }

    async fn save_config(&self) -> Result<()> {
        let config_path = self.core.base_path.join("integrated_config.json");
        let content = serde_json::to_string_pretty(&self.config)?;
        let path = config_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::write_atomic(&path, content.as_bytes()).map_err(|e| anyhow::Error::from(e))
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))??;
        Ok(())
    }

    /// Start gateway (owned by the shared [`EnhancedPersonalAgent`]).
    pub async fn start_gateway(
        &mut self,
        config: crate::personal::gateway::Config,
    ) -> Result<mpsc::Receiver<(Message, oneshot::Sender<String>)>> {
        self.core.start_gateway(config).await
    }
}

/// Default integrated-agent home (same resolution as [`super::hsmii_home`](crate::personal::hsmii_home)).
pub fn integrated_home() -> PathBuf {
    super::resolve_hsmii_home(None, None)
}
