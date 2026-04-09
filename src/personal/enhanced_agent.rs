//! Enhanced Personal Agent - Full HSM-II Integration
//!
//! This transforms the PersonalAgent into a complete HSM-II system with:
//! - HyperStigmergicMorphogenesis (multi-agent world)
//! - LadybugDB persistence (vector + graph storage)
//! - CASS (skill learning)
//! - Council (deliberation for complex decisions) — sub-agent style deliberation
//! - Ralph Loop (iterative coding with worker-reviewer)
//! - RLM (Recursive Language Model for large documents)
//! - DKS (distributed knowledge)
//! - JouleWork (thermodynamic compensation)
//! - Tool Execution (60+ tools: files, shell, browser automation, HTTP, web search, etc. — see `crate::tools`)
//! - AutoContext: persisted playbooks/hints under `<home>/autocontext/` (session-to-session learning)
//! - Hermes-style **MEMORY.md** / **USER.md** / **AGENTS.md** / **prompt.template.md** excerpts injected when present
//! - On-disk **SKILL.md** skills under `<home>/skills` and `HSM_SKILL_EXTERNAL_DIRS` (index in prompt; `skills_list` / `skill_md_read`; `/skills`, `/skill <slug>`)
//! - **MCP tools** on the personal path: same plugin manifests as the coder assistant (`tools/call`, optional `tools/list` via `HSM_PERSONAL_MCP_DISCOVER`)
//! - **autoDream** consolidation (`HSM_AUTODREAM=1`): scheduled rollups under `memory/consolidated/`
//! - Optional **BusinessPack** (`business/pack.yaml`) for domain packs (e.g. building management)
//! - **Phased ops:** message-driven `HEARTBEAT.md` ticks (`HSM_HEARTBEAT_INTERVAL_SECS` / `heartbeat_interval_secs` in config), optional turn journal (`HSM_MEMORY_JOURNAL` / `memory_journal`), optional outbound JSON webhook (`HSM_OUTBOUND_WEBHOOK_URL`) for Zapier/Make/Monday-style inbound hooks

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use hermes_bridge::HermesClient;

use super::business_pack::BusinessPack;
use crate::hyper_stigmergy::{Belief, BeliefSource, Experience, ExperienceOutcome, HyperEdge};
use crate::skill_markdown::SkillMdCatalog;
use crate::{
    autocontext::{AutoContextLoop, AutoContextStore, LoopConfig},
    cass::{embedding::EmbeddingEngine, ContextSnapshot},
    council::{
        CouncilEvidence, CouncilEvidenceKind, CouncilFactory, Decision, ModeConfig, RalphVerdict,
        StigmergicCouncilContext,
    },
    llm::client::resolve_risk_based_model,
    ollama_client::{OllamaClient, OllamaConfig},
    personal::{
        gateway::{Message, Platform},
        prompt_defaults,
    },
    rlm::LivingPrompt,
    social_memory::DataSensitivity,
    tools::scored_tool_router::rank_tools_for_prompt,
    tools::{RlmProcessTool, Tool, ToolCall as ToolCallEntry, ToolRegistry},
    trace2skill::{self, ToolStepRecord, TrajectoryRecord},
    AgentId, CouncilMember, DKSConfig, DKSSystem, DKSTickResult, EmbeddedGraphStore,
    HyperStigmergicMorphogenesis, Proposal, RalphConfig, RalphCouncil, Role, CASS,
};

/// Max bytes of `USER.md` injected per turn.
const MAX_USER_MD_INJECT_BYTES: usize = 8 * 1024;
/// Max bytes of `prompt.template.md` (optional system template).
const MAX_PROMPT_TEMPLATE_MD_INJECT_BYTES: usize = 10 * 1024;

fn max_memory_md_inject_bytes() -> usize {
    std::env::var("HSM_MEMORY_MD_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0 && n <= 512_000)
        .unwrap_or(2560)
}

fn max_agents_md_inject_bytes() -> usize {
    std::env::var("HSM_AGENTS_MD_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0 && n <= 512_000)
        .unwrap_or(24 * 1024)
}

/// Max markdown SKILL.md entries listed by slug in the system prompt (blurbs only).
const MAX_SKILL_MD_PROMPT_ENTRIES: usize = 40;
/// Max characters per skill blurb line in that index.
const MAX_SKILL_MD_LINE_CHARS: usize = 120;
const DEFAULT_TOOL_PROMPT_CAP: usize = 24;

#[derive(Clone, Copy)]
struct RuntimeModelOption {
    slug: &'static str,
    model_id: &'static str,
    provider: &'static str,
    context_window: &'static str,
    pricing: &'static str,
    free_tier: bool,
}

const RUNTIME_MODEL_OPTIONS: &[RuntimeModelOption] = &[
    RuntimeModelOption {
        slug: "llama3.2",
        model_id: "llama3.2",
        provider: "ollama",
        context_window: "128k",
        pricing: "local/private",
        free_tier: true,
    },
    RuntimeModelOption {
        slug: "gpt-4o-mini",
        model_id: "gpt-4o-mini",
        provider: "openai",
        context_window: "128k",
        pricing: "paid",
        free_tier: false,
    },
    RuntimeModelOption {
        slug: "gemini-2.5-flash",
        model_id: "gemini-2.5-flash",
        provider: "gemini",
        context_window: "auto-detected",
        pricing: "paid",
        free_tier: false,
    },
    RuntimeModelOption {
        slug: "mimo-v2-pro",
        model_id: "mimo-v2-pro",
        provider: "xiaomi",
        context_window: "256k",
        pricing: "free-tier auxiliary",
        free_tier: true,
    },
];

fn resolve_runtime_model_alias(alias: &str) -> Option<&'static RuntimeModelOption> {
    let q = alias.trim().to_ascii_lowercase();
    if q.is_empty() {
        return None;
    }
    RUNTIME_MODEL_OPTIONS.iter().find(|m| {
        m.slug.eq_ignore_ascii_case(&q)
            || m.model_id.eq_ignore_ascii_case(&q)
            || m.slug.contains(&q)
            || m.model_id.contains(&q)
    })
}

fn format_runtime_model_list(current: &str, platform: Platform) -> String {
    let mut out = String::from("## Runtime Models\n\n");
    for m in RUNTIME_MODEL_OPTIONS {
        let current_flag = if current.eq_ignore_ascii_case(m.model_id) {
            "▶ "
        } else {
            "  "
        };
        let tier = if m.free_tier { "free" } else { "paid" };
        out.push_str(&format!(
            "{}**{}** (`{}`)\n  - provider: {}\n  - context: {}\n  - tier: {} ({})\n\n",
            current_flag, m.slug, m.model_id, m.provider, m.context_window, tier, m.pricing
        ));
    }
    out.push_str("Use `/model <name>` to switch.\n");
    if matches!(platform, Platform::Telegram | Platform::Discord) {
        out.push_str(
            "\nQuick picks:\n`/model llama3.2`  `/model gpt-4o-mini`  `/model gemini-2.5-flash`  `/model mimo-v2-pro`\n",
        );
    }
    out
}

fn detect_workflow_pack(msg: &str) -> &'static str {
    let m = msg.to_ascii_lowercase();
    if m.contains("email") || m.contains("inbox") || m.contains("reply") {
        return "email_ops";
    }
    if m.contains("ads") || m.contains("campaign") || m.contains("growth") {
        return "growth_campaigns";
    }
    if m.contains("invoice") || m.contains("refund") || m.contains("payment") {
        return "finance_ops";
    }
    if m.contains("support") || m.contains("ticket") {
        return "support_triage";
    }
    "general_ops"
}

fn detect_risk_band(msg: &str) -> &'static str {
    let m = msg.to_ascii_lowercase();
    if m.contains("wire")
        || m.contains("bank")
        || m.contains("refund")
        || m.contains("legal")
        || m.contains("delete")
        || m.contains("credential")
        || m.contains("password")
    {
        return "high";
    }
    if m.contains("billing") || m.contains("payment") || m.contains("escalat") {
        return "medium";
    }
    "low"
}

fn model_routing_enabled() -> bool {
    std::env::var("HSM_MODEL_ROUTING_AUTO")
        .ok()
        .map(|v| {
            let s = v.trim().to_ascii_lowercase();
            s == "1" || s == "true" || s == "yes" || s == "on"
        })
        .unwrap_or(true)
}

fn approx_tokens_from_chars(chars: usize) -> usize {
    chars.div_ceil(4)
}

fn tool_prompt_cap_from_env() -> usize {
    std::env::var("HSM_TOOL_PROMPT_CAP")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_TOOL_PROMPT_CAP)
}

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
    /// When [`BusinessPack`] is loaded, inject this persona key (e.g. `accounting`). Overridden by `HSM_BUSINESS_PERSONA`.
    #[serde(default)]
    pub business_persona: Option<String>,
    /// Run [`crate::personal::heartbeat::Heartbeat`] at most once per this many seconds while messages are processed (message-driven cron). Override with `HSM_HEARTBEAT_INTERVAL_SECS`.
    #[serde(default)]
    pub heartbeat_interval_secs: Option<u64>,
    /// Append each turn to `memory/journal/`. Override with `HSM_MEMORY_JOURNAL=1`.
    #[serde(default)]
    pub memory_journal: bool,
    /// Enable Hermes bridge for real tool execution (web, terminal, browser). Override with `HSM_HERMES=1`.
    #[serde(default)]
    pub enable_hermes: bool,
    /// Hermes Agent endpoint URL. Override with `HSM_HERMES_ENDPOINT`.
    #[serde(default)]
    pub hermes_endpoint: Option<String>,
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
            business_persona: None,
            heartbeat_interval_secs: None,
            memory_journal: false,
            enable_hermes: false,
            hermes_endpoint: None,
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
    /// Tool steps for Trace2Skill export (skills path and similar).
    pub tool_steps: Vec<ToolStepRecord>,
    pub tool_prompt_tokens: usize,
    pub skill_prompt_tokens: usize,
    pub tool_prompt_exposed_count: usize,
    pub tool_prompt_hidden_count: usize,
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
    /// Last time we attempted a disk skill-bank reload (`HSM_SKILL_BANK_RELOAD_SECS`).
    last_skill_bank_reload: Instant,
    /// Gateway message channel
    pub gateway_tx: Option<mpsc::Sender<(Message, oneshot::Sender<String>)>>,
    /// Gateway instance (kept alive to keep bots running)
    pub gateway: Option<crate::personal::gateway::Gateway>,
    /// AutoContext closed-loop learning system
    pub autocontext: AutoContextLoop,
    /// Optional YAML business context (`business/pack.yaml` or `business_pack.yaml`).
    pub business_pack: Option<BusinessPack>,
    /// Raw excerpt from `<home>/MEMORY.md` (editable persistent notes; truncated for prompt budget).
    pub memory_md_excerpt: Option<String>,
    /// Raw excerpt from `<home>/USER.md` (user profile notes; truncated for prompt budget).
    pub user_md_excerpt: Option<String>,
    /// Excerpt from `<home>/AGENTS.md` when present.
    pub agents_md_excerpt: Option<String>,
    /// Excerpt from `<home>/prompt.template.md` when present.
    pub prompt_template_excerpt: Option<String>,
    /// Last time [`Heartbeat`](crate::personal::heartbeat::Heartbeat) tick ran (message-driven scheduling).
    last_heartbeat_tick: Instant,
    /// Last time autoDream consolidation was considered (`HSM_AUTODREAM=1`).
    last_autodream_tick: Instant,
    /// Append-only JSONL: turns, `tool_denied`, optional [`Self::record_hyperedge`] (governed by `HSM_TASK_TRAIL`).
    pub task_trail: super::task_trail::TaskTrail,
    /// Keyword → persona / system template (`config/prompt_routes.yaml`).
    prompt_router: Option<super::agent_memory_pipeline::PromptRouter>,
    /// Hermes-style multi-turn agentic execution enabled (uses existing LLM + tool_registry).
    pub hermes_enabled: bool,
    /// Optional Hermes extension server client (for when the Hermes Agent daemon is running).
    pub hermes_client: Option<HermesClient>,

    // ── Honcho cross-session user inference ───────────────────────────────────
    /// Shared HybridMemory used exclusively by the Honcho inference worker.
    /// Stores `EntitySummary` entries for each peer across all sessions.
    pub honcho_memory: std::sync::Arc<tokio::sync::RwLock<crate::memory::HybridMemory>>,
    /// How many turns have been processed this process lifetime (drives inference cadence).
    honcho_turn_count: u32,
    /// Pre-rendered peer context block injected into the system prompt.
    /// Refreshed at session start and after each inference pass.
    pub honcho_peer_repr: Option<String>,
    /// On-disk SKILL.md catalog (`<home>/skills`, `HSM_SKILL_EXTERNAL_DIRS`); tools `skills_list` / `skill_md_read`.
    pub skill_md_catalog: Arc<RwLock<SkillMdCatalog>>,
    last_skill_md_reload: Instant,
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

        Self::finish_from_world(base_path, world, config).await
    }

    /// Assemble an enhanced agent around an existing world (used by the integration layer).
    pub async fn assemble_from_world(
        base_path: impl AsRef<Path>,
        world: HyperStigmergicMorphogenesis,
        config: EnhancedAgentConfig,
    ) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        Self::ensure_structure(&base_path).await?;
        Self::finish_from_world(base_path, world, config).await
    }

    async fn finish_from_world(
        base_path: PathBuf,
        world: HyperStigmergicMorphogenesis,
        config: EnhancedAgentConfig,
    ) -> Result<Self> {
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

        let task_trail = super::task_trail::TaskTrail::from_home(&base_path);

        // Initialize tool registry with ALL tools (60+)
        let mut tool_registry = ToolRegistry::new();
        tool_registry.set_audit_trail(Some(task_trail.clone()));
        crate::tools::register_all_tools(&mut tool_registry);
        crate::tools::mcp_bridge::register_personal_mcp_tools(&mut tool_registry).await;
        crate::tools::register_personal_ops_tools(&mut tool_registry, &base_path);
        let skill_md_catalog = Arc::new(RwLock::new(SkillMdCatalog::refresh_from_env_home(
            &base_path,
        )));
        crate::tools::register_skill_md_tools(&mut tool_registry, skill_md_catalog.clone());
        info!(
            "Loaded {} tools (native + MCP + operations + skill md)",
            tool_registry.list_tools().len()
        );

        // Initialize council factory with automatic mode selection
        let council_factory = CouncilFactory::new(ModeConfig::default());
        info!("Council factory initialized with automatic mode selection (Debate/Orchestrate/Simple/LLM)");

        // Initialize RLM LivingPrompt for self-evolving prompt enrichment
        let living_prompt = LivingPrompt::new(prompt_defaults::LIVING_PROMPT_SEED);
        info!("RLM LivingPrompt initialized for prompt evolution");

        // Load metrics
        let agent_metrics = Self::load_metrics(&base_path).await.unwrap_or_default();

        // Initialize AutoContext learning loop
        let ac_store = AutoContextStore::new(base_path.join("autocontext"));
        let mut autocontext = AutoContextLoop::new(ac_store, LoopConfig::default());
        if let Err(e) = autocontext.load().await {
            warn!("AutoContext load failed (starting fresh): {}", e);
        }
        info!(
            "AutoContext initialized: {} playbooks, {} hints",
            autocontext.knowledge_base.playbooks.len(),
            autocontext.knowledge_base.hints.len()
        );

        let business_pack = match BusinessPack::try_load(&base_path).await {
            Ok(Some((pack, _))) => {
                info!(
                    target: "hsm_business_pack",
                    company = %pack.company.name,
                    industry = %pack.company.industry,
                    schema_version = pack.schema_version,
                    last_reviewed = ?pack.last_reviewed,
                    persona_keys = ?pack.personas.keys().cloned().collect::<Vec<_>>(),
                    "business pack loaded"
                );
                Some(pack)
            }
            Ok(None) => None,
            Err(e) => {
                warn!(target: "hsm_business_pack", "business pack not loaded: {}", e);
                None
            }
        };

        let memory_md_excerpt =
            Self::load_markdown_excerpt(&base_path.join("MEMORY.md"), max_memory_md_inject_bytes())
                .await;
        let user_md_excerpt =
            Self::load_markdown_excerpt(&base_path.join("USER.md"), MAX_USER_MD_INJECT_BYTES).await;
        let agents_md_excerpt =
            Self::load_markdown_excerpt(&base_path.join("AGENTS.md"), max_agents_md_inject_bytes())
                .await;
        let prompt_template_excerpt = Self::load_markdown_excerpt(
            &base_path.join("prompt.template.md"),
            MAX_PROMPT_TEMPLATE_MD_INJECT_BYTES,
        )
        .await;
        if memory_md_excerpt.is_some()
            || user_md_excerpt.is_some()
            || agents_md_excerpt.is_some()
            || prompt_template_excerpt.is_some()
        {
            info!("Loaded Hermes-style instruction excerpts (MEMORY/USER/AGENTS/prompt.template)");
        }

        let prompt_router = super::agent_memory_pipeline::PromptRouter::try_load(
            &base_path.join("config/prompt_routes.yaml"),
        )
        .await;

        // Hermes-style multi-turn agentic mode — uses existing LLM + tool_registry
        let hermes_enabled = config.enable_hermes
            || std::env::var("HSM_HERMES")
                .map(|v| v.trim() == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

        // Optionally connect to the Hermes extension server for NousResearch tool ecosystem
        let hermes_endpoint = config
            .hermes_endpoint
            .clone()
            .or_else(|| std::env::var("HSM_HERMES_ENDPOINT").ok());
        let hermes_client = if let Some(ref ep) = hermes_endpoint {
            match hermes_bridge::HermesClientBuilder::new()
                .endpoint(ep)
                .build()
            {
                Ok(client) => {
                    info!("Hermes server client ready (endpoint: {})", ep);
                    Some(client)
                }
                Err(e) => {
                    warn!(
                        "Hermes server client init failed: {} — will use native tool loop",
                        e
                    );
                    None
                }
            }
        } else {
            None
        };

        if hermes_enabled {
            if hermes_client.is_some() {
                info!("Hermes agentic mode: server + native fallback (LLM: OpenRouter/Qwen)");
            } else {
                info!("Hermes agentic mode: native tool loop (LLM: OpenRouter/Qwen)");
            }
        }

        info!(
            "EnhancedPersonalAgent initialized with {} agents",
            world.agents.len()
        );

        // ── Honcho: load or create shared HybridMemory ──────────────────────
        let honcho_home = base_path.join("honcho");
        let honcho_hybrid_mem =
            crate::honcho::HonchoInferenceWorker::load_or_create_memory(&honcho_home).await;
        let honcho_memory = std::sync::Arc::new(tokio::sync::RwLock::new(honcho_hybrid_mem));

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
            last_skill_bank_reload: Instant::now(),
            gateway_tx: None,
            gateway: None,
            autocontext,
            business_pack,
            memory_md_excerpt,
            user_md_excerpt,
            agents_md_excerpt,
            prompt_template_excerpt,
            last_heartbeat_tick: Instant::now(),
            last_autodream_tick: Instant::now(),
            task_trail,
            prompt_router,
            hermes_enabled,
            hermes_client,
            honcho_memory,
            honcho_turn_count: 0,
            honcho_peer_repr: None,
            skill_md_catalog,
            last_skill_md_reload: Instant::now(),
        })
    }

    /// Record a multi-participant relation for stigmergy / graph-like audit (`memory/task_trail.jsonl`).
    pub async fn record_hyperedge(
        &self,
        rel: impl Into<String>,
        participants: Vec<String>,
        payload: serde_json::Value,
    ) -> Result<()> {
        self.task_trail
            .append_hyperedge(rel, participants, payload)
            .await
    }

    fn effective_heartbeat_interval_secs(&self) -> Option<u64> {
        std::env::var("HSM_HEARTBEAT_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .or(self.config.heartbeat_interval_secs)
            .filter(|&s| s > 0)
    }

    /// Reload `AGENTS.md`, `prompt.template.md`, `MEMORY.md`, `USER.md` into prompt excerpts (disk → RAM).
    pub async fn reload_instruction_excerpts(&mut self) {
        self.memory_md_excerpt = Self::load_markdown_excerpt(
            &self.base_path.join("MEMORY.md"),
            max_memory_md_inject_bytes(),
        )
        .await;
        self.user_md_excerpt =
            Self::load_markdown_excerpt(&self.base_path.join("USER.md"), MAX_USER_MD_INJECT_BYTES)
                .await;
        self.agents_md_excerpt = Self::load_markdown_excerpt(
            &self.base_path.join("AGENTS.md"),
            max_agents_md_inject_bytes(),
        )
        .await;
        self.prompt_template_excerpt = Self::load_markdown_excerpt(
            &self.base_path.join("prompt.template.md"),
            MAX_PROMPT_TEMPLATE_MD_INJECT_BYTES,
        )
        .await;
        self.refresh_skill_md_catalog_blocking();
    }

    /// Scheduled autoDream consolidation (`memory/consolidated/`) when `HSM_AUTODREAM=1`.
    pub async fn maybe_run_autodream_tick(&mut self) {
        let interval = crate::personal::autodream::effective_interval_secs(
            self.effective_heartbeat_interval_secs(),
        );
        if let Err(e) = crate::personal::autodream::maybe_consolidate(
            &self.base_path,
            &mut self.last_autodream_tick,
            interval,
        )
        .await
        {
            warn!(target: "hsm_autodream", "consolidation failed: {e}");
        }
    }

    /// Message-driven heartbeat: runs `HEARTBEAT.md` checklist / cron / routines when the interval has elapsed.
    pub async fn maybe_run_heartbeat_tick(&mut self) {
        let Some(interval) = self.effective_heartbeat_interval_secs() else {
            return;
        };
        if self.last_heartbeat_tick.elapsed().as_secs() < interval {
            return;
        }
        self.last_heartbeat_tick = Instant::now();
        match crate::personal::heartbeat::Heartbeat::load(&self.base_path).await {
            Ok(mut hb) => match hb.tick(&self.base_path).await {
                Ok(results) => {
                    self.record_ops_heartbeat_state(
                        "ok",
                        Some(format!("{} heartbeat actions", results.len())),
                    );
                    if !results.is_empty() {
                        info!(
                            target: "hsm_heartbeat",
                            n = results.len(),
                            "heartbeat tick completed"
                        );
                    }
                }
                Err(e) => {
                    self.record_ops_heartbeat_state("error", Some(e.to_string()));
                    warn!(target: "hsm_heartbeat", "heartbeat tick failed: {e}");
                }
            },
            Err(e) => {
                self.record_ops_heartbeat_state("error", Some(e.to_string()));
                warn!(target: "hsm_heartbeat", "heartbeat load failed: {e}");
            }
        }
    }

    fn record_ops_heartbeat_state(&self, status: &str, message: Option<String>) {
        let path = crate::personal::resolve_ops_config_path(&self.base_path);
        let Ok(cfg) = crate::personal::load_ops_config(&path).and_then(|cfg| {
            cfg.validate()?;
            Ok(cfg)
        }) else {
            return;
        };
        let _ = crate::personal::ops_config::record_heartbeat_tick(
            &self.base_path,
            &cfg,
            status,
            message,
        );
    }

    fn memory_journal_enabled(&self) -> bool {
        self.config.memory_journal
            || std::env::var("HSM_MEMORY_JOURNAL")
                .map(|v| {
                    let t = v.trim();
                    t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes")
                })
                .unwrap_or(false)
    }

    fn outbound_webhook_url(&self) -> Option<String> {
        let Ok(v) = std::env::var("HSM_OUTBOUND_WEBHOOK_URL") else {
            return None;
        };
        let u = v.trim().to_string();
        if u.is_empty() {
            None
        } else {
            Some(u)
        }
    }

    /// Journal append + optional outbound JSON webhook (background).
    fn post_turn_hooks(&self, msg: &Message, response: &AgentResponse) {
        let journal = self.memory_journal_enabled();
        let url = self.outbound_webhook_url();
        if !journal && url.is_none() {
            return;
        }
        let base = self.base_path.clone();
        let user = msg.content.clone();
        let assistant = response.content.clone();
        tokio::spawn(async move {
            if journal {
                if let Err(e) =
                    crate::personal::memory::append_turn_journal(&base, &user, &assistant).await
                {
                    warn!(target: "hsm_memory_journal", "append failed: {e}");
                }
            }
            if let Some(ref u) = url {
                let payload = serde_json::json!({
                    "source": "hsm-enhanced-agent",
                    "user_preview": user.chars().take(800).collect::<String>(),
                    "assistant_preview": assistant.chars().take(1200).collect::<String>(),
                });
                if let Err(e) = crate::personal::outbound::post_json_webhook(u, &payload).await {
                    warn!(target: "hsm_outbound", "webhook failed: {e}");
                }
            }
        });
    }

    async fn load_markdown_excerpt(path: &Path, max_bytes: usize) -> Option<String> {
        let s = tokio::fs::read_to_string(path).await.ok()?;
        let t = s.trim();
        if t.is_empty() {
            return None;
        }
        let mut out = t.to_string();
        if out.len() > max_bytes {
            let mut end = max_bytes;
            while end > 0 && !out.is_char_boundary(end) {
                end -= 1;
            }
            out.truncate(end);
            out.push_str("\n\n_(truncated to fit prompt budget.)_");
        }
        Some(out)
    }

    /// Markdown from `AGENTS.md` / `USER.md` / `MEMORY.md` at agent home (human-editable, survives restarts).
    ///
    /// Order: **AGENTS** first (stable repo rules), then user profile and Honcho, then a **small** MEMORY excerpt
    /// so long-running notes do not drown out instructions.
    fn persistent_memory_addon(&self) -> String {
        let mut out = String::new();
        if let Some(ref a) = self.agents_md_excerpt {
            out.push_str("\n\n## Agent instructions (AGENTS.md)\n\n");
            out.push_str(a);
            out.push('\n');
        }
        if let Some(ref u) = self.user_md_excerpt {
            out.push_str("\n\n## User profile (USER.md)\n\n");
            out.push_str(u);
            out.push('\n');
        }
        // ── Honcho: inferred cross-session peer representation ─────────────
        if let Some(ref repr) = self.honcho_peer_repr {
            if !repr.is_empty() {
                out.push_str("\n\n");
                out.push_str(repr);
                out.push('\n');
            }
        }
        if let Some(ref m) = self.memory_md_excerpt {
            out.push_str("\n\n## Persistent memory (MEMORY.md)\n\n");
            out.push_str(m);
            out.push('\n');
        }
        if let Some(ref p) = self.prompt_template_excerpt {
            out.push_str("\n\n## Prompt template (prompt.template.md)\n\n");
            out.push_str(p);
            out.push('\n');
        }
        out.push_str(&prompt_defaults::coowner_manager_role_addon());
        out
    }

    /// Markdown block from [`BusinessPack`] + active persona (`HSM_BUSINESS_PERSONA` or config).
    fn business_prompt_addon(&self) -> String {
        self.business_prompt_addon_for_persona(None)
    }

    /// Same as [`Self::business_prompt_addon`], with optional per-turn persona from the prompt router.
    fn business_prompt_addon_for_persona(&self, persona_override: Option<&str>) -> String {
        let persona = persona_override
            .map(|s| s.to_string())
            .or_else(|| {
                std::env::var("HSM_BUSINESS_PERSONA")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                self.config
                    .business_persona
                    .as_ref()
                    .filter(|s| !s.trim().is_empty())
                    .cloned()
            });
        if let Some(ref pack) = self.business_pack {
            let pid = persona.as_deref();
            let files = pack.bound_asset_paths(pid);
            tracing::info!(
                target: "hsm_business_pack",
                active_persona = ?pid,
                bound_file_paths = ?files,
                "injecting business pack into prompt"
            );
            return pack.render_prompt_addon(pid);
        }
        String::new()
    }

    /// Draft an email reply: expands `@/path/to/file` tokens from [`path_attachments`], then runs a short tool loop or a single LLM call.
    ///
    /// Set `HSM_EMAIL_DRAFT_SIMPLE=1` to skip the tool loop (read_eml / Maildir / read_file).
    pub async fn draft_email_reply(&mut self, inbound_raw: &str) -> Result<String> {
        let simple = std::env::var("HSM_EMAIL_DRAFT_SIMPLE")
            .map(|v| {
                let s = v.trim();
                s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(false);
        self.draft_email_reply_with_options(inbound_raw, simple)
            .await
    }

    /// Same as [`Self::draft_email_reply`], but `simple` is explicit (e.g. HTTP API) instead of only `HSM_EMAIL_DRAFT_SIMPLE`.
    pub async fn draft_email_reply_with_options(
        &mut self,
        inbound_raw: &str,
        simple: bool,
    ) -> Result<String> {
        let t = inbound_raw.trim();
        if t.is_empty() {
            anyhow::bail!("empty inbound email");
        }
        let expanded =
            crate::personal::path_attachments::expand_at_paths(t, &self.base_path).await?;

        if simple {
            return self.draft_email_reply_one_shot(&expanded).await;
        }

        self.draft_email_reply_with_tools(&expanded).await
    }

    async fn draft_email_reply_one_shot(&self, inbound: &str) -> Result<String> {
        let system = format!(
            "{}{}{}\n\n## Task\nYou are helping draft an **email reply**.\n\
            - The user pasted an inbound message (optionally with From/Subject headers).\n\
            - `@/path` references were expanded into attached sections if paths existed.\n\
            - Produce a **ready-to-send reply** (email body). Put `Subject: …` as the first line only if a new subject is clearly needed.\n\
            - Match tone to the thread (usually professional and courteous).\n\
            - Do not invent binding commitments, fees, or legal facts; if information is missing, say you will verify or ask one short clarifying question.\n\
            - Apply any **business pack** policies above when relevant.\n\
            - Output plain text suitable for email (no markdown code fences unless quoting code).\n\
            - Do **not** output JSON, code blocks containing only `{{...}}`, or `tool` / `read_file` calls — you cannot run tools here; MEMORY.md context is already in the system prompt when configured.\n",
            self.living_prompt.render(),
            self.persistent_memory_addon(),
            self.business_prompt_addon(),
        );
        let user = format!("--- Inbound message ---\n\n{inbound}\n\n---\nDraft my reply.");
        let llm_result = self.llm.chat(&system, &user, &[]).await;
        if llm_result.timed_out || llm_result.text.is_empty() {
            anyhow::bail!(
                "LLM unavailable or returned empty text (check Ollama / OPENAI / cloud model config)"
            );
        }
        let out = Self::clean_response(&llm_result.text);
        if Self::response_looks_like_tool_json(&out) {
            anyhow::bail!(
                "The model returned a tool-call JSON instead of email text. Turn on **Simple mode** in the console (or set HSM_EMAIL_DRAFT_SIMPLE=1) and try again."
            );
        }
        Ok(out)
    }

    /// Up to 4 turns: model may call `read_eml`, `maildir_list`, `maildir_read`, or `read_file`, then must emit `FINAL_DRAFT`.
    async fn draft_email_reply_with_tools(&mut self, inbound_expanded: &str) -> Result<String> {
        const ALLOWED: &[&str] = &["read_eml", "maildir_list", "maildir_read", "read_file"];
        const MAX_ROUNDS: usize = 4;

        let tools_help = "\n## Optional tools (one JSON object per turn)\n\
            You may read local files before drafting. Respond with exactly one JSON object:\n\
            `{\"tool\":\"read_eml\",\"parameters\":{\"path\":\"/path/to.msg.eml\"}}`\n\
            - **read_eml** — parse `.eml` / RFC822: headers, text body, **paperclip** attachment list (and inline text for small text attachments).\n\
            - **maildir_list** — `{\"tool\":\"maildir_list\",\"parameters\":{\"maildir_root\":\"/path/Maildir\",\"limit\":20}}`\n\
            - **maildir_read** — `{\"tool\":\"maildir_read\",\"parameters\":{\"path\":\"/path/Maildir/cur/…\"}}`\n\
            - **read_file** — `{\"tool\":\"read_file\",\"parameters\":{\"path\":\"…\",\"limit\":400}}`\n\n\
            When ready, output the line `FINAL_DRAFT` on its own line, then the **plain-text** email reply (no JSON).\n";

        let system = format!(
            "{}{}{}\n{}{}",
            self.living_prompt.render(),
            self.persistent_memory_addon(),
            self.business_prompt_addon(),
            tools_help,
            "## Task\nDraft a professional email reply. Do not invent legal/financial facts; use tool output when you read messages or policies.\n"
        );

        let mut user_msg = format!(
            "--- Inbound (paste + any `## Attached file` blocks from @paths) ---\n\n{inbound_expanded}\n\n\
            Either emit one allowed tool JSON ({ALLOWED:?}), or `FINAL_DRAFT` plus the reply body.\n",
            inbound_expanded = inbound_expanded,
            ALLOWED = ALLOWED
        );

        let mut scratch = String::new();
        let mut last_text = String::new();

        for round in 0..MAX_ROUNDS {
            let llm_result = self.llm.chat(&system, &user_msg, &[]).await;
            if llm_result.timed_out || llm_result.text.is_empty() {
                anyhow::bail!("LLM unavailable or returned empty text");
            }
            let text = Self::clean_response(&llm_result.text);
            last_text = text.clone();

            if let Some(idx) = text.find("FINAL_DRAFT") {
                let rest = text[idx + "FINAL_DRAFT".len()..].trim();
                let body = rest.strip_prefix(':').map(str::trim).unwrap_or(rest).trim();
                if !body.is_empty() {
                    return Ok(body.to_string());
                }
            }

            if let Some(json) = Self::extract_tool_call_json(&text) {
                let params = json.get("parameters").or_else(|| json.get("arguments"));
                if let (Some(tool_name), Some(params)) =
                    (json.get("tool").and_then(|v| v.as_str()), params)
                {
                    if ALLOWED.contains(&tool_name) && self.tool_registry.has(tool_name) {
                        let call = ToolCallEntry {
                            name: tool_name.to_string(),
                            parameters: params.clone(),
                            call_id: uuid::Uuid::new_v4().to_string(),
                            harness_run: None,
                            idempotency_key: None,
                        };
                        let result = self.tool_registry.execute(call).await;
                        if result.output.success {
                            crate::tools::web_ingest::ingest_web_tool_success(
                                &mut self.world,
                                tool_name,
                                params,
                                &result.output,
                            );
                        }
                        let tool_out = if result.output.success {
                            result.output.result.clone()
                        } else {
                            result
                                .output
                                .error
                                .clone()
                                .unwrap_or_else(|| "tool failed".into())
                        };
                        scratch
                            .push_str(&format!("\n--- tool `{}` ---\n{}\n", tool_name, tool_out));
                        user_msg = format!(
                            "Accumulated tool output:{scratch}\n\n\
                            Either call **one** more tool as JSON (tools: {:?}), or output `FINAL_DRAFT` and the reply body.\n",
                            ALLOWED
                        );
                        continue;
                    }
                }
            }

            // No tool and no FINAL_DRAFT: treat whole message as draft on last round
            if round + 1 == MAX_ROUNDS {
                if Self::response_looks_like_tool_json(&text) {
                    anyhow::bail!(
                        "Model kept returning tool JSON (e.g. read_file) without a reply. \
                        Many models use `arguments` instead of `parameters` — now supported. \
                        If this persists, use Simple mode (one-shot draft, no tools) or shorten the thread."
                    );
                }
                return Ok(text);
            }

            user_msg = format!(
                "Your last response did not include a valid tool JSON or FINAL_DRAFT. Last output:\n\n{text}\n\n\
                Please either call one tool as JSON (tools: {ALLOWED:?}) or output FINAL_DRAFT then the reply.\n",
                text = text,
                ALLOWED = ALLOWED
            );
        }

        Ok(last_text)
    }

    /// Create a new world with configured agents
    pub(crate) async fn create_new_world(
        config: &EnhancedAgentConfig,
    ) -> Result<HyperStigmergicMorphogenesis> {
        let mut world = HyperStigmergicMorphogenesis::new(config.agent_count);

        // Assign specific roles to agents
        let roles = vec![
            Role::Architect,  // Structure and coherence
            Role::Catalyst,   // Innovation and novelty
            Role::Chronicler, // Memory and documentation
            Role::Critic,     // Risk assessment
            Role::Explorer,   // Diversity and exploration
        ];

        for (i, agent) in world.agents.iter_mut().enumerate() {
            if i < roles.len() {
                agent.role = roles[i].clone();
                agent.description = format!("{:?} specializing in personal assistance", roles[i]);
            }
        }

        // Initialize with some default beliefs
        let content0 = "The user is a human seeking assistance from a multi-agent system";
        let (l0_0, l1_0) = crate::memory::derive_hierarchy(content0);
        world.beliefs.push(Belief {
            id: 0,
            content: content0.to_string(),
            abstract_l0: Some(l0_0),
            overview_l1: Some(l1_0),
            confidence: 0.9,
            source: BeliefSource::UserProvided,
            supporting_evidence: vec!["Initial setup".to_string()],
            contradicting_evidence: vec![],
            created_at: current_timestamp(),
            updated_at: current_timestamp(),
            update_count: 0,
            owner_namespace: None,
            supersedes_belief_id: None,
            evidence_belief_ids: Vec::new(),
            human_committed: false,
        });

        let content1 = "Complex decisions benefit from multi-agent council deliberation";
        let (l0_1, l1_1) = crate::memory::derive_hierarchy(content1);
        world.beliefs.push(Belief {
            id: 1,
            content: content1.to_string(),
            abstract_l0: Some(l0_1),
            overview_l1: Some(l1_1),
            confidence: 0.85,
            source: BeliefSource::Inference,
            supporting_evidence: vec!["System design".to_string()],
            contradicting_evidence: vec![],
            created_at: current_timestamp(),
            updated_at: current_timestamp(),
            update_count: 0,
            owner_namespace: None,
            supersedes_belief_id: None,
            evidence_belief_ids: Vec::new(),
            human_committed: false,
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
                creator: None,
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
    /// Reload `world.skill_bank` from `EmbeddedGraphStore` if `HSM_SKILL_BANK_RELOAD_SECS` elapsed.
    async fn maybe_reload_skill_bank_from_disk(&mut self) {
        let Ok(s) = std::env::var("HSM_SKILL_BANK_RELOAD_SECS") else {
            return;
        };
        let Ok(interval) = s.parse::<u64>() else {
            return;
        };
        if interval == 0 {
            return;
        }
        let period = std::time::Duration::from_secs(interval);
        if self.last_skill_bank_reload.elapsed() < period {
            return;
        }
        self.last_skill_bank_reload = Instant::now();
        if !EmbeddedGraphStore::exists() {
            return;
        }
        match EmbeddedGraphStore::load_skill_bank() {
            Ok(bank) => {
                self.world.skill_bank = bank;
                let mut cass = CASS::new(self.world.skill_bank.clone());
                if let Err(e) = cass.initialize().await {
                    warn!("CASS re-init after skill bank reload failed: {}", e);
                } else {
                    self.services.cass = cass;
                    self.services.cass_initialized = true;
                    info!(
                        "Skill bank reloaded from disk (HSM_SKILL_BANK_RELOAD_SECS={})",
                        interval
                    );
                }
            }
            Err(e) => warn!("Skill bank reload skipped: {}", e),
        }
    }

    /// Rescan `SKILL.md` trees under `<home>/skills` and `HSM_SKILL_EXTERNAL_DIRS`.
    fn refresh_skill_md_catalog_blocking(&mut self) {
        let roots = crate::skill_markdown::collect_skill_roots(&self.base_path);
        if let Ok(mut w) = self.skill_md_catalog.write() {
            *w = SkillMdCatalog::from_roots(&roots);
        }
        self.last_skill_md_reload = Instant::now();
    }

    /// Periodic rescan when `HSM_SKILL_MD_RELOAD_SECS` is set (seconds, 0 = disabled).
    fn maybe_reload_skill_md_catalog(&mut self) {
        let interval = std::env::var("HSM_SKILL_MD_RELOAD_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if interval == 0 {
            return;
        }
        let period = std::time::Duration::from_secs(interval);
        if self.last_skill_md_reload.elapsed() < period {
            return;
        }
        let roots = crate::skill_markdown::collect_skill_roots(&self.base_path);
        if let Ok(mut w) = self.skill_md_catalog.write() {
            *w = SkillMdCatalog::from_roots(&roots);
            let n = w.len();
            if n > 0 {
                info!(
                    target: "hsm_skill_md",
                    count = n,
                    "markdown skills catalog reloaded (HSM_SKILL_MD_RELOAD_SECS={})",
                    interval
                );
            }
        }
        self.last_skill_md_reload = Instant::now();
    }

    async fn initialize_services(world: &HyperStigmergicMorphogenesis) -> Result<RuntimeServices> {
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
        self.maybe_reload_skill_bank_from_disk().await;
        self.maybe_reload_skill_md_catalog();
        self.maybe_run_heartbeat_tick().await;
        self.maybe_run_autodream_tick().await;
        if std::env::var("HSM_INSTR_HOT_RELOAD")
            .map(|v| {
                let s = v.trim();
                s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(false)
        {
            self.reload_instruction_excerpts().await;
        }

        let trimmed_msg = msg.content.trim_start();
        if trimmed_msg == "/skills" || trimmed_msg.starts_with("/skills ") {
            let limit = trimmed_msg
                .strip_prefix("/skills")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse::<usize>().ok());
            let content = match self.skill_md_catalog.read() {
                Ok(cat) => cat.format_list_markdown(limit),
                Err(_) => "Skills catalog is temporarily unavailable.".to_string(),
            };
            return Ok(content);
        }
        if trimmed_msg == "/skill" || trimmed_msg.starts_with("/skill ") {
            let slug = trimmed_msg
                .strip_prefix("/skill")
                .map(str::trim)
                .unwrap_or("")
                .to_string();
            if slug.is_empty() {
                return Ok(
                    "Usage: `/skill <slug>` — load the full SKILL.md body. Use `/skills` to list slugs."
                        .to_string(),
                );
            }
            const MAX: usize = 96 * 1024;
            let content = match self.skill_md_catalog.read() {
                Ok(cat) => match cat.read_body(&slug, MAX) {
                    Ok(body) => format!("# Skill `{slug}`\n\n{body}"),
                    Err(e) => format!("Could not load skill `{slug}`: {e}"),
                },
                Err(_) => "Skills catalog is temporarily unavailable.".to_string(),
            };
            return Ok(content);
        }
        if trimmed_msg == "/model" || trimmed_msg.starts_with("/model ") {
            let arg = trimmed_msg.strip_prefix("/model").map(str::trim).unwrap_or("");
            if arg.is_empty() {
                return Ok(format!(
                    "## Current Runtime Model\n\n`{}`\n\n{}",
                    self.llm.model(),
                    format_runtime_model_list(self.llm.model(), msg.platform)
                ));
            }
            if arg.eq_ignore_ascii_case("list") {
                return Ok(format_runtime_model_list(self.llm.model(), msg.platform));
            }
            if arg.eq_ignore_ascii_case("policy") {
                let current = self.llm.model().to_string();
                let low = resolve_risk_based_model(&current, "general_ops", "low").0;
                let medium = resolve_risk_based_model(&current, "general_ops", "medium").0;
                let high = resolve_risk_based_model(&current, "general_ops", "high").0;
                return Ok(format!(
                    "## Model Routing Policy\n\n- auto: `{}`\n- low risk: `{}`\n- medium risk: `{}`\n- high risk: `{}`",
                    model_routing_enabled(),
                    low,
                    medium,
                    high
                ));
            }
            let Some(opt) = resolve_runtime_model_alias(arg) else {
                return Ok(format!(
                    "Unknown model `{}`.\n\n{}",
                    arg,
                    format_runtime_model_list(self.llm.model(), msg.platform)
                ));
            };
            self.llm.set_model(opt.model_id.to_string());
            if opt.slug == "mimo-v2-pro" {
                return Ok(format!(
                    "Switched model to `{}` (provider: {}).\n\nMiMo v2 Pro is gated as an **auxiliary free-tier** runtime lane.",
                    opt.model_id, opt.provider
                ));
            }
            return Ok(format!(
                "Switched model to `{}` (provider: {}, context: {}).",
                opt.model_id, opt.provider, opt.context_window
            ));
        }
        if model_routing_enabled() {
            let workflow = detect_workflow_pack(trimmed_msg);
            let risk = detect_risk_band(trimmed_msg);
            let current = self.llm.model().to_string();
            let (next_model, _policy_source) = resolve_risk_based_model(&current, workflow, risk);
            if !next_model.eq_ignore_ascii_case(&current) {
                self.llm.set_model(next_model);
            }
        }

        let start_time = Instant::now();
        let message_id = msg.id.clone();
        let mut harness_env = crate::harness::HarnessRunEnvelope::lead_thread(message_id.clone());
        let no_auto_corr = std::env::var("HSM_HARNESS_NO_AUTO_CORRELATION")
            .map(|v| {
                let s = v.trim();
                s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(false);
        if !no_auto_corr {
            harness_env.run.correlation_id = Some(uuid::Uuid::new_v4().to_string());
        }
        if let Ok(c) = std::env::var("HSM_HARNESS_CORRELATION_ID") {
            let t = c.trim();
            if !t.is_empty() {
                harness_env.run.correlation_id = Some(t.to_string());
            }
        }
        self.tool_registry.set_harness_context(Some(harness_env));
        let _harness_turn = crate::harness::HarnessTurnCleanup::new(&mut self.tool_registry);
        if let Err(e) = crate::harness::activate_thread_workspace(&message_id) {
            tracing::warn!(target: "hsm.harness.workspace", "{}", e);
        }

        // Create message context
        let mut context = MessageContext {
            message_id: message_id.clone(),
            user_id: msg.user_id.clone(),
            content: msg.content.clone(),
            platform: msg.platform,
            assigned_agents: Vec::new(),
            council_used: false,
            skills_accessed: Vec::new(),
            tool_steps: Vec::new(),
            tool_prompt_tokens: 0,
            skill_prompt_tokens: 0,
            tool_prompt_exposed_count: 0,
            tool_prompt_hidden_count: 0,
            start_time,
            joulework_contributions: HashMap::new(),
        };

        // Check for special commands first
        let response = if msg.content.trim_start().starts_with("/email answer")
            || msg.content.trim_start().starts_with("/email reply")
        {
            let trimmed = msg.content.trim();
            let rest = trimmed
                .strip_prefix("/email answer")
                .or_else(|| trimmed.strip_prefix("/email reply"))
                .map(str::trim)
                .unwrap_or("");
            if rest.is_empty() {
                AgentResponse {
                    content: "📧 **Usage:** paste the inbound email in the **same message** after the command, e.g.:\n\n\
                        `/email answer`\n\
                        From: someone@example.com\n\
                        Subject: Question about…\n\n\
                        Message body…\n\n\
                        (`/email reply` works the same.) Business pack + MEMORY.md/USER.md are included when configured. Review the draft before sending."
                        .to_string(),
                    primary_agent: 0,
                    council_used: false,
                    confidence: 1.0,
                    skills_used: vec!["email_draft".to_string()],
                    joulework_contributions: HashMap::new(),
                    processing_time_ms: 0,
                }
            } else {
                match self.draft_email_reply(rest).await {
                    Ok(draft) => AgentResponse {
                        content: format!("📧 **Draft reply** (review before sending)\n\n{draft}"),
                        primary_agent: 0,
                        council_used: false,
                        confidence: 0.85,
                        skills_used: vec!["email_draft".to_string()],
                        joulework_contributions: HashMap::new(),
                        processing_time_ms: 0,
                    },
                    Err(e) => AgentResponse {
                        content: format!("❌ Could not draft reply: {e}"),
                        primary_agent: 0,
                        council_used: false,
                        confidence: 0.0,
                        skills_used: vec![],
                        joulework_contributions: HashMap::new(),
                        processing_time_ms: 0,
                    },
                }
            }
        } else if msg.content.starts_with("/ralph") {
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
            self.process_with_ralph(&msg, &mut context, &msg.content)
                .await?
        } else if self.should_use_rlm(&msg.content) {
            // Auto-detect large document processing
            info!("Auto-detected document processing, using RLM");
            self.process_with_rlm(&msg, &mut context).await?
        } else if msg.content.starts_with("/hermes ") || msg.content.trim() == "/hermes" {
            // Explicit Hermes execution command
            let task = msg.content.trim_start_matches("/hermes").trim().to_string();
            if task.is_empty() {
                AgentResponse {
                    content: "Usage: /hermes <task>\n\nRuns the task through the Hermes Agent with real tools (web search, terminal, browser).\nRequires Hermes Agent running at HSM_HERMES_ENDPOINT (default: http://localhost:8000).".to_string(),
                    primary_agent: 0,
                    council_used: false,
                    confidence: 1.0,
                    skills_used: vec![],
                    joulework_contributions: HashMap::new(),
                    processing_time_ms: 0,
                }
            } else {
                self.process_with_hermes(&msg, &mut context, &task).await?
            }
        } else if self.should_use_council(&msg.content) && self.config.enable_council {
            // Complex decision - use council
            self.process_with_council(&msg, &mut context).await?
        } else if self.should_use_hermes(&msg.content) && self.hermes_enabled {
            // Auto-detected real-world task (web, terminal, files) — route to Hermes
            let content = msg.content.clone();
            self.process_with_hermes(&msg, &mut context, &content)
                .await?
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

        if let Err(e) = self
            .task_trail
            .append_turn(
                &message_id,
                &msg.content,
                &response.content,
                &response.skills_used,
                response.council_used,
                context.tool_steps.len(),
                context.tool_prompt_tokens,
                context.skill_prompt_tokens,
                context.tool_prompt_exposed_count,
                context.tool_prompt_hidden_count,
                self.world.edges.len(),
                self.world.beliefs.len(),
            )
            .await
        {
            warn!("task trail append_turn failed: {}", e);
        }

        // Update chat history for conversation context (keep last 20 exchanges)
        self.chat_history
            .push(("user".to_string(), msg.content.clone()));
        self.chat_history
            .push(("assistant".to_string(), response.content.clone()));
        if self.chat_history.len() > 40 {
            // Trim oldest messages, keep last 40 entries (20 exchanges)
            let drain_count = self.chat_history.len() - 40;
            self.chat_history.drain(..drain_count);
        }

        // Post-chat belief extraction: learn business facts from conversation
        // Runs every 3rd message to avoid excessive LLM calls
        if self.messages_since_reflection % 3 == 0 {
            let extracted_count = crate::onboard::post_chat_extract_and_store(
                &self.llm,
                &mut self.world,
                &mut self.living_prompt,
                &msg.content,
                &response.content,
            )
            .await;
            if extracted_count > 0 {
                info!(
                    "Post-chat extraction: learned {} new belief(s)",
                    extracted_count
                );
            }
        }

        // Social memory: record promise if agent committed to something
        {
            let now_ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let lower_resp = response.content.to_lowercase();
            let made_commitment = lower_resp.contains("i'll ")
                || lower_resp.contains("i will ")
                || lower_resp.contains("let me ")
                || lower_resp.contains("here's what i'll do");
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
                info!(
                    "Social memory: recorded promise {} from agent {}",
                    promise_id, agent_id
                );
            }

            // Resolve previous promises if response indicates completion
            let completed = lower_resp.contains("done")
                || lower_resp.contains("completed")
                || lower_resp.contains("here's the result")
                || lower_resp.contains("finished");
            if completed {
                // Find and resolve pending promises from this agent
                let pending: Vec<String> = self
                    .world
                    .social_memory
                    .promises
                    .iter()
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

        // AutoContext: Record interaction and optionally trigger learning loop
        {
            let ac_ctx = self.autocontext.retrieve_context(&msg.content);
            if !ac_ctx.playbooks.is_empty() || !ac_ctx.hints.is_empty() {
                info!(
                    "AutoContext: {} playbooks, {} hints matched for '{}'",
                    ac_ctx.playbooks.len(),
                    ac_ctx.hints.len(),
                    &msg.content[..msg.content.len().min(50)]
                );
            }

            // Feed outcome back to autocontext on every 20th message
            if self.agent_metrics.total_messages_processed > 0
                && self.agent_metrics.total_messages_processed % 20 == 0
            {
                let scenario = msg.content[..msg.content.len().min(100)].to_string();
                match self
                    .autocontext
                    .tick(&scenario, &mut self.tool_registry, &self.llm)
                    .await
                {
                    Ok(result) => {
                        info!(
                            "AutoContext gen #{}: best={:.3}, playbooks_new={}, hints={}",
                            result.generation_id,
                            result.best_score,
                            result.playbooks_created,
                            result.hints_created
                        );
                    }
                    Err(e) => {
                        warn!("AutoContext tick failed: {}", e);
                    }
                }
            }
        }

        // CASS: Record experience for skill distillation
        {
            let _coherence = self.world.global_coherence();
            let outcome = if response.confidence > 0.7 {
                ExperienceOutcome::Positive {
                    coherence_delta: response.confidence - 0.5,
                }
            } else {
                ExperienceOutcome::Negative {
                    coherence_delta: response.confidence - 0.5,
                }
            };
            let desc = msg.content[..msg.content.len().min(200)].to_string();
            let (exp_l0, exp_l1) = crate::memory::derive_hierarchy(&desc);
            let exp = Experience {
                id: self.world.experiences.len(),
                description: desc,
                context: format!(
                    "council={} skills={:?} confidence={:.2}",
                    response.council_used, response.skills_used, response.confidence
                ),
                abstract_l0: Some(exp_l0),
                overview_l1: Some(exp_l1),
                outcome,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
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
                    info!(
                        "CASS: Distilled {} new skills from {} experiences",
                        result.new_skills,
                        self.world.experiences.len()
                    );
                    self.agent_metrics.skills_distilled += result.new_skills as u64;
                }
            }
        }

        // DKS: Trigger evolution on meaningful interactions (council or high-confidence)
        if self.config.enable_dks && (response.council_used || response.confidence > 0.8) {
            let dks_tick = self.services.dks.tick();
            self.services.last_dks_tick = Some(dks_tick);
            info!(
                "DKS: Event-driven evolution tick (council={}, confidence={:.2})",
                response.council_used, response.confidence
            );
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
            self.living_prompt
                .evolve(&summary, coherence_before, coherence_after);
            self.messages_since_reflection = 0;
            info!(
                "RLM: Living prompt evolved (coherence {:.4} → {:.4})",
                coherence_before, coherence_after
            );
        }

        self.post_turn_hooks(&msg, &response);

        if std::env::var("HSM_MEMORY_EXTRACT")
            .map(|v| {
                let s = v.trim();
                s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(false)
        {
            let home = self.base_path.clone();
            let u = msg.content.clone();
            let a = response.content.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    super::agent_memory_pipeline::run_post_turn_extract(&home, &u, &a).await
                {
                    tracing::warn!(target: "hsm_memory_extract", "{}", e);
                }
            });
        }

        // ── Honcho: cross-session user inference (HSM_HONCHO=1) ──────────────
        // Runs an async inference pass over today's journal every N turns,
        // extracting a psychological profile of the user and upserting it into
        // the EntitySummary network of the shared HybridMemory.
        if std::env::var("HSM_HONCHO")
            .map(|v| {
                let s = v.trim();
                s == "1" || s.eq_ignore_ascii_case("true")
            })
            .unwrap_or(false)
        {
            self.honcho_turn_count = self.honcho_turn_count.saturating_add(1);
            let interval: u32 = std::env::var("HSM_HONCHO_INTERVAL")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(10);

            // Load peer representation on the first turn of a session
            if self.honcho_turn_count == 1 || self.honcho_peer_repr.is_none() {
                let worker = crate::honcho::HonchoInferenceWorker::new(
                    &self.base_path,
                    std::sync::Arc::clone(&self.honcho_memory),
                );
                let peer_id = msg.user_id.clone();
                match worker.load_peer_context(&peer_id).await {
                    Ok(repr) => {
                        let block = repr.render_context(4096);
                        self.honcho_peer_repr = if block.is_empty() { None } else { Some(block) };
                    }
                    Err(e) => warn!("honcho: load_peer_context failed: {e}"),
                }
            }

            // Every N turns: fire-and-forget inference pass
            if self.honcho_turn_count % interval == 0 {
                let worker = crate::honcho::HonchoInferenceWorker::new(
                    &self.base_path,
                    std::sync::Arc::clone(&self.honcho_memory),
                );
                let peer_id = msg.user_id.clone();
                worker.spawn_post_session_from_journal(peer_id);
            }
        }

        // Save if needed
        self.maybe_save().await?;

        if let Ok(path) = std::env::var("HSM_TRACE2SKILL_JSONL") {
            let path = path.trim();
            if !path.is_empty() {
                let rec = TrajectoryRecord::from_turn(
                    &msg.content,
                    context.council_used,
                    response.primary_agent,
                    &context.skills_accessed,
                    &response.skills_used,
                    &context.tool_steps,
                    response.confidence,
                    &response.content,
                );
                if let Err(e) = trace2skill::append_jsonl(std::path::Path::new(path), &rec) {
                    warn!("Trace2Skill export failed: {}", e);
                }
            }
        }

        // Store context
        self.active_contexts.insert(message_id, context);

        Ok(response.content)
    }

    /// Determine if message requires council deliberation
    fn should_use_council(&self, content: &str) -> bool {
        let lower = content.to_lowercase();

        // Keywords that suggest complexity
        let complex_keywords = [
            "should i",
            "decide",
            "choose between",
            "compare",
            "analyze",
            "strategy",
            "plan",
            "design",
            "architecture",
            "important",
            "consequences",
            "risk",
            "complex",
            "multi-step",
            "coordinate",
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
            "code",
            "program",
            "function",
            "implement",
            "refactor",
            "bug",
            "fix",
            "debug",
            "error",
            "compile",
            "build",
            "write a script",
            "create a tool",
            "develop",
            "class",
            "rust",
            "python",
            "javascript",
            "typescript",
            "java",
            "file processing",
            "parse",
            "extract",
            "transform",
        ];

        // Check for coding indicators
        let has_coding_keyword = coding_keywords.iter().any(|kw| lower.contains(kw));
        let mentions_file_with_code =
            lower.contains("file") && (lower.contains("code") || lower.contains("script"));
        let asks_for_implementation =
            lower.contains("implement") || lower.contains("write") || lower.contains("create");

        // Ralph is best for implementation tasks that may need iteration
        (has_coding_keyword && asks_for_implementation) || mentions_file_with_code
    }

    /// Determine if message should use RLM (large document processing)
    fn should_use_rlm(&self, content: &str) -> bool {
        let lower = content.to_lowercase();

        // Keywords that suggest large document processing
        let document_keywords = [
            "summarize",
            "analyze document",
            "process file",
            "read file",
            "extract from",
            "scan document",
            "large text",
            "long document",
            "multiple files",
            "directory",
            "folder",
            "codebase",
            "repository",
        ];

        // File extensions that suggest large documents
        let document_extensions = [".txt", ".md", ".pdf", ".doc", ".csv", ".json", ".xml"];

        let has_doc_keyword = document_keywords.iter().any(|kw| lower.contains(kw));
        let mentions_file = document_extensions.iter().any(|ext| lower.contains(ext));
        let mentions_directory =
            lower.contains("/") || lower.contains("\\") || lower.contains("directory");

        has_doc_keyword || mentions_file || mentions_directory
    }

    /// Determine if message should be routed to Hermes for real tool execution
    fn should_use_hermes(&self, content: &str) -> bool {
        let lower = content.to_lowercase();
        let real_world_keywords = [
            "search the web",
            "google",
            "browse",
            "open browser",
            "download",
            "fetch url",
            "visit website",
            "run command",
            "run terminal",
            "execute shell",
            "bash command",
            "curl",
            "install package",
            "npm install",
            "pip install",
            "cargo install",
            "take screenshot",
        ];
        real_world_keywords.iter().any(|kw| lower.contains(kw))
    }

    /// Multi-turn agentic execution with two strategies:
    /// 1. If Hermes extension server is available → delegate to NousResearch Hermes (web, browser, terminal)
    /// 2. Otherwise → native multi-turn tool loop via existing LLM (OpenRouter/Qwen) + tool_registry
    async fn process_with_hermes(
        &mut self,
        _msg: &Message,
        context: &mut MessageContext,
        task: &str,
    ) -> Result<AgentResponse> {
        let start = Instant::now();

        // Strategy 1: Try the Hermes extension server if available
        if let Some(ref hermes) = self.hermes_client {
            info!("Hermes server mode: delegating to extension server");
            context.skills_accessed.push("hermes_server".to_string());
            match hermes.execute(task).await {
                Ok(output) => {
                    info!(
                        "Hermes server completed task in {:.1}s",
                        start.elapsed().as_secs_f64()
                    );
                    return Ok(AgentResponse {
                        content: format!(
                            "{output}\n\n---\n*Hermes server, {:.1}s*",
                            start.elapsed().as_secs_f64()
                        ),
                        primary_agent: 0,
                        council_used: false,
                        confidence: 0.9,
                        skills_used: vec!["hermes_server".to_string()],
                        joulework_contributions: HashMap::new(),
                        processing_time_ms: start.elapsed().as_millis() as u64,
                    });
                }
                Err(e) => {
                    warn!(
                        "Hermes server failed ({}), falling back to native tool loop",
                        e
                    );
                }
            }
        }

        // Strategy 2: Native multi-turn tool loop (OpenRouter/Qwen + tool_registry)
        let max_turns: usize = std::env::var("HSM_HERMES_MAX_TURNS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        info!("Hermes native loop: {}", &task[..task.len().min(80)]);
        context.skills_accessed.push("hermes_agentic".to_string());

        // Build tool schema list for the system prompt
        let rank_cap = tool_prompt_cap_from_env();
        let ranked_tools = rank_tools_for_prompt(&self.tool_registry, task, rank_cap);
        let tool_catalog = self.tool_registry.list_tools();
        let tool_descs: std::collections::HashMap<&str, &str> =
            tool_catalog.iter().copied().collect();
        let mut visible_tool_count = 0usize;
        let mut tool_lines = ranked_tools
            .iter()
            .filter_map(|t| {
                tool_descs.get(t.name.as_str()).map(|desc| {
                    visible_tool_count += 1;
                    format!("- {}: {} [score={:.1}]", t.name, desc, t.score)
                })
            })
            .collect::<Vec<_>>();
        if tool_lines.is_empty() {
            tool_lines = tool_catalog
                .iter()
                .take(rank_cap)
                .map(|(name, desc)| format!("- {}: {}", name, desc))
                .collect();
            visible_tool_count = tool_lines.len();
        }
        let hidden_count = tool_catalog.len().saturating_sub(visible_tool_count);
        if hidden_count > 0 {
            tool_lines.push(format!(
                "- ... {} additional tools hidden by prompt budget cap",
                hidden_count
            ));
        }
        let tools_description = tool_lines.join("\n");
        context.tool_prompt_tokens = approx_tokens_from_chars(tools_description.len());
        context.skill_prompt_tokens = approx_tokens_from_chars(
            context
                .skills_accessed
                .iter()
                .map(|s| s.len() + 1)
                .sum::<usize>(),
        );
        info!(
            "tool prompt budget: exposed={} hidden={} approx_tool_tokens={} approx_skill_tokens={}",
            visible_tool_count,
            hidden_count,
            context.tool_prompt_tokens,
            context.skill_prompt_tokens
        );

        let system_prompt = format!(
            "You are an autonomous agent with access to real tools. Complete the user's task by calling tools as needed.\n\n\
             ## Available Tools\n{tools_description}\n\n\
             ## Tool Calling Format\n\
             To call a tool, respond with ONLY a JSON object:\n\
             {{\"tool\": \"tool_name\", \"parameters\": {{\"param\": \"value\"}}}}\n\n\
             ## Rules\n\
             - Call ONE tool at a time, wait for the result, then decide next step.\n\
             - When the task is COMPLETE, respond with a normal text summary (no JSON).\n\
             - If a tool fails, try an alternative approach.\n\
             - Be concise. Do not explain what you will do — just do it."
        );

        let mut conversation: Vec<(String, String)> = Vec::new();
        let mut current_query = task.to_string();
        let mut final_content = String::new();
        let mut tools_executed: Vec<String> = Vec::new();

        for turn in 0..max_turns {
            let llm_result = self
                .llm
                .chat(&system_prompt, &current_query, &conversation)
                .await;

            if llm_result.timed_out || llm_result.text.is_empty() {
                if turn == 0 {
                    final_content = format!("LLM unavailable for agentic execution of: '{}'", task);
                }
                break;
            }

            let response_text = Self::clean_response(&llm_result.text);

            // Try to parse a tool call from the response
            let json_candidate = Self::extract_tool_call_json(&response_text);
            if let Some(json) = json_candidate {
                if let (Some(tool_name), Some(params)) = (
                    json.get("tool").and_then(|v| v.as_str()),
                    json.get("parameters"),
                ) {
                    if self.tool_registry.has(tool_name) {
                        info!("Hermes turn {}: calling tool {}", turn + 1, tool_name);
                        let call = ToolCallEntry {
                            name: tool_name.to_string(),
                            parameters: params.clone(),
                            call_id: uuid::Uuid::new_v4().to_string(),
                            harness_run: None,
                            idempotency_key: None,
                        };

                        let result = self.tool_registry.execute(call).await;
                        if result.output.success {
                            crate::tools::web_ingest::ingest_web_tool_success(
                                &mut self.world,
                                tool_name,
                                params,
                                &result.output,
                            );
                        }
                        tools_executed.push(tool_name.to_string());

                        // Record for trace2skill
                        let args_redacted = serde_json::to_string(&trace2skill::redact_params(
                            &result.call.parameters,
                        ))
                        .unwrap_or_else(|_| "{}".to_string());
                        let result_summary = trace2skill::summarize_tool_output(
                            result.output.success,
                            &result.output.result,
                            result.output.error.as_deref(),
                        );
                        context.tool_steps.push(ToolStepRecord {
                            name: tool_name.to_string(),
                            args_redacted,
                            ok: result.output.success,
                            result_summary,
                        });

                        let tool_output = if result.output.success {
                            result.output.result.clone()
                        } else {
                            format!(
                                "ERROR: {}",
                                result
                                    .output
                                    .error
                                    .as_deref()
                                    .unwrap_or("Tool execution failed")
                            )
                        };

                        // Feed tool result back as next user message
                        conversation.push(("assistant".to_string(), response_text.clone()));
                        current_query = format!(
                            "Tool '{}' returned:\n{}\n\nContinue with the task or provide the final answer.",
                            tool_name, tool_output
                        );
                        continue;
                    } else {
                        warn!("Hermes turn {}: unknown tool '{}'", turn + 1, tool_name);
                        conversation.push(("assistant".to_string(), response_text.clone()));
                        current_query = format!(
                            "Tool '{}' does not exist. Pick from the available tools list.",
                            tool_name
                        );
                        continue;
                    }
                }
            }

            // No tool call detected — this is the final answer
            final_content = response_text;
            break;
        }

        if final_content.is_empty() {
            final_content =
                "Hermes agentic loop ended without a final answer (max turns reached).".to_string();
        }

        let suffix = if !tools_executed.is_empty() {
            format!(
                "\n\n---\n*Hermes: {} tool calls ({}) in {:.1}s*",
                tools_executed.len(),
                tools_executed.join(", "),
                start.elapsed().as_secs_f64()
            )
        } else {
            String::new()
        };

        Ok(AgentResponse {
            content: format!("{final_content}{suffix}"),
            primary_agent: 0,
            council_used: false,
            confidence: if tools_executed.is_empty() { 0.6 } else { 0.85 },
            skills_used: vec!["hermes_agentic".to_string()],
            joulework_contributions: HashMap::new(),
            processing_time_ms: start.elapsed().as_millis() as u64,
        })
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
            format!("📊 **RLM Analysis Complete**\n\n{}", output.result)
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
        let query = parts
            .get(1)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Analyze this document".to_string());

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
            format!(
                "Error: {}",
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
            let tool_list: Vec<String> = tools
                .iter()
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
                content: format!(
                    "❌ Unknown tool: `{}`\n\nUse `/tool` to list available tools.",
                    tool_name
                ),
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
            format!(
                "❌ **Tool Error: {}**\n\n{}",
                tool_name,
                output.error.unwrap_or_else(|| "Unknown error".to_string())
            )
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
                        if path_str.contains('.')
                            || path_str.starts_with('/')
                            || path_str.starts_with("./")
                        {
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
        let members: Vec<CouncilMember> = self
            .world
            .agents
            .iter()
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
        info!(
            "Council mode selected: {:?} (complexity={:.2}, urgency={:.2})",
            council.mode, proposal.complexity, proposal.urgency
        );

        // Run evaluation
        let start = Instant::now();
        let decision = council.evaluate().await?;
        let deliberation_time = start.elapsed().as_millis() as u64;

        // Record council usage
        context.council_used = true;
        self.agent_metrics.council_invocations += 1;

        // Track agent contributions
        for member in &council.members {
            let jw = self
                .world
                .agents
                .get(member.agent_id as usize)
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
        let mem = self.persistent_memory_addon();
        let biz = self.business_prompt_addon();
        let council_prompt = format!(
            "{}{}{}\n\n## Council Decision\nMode: {:?}\nOutcome: {}\nConfidence: {:.2}\nAgents: {:?}\n\nBased on the council's deliberation above, provide a helpful response to the user's query:\n{}",
            enriched_prompt,
            mem,
            biz,
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
            dominant_roles: self
                .world
                .agents
                .iter()
                .take(3)
                .map(|a| a.role.clone())
                .collect(),
            current_goals: vec![msg.content.clone()],
            recent_skills_used: vec![],
            system_load: 0.5,
            error_rate: 0.0,
            coherence_score: self.world.global_coherence(),
        };

        self.services.cass.update_context(context_snapshot.clone());

        // Search for skills
        let skill_matches = self
            .services
            .cass
            .search(&msg.content, Some(context_snapshot), 3)
            .await;

        // Select agent based on message type
        let selected_agent = self.select_agent_for_message(&msg.content);
        context.assigned_agents.push(selected_agent);

        // Build enriched system prompt with RLM LivingPrompt + tool schemas
        let enriched_prompt = self.living_prompt.render();
        let mem = self.persistent_memory_addon();
        let rank_cap = tool_prompt_cap_from_env();
        let ranked_tools = rank_tools_for_prompt(&self.tool_registry, &msg.content, rank_cap);
        let tool_catalog = self.tool_registry.list_tools();
        let tool_descs: std::collections::HashMap<&str, &str> =
            tool_catalog.iter().copied().collect();
        let mut visible_tool_count = 0usize;
        let mut tool_lines = ranked_tools
            .iter()
            .filter_map(|t| {
                tool_descs.get(t.name.as_str()).map(|desc| {
                    visible_tool_count += 1;
                    format!("- {}: {} [score={:.1}]", t.name, desc, t.score)
                })
            })
            .collect::<Vec<_>>();
        if tool_lines.is_empty() {
            tool_lines = tool_catalog
                .iter()
                .take(rank_cap)
                .map(|(name, desc)| format!("- {}: {}", name, desc))
                .collect();
            visible_tool_count = tool_lines.len();
        }
        let hidden_count = tool_catalog.len().saturating_sub(visible_tool_count);
        if hidden_count > 0 {
            tool_lines.push(format!(
                "- ... {} additional tools hidden by prompt budget cap",
                hidden_count
            ));
        }
        let tools_description = tool_lines.join("\n");
        context.tool_prompt_tokens = approx_tokens_from_chars(tools_description.len());
        context.skill_prompt_tokens = approx_tokens_from_chars(
            context
                .skills_accessed
                .iter()
                .map(|s| s.len() + 1)
                .sum::<usize>(),
        );
        context.tool_prompt_exposed_count = visible_tool_count;
        context.tool_prompt_hidden_count = hidden_count;

        let belief_context = if !beliefs.is_empty() {
            let belief_strs: Vec<String> = beliefs
                .iter()
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
            format!(
                "\n\n## Matched Skill: {} (score: {:.2})\n{}",
                best.skill.title, best.semantic_score, best.skill.principle
            )
        } else {
            String::new()
        };

        // Inject AutoContext hints for this query
        let ac_ctx = self.autocontext.retrieve_context(&msg.content);
        let autocontext_section = if !ac_ctx.hints.is_empty() || !ac_ctx.playbooks.is_empty() {
            let hints_text = ac_ctx
                .hints
                .iter()
                .take(3)
                .map(|h| format!("- [hint {:.0}%] {}", h.confidence * 100.0, h.content))
                .collect::<Vec<_>>()
                .join("\n");
            let pb_text = ac_ctx
                .playbooks
                .iter()
                .take(2)
                .map(|p| {
                    format!(
                        "- [playbook {:.0}%] {}: {}",
                        p.quality_score * 100.0,
                        p.name,
                        p.description
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "\n\n## AutoContext Guidance\n{}{}",
                hints_text,
                if pb_text.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", pb_text)
                }
            )
        } else {
            String::new()
        };

        let route_hit = self
            .prompt_router
            .as_ref()
            .map(|r| r.route(&msg.content))
            .unwrap_or_default();
        let biz = self.business_prompt_addon_for_persona(route_hit.persona_key.as_deref());

        let mut prefetch_block = String::new();
        let prefetch_on = std::env::var("HSM_MEMORY_PREFETCH")
            .map(|v| {
                let s = v.trim();
                !(s == "0" || s.eq_ignore_ascii_case("false") || s.eq_ignore_ascii_case("no"))
            })
            .unwrap_or(false);
        if prefetch_on {
            let n = std::env::var("HSM_MEMORY_PREFETCH_N")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1)
                .clamp(1, 12);
            match super::agent_memory_pipeline::prefetch_memory_context(
                &self.llm,
                &self.base_path,
                &msg.content,
                n,
            )
            .await
            {
                Ok(s) => prefetch_block = s,
                Err(e) => warn!("memory prefetch: {}", e),
            }
        }

        let md_skills_section = match self.skill_md_catalog.read() {
            Ok(cat) if !cat.is_empty() => format!(
                "\n\n{}",
                cat.format_prompt_index(MAX_SKILL_MD_PROMPT_ENTRIES, MAX_SKILL_MD_LINE_CHARS)
            ),
            _ => String::new(),
        };

        let tail = format!(
            "\n\n## Available Tools\n{tools_description}\n\n\
             ## Tool Usage\n\
             When the user asks you to perform an action (search, read files, run commands, calculate, etc.), \
             respond with a JSON tool call:\n\
             {{\"tool\": \"tool_name\", \"parameters\": {{\"param\": \"value\"}}}}\n\n\
             If no tool is needed, respond normally with helpful text.",
        );
        let policy = super::prompt_assembly::PromptAssemblyPolicy::from_env();
        let section_pairs: Vec<(String, String)> = vec![
            ("living".into(), enriched_prompt),
            ("memory".into(), mem),
            ("route".into(), route_hit.system_block.clone()),
            ("business".into(), biz),
            ("prefetch".into(), prefetch_block),
            ("belief".into(), belief_context),
            ("skill".into(), skill_context),
            ("autocontext".into(), autocontext_section),
            ("md_skills".into(), md_skills_section),
            ("tail".into(), tail),
        ];
        crate::policy_config::ensure_loaded();
        let loaded = crate::policy_config::get();
        let (system_prompt, context_manifest) =
            super::prompt_assembly::assemble_prompt_sections_with_manifest(
                &section_pairs,
                &policy,
                |k| loaded.tier_for_section(k),
            );
        tracing::info!(
            target: "hsm.context_manifest",
            summary = %context_manifest.summary_line(),
            "assembled personal agent system prompt"
        );
        if std::env::var("HSM_LOG_CONTEXT_MANIFEST")
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false)
        {
            if let Ok(json) = serde_json::to_string(&context_manifest) {
                tracing::debug!(target: "hsm.context_manifest", %json, "full context manifest");
            }
        }

        // Generate response with tool-aware prompt
        let start = Instant::now();
        let tier_chat = crate::harness::TierPolicy::from_env().clip_chat_pairs(&self.chat_history);
        let llm_result = self
            .llm
            .chat(&system_prompt, &msg.content, &tier_chat)
            .await;

        let mut final_content = if llm_result.timed_out || llm_result.text.is_empty() {
            // LLM unavailable fallback
            format!(
                "I'd like to help with '{}', but the LLM is currently unavailable.",
                msg.content
            )
        } else {
            Self::clean_response(&llm_result.text)
        };

        // Tool execution loop: parse tool calls from LLM response and execute them
        // Support: raw JSON, ```json ... ```, or embedded {...}
        let mut tool_used = false;
        let json_candidate = Self::extract_tool_call_json(&final_content);
        if let Some(json) = json_candidate {
            if let Some(tool_name) = json.get("tool").and_then(|v| v.as_str()) {
                if let Some(params) = json.get("parameters") {
                    if self.tool_registry.has(tool_name) {
                        info!(
                            "Tool call detected: {} with params: {:?}",
                            tool_name, params
                        );
                        let call = ToolCallEntry {
                            name: tool_name.to_string(),
                            parameters: params.clone(),
                            call_id: uuid::Uuid::new_v4().to_string(),
                            harness_run: None,
                            idempotency_key: None,
                        };

                        let result = self.tool_registry.execute(call).await;
                        if result.output.success {
                            crate::tools::web_ingest::ingest_web_tool_success(
                                &mut self.world,
                                tool_name,
                                params,
                                &result.output,
                            );
                        }
                        tool_used = true;

                        let args_redacted = serde_json::to_string(&trace2skill::redact_params(
                            &result.call.parameters,
                        ))
                        .unwrap_or_else(|_| "{}".to_string());
                        let result_summary = trace2skill::summarize_tool_output(
                            result.output.success,
                            &result.output.result,
                            result.output.error.as_deref(),
                        );
                        context.tool_steps.push(ToolStepRecord {
                            name: tool_name.to_string(),
                            args_redacted,
                            ok: result.output.success,
                            result_summary,
                        });

                        let tool_output = if result.output.success {
                            result.output.result.clone()
                        } else {
                            result
                                .output
                                .error
                                .clone()
                                .unwrap_or_else(|| "Tool execution failed".to_string())
                        };

                        info!(
                            "Tool {} executed: success={}",
                            tool_name, result.output.success
                        );

                        // Feed tool result back to LLM for synthesis
                        let synthesis_prompt = format!(
                            "You executed the tool '{}' and got this result:\n\n{}\n\n\
                             Now provide a helpful, concise response to the user based on this result.",
                            tool_name, tool_output
                        );
                        let synthesis = self
                            .llm
                            .chat(&system_prompt, &synthesis_prompt, &tier_chat)
                            .await;
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
            confidence: if !skill_matches.is_empty() {
                skill_matches[0].semantic_score
            } else {
                0.6
            },
            skills_used: skill_matches
                .into_iter()
                .take(1)
                .map(|s| s.skill.id)
                .collect(),
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
        } else if lower.contains("explore") || lower.contains("discover") || lower.contains("find")
        {
            Role::Explorer
        } else if lower.contains("structure")
            || lower.contains("organize")
            || lower.contains("plan")
        {
            Role::Architect
        } else {
            Role::Catalyst // Default - handles general queries
        };

        // Find agent with matching role and highest JW
        self.world
            .agents
            .iter()
            .filter(|a| a.role == target_role)
            .max_by(|a, b| {
                let jwa = a.calculate_jw(self.world.global_coherence(), 3);
                let jwb = b.calculate_jw(self.world.global_coherence(), 3);
                jwa.partial_cmp(&jwb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|a| a.id)
            .unwrap_or(0)
    }

    /// True if the model output is (mostly) a single JSON tool call — not acceptable as email body.
    fn response_looks_like_tool_json(text: &str) -> bool {
        let t = text.trim();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            if v.get("tool").and_then(|x| x.as_str()).is_some() {
                return v.get("parameters").is_some() || v.get("arguments").is_some();
            }
        }
        if let Some(v) = Self::extract_tool_call_json(t) {
            if v.get("tool").and_then(|x| x.as_str()).is_some() {
                return v.get("parameters").is_some() || v.get("arguments").is_some();
            }
        }
        false
    }

    /// Extract tool call JSON from LLM response (handles markdown-wrapped and inline JSON)
    fn extract_tool_call_json(content: &str) -> Option<serde_json::Value> {
        let trimmed = content.trim();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return Some(v);
        }
        if let Some(re) = regex::Regex::new(r"(?is)```(?:json)?\s*(\{[^`]*\})\s*```").ok() {
            if let Some(cap) = re.captures(trimmed) {
                if let Some(m) = cap.get(1) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(m.as_str().trim()) {
                        return Some(v);
                    }
                }
            }
        }
        if let Some(re) = regex::Regex::new(r"(\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\})").ok() {
            for cap in re.captures_iter(trimmed) {
                if let Some(m) = cap.get(1) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(m.as_str()) {
                        if v.get("tool").and_then(|t| t.as_str()).is_some() {
                            return Some(v);
                        }
                    }
                }
            }
        }
        None
    }

    /// Strip leaked chat template tokens from LLM output
    fn clean_response(text: &str) -> String {
        let mut cleaned = text.to_string();
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
        while let Some(start) = cleaned.find("<|") {
            if let Some(end) = cleaned[start..].find("|>") {
                cleaned.replace_range(start..start + end + 2, "");
            } else {
                break;
            }
        }
        cleaned.trim().to_string()
    }

    /// Extract keywords for belief search (drops generic question words).
    fn extract_search_keywords(&self, content: &str) -> Vec<String> {
        let lower = content.to_lowercase();
        let low_confidence = [
            "discuss",
            "talk",
            "mention",
            "say",
            "tell",
            "yesterday",
            "last week",
            "recently",
            "thing",
            "stuff",
            "issue",
            "problem",
            "conversation",
            "chat",
            "question",
            "did",
            "do",
            "does",
            "what",
            "when",
            "where",
            "who",
            "why",
            "how",
        ];
        lower
            .split_whitespace()
            .filter(|w| {
                let t = w.trim_matches(|c: char| !c.is_alphabetic());
                !low_confidence.contains(&t)
            })
            .take(5)
            .map(|s| s.to_string())
            .collect()
    }

    /// Get relevant beliefs from world based on content (public for CLI access)
    pub async fn get_relevant_beliefs(
        &self,
        content: &str,
    ) -> Result<Vec<crate::hyper_stigmergy::Belief>> {
        // Use extracted keywords when available (filters noise like "what", "when")
        let keywords = self.extract_search_keywords(content);
        let words: Vec<String> = if keywords.is_empty() {
            content
                .to_lowercase()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        } else {
            keywords
        };

        let relevant: Vec<_> = self
            .world
            .beliefs
            .iter()
            .filter(|b| {
                let belief_lower = b.content.to_lowercase();
                let belief_words: Vec<&str> = belief_lower.split_whitespace().collect();
                words.iter().any(|w| belief_words.contains(&w.as_str()))
            })
            .cloned()
            .take(5)
            .collect();

        Ok(relevant)
    }

    /// Track JouleWork contributions
    pub(crate) async fn track_contributions(
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
                    description: format!(
                        "Processed message: {}",
                        context.content.chars().take(50).collect::<String>()
                    ),
                };

                self.agent_metrics.joulework_history.push(record);
            }
        }

        self.agent_metrics.total_messages_processed += 1;

        Ok(())
    }

    /// Finish a turn handled outside `handle_message` (integration-layer commands).
    pub(crate) async fn finalize_integration_turn(
        &mut self,
        msg: &Message,
        context: MessageContext,
        response: &AgentResponse,
    ) -> Result<()> {
        self.track_contributions(&context, response).await?;
        self.world.tick();
        if self.config.enable_dks {
            let dks_tick = self.services.dks.tick();
            self.services.last_dks_tick = Some(dks_tick);
        }
        if let Err(e) = self
            .task_trail
            .append_turn(
                &context.message_id,
                &msg.content,
                &response.content,
                &response.skills_used,
                response.council_used,
                context.tool_steps.len(),
                context.tool_prompt_tokens,
                context.skill_prompt_tokens,
                context.tool_prompt_exposed_count,
                context.tool_prompt_hidden_count,
                self.world.edges.len(),
                self.world.beliefs.len(),
            )
            .await
        {
            warn!("task trail append_turn failed: {}", e);
        }
        self.chat_history
            .push(("user".to_string(), msg.content.clone()));
        self.chat_history
            .push(("assistant".to_string(), response.content.clone()));
        if self.chat_history.len() > 40 {
            let drain_count = self.chat_history.len() - 40;
            self.chat_history.drain(..drain_count);
        }
        self.post_turn_hooks(msg, response);
        self.maybe_save().await?;
        self.active_contexts
            .insert(context.message_id.clone(), context);
        Ok(())
    }

    /// Save state to LadybugDB if interval elapsed
    pub async fn maybe_save(&mut self) -> Result<()> {
        let elapsed = self.last_save.elapsed().as_secs();
        if elapsed >= self.config.save_interval_secs {
            self.save().await?;
            self.last_save = Instant::now();
        }
        Ok(())
    }

    /// Start gateway for external communication
    /// Returns a channel receiver for processing messages
    pub async fn start_gateway(
        &mut self,
        config: crate::personal::gateway::Config,
    ) -> Result<mpsc::Receiver<(Message, oneshot::Sender<String>)>> {
        use crate::personal::gateway::Gateway;

        let mut gateway = Gateway::new(config);

        // Create channel for message passing (avoids circular reference)
        let (tx, rx) = mpsc::channel::<(Message, oneshot::Sender<String>)>(100);

        self.gateway_tx = Some(tx.clone());

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

        // Save AutoContext knowledge base
        if let Err(e) = self.autocontext.save().await {
            warn!("AutoContext save failed: {}", e);
        }

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
        tokio::fs::create_dir_all(base_path.join("business")).await?;
        tokio::fs::create_dir_all(base_path.join("metrics")).await?;
        tokio::fs::create_dir_all(base_path.join("skills")).await?;
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
        let path = config_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::write_atomic(&path, content.as_bytes()).map_err(|e| anyhow::Error::from(e))
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))??;
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
        let path = metrics_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::write_atomic(&path, content.as_bytes()).map_err(|e| anyhow::Error::from(e))
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))??;
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
