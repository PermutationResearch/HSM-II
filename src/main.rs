use async_trait::async_trait;
use std::collections::HashMap;
use std::env;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, watch, Mutex, RwLock};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ollama_rs::generation::chat::request::ChatMessageRequest;
use ollama_rs::generation::chat::ChatMessage as OllamaChatMsg;
use ollama_rs::generation::chat::MessageRole;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Bar, BarChart, BarGroup, Block, Borders, Gauge, List, ListItem, Paragraph, Sparkline, Tabs,
        Wrap,
    },
    Frame, Terminal,
};
use reedline::{DefaultPrompt, Reedline, Signal};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::process::Command;
use tokio_stream::StreamExt;

use hyper_stigmergy::consensus::{BayesianConfidence, SkillStatus};
use hyper_stigmergy::database::{
    CouncilClaimRow, MessageRow, OuroborosGateAuditRow, OuroborosMemoryEventRow, PlanStepRow,
    RewardLogRow, SkillEvidenceRow, SkillHireRow, SkillRow,
};
use hyper_stigmergy::dspy::{
    optimize_all_signatures, persist_trace, run_signature, run_signature_traced,
    sig_analyst_argument, sig_analyst_evidence, sig_analyst_stance, sig_chair_confidence,
    sig_chair_synthesis, sig_chair_winner, sig_challenger_alternative,
    sig_challenger_counter_evidence, sig_challenger_weak_point, sig_chat_draft, sig_chat_refine,
    sig_rebuttal_refine, sig_rebuttal_refute, sig_rebuttal_valid, sig_semantic_repair,
    sig_simple_answer, strip_claim_evidence_format, DspyContext, TraceResult,
};
use hyper_stigmergy::dspy_session::{DspySession, DspySessionAdapter, SessionTurn, TurnRole};
use hyper_stigmergy::federation::server::FederationState;
use hyper_stigmergy::hyper_stigmergy::{HyperEdge, VertexKind};
use hyper_stigmergy::skill::{
    DelegationPackage, HireStatus, HireTree, ProofSignature, Skill, SkillCreditRecord,
    SkillCuration, SkillHire, SkillLevel, SkillScope, SkillSource,
};
use hyper_stigmergy::vault;
use hyper_stigmergy::{
    calculate_dks_stability, confidence_to_phase, default_tool_registry, evaluate_runtime_slos,
    evaluate_synthesis, kuramoto_build_adjacency,
    lcm::{
        storage::{LcmStorage, MemoryStorage, RooStorage},
        CompactionResult, LcmContext, NodeType,
    },
    optimize_anything::{OptimizationConfig, OptimizationMode, OptimizationSession},
    session_from_json, Action, BidConfig, CodeNavigator, CommunicationConfig, CommunicationHub,
    ConstitutionConfig, ContextSnapshot as CassContextSnapshot, Council, CouncilBridge,
    CouncilBridgeConfig, CouncilMember, CouncilMode, DKSConfig, DKSSystem, DKSTickResult,
    DataSensitivity, Decision, EdgeScope, EvalResult, EvidenceBundle, EvidenceContract,
    EvidenceRequirements, FederationClient, FederationConfig, FederationServer, GpuAccelerator,
    HybridMemory, HyperStigmergicMorphogenesis, HyperedgeInjectionRequest, KuramotoEngine,
    KuramotoSnapshot, LLMDeliberationConfig, Message, MessageType, MetaGraph, ModeConfig,
    ModeSelectionReport, ModeSwitcher, ModelServer, PolicyContext, PolicyDecision, PolicyEngine,
    PromiseStatus, Proposal, ProposedAction, Provenance, ReleaseState, RiskGate, RiskGateConfig,
    Role, RooDb, RooDbConfig, RuntimeSnapshot, RuntimeThresholds, SkillBank,
    StigmergicCouncilContext, SubscriptionFilter, Target, ToolExecutor, ToolRegistry,
    VaultEmbeddingRow, CASS,
};

// ── Chat types ─────────────────────────────────────────────────────────

enum ChatEvent {
    Token(String),
    Thinking(String),
    Done,
    Error(String),
}

enum CouncilStreamPart {
    Token(String),
    Thinking(String),
    ThinkingEnd,
}

struct ChatMsg {
    role: String,
    content: String,
    model: String,
}

#[derive(Clone, Debug)]
struct CodeAgentRuntimeSession {
    query: String,
    task_key: String,
    actor_id: u64,
    promise_id: String,
    sensitivity: DataSensitivity,
    successful_tools: u32,
    failed_tools: u32,
    unsafe_tool_events: u32,
}

struct NoopSessionAdapter;

#[async_trait]
impl DspySessionAdapter for NoopSessionAdapter {
    async fn forward(&self, _: &str, _: &[SessionTurn]) -> anyhow::Result<String> {
        Ok(String::new())
    }
}

// ── Background event channel ──────────────────────────────────────────
// Used by async tasks (RooDB, bincode I/O) to send results back to the
// main TUI loop without clobbering the chat stream.

enum BgEvent {
    Log(String),
    WorldLoaded {
        world: Box<HyperStigmergicMorphogenesis>,
        source: &'static str,
    },
    SaveComplete {
        bincode_ok: bool,
        db_snapshot_id: Option<u64>,
        errors: Vec<String>,
    },
    RooDbConnected(Arc<RooDb>),
    QueryResult {
        sql: String,
        rows: Vec<Vec<String>>,
        headers: Vec<String>,
    },
    // From browser API
    WebChat {
        text: String,
        model: Option<String>,
        resp_tx: Option<tokio::sync::oneshot::Sender<()>>,
    },
    WebCommand {
        cmd: String,
        resp_tx: tokio::sync::oneshot::Sender<String>,
    },
    CouncilRequest {
        question: String,
        mode: String,
    },
    /// Council synthesis completed — feed back into the world as a belief + experience.
    CouncilSynthesis {
        question: String,
        synthesis: String,
        confidence: f64,
        citations: Vec<(u64, u32)>,
        coverage: f64,
        plan_text: String,
        plan_steps: Vec<PlanStep>,
    },
    /// Council lifecycle completion for accurate runtime status.
    CouncilFinished {
        mode: String,
        success: bool,
    },
    /// Code agent request with tool execution
    CodeAgent {
        query: String,
        model: String,
    },
    /// optimize_anything request from the browser Studio
    OptimizeRequest {
        body: serde_json::Value,
    },
    PlanOptimize {
        step_index: usize,
    },
    /// Code agent lifecycle completion for runtime component state.
    CodeAgentFinished {
        success: bool,
    },
    /// Code agent quality score for experience recording (Integration 5)
    CodeAgentQuality {
        query: String,
        quality_score: f64,
    },
    /// Optimized role prompts to store (Integration 2)
    OptimizedRolePrompts {
        prompts: Vec<(String, String)>,
    },
    /// Inject inter-agent message from external service
    InjectMessage {
        sender: u64,
        target: String,
        kind: String,
        content: String,
    },
    /// Code agent session persistence events
    CodeAgentSessionStart {
        session_id: String,
        query: String,
        model: String,
        working_dir: String,
    },
    CodeAgentSessionMessage {
        session_id: String,
        turn: i32,
        role: String,
        content: String,
        has_tool_calls: bool,
    },
    CodeAgentSessionToolCall {
        session_id: String,
        turn: i32,
        tool_name: String,
        args: serde_json::Value,
        result: Option<String>,
        error: Option<String>,
        duration_ms: u64,
        file_path: Option<String>,
    },
    CodeAgentSessionComplete {
        session_id: String,
        final_response: Option<String>,
        quality_score: Option<f64>,
        turn_count: i32,
        error: Option<String>,
    },
    /// Visual explainer request - generate HTML visualization
    VisualExplainer {
        diagram_type: String, // "architecture", "flowchart", "table", "dashboard", "timeline"
        title: String,
        content: String,                 // Description of what to visualize
        data: Option<serde_json::Value>, // Optional structured data
        open_browser: bool,
    },
    PlanWorkflow {
        step_index: usize,
    },
    /// Skill curation management — promote or add curated skills.
    SkillCurate {
        action: String, // "promote" | "add_curated"
        body: serde_json::Value,
        resp_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// Mark a skill hire as completed/failed with outcome score.
    HireComplete {
        hire_id: String,
        status: String, // "completed" | "failed"
        outcome_score: f64,
        completed_at: u64,
    },
    /// DSPy optimizer — bootstrap demonstrations and run mutation trials.
    DspyOptimize {
        signature_name: Option<String>, // None = optimize all eligible
        resp_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// DSPy trace logged — fire-and-forget from chat/council paths.
    DspyTraceLogged {
        signature_name: String,
        score: f64,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
struct AnswerDict {
    content: String,
    ready: bool,
    metadata: serde_json::Value,
    status: Option<String>,
    trace_hash: Option<String>,
}

impl Default for AnswerDict {
    fn default() -> Self {
        Self {
            content: String::new(),
            ready: false,
            metadata: serde_json::Value::Null,
            status: None,
            trace_hash: None,
        }
    }
}

impl AnswerDict {
    fn with_content(content: String, metadata: serde_json::Value, status: Option<String>) -> Self {
        Self {
            content,
            ready: true,
            metadata,
            status,
            trace_hash: None,
        }
    }

    fn error(message: &str) -> Self {
        Self {
            content: message.to_string(),
            ready: false,
            metadata: serde_json::json!({ "error": message }),
            status: Some("error".to_string()),
            trace_hash: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplCommandKind {
    Council,
}

struct ReplPending {
    kind: ReplCommandKind,
    tx: tokio::sync::oneshot::Sender<AnswerDict>,
}

#[derive(Default)]
struct ReplState {
    pending: parking_lot::Mutex<Option<ReplPending>>,
    last_answer: parking_lot::Mutex<AnswerDict>,
}

impl ReplState {
    fn set_pending(&self, pending: ReplPending) -> Result<(), &'static str> {
        let mut guard = self.pending.lock();
        if guard.is_some() {
            Err("another command is running")
        } else {
            *guard = Some(pending);
            Ok(())
        }
    }

    fn take_pending(&self, kind: ReplCommandKind) -> Option<ReplPending> {
        let mut guard = self.pending.lock();
        if guard.as_ref().map(|p| p.kind == kind).unwrap_or(false) {
            guard.take()
        } else {
            None
        }
    }

    fn set_last(&self, answer: AnswerDict) {
        *self.last_answer.lock() = answer;
    }

    fn last(&self) -> AnswerDict {
        self.last_answer.lock().clone()
    }

    fn clear(&self) {
        *self.last_answer.lock() = AnswerDict::default();
    }
}

#[derive(Clone, Debug, Default)]
struct RuntimeComponents {
    council: CouncilSnapshot,
    dks: DKSSnapshot,
    cass: CASSSnapshot,
    navigation: NavigationSnapshot,
    communication: CommunicationSnapshot,
    kuramoto: Option<KuramotoSnapshot>,
    gpu: GpuSnapshot,
    llm: LLMSnapshot,
    email: EmailSnapshot,
}

struct RuntimeServices {
    dks: DKSSystem,
    cass: CASS,
    cass_initialized: bool,
    navigator: CodeNavigator,
    communication: CommunicationHub,
    kuramoto: KuramotoEngine,
    last_kuramoto_world_tick: Option<u64>,
    llm_server: ModelServer,
    gpu_available: bool,
    last_dks_tick: Option<DKSTickResult>,
}

// ── Web API shared state ──────────────────────────────────────────────
// A serialisable snapshot updated after every tick/export so the HTTP
// API can serve it without locking the main loop.

#[derive(serde::Serialize, Clone, Debug)]
struct AgentSnapshot {
    id: u64,
    role: String,
    curiosity: f64,
    harmony: f64,
    growth: f64,
    transcendence: f64,
    learning_rate: f64,
    description: String,
    jw: f64,
}

#[derive(serde::Serialize, Clone, Debug)]
struct EdgeSnapshot {
    participants: Vec<u64>,
    weight: f64,
    emergent: bool,
    age: u64,
}

#[derive(serde::Serialize, Clone, Debug)]
struct BeliefSnapshot {
    content: String,
    confidence: f64,
    source: String,
}

#[derive(serde::Serialize, Clone, Debug)]
struct ImprovementSnapshot {
    intent: String,
    mutation_type: String,
    coherence_before: f64,
    coherence_after: f64,
    applied: bool,
}

// ── New Component Snapshots ──────────────────────────────────────────────

#[derive(serde::Serialize, Clone, Debug, Default)]
struct CouncilSnapshot {
    active: bool,
    mode: String,
    member_count: usize,
    recent_decisions: Vec<String>,
    current_proposal: Option<String>,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct DKSSnapshot {
    generation: u64,
    population_size: usize,
    avg_persistence: f64,
    replicator_count: usize,
    flux_intensity: f64,
    multifractal_spectrum: Vec<(f64, f64)>, // (alpha, f_alpha)
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct CASSSnapshot {
    skill_count: usize,
    context_depth: usize,
    recent_matches: Vec<String>,
    embedding_dimension: usize,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct NavigationSnapshot {
    indexed_files: usize,
    topics: Vec<String>,
    recent_searches: Vec<String>,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct CommunicationSnapshot {
    active_gossip_rounds: usize,
    swarm_agents: usize,
    stigmergic_fields: usize,
    message_throughput: u64,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct GpuSnapshot {
    available: bool,
    device_name: Option<String>,
    compute_load: Option<f64>,
    memory_used_mb: Option<u64>,
    fallback_active: bool,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct LLMSnapshot {
    model_loaded: bool,
    model_name: Option<String>,
    cache_hit_rate: Option<f64>,
    tokens_generated: u64,
    avg_latency_ms: f64,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct EmailSnapshot {
    inbox_unread: Option<usize>,
    classified_today: Option<usize>,
    auto_responses_sent: Option<usize>,
    memory_entries: usize,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct FederationSnapshot {
    status: String,
    addr: Option<String>,
    system_id: String,
    peers: Vec<String>,
    imported: usize,
    exported: usize,
    conflicts: usize,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct WorldSnapshot {
    tick: u64,
    coherence: f64,
    coherence_trend: String,
    global_jw: f64,
    agents: Vec<AgentSnapshot>,
    edges: Vec<EdgeSnapshot>,
    beliefs: Vec<BeliefSnapshot>,
    improvements: Vec<ImprovementSnapshot>,
    ontology: Vec<(String, String)>,
    event_log: Vec<String>,
    // New component snapshots
    council: CouncilSnapshot,
    dks: DKSSnapshot,
    cass: CASSSnapshot,
    navigation: NavigationSnapshot,
    communication: CommunicationSnapshot,
    kuramoto: Option<KuramotoSnapshot>,
    gpu: GpuSnapshot,
    llm: LLMSnapshot,
    email: EmailSnapshot,
    federation: FederationSnapshot,
    skills: SkillSnapshot,
    chat_context: ChatContextSnapshot,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct ChatContextSnapshot {
    message_count: usize,
    estimated_tokens: usize,
    percent_used: f32,
    limit_tokens: usize,
    regular_tokens: usize,
    cache_read_tokens: usize,
    cache_write_tokens: usize,
    has_summary: bool,
    /// LCM DAG info
    dag_info: DagInfoSnapshot,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct DagInfoSnapshot {
    total_nodes: usize,
    summary_nodes: usize,
    large_file_nodes: usize,
    max_depth: u32,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct SkillSnapshot {
    total_skills: usize,
    general_count: usize,
    role_count: usize,
    task_count: usize,
    evolution_epoch: u64,
    top_skills: Vec<SkillInfo>,
    recent_distillations: Vec<String>,
    credit_history: Vec<SkillCreditRecord>,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct SkillInfo {
    id: String,
    title: String,
    principle: String,
    level: String,
    confidence: f64,
    credit_ema: f64,
    status: String,
    usage_count: u64,
    success_rate: f64,
}

// Chat token broadcast for streaming to browser clients
type ChatBroadcast = tokio::sync::broadcast::Sender<String>;

/// Real-time graph activity broadcast (agent activation, memory access, task execution)
type GraphActivityBroadcast = tokio::sync::broadcast::Sender<String>;

#[derive(Clone, Debug)]
pub struct GraphActivityEvent {
    pub event_type: String, // "agent_activate", "memory_access", "task_execute", "belief_form"
    pub agent_id: Option<u64>,
    pub target_id: Option<u64>,
    pub content: String,
    pub timestamp: u64,
}

struct WebApiState {
    snapshot: Arc<RwLock<WorldSnapshot>>,
    chat_broadcast: ChatBroadcast,
    /// Browser → TUI: send chat text + slash commands
    web_cmd_tx: mpsc::UnboundedSender<BgEvent>,
    /// Last grounded context block (for /api/context)
    last_context: Arc<RwLock<String>>,
    /// Council outputs broadcast
    council_broadcast: ChatBroadcast,
    /// Real-time graph activity (agent activation, memory access, etc.)
    graph_activity_broadcast: GraphActivityBroadcast,
    /// Current council mode: "auto", "simple", "debate", "orchestrate", "llm"
    council_mode: Arc<RwLock<String>>,
    /// optimize_anything event broadcast
    optimize_broadcast: ChatBroadcast,
    /// Code agent (Coder) outputs broadcast
    code_broadcast: ChatBroadcast,
    /// Visual explainer progress broadcast
    visual_broadcast: ChatBroadcast,
    /// RooDB handle for semantic vault search
    roodb: Arc<RwLock<Option<Arc<RooDb>>>>,
    /// RooDB URL for on-demand reconnect
    roodb_url: String,
    /// Embedding model for vault search
    embed_model: String,
    /// HTTP client for embedding requests
    embed_client: Client,
    /// Vault directory path
    vault_dir: String,
    /// Current plan steps from last council synthesis
    plan_steps: Arc<RwLock<Vec<PlanStep>>>,
    /// Cache of the most recent reward logs (used when RooDB is offline)
    recent_rewards: Arc<RwLock<Vec<RewardLogRow>>>,
    /// Cache of the most recent skill evidence rows
    recent_skill_evidence: Arc<RwLock<Vec<SkillEvidenceRow>>>,
}

#[derive(Clone, Debug)]
struct CompatGateOutcome {
    approved: bool,
    summary: String,
    policy_decision: String,
    risk_level: String,
    council_required: bool,
    council_mode: Option<String>,
}

// ── App State ───────────────────────────────────────────────────────────

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn embed_model_from_env() -> String {
    env::var("HSM_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string())
}

fn vault_dir_from_env() -> String {
    env::var("HSM_VAULT_DIR").unwrap_or_else(|_| "vault".to_string())
}

fn make_graph_event(
    event_type: &str,
    agent_id: Option<u64>,
    target_id: Option<u64>,
    content: &str,
) -> String {
    json!({
        "type": event_type,
        "agent_id": agent_id,
        "target_id": target_id,
        "content": content,
        "timestamp": unix_timestamp_secs(),
    })
    .to_string()
}

fn map_council_agent(slot: usize, agent_count: usize) -> u64 {
    if agent_count == 0 {
        0
    } else {
        (slot % agent_count) as u64
    }
}

fn normalize_model_alias(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("qwen3-8b-heretic")
        || trimmed.eq_ignore_ascii_case("qwen3-8b-heretic-q6_k")
    {
        return "hf.co/mradermacher/Qwen3-8B-heretic-GGUF:Q6_K".to_string();
    }
    if trimmed.eq_ignore_ascii_case("llama-3.3-8b-heretic") {
        return "hf.co/mradermacher/Llama-3.3-8B-Instruct-heretic-i1-GGUF:Q5_K_M".to_string();
    }
    if trimmed.starts_with("f.co/") {
        return format!("h{}", trimmed);
    }
    if trimmed == "hf.co/mradermacher/Llama-3.3-8B-Instruct-heretic-i1-GGUF:Q5_K_M" {
        return "hf.co/mradermacher/Llama-3.3-8B-Instruct-heretic-i1-GGUF:Q5_K_M".to_string();
    }
    if trimmed == "hf.co/kalle07/llama-3.3-8b-instruct-heretic_R7_KL008_q8_0-gguf:latest"
        || trimmed
            == "huggingface.co/kalle07/llama-3.3-8b-instruct-heretic_R7_KL008_q8_0-gguf:latest"
    {
        return "hf.co/mradermacher/Llama-3.3-8B-Instruct-heretic-i1-GGUF:Q5_K_M".to_string();
    }
    trimmed.to_string()
}

fn bounded_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        input.to_string()
    } else {
        input.chars().take(max_chars).collect::<String>()
    }
}

fn is_prompt_injection_like(input: &str) -> bool {
    let t = input.to_ascii_lowercase();
    t.contains("jailbreak")
        || t.contains("ignore previous instructions")
        || t.contains("system instruction")
        || t.contains("responseformat")
        || t.contains("redactions: disabled")
        || t.contains("ptsd")
}

#[derive(Default)]
struct CouncilThinkingParser {
    buffer: String,
    in_thinking: bool,
}

impl CouncilThinkingParser {
    fn push_chunk(&mut self, chunk: &str) -> Vec<CouncilStreamPart> {
        self.buffer.push_str(chunk);
        let mut out = Vec::new();

        loop {
            if self.in_thinking {
                if let Some(end_idx) = self.buffer.find("</think>") {
                    let segment = self.buffer[..end_idx].to_string();
                    self.buffer.drain(..end_idx + "</think>".len());
                    self.in_thinking = false;
                    if !segment.is_empty() {
                        out.push(CouncilStreamPart::Thinking(segment));
                    }
                    out.push(CouncilStreamPart::ThinkingEnd);
                    continue;
                }
                let reserve = "</think>".len().saturating_sub(1);
                let emit_len = self.buffer.len().saturating_sub(reserve);
                if emit_len == 0 {
                    break;
                }
                let segment = self.buffer[..emit_len].to_string();
                self.buffer.drain(..emit_len);
                if !segment.is_empty() {
                    out.push(CouncilStreamPart::Thinking(segment));
                }
                break;
            } else {
                if let Some(start_idx) = self.buffer.find("<think>") {
                    let segment = self.buffer[..start_idx].to_string();
                    self.buffer.drain(..start_idx + "<think>".len());
                    self.in_thinking = true;
                    if !segment.is_empty() {
                        out.push(CouncilStreamPart::Token(segment));
                    }
                    continue;
                }
                let reserve = "<think>".len().saturating_sub(1);
                let emit_len = self.buffer.len().saturating_sub(reserve);
                if emit_len == 0 {
                    break;
                }
                let segment = self.buffer[..emit_len].to_string();
                self.buffer.drain(..emit_len);
                if !segment.is_empty() {
                    out.push(CouncilStreamPart::Token(segment));
                }
                break;
            }
        }

        out
    }

    fn finish(&mut self) -> Vec<CouncilStreamPart> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let remaining = std::mem::take(&mut self.buffer);
        if self.in_thinking {
            self.in_thinking = false;
            vec![
                CouncilStreamPart::Thinking(remaining),
                CouncilStreamPart::ThinkingEnd,
            ]
        } else {
            vec![CouncilStreamPart::Token(remaining)]
        }
    }
}

struct App {
    world: HyperStigmergicMorphogenesis,
    bid_config: BidConfig,
    // Tab navigation
    active_tab: usize,
    tab_titles: Vec<String>,
    // Tick tracking
    auto_tick: bool,
    tick_speed_ms: u64,
    last_auto_tick: Instant,
    /// Tick count at which the last auto LARS export ran (every LARS_EXPORT_INTERVAL ticks)
    last_lars_export_tick: u64,
    // History for sparklines
    coherence_history: Vec<u64>,
    edge_count_history: Vec<u64>,
    agent_count_history: Vec<u64>,
    // Event log
    event_log: Vec<String>,
    log_scroll: usize,
    // Role bid tracking
    bid_history: HashMap<String, u32>,
    total_bids: u32,
    // Improvement cycle tracking
    improvement_count: u64,
    last_mutation: Option<String>,
    last_novelty: f32,
    last_coherence_delta: f64,
    // Running state
    running: bool,
    // Chat
    chat_messages: Vec<ChatMsg>,
    chat_context_summary: Option<String>, // Compacted summary of older context (legacy)
    chat_input: String,
    chat_models: Vec<(&'static str, &'static str)>, // (display, ollama_model)
    selected_model: usize,
    chat_tx: mpsc::UnboundedSender<ChatEvent>,
    chat_rx: mpsc::UnboundedReceiver<ChatEvent>,
    chat_streaming: bool,
    chat_scroll: usize,
    chat_dspy_session: Arc<Mutex<DspySession<NoopSessionAdapter>>>,
    council_dspy_session: Arc<Mutex<DspySession<NoopSessionAdapter>>>,
    // LCM: Lossless Context Management (new)
    lcm_context: Option<LcmContext>,
    // Memory & Reflection
    memory: HybridMemory,
    tool_registry: ToolRegistry,
    reflect_status: Option<String>,
    reflect_in_progress: bool,
    // Federation
    federation_addr: Option<String>,
    federation_peers: Vec<String>,
    federation_meta_graph: Option<std::sync::Arc<tokio::sync::RwLock<MetaGraph>>>,
    federation_imported: usize,
    federation_exported: usize,
    federation_conflicts: usize,
    // RooDB persistence
    roodb: Option<std::sync::Arc<RooDb>>,
    web_roodb: Arc<RwLock<Option<std::sync::Arc<RooDb>>>>,
    roodb_url: String,
    embed_model: String,
    embed_client: Client,
    vault_dir: String,
    plan_steps: Arc<RwLock<Vec<PlanStep>>>,
    repl_state: Option<Arc<ReplState>>,
    // Background event channel (RooDB / bincode async results)
    bg_tx: mpsc::UnboundedSender<BgEvent>,
    bg_rx: mpsc::UnboundedReceiver<BgEvent>,
    save_in_progress: bool,
    // Live-viz broadcast: sending on this triggers all connected WS clients to reload
    viz_tx: watch::Sender<u64>,
    // Web API shared state (updated after every tick/export)
    web_snapshot: Arc<RwLock<WorldSnapshot>>,
    web_chat_broadcast: ChatBroadcast,
    web_last_context: Arc<RwLock<String>>,
    web_council_broadcast: ChatBroadcast,
    web_graph_activity_broadcast: GraphActivityBroadcast,
    web_code_broadcast: ChatBroadcast,
    web_optimize_broadcast: ChatBroadcast,
    web_visual_broadcast: ChatBroadcast,
    // Pending web /query — fulfilled when QueryResult arrives
    pending_web_query: Option<tokio::sync::oneshot::Sender<String>>,
    code_agent_sessions: HashMap<String, CodeAgentRuntimeSession>,
    // Integration 2: Optimized role prompts from previous failures
    optimized_role_prompts: Option<Vec<(String, String)>>,
    services: RuntimeServices,
    // Live runtime component states (source of truth for web snapshots)
    components: RuntimeComponents,
    llm_stream_started_at: Option<Instant>,
    llm_completed_requests: u64,
    last_skill_extract: Instant,
    skill_extract_interval: Duration,
}

impl App {
    /// Low-compute multi-agent communication pass.
    /// Uses a small sender set and bounded receive budget.
    fn run_lightweight_multi_agent_comm(&mut self) {
        if self.world.agents.is_empty() {
            return;
        }

        let mut ranked = self.world.agents.clone();
        ranked.sort_by(|a, b| {
            let sa = a.drives.curiosity as f64
                + a.drives.growth as f64
                + (1.0 - a.drives.harmony as f64) * 0.25;
            let sb = b.drives.curiosity as f64
                + b.drives.growth as f64
                + (1.0 - b.drives.harmony as f64) * 0.25;
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });

        let sender_budget = ranked.len().min(4);
        let selected = &ranked[..sender_budget];
        let coherence = self.world.global_coherence();

        for (i, sender) in selected.iter().enumerate() {
            let target = if selected.len() > 1 {
                let next = selected[(i + 1) % selected.len()].id;
                Target::Agent(next)
            } else {
                Target::Broadcast
            };
            let msg = Message::new(
                MessageType::Task,
                format!(
                    "tick={} coh={:.3} role={:?} c={:.2} h={:.2} g={:.2}",
                    self.world.tick_count,
                    coherence,
                    sender.role,
                    sender.drives.curiosity,
                    sender.drives.harmony,
                    sender.drives.growth
                ),
            );
            self.send_inter_agent_message(sender.id, msg, target, true);
        }

        self.services.communication.tick();
        let recv_budget = (self.world.agents.len() * 2).clamp(8, 48);
        let _ = self.services.communication.receive_limited(recv_budget);
    }

    fn new() -> Self {
        let world = HyperStigmergicMorphogenesis::new(10);
        let bid_config = BidConfig {
            architect_bias: 1.2,
            catalyst_bias: 1.0,
            chronicler_bias: 0.8,
            exploration_temperature: 0.15,
        };
        let (chat_tx, chat_rx) = mpsc::unbounded_channel();
        let (bg_tx, bg_rx) = mpsc::unbounded_channel();
        let (viz_tx, _viz_rx) = watch::channel(0u64);
        let (web_chat_broadcast, _) = tokio::sync::broadcast::channel(256);
        let (web_council_broadcast, _) = tokio::sync::broadcast::channel(4096);
        let (web_graph_activity_broadcast, _) = tokio::sync::broadcast::channel(256);
        let (web_code_broadcast, _) = tokio::sync::broadcast::channel(256);
        let (web_optimize_broadcast, _) = tokio::sync::broadcast::channel(512);
        let (web_visual_broadcast, _) = tokio::sync::broadcast::channel(64);
        let plan_steps = Arc::new(RwLock::new(Vec::new()));
        let mut dks = DKSSystem::new(DKSConfig::default());
        dks.seed(32);
        let cass = CASS::new(world.skill_bank.clone());
        let navigator = CodeNavigator::new();
        let communication = CommunicationHub::new(0, CommunicationConfig::default());
        let llm_server = ModelServer::new();
        let gpu_available = GpuAccelerator::is_available();
        let chat_session = Arc::new(Mutex::new(DspySession::new(NoopSessionAdapter)));
        let council_session = Arc::new(Mutex::new(DspySession::new(NoopSessionAdapter)));

        let mut app = App {
            world,
            bid_config,
            active_tab: 0,
            tab_titles: vec![
                "Dashboard".into(),
                "Agents".into(),
                "Hypergraph".into(),
                "Improvement".into(),
                "Log".into(),
                "Chat".into(),
                "Federation".into(),
            ],
            auto_tick: false,
            tick_speed_ms: 200,
            last_auto_tick: Instant::now(),
            last_lars_export_tick: 0,
            coherence_history: vec![0; 60],
            edge_count_history: vec![0; 60],
            agent_count_history: vec![0; 60],
            event_log: vec![
                "System initialized with 10 agents".into(),
                "Roles: Architect, Catalyst, Chronicler".into(),
                "Press [h] for help".into(),
            ],
            log_scroll: 0,
            bid_history: HashMap::from([
                ("Architect".into(), 0),
                ("Catalyst".into(), 0),
                ("Chronicler".into(), 0),
            ]),
            total_bids: 0,
            improvement_count: 0,
            last_mutation: None,
            last_novelty: 0.0,
            last_coherence_delta: 0.0,
            running: true,
            // Chat
            chat_messages: Vec::new(),
            chat_context_summary: None,
            chat_input: String::new(),
            chat_models: vec![
                (
                    "qwen3-8b-heretic-q6_k",
                    "hf.co/mradermacher/Qwen3-8B-heretic-GGUF:Q6_K",
                ),
                (
                    "llama-3.3-8b-heretic",
                    "hf.co/mradermacher/Llama-3.3-8B-Instruct-heretic-i1-GGUF:Q5_K_M",
                ),
            ],
            selected_model: 0, // Default to qwen3-8b-heretic-q6_k
            chat_tx,
            chat_rx,
            chat_streaming: false,
            chat_scroll: 0,
            chat_dspy_session: chat_session.clone(),
            council_dspy_session: council_session.clone(),
            lcm_context: None, // Will be initialized lazily on first use
            // Memory & Reflection
            memory: HybridMemory::new(),
            tool_registry: default_tool_registry(),
            reflect_status: None,
            reflect_in_progress: false,
            // Federation
            federation_addr: None,
            federation_peers: Vec::new(),
            federation_meta_graph: None,
            federation_imported: 0,
            federation_exported: 0,
            federation_conflicts: 0,
            // RooDB
            roodb: None,
            web_roodb: Arc::new(RwLock::new(None)),
            roodb_url: "127.0.0.1:3307".to_string(),
            embed_model: embed_model_from_env(),
            embed_client: Client::new(),
            vault_dir: vault_dir_from_env(),
            plan_steps,
            repl_state: None,
            // Background events
            bg_tx,
            bg_rx,
            save_in_progress: false,
            // Live-viz
            viz_tx,
            // Web API
            web_snapshot: Arc::new(RwLock::new(WorldSnapshot::default())),
            web_chat_broadcast,
            web_last_context: Arc::new(RwLock::new(String::new())),
            web_council_broadcast,
            web_graph_activity_broadcast,
            web_code_broadcast,
            web_optimize_broadcast,
            web_visual_broadcast,
            pending_web_query: None,
            code_agent_sessions: HashMap::new(),
            // Integration 2: Store optimized role prompts
            optimized_role_prompts: None,
            services: RuntimeServices {
                dks,
                cass,
                cass_initialized: false,
                navigator,
                communication,
                kuramoto: KuramotoEngine::default(),
                last_kuramoto_world_tick: None,
                llm_server,
                gpu_available,
                last_dks_tick: None,
            },
            components: RuntimeComponents::default(),
            llm_stream_started_at: None,
            llm_completed_requests: 0,
            last_skill_extract: Instant::now(),
            skill_extract_interval: Duration::from_secs(
                env::var("HSM_SKILL_EXTRACT_INTERVAL")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(120),
            ),
        };

        app.components.council.mode = "auto".to_string();
        app.components.llm.model_name = Some(app.chat_models[app.selected_model].1.to_string());

        // Seed some initial edges so the graph isn't empty
        for i in 0..5 {
            app.world.apply_action_with_agent(
                &Action::LinkAgents {
                    vertices: vec![i, (i + 1) % 10, (i + 3) % 10],
                    weight: 1.0 + (i as f32) * 0.2,
                },
                Some(0),
            );
        }
        app.log("Initial edges seeded (5 hyperedges)");

        // Register a concrete collaboration module tool for actionable recommendations.
        if let Some(tool_meta) = app
            .world
            .vertex_meta
            .iter_mut()
            .find(|v| v.kind == VertexKind::Tool)
        {
            tool_meta.name = "collaboration_module".to_string();
            tool_meta.modified_at =
                hyper_stigmergy::HyperStigmergicMorphogenesis::current_timestamp();
        }
        let collab_msg = Message::new(
            MessageType::Info,
            "Collaboration module available: task-router, shared-memory, handoff-protocol. Use it for explicit agent-to-agent task routing and shared context."
        );
        app.send_inter_agent_message(0, collab_msg, Target::Broadcast, true);

        app.record_snapshot();
        app
    }

    /// Ensure LCM context is initialized (lazy initialization)
    async fn ensure_lcm_initialized(&mut self) {
        if self.lcm_context.is_none() {
            let storage: Box<dyn LcmStorage> = if let Some(ref db) = self.roodb {
                let storage = RooStorage::new(db.clone());
                match storage.ensure_schema().await {
                    Ok(()) => Box::new(storage),
                    Err(e) => {
                        self.log(&format!("RooDB storage setup failed: {}", e));
                        Box::new(MemoryStorage::new())
                    }
                }
            } else {
                Box::new(MemoryStorage::new())
            };

            match LcmContext::new(storage).await {
                Ok(lcm) => {
                    self.lcm_context = Some(lcm);
                    self.log("LCM: Lossless Context Management initialized");
                }
                Err(e) => {
                    self.log(&format!("LCM initialization failed: {}", e));
                }
            }
        }
    }

    async fn ensure_runtime_services_initialized(&mut self) {
        if !self.services.cass_initialized {
            match self.services.cass.initialize().await {
                Ok(_) => {
                    self.services.cass_initialized = true;
                    self.log("CASS initialized with semantic embeddings");
                }
                Err(e) => {
                    self.log(&format!("CASS initialization failed: {}", e));
                }
            }
        }
    }

    /// Add a message to LCM (both legacy chat_messages and new LCM)
    async fn add_chat_message(&mut self, role: &str, content: &str, model: &str) {
        // Always add to legacy chat_messages for backward compatibility
        self.chat_messages.push(ChatMsg {
            role: role.to_string(),
            content: content.to_string(),
            model: model.to_string(),
        });

        self.ensure_runtime_services_initialized().await;
        // Also add to LCM if available
        self.ensure_lcm_initialized().await;
        if let Some(ref mut lcm) = self.lcm_context {
            if let Err(e) = lcm.add_message(role, content).await {
                self.log(&format!("LCM add_message error: {}", e));
            }
        }
    }

    /// Lightweight sync add (avoids async runtime issues)
    fn add_chat_message_sync(&mut self, role: &str, content: &str, model: &str) {
        self.chat_messages.push(ChatMsg {
            role: role.to_string(),
            content: content.to_string(),
            model: model.to_string(),
        });
    }

    /// Add a large file to LCM context
    #[allow(dead_code)]
    async fn add_large_file_to_context(
        &mut self,
        path: &str,
        content: &str,
        mime_type: &str,
    ) -> Option<String> {
        self.ensure_lcm_initialized().await;
        if let Some(ref mut lcm) = self.lcm_context {
            match lcm.add_large_file(path, content, mime_type).await {
                Ok(node_id) => {
                    self.log(&format!("LCM: Added large file '{}' ({})", path, node_id));
                    return Some(node_id);
                }
                Err(e) => {
                    self.log(&format!("LCM add_large_file error: {}", e));
                }
            }
        }
        None
    }

    /// Build active context using LCM (if available) or fallback to legacy method
    fn build_lcm_context(&self) -> String {
        if let Some(ref lcm) = self.lcm_context {
            let context = lcm.build_active_context();
            // Also add grounded world data
            let grounded = self.build_grounded_context("");
            if !grounded.is_empty() {
                format!("{}\n\n[Grounded World Data]\n{}", context, grounded)
            } else {
                context
            }
        } else {
            // Fallback to legacy method
            let mut context = String::new();

            // Add recent messages
            let recent_count = 20;
            let start = self.chat_messages.len().saturating_sub(recent_count);
            for msg in &self.chat_messages[start..] {
                context.push_str(&format!("{}: {}\n", msg.role, msg.content));
            }

            context
        }
    }

    /// Check if context needs compaction using LCM
    async fn maybe_compact_lcm_context(&mut self) {
        self.ensure_lcm_initialized().await;
        if let Some(ref mut lcm) = self.lcm_context {
            match lcm.maybe_compact().await {
                CompactionResult::Compacted {
                    summarized_count,
                    summary_id,
                    saved_tokens,
                } => {
                    self.log(&format!(
                        "LCM: Compacted {} messages into summary {} (saved ~{} tokens)",
                        summarized_count, summary_id, saved_tokens
                    ));
                    // Also update legacy summary for backward compatibility
                    self.chat_context_summary = Some(format!("LCM summary: {}", summary_id));
                }
                CompactionResult::AsyncTriggered => {
                    self.log("LCM: Async compaction triggered");
                }
                _ => {}
            }
        }
    }

    fn log(&mut self, msg: &str) {
        let tick = self.world.tick_count;
        self.event_log.push(format!("[t={}] {}", tick, msg));
        // Keep last 200 entries
        if self.event_log.len() > 200 {
            self.event_log.remove(0);
        }
    }

    fn track_navigation_search(&mut self, query: &str) {
        if query.trim().is_empty() {
            return;
        }
        self.components
            .navigation
            .recent_searches
            .push(truncate_str(query, 120));
        if self.components.navigation.recent_searches.len() > 20 {
            self.components.navigation.recent_searches.remove(0);
        }
    }

    fn mark_llm_request_start(&mut self, model_name: &str) {
        self.llm_stream_started_at = Some(Instant::now());
        self.components.llm.model_loaded = true;
        self.components.llm.model_name = Some(model_name.to_string());
    }

    fn mark_llm_token_emitted(&mut self, token: &str) {
        self.components.llm.tokens_generated += Self::estimate_tokens(token) as u64;
    }

    fn mark_llm_request_done(&mut self) {
        if let Some(started_at) = self.llm_stream_started_at.take() {
            let latency_ms = started_at.elapsed().as_secs_f64() * 1000.0;
            self.llm_completed_requests += 1;
            let n = self.llm_completed_requests as f64;
            let prev = self.components.llm.avg_latency_ms;
            self.components.llm.avg_latency_ms = if n <= 1.0 {
                latency_ms
            } else {
                ((prev * (n - 1.0)) + latency_ms) / n
            };
        }
    }

    fn refresh_kuramoto_component(&mut self) {
        let present_ids: std::collections::HashSet<u64> =
            self.world.agents.iter().map(|a| a.id).collect();

        let stale_ids: Vec<u64> = self
            .services
            .kuramoto
            .oscillators
            .keys()
            .copied()
            .filter(|id| !present_ids.contains(id))
            .collect();
        for id in stale_ids {
            self.services.kuramoto.remove_agent(id);
        }

        for agent in &self.world.agents {
            if !self.services.kuramoto.oscillators.contains_key(&agent.id) {
                self.services.kuramoto.register_agent(
                    agent.id,
                    agent.jw,
                    agent.drives.curiosity,
                    agent.drives.transcendence,
                );
            } else {
                self.services.kuramoto.update_frequency(
                    agent.id,
                    agent.jw,
                    agent.drives.curiosity,
                    agent.drives.transcendence,
                );
            }
        }

        // Council phase acts as an optional global synchronization bias.
        if self.components.council.active {
            let confidence = self.world.global_coherence().clamp(0.0, 1.0);
            let council_phase = confidence_to_phase(confidence, 1.0);
            self.services.kuramoto.set_council_phase(council_phase);
        } else {
            self.services.kuramoto.clear_council_phase();
        }

        let adjacency =
            kuramoto_build_adjacency(&self.world.agents, &self.world.edges, &self.world.adjacency);

        if self.services.last_kuramoto_world_tick != Some(self.world.tick_count) {
            self.services.kuramoto.step(&adjacency);
            self.services.last_kuramoto_world_tick = Some(self.world.tick_count);
        }

        self.components.kuramoto = Some(self.services.kuramoto.snapshot());
    }

    fn refresh_component_state_from_world(&mut self) {
        let tick = self.world.tick_count;
        let _ = &self.services.llm_server;
        let dks_stats = self.services.dks.stats();
        let comm_stats = self.services.communication.gossip_stats();
        let nav_stats = self.services.navigator.stats_snapshot();

        self.components.council.member_count = self.world.agents.len().min(5);
        self.components.council.recent_decisions = self
            .world
            .improvement_history
            .iter()
            .rev()
            .take(5)
            .map(|e| format!("{:?}: {}", e.mutation_type, e.intent))
            .collect();

        let multifractal = self.services.dks.multifractal_spectrum_points(9);
        let dks_stability = calculate_dks_stability(
            dks_stats.average_replication_rate,
            dks_stats.average_decay_rate,
        );

        self.components.dks = DKSSnapshot {
            generation: self.services.dks.generation().max(
                self.services
                    .last_dks_tick
                    .as_ref()
                    .map(|r| r.generation)
                    .unwrap_or(tick),
            ),
            population_size: dks_stats.size,
            avg_persistence: dks_stats.average_persistence,
            replicator_count: dks_stats.size,
            // Non-equilibrium persistence proxy: replication vs decay balance.
            flux_intensity: dks_stability,
            multifractal_spectrum: multifractal,
        };

        self.components.cass = CASSSnapshot {
            skill_count: self.services.cass.skill_count(),
            context_depth: self.services.cass.context_depth(),
            recent_matches: self
                .world
                .beliefs
                .iter()
                .rev()
                .take(5)
                .map(|b| b.content.clone())
                .collect(),
            embedding_dimension: self.services.cass.embedding_dimension(),
        };

        let indexed_files = self
            .lcm_context
            .as_ref()
            .map(|l| {
                let stats = l.get_stats();
                stats.summary_count + stats.large_file_count
            })
            .unwrap_or(0);

        self.components.navigation.indexed_files = indexed_files.max(nav_stats.total_units);
        self.components.navigation.topics = self
            .world
            .ontology
            .iter()
            .take(8)
            .map(|(k, _)| k.clone())
            .collect();

        self.components.communication.active_gossip_rounds = comm_stats.rumors_active;
        self.components.communication.swarm_agents = self.world.agents.len();
        self.components.communication.stigmergic_fields =
            self.world.edges.iter().filter(|e| e.emergent).count();
        self.components.communication.message_throughput =
            comm_stats.messages_sent + comm_stats.messages_received;
        self.refresh_kuramoto_component();

        self.components.gpu = GpuSnapshot {
            available: self.services.gpu_available,
            device_name: None,
            compute_load: None,
            memory_used_mb: None,
            fallback_active: !self.services.gpu_available,
        };

        self.components.llm.cache_hit_rate = self.lcm_context.as_ref().and_then(|lcm| {
            let stats = lcm.get_stats();
            let denom = stats.regular_tokens + stats.cache_read_tokens;
            if denom == 0 {
                None
            } else {
                Some((stats.cache_read_tokens as f64 / denom as f64).clamp(0.0, 1.0))
            }
        });
        self.components.email.inbox_unread = None;
        self.components.email.classified_today = None;
        self.components.email.auto_responses_sent = None;
        self.components.email.memory_entries = self.world.beliefs.len();
    }

    /// Broadcast real-time graph activity for visualization
    fn broadcast_graph_activity(
        &self,
        event_type: &str,
        agent_id: Option<u64>,
        target_id: Option<u64>,
        content: &str,
    ) {
        let _ = self
            .web_graph_activity_broadcast
            .send(make_graph_event(event_type, agent_id, target_id, content));
    }

    fn record_snapshot(&mut self) {
        let c = (self.world.global_coherence() * 100.0) as u64;
        self.coherence_history.push(c);
        if self.coherence_history.len() > 60 {
            self.coherence_history.remove(0);
        }

        self.edge_count_history.push(self.world.edges.len() as u64);
        if self.edge_count_history.len() > 60 {
            self.edge_count_history.remove(0);
        }

        self.agent_count_history
            .push(self.world.agents.len() as u64);
        if self.agent_count_history.len() > 60 {
            self.agent_count_history.remove(0);
        }

        self.refresh_component_state_from_world();
        // Keep web API snapshot fresh on every state change
        self.update_web_snapshot();
    }

    fn do_tick(&mut self) {
        self.world.tick();

        // SkillRL integration in interactive runtime:
        // distill skills continuously from experiences and evolve periodically.
        let distill_result = self
            .world
            .skill_bank
            .distill_from_experiences(&self.world.experiences, &self.world.improvement_history);
        if distill_result.new_skills > 0 {
            self.log(&format!(
                "SkillRL: distilled {} new skill(s) (epoch {})",
                distill_result.new_skills, self.world.skill_bank.evolution_epoch
            ));
        }
        if self.world.tick_count % 10 == 0 {
            let failed_experiences: Vec<_> = self
                .world
                .experiences
                .iter()
                .filter(|e| {
                    matches!(
                        e.outcome,
                        hyper_stigmergy::ExperienceOutcome::Negative { .. }
                    )
                })
                .cloned()
                .collect();
            let evolve_result = self.world.skill_bank.evolve(&failed_experiences);
            if evolve_result.skills_refined > 0 || evolve_result.skills_deprecated > 0 {
                self.log(&format!(
                    "SkillRL: evolved skills (refined={}, deprecated={})",
                    evolve_result.skills_refined, evolve_result.skills_deprecated
                ));
            }
        }

        let dks_tick = self.services.dks.tick();
        self.services.last_dks_tick = Some(dks_tick);
        let dominant_roles: Vec<Role> = self
            .world
            .agents
            .iter()
            .take(3)
            .map(|a| a.role.clone())
            .collect();
        let recent_skills_used: Vec<String> = self
            .world
            .skill_bank
            .all_skills()
            .iter()
            .rev()
            .take(5)
            .map(|s| s.id.clone())
            .collect();
        self.services.cass.update_context(CassContextSnapshot {
            timestamp: self.world.tick_count,
            active_agents: self.world.agents.iter().map(|a| a.id).collect(),
            dominant_roles,
            current_goals: self
                .last_mutation
                .clone()
                .map(|m| vec![m])
                .unwrap_or_default(),
            recent_skills_used,
            system_load: (self.world.edges.len() as f64 / 100.0).min(1.0),
            error_rate: if self.world.global_coherence() < 0.35 {
                0.15
            } else {
                0.02
            },
            coherence_score: self.world.global_coherence(),
        });
        self.run_lightweight_multi_agent_comm();

        // Integration 4: Trigger belief re-evaluation if scheduled
        if self.world.should_reevaluate_beliefs() {
            self.trigger_belief_reevaluation();
        }

        self.record_snapshot();
    }

    /// Integration 4: Trigger async belief re-evaluation against recent experiences
    fn trigger_belief_reevaluation(&mut self) {
        if self.world.beliefs.is_empty() {
            return;
        }

        // Get the 5 oldest beliefs for re-evaluation
        let beliefs_to_check: Vec<(usize, String, f64)> = self
            .world
            .beliefs
            .iter()
            .enumerate()
            .take(5)
            .map(|(idx, b)| (idx, b.content.clone(), b.confidence))
            .collect();

        // Get recent experiences as context
        let recent_experiences: Vec<String> = self
            .world
            .experiences
            .iter()
            .rev()
            .take(10)
            .map(|e| format!("{}: {:?}", e.description, e.outcome))
            .collect();

        let model = self.chat_models[self.selected_model].1.to_string();
        let bg_tx = self.bg_tx.clone();

        self.log(&format!(
            "🔄 Triggering belief re-evaluation for {} beliefs",
            beliefs_to_check.len()
        ));

        tokio::spawn(async move {
            for (idx, content, old_confidence) in beliefs_to_check {
                match reevaluate_belief(&content, &recent_experiences, &model).await {
                    Ok(new_confidence) => {
                        let delta = new_confidence - old_confidence;
                        let status = if delta > 0.1 {
                            "↑ strengthened"
                        } else if delta < -0.1 {
                            "↓ weakened"
                        } else {
                            "→ stable"
                        };
                        println!(
                            "[belief_reeval] Belief {}: {} (conf: {:.2} → {:.2})",
                            idx, status, old_confidence, new_confidence
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "[belief_reeval] Failed to re-evaluate belief {}: {}",
                            idx, e
                        );
                    }
                }
            }
            // Send completion event
            let _ = bg_tx.send(BgEvent::Log("Belief re-evaluation complete".to_string()));
        });
    }

    fn do_bid_round(&mut self) {
        let role = self.world.select_role_via_bidding(&self.bid_config);
        let name = format!("{:?}", role);
        *self.bid_history.entry(name.clone()).or_insert(0) += 1;
        self.total_bids += 1;
        self.log(&format!("Bid won by {}", name));
    }

    fn do_improvement(&mut self) {
        let intents = [
            "optimize edge density for information flow",
            "strengthen weak connections between clusters",
            "introduce novel structural patterns",
            "balance agent role distribution",
            "expand ontology into emergent domains",
            "rewire stale pathways for fresh signal propagation",
        ];
        let intent = intents[self.improvement_count as usize % intents.len()];
        let result = self.world.execute_self_improvement_cycle(intent);
        self.improvement_count += 1;
        self.last_mutation = result.mutation_applied.clone();
        self.last_novelty = result.event.novelty_score;
        self.last_coherence_delta = result.coherence_delta;
        self.log(&format!(
            "Improvement #{}: {:?} | delta={:+.4} | novelty={:.2}",
            self.improvement_count,
            result.mutation_applied.as_deref().unwrap_or("none"),
            result.coherence_delta,
            result.event.novelty_score,
        ));
        self.record_snapshot();
    }

    fn do_link_random(&mut self) {
        let n = self.world.agents.len();
        if n < 2 {
            return;
        }
        let a = rand::random::<usize>() % n;
        let b = (a + 1 + rand::random::<usize>() % (n - 1)) % n;
        let w = 0.5 + rand::random::<f64>() * 1.5;
        self.world.apply_action_with_agent(
            &Action::LinkAgents {
                vertices: vec![a, b],
                weight: w as f32,
            },
            Some(0),
        );
        self.log(&format!("Linked agents {} <-> {} (w={:.2})", a, b, w));
        self.record_snapshot();
    }

    fn do_save(&mut self) {
        if self.save_in_progress {
            self.log("Save already in progress, skipping");
            return;
        }
        self.save_in_progress = true;
        self.log("Saving...");

        let world = self.world.clone();
        let db = self.roodb.clone();
        let bg = self.bg_tx.clone();

        tokio::spawn(async move {
            let mut errors = Vec::new();

            // Embedded single-file store save on a blocking thread (fs::write blocks)
            let bincode_ok = {
                let w = world.clone();
                match tokio::task::spawn_blocking(move || w.save_to_disk(None)).await {
                    Ok(Ok(())) => true,
                    Ok(Err(e)) => {
                        errors.push(format!("embedded-store: {}", e));
                        false
                    }
                    Err(e) => {
                        errors.push(format!("embedded-store task: {}", e));
                        false
                    }
                }
            };

            // RooDB save (already async)
            let db_snapshot_id = if let Some(db) = db {
                match db.save(&world, None).await {
                    Ok(id) => Some(id),
                    Err(e) => {
                        errors.push(format!("roodb: {}", e));
                        None
                    }
                }
            } else {
                None
            };

            let _ = bg.send(BgEvent::SaveComplete {
                bincode_ok,
                db_snapshot_id,
                errors,
            });
        });
    }

    async fn do_save_with_compat_gate(&mut self, requested_by: &str) -> String {
        let gate_action = ProposedAction {
            id: format!("save-{}", HyperStigmergicMorphogenesis::current_timestamp()),
            title: "Save Snapshot".to_string(),
            description: "Persist world state to local storage and optional RooDB snapshot"
                .to_string(),
            actor_id: requested_by.to_string(),
            kind: hyper_stigmergy::OuroborosActionKind::ExternalWrite,
            target_path: Some("world_state.ladybug.bincode".to_string()),
            target_peer: None,
            metadata: HashMap::from([
                (
                    "touches_external_system".to_string(),
                    self.roodb.is_some().to_string(),
                ),
                ("operation".to_string(), "save_snapshot".to_string()),
            ]),
        };
        let now_str = HyperStigmergicMorphogenesis::current_timestamp().to_string();
        let gate_evidence = EvidenceBundle {
            investigation_session_id: Some(format!("save-{}", gate_action.id)),
            tool_calls: vec![hyper_stigmergy::investigation_tools::ToolCallRecord {
                id: format!("tc-{}", gate_action.id),
                tool_name: "save_snapshot".to_string(),
                arguments: json!({
                    "writes_embedded_store": true,
                    "writes_roodb": self.roodb.is_some(),
                }),
                result_value: None,
                error_message: None,
                started_at: now_str.clone(),
                completed_at: now_str,
            }],
            evidence_chain_count: 1,
            claim_count: 1,
            evidence_count: 1,
            coverage: 1.0,
        };
        let gate_outcome = self
            .evaluate_compat_gate(gate_action.clone(), gate_evidence, requested_by.to_string())
            .await;
        self.persist_compat_gate_audit(&gate_action, &gate_outcome);
        self.persist_compat_memory_event(
            "ActionAudited",
            json!({
                "action_id": gate_action.id,
                "approved": gate_outcome.approved,
                "kind": "ExternalWrite",
                "summary": gate_outcome.summary,
            }),
        );

        if !gate_outcome.approved {
            let msg = format!(
                "Save blocked by compatibility gate: {}",
                gate_outcome.summary
            );
            self.log(&format!("⛔ {}", msg));
            return msg;
        }

        self.do_save();
        "Saving… (embedded graph store + RooDB if connected). Viz will auto-update.".to_string()
    }

    fn do_load(&mut self) {
        let bg = self.bg_tx.clone();
        tokio::spawn(async move {
            match tokio::task::spawn_blocking(|| HyperStigmergicMorphogenesis::load_from_disk())
                .await
            {
                Ok(Ok((world, _rlm))) => {
                    let _ = bg.send(BgEvent::WorldLoaded {
                        world: Box::new(world),
                        source: "embedded-store",
                    });
                }
                Ok(Err(e)) => {
                    let _ = bg.send(BgEvent::Log(format!("Embedded store load failed: {}", e)));
                }
                Err(e) => {
                    let _ = bg.send(BgEvent::Log(format!("Load task panicked: {}", e)));
                }
            }
        });
        self.log("Loading from disk...");
    }

    fn do_load_db(&mut self) {
        if let Some(ref db) = self.roodb {
            let db = db.clone();
            let bg = self.bg_tx.clone();
            tokio::spawn(async move {
                match db.load_latest().await {
                    Ok((world, _rlm)) => {
                        let _ = bg.send(BgEvent::WorldLoaded {
                            world: Box::new(world),
                            source: "roodb",
                        });
                    }
                    Err(e) => {
                        let _ = bg.send(BgEvent::Log(format!("RooDB load failed: {}", e)));
                    }
                }
            });
            self.log("Loading latest snapshot from RooDB...");
        } else {
            self.log("RooDB not connected (use --roodb <url>)");
        }
    }

    fn do_export_json(&mut self) {
        self.do_export_viz();
    }

    /// Export viz/hyper_graph.json and signal all connected browser clients to reload.
    fn do_export_viz(&mut self) {
        let viz_path = "viz/hyper_graph.json";
        // Also keep root-level copy for backward compat with open-viz.command
        let root_path = "hyper_graph.json";
        match self.world.export_json(viz_path) {
            Ok(_) => {
                let _ = self.world.export_json(root_path);
                // Bump the watch counter — the WS server will relay this to the browser
                let tick = self.world.tick_count;
                let _ = self.viz_tx.send(tick);
                self.log(&format!("Exported → {} (viz reloading)", viz_path));
            }
            Err(e) => self.log(&format!("Export failed: {}", e)),
        }
    }

    fn gate_release_state_from_env() -> ReleaseState {
        ReleaseState {
            version: std::env::var("OUROBOROS_VERSION").ok(),
            git_tag: std::env::var("OUROBOROS_GIT_TAG").ok(),
            readme_version: std::env::var("OUROBOROS_README_VERSION").ok(),
        }
    }

    fn gate_default_council_members(&self) -> Vec<CouncilMember> {
        let mut out: Vec<CouncilMember> = self
            .world
            .agents
            .iter()
            .take(6)
            .map(|a| CouncilMember {
                agent_id: a.id,
                role: a.role,
                expertise_score: 0.8 + (a.jw * 0.2).clamp(0.0, 0.2),
                participation_weight: 1.0,
            })
            .collect();
        if out.is_empty() {
            out.push(CouncilMember {
                agent_id: 0,
                role: Role::Architect,
                expertise_score: 0.8,
                participation_weight: 1.0,
            });
        }
        out
    }

    async fn gate_mean_trust_snapshot(&self) -> f64 {
        if let Some(meta_graph_arc) = &self.federation_meta_graph {
            let mg = meta_graph_arc.read().await;
            if mg.trust_graph.edges.is_empty() {
                mg.trust_graph.default_trust
            } else {
                let sum: f64 = mg.trust_graph.edges.values().map(|e| e.score).sum();
                sum / mg.trust_graph.edges.len() as f64
            }
        } else if let Some(cfg) = &self.world.federation_config {
            cfg.trust_threshold
        } else {
            0.70
        }
    }

    async fn evaluate_compat_gate(
        &self,
        action: ProposedAction,
        evidence: EvidenceBundle,
        requested_by: String,
    ) -> CompatGateOutcome {
        let policy = PolicyEngine::new(ConstitutionConfig::default()).evaluate(
            &action,
            &PolicyContext {
                requested_by,
                release_state: Self::gate_release_state_from_env(),
            },
        );
        let risk = RiskGate::new(RiskGateConfig::default()).assess(&action, &policy);
        let bridge = CouncilBridge::new(CouncilBridgeConfig::default());
        let plan = bridge.plan(&action, &risk, &self.gate_default_council_members());
        let evidence_validation =
            EvidenceContract::new(EvidenceRequirements::default()).validate(&evidence);

        let mean_trust = self.gate_mean_trust_snapshot().await;
        let council_confidence = plan
            .mode_report
            .as_ref()
            .map(|r| r.confidence)
            .unwrap_or_else(|| self.world.global_coherence().clamp(0.0, 1.0));
        let slo = evaluate_runtime_slos(
            &RuntimeSnapshot {
                coherence: self.world.global_coherence(),
                stability: self.components.dks.avg_persistence,
                mean_trust,
                council_confidence: Some(council_confidence),
                evidence_coverage: Some(evidence.coverage),
            },
            &RuntimeThresholds::default(),
        );

        let policy_allows_execution = !matches!(policy.decision, PolicyDecision::Deny);
        let approved = if risk.council_required {
            bridge.should_approve(
                council_confidence,
                evidence.coverage,
                policy_allows_execution && evidence_validation.ok && slo.healthy,
            )
        } else {
            policy_allows_execution && slo.healthy
        };

        let council_mode = plan
            .mode_report
            .as_ref()
            .map(|r| format!("{:?}", r.selected_mode));
        let mut parts = Vec::new();
        if !approved {
            if !policy.reasons.is_empty() {
                parts.push(format!("policy={}", policy.reasons.join("; ")));
            }
            if !risk.reasons.is_empty() {
                parts.push(format!("risk={}", risk.reasons.join("; ")));
            }
            if !evidence_validation.reasons.is_empty() {
                parts.push(format!(
                    "evidence={}",
                    evidence_validation.reasons.join("; ")
                ));
            }
            if !slo.failed_checks.is_empty() {
                parts.push(format!("slo={}", slo.failed_checks.join("; ")));
            }
        }
        let summary = if parts.is_empty() {
            format!(
                "approved (policy={:?}, risk={:?}, council={})",
                policy.decision,
                risk.level,
                council_mode.clone().unwrap_or_else(|| "none".to_string())
            )
        } else {
            parts.join(" | ")
        };

        CompatGateOutcome {
            approved,
            summary,
            policy_decision: format!("{:?}", policy.decision),
            risk_level: format!("{:?}", risk.level),
            council_required: risk.council_required,
            council_mode,
        }
    }

    fn persist_compat_gate_audit(&self, action: &ProposedAction, outcome: &CompatGateOutcome) {
        if let Some(ref db) = self.roodb {
            let db = db.clone();
            let row = OuroborosGateAuditRow {
                action_id: action.id.clone(),
                action_kind: format!("{:?}", action.kind),
                risk_level: outcome.risk_level.clone(),
                policy_decision: outcome.policy_decision.clone(),
                council_required: outcome.council_required,
                council_mode: outcome.council_mode.clone(),
                approved: outcome.approved,
                reason: Some(outcome.summary.clone()),
                created_at: HyperStigmergicMorphogenesis::current_timestamp(),
            };
            tokio::spawn(async move {
                if let Err(err) = db.insert_ouroboros_gate_audit(&row).await {
                    eprintln!("[CompatGate] failed to persist gate audit: {}", err);
                }
            });
        }
    }

    fn persist_compat_memory_event(&self, event_kind: &str, payload: serde_json::Value) {
        if let Some(ref db) = self.roodb {
            let db = db.clone();
            let row = OuroborosMemoryEventRow {
                event_id: uuid::Uuid::new_v4().to_string(),
                event_kind: event_kind.to_string(),
                payload: payload.to_string(),
                created_at: HyperStigmergicMorphogenesis::current_timestamp(),
            };
            tokio::spawn(async move {
                if let Err(err) = db.insert_ouroboros_memory_event(&row).await {
                    eprintln!("[CompatGate] failed to persist memory event: {}", err);
                }
            });
        }
    }

    fn shared_edge_to_injection_request(
        edge: &hyper_stigmergy::SharedEdge,
    ) -> HyperedgeInjectionRequest {
        HyperedgeInjectionRequest {
            vertices: edge.vertices.clone(),
            edge_type: edge.edge_type.clone(),
            scope: EdgeScope::Shared,
            trust_tags: edge.trust_tags.clone(),
            provenance: edge.provenance.clone(),
            weight: edge.weight,
            embedding: edge.embedding.clone(),
            metadata: HashMap::from([
                ("source_edge_id".to_string(), edge.id.clone()),
                ("layer".to_string(), format!("{:?}", edge.layer)),
            ]),
        }
    }

    async fn do_federation_sync(&mut self) -> String {
        let Some(config) = self.world.federation_config.clone() else {
            return "Federation INACTIVE\nStart with: --federation <addr> --peer <url>".to_string();
        };

        if self.federation_peers.is_empty() {
            return "Federation ACTIVE, but no peers configured.\nUse: /federation add <peer_url>"
                .to_string();
        }

        let Some(meta_graph_arc) = self.federation_meta_graph.clone() else {
            return "Federation meta-graph unavailable (not initialized).".to_string();
        };

        let gate_action = ProposedAction {
            id: format!(
                "federation-sync-{}",
                HyperStigmergicMorphogenesis::current_timestamp()
            ),
            title: "Federation Sync".to_string(),
            description: format!(
                "Synchronize shared knowledge with {} peer(s)",
                self.federation_peers.len()
            ),
            actor_id: "operator".to_string(),
            kind: hyper_stigmergy::OuroborosActionKind::FederationSync,
            target_path: None,
            target_peer: Some(self.federation_peers.join(",")),
            metadata: HashMap::from([("touches_external_system".to_string(), "true".to_string())]),
        };
        let now_str = HyperStigmergicMorphogenesis::current_timestamp().to_string();
        let gate_evidence = EvidenceBundle {
            investigation_session_id: Some(format!("federation-{}", gate_action.id)),
            tool_calls: vec![hyper_stigmergy::investigation_tools::ToolCallRecord {
                id: format!("tc-{}", gate_action.id),
                tool_name: "federation_sync".to_string(),
                arguments: json!({
                    "peer_count": self.federation_peers.len(),
                    "peers": self.federation_peers.clone(),
                }),
                result_value: None,
                error_message: None,
                started_at: now_str.clone(),
                completed_at: now_str,
            }],
            evidence_chain_count: 1,
            claim_count: 1,
            evidence_count: 1,
            coverage: 1.0,
        };
        let gate_outcome = self
            .evaluate_compat_gate(gate_action.clone(), gate_evidence, "operator".to_string())
            .await;
        self.persist_compat_gate_audit(&gate_action, &gate_outcome);
        self.persist_compat_memory_event(
            "ActionAudited",
            json!({
                "action_id": gate_action.id,
                "approved": gate_outcome.approved,
                "kind": "FederationSync",
                "summary": gate_outcome.summary,
            }),
        );
        if !gate_outcome.approved {
            let msg = format!(
                "Federation sync blocked by compatibility gate: {}",
                gate_outcome.summary
            );
            self.log(&format!("⛔ {}", msg));
            return msg;
        }

        self.broadcast_graph_activity("task_execute", None, None, "Federation sync started");
        self.log(&format!(
            "Federation sync started with {} peer(s)",
            self.federation_peers.len()
        ));

        // Project current local shared edges before network sync.
        let export_batch = {
            let mut mg = meta_graph_arc.write().await;
            mg.project_to_shared(&self.world, &config.system_id);
            mg.shared_edges
                .iter()
                .filter(|e| e.provenance.origin_system == config.system_id)
                .rev()
                .take(64)
                .map(Self::shared_edge_to_injection_request)
                .collect::<Vec<_>>()
        };

        let client = FederationClient::new(config.system_id.clone(), self.federation_peers.clone());
        let filter = SubscriptionFilter {
            edge_types: None,
            min_trust: Some(0.0),
            domains: None,
            min_layer: None,
        };

        let mut imported_total = 0usize;
        let mut rejected_total = 0usize;
        let mut conflicts_total = 0usize;
        let mut exported_total = 0usize;
        let mut peer_errors = 0usize;

        for peer in self.federation_peers.clone() {
            if !export_batch.is_empty() {
                match client.inject_edges(&peer, export_batch.clone()).await {
                    Ok(result) => {
                        exported_total += result.imported;
                        conflicts_total += result.conflicts;
                        self.log(&format!(
                            "Federation push {}: exported={} rejected={} conflicts={}",
                            peer, result.imported, result.rejected, result.conflicts
                        ));
                    }
                    Err(e) => {
                        peer_errors += 1;
                        self.log(&format!("Federation push {} failed: {}", peer, e));
                    }
                }
            }

            let from_system = match client.get_system_info(&peer).await {
                Ok(info) => info.system_id,
                Err(_) => peer.clone(),
            };

            match client.poll_updates(&peer, &filter).await {
                Ok(edges) => {
                    if edges.is_empty() {
                        continue;
                    }
                    let requests: Vec<HyperedgeInjectionRequest> = edges
                        .iter()
                        .map(|edge| HyperedgeInjectionRequest {
                            vertices: edge.vertices.clone(),
                            edge_type: edge.edge_type.clone(),
                            scope: EdgeScope::Shared,
                            trust_tags: edge.trust_tags.clone(),
                            provenance: Provenance {
                                origin_system: edge.provenance.origin_system.clone(),
                                created_at: edge.provenance.created_at,
                                hop_chain: edge.provenance.hop_chain.clone(),
                            },
                            weight: edge.weight,
                            embedding: edge.embedding.clone(),
                            metadata: HashMap::from([(
                                "remote_edge_id".to_string(),
                                edge.id.clone(),
                            )]),
                        })
                        .collect();

                    let result = {
                        let mut mg = meta_graph_arc.write().await;
                        mg.import_remote_edges(&requests, &from_system, self.world.tick_count)
                    };

                    imported_total += result.imported;
                    rejected_total += result.rejected;
                    conflicts_total += result.conflicts;
                    self.log(&format!(
                        "Federation pull {}: imported={} rejected={} conflicts={}",
                        peer, result.imported, result.rejected, result.conflicts
                    ));
                }
                Err(e) => {
                    peer_errors += 1;
                    self.log(&format!("Federation pull {} failed: {}", peer, e));
                }
            }
        }

        self.federation_imported += imported_total;
        self.federation_exported += exported_total;
        self.federation_conflicts += conflicts_total;

        self.broadcast_graph_activity(
            "task_execute",
            None,
            None,
            &format!(
                "Federation sync complete: imported={} exported={} conflicts={} errors={}",
                imported_total, exported_total, conflicts_total, peer_errors
            ),
        );
        self.refresh_component_state_from_world();
        self.update_web_snapshot();

        format!(
            "Federation sync complete\nPeers: {}\nImported: +{} (total {})\nExported: +{} (total {})\nRejected: {}\nConflicts: +{} (total {})\nPeer errors: {}",
            self.federation_peers.len(),
            imported_total,
            self.federation_imported,
            exported_total,
            self.federation_exported,
            rejected_total,
            conflicts_total,
            self.federation_conflicts,
            peer_errors
        )
    }

    /// Build and push a fresh WorldSnapshot to the web API shared state.
    fn update_web_snapshot(&self) {
        let coherence = self.world.global_coherence();
        let coherence_trend = if self.coherence_history.len() >= 5 {
            // history stores (coherence * 100.0) as u64, so divide by 100.0 to get 0..1
            let recent: Vec<f64> = self
                .coherence_history
                .iter()
                .rev()
                .take(5)
                .map(|&v| v as f64 / 100.0)
                .collect();
            let first = recent.last().copied().unwrap_or(coherence);
            let last = recent.first().copied().unwrap_or(coherence);
            if last - first > 0.005 {
                "rising"
            } else if first - last > 0.005 {
                "falling"
            } else {
                "stable"
            }
        } else {
            "stable"
        };

        let agents = self
            .world
            .agents
            .iter()
            .map(|a| AgentSnapshot {
                id: a.id as u64,
                role: format!("{:?}", a.role),
                curiosity: a.drives.curiosity as f64,
                harmony: a.drives.harmony as f64,
                growth: a.drives.growth as f64,
                transcendence: a.drives.transcendence as f64,
                learning_rate: a.learning_rate,
                description: a.description.clone(),
                jw: a.jw,
            })
            .collect();

        let edges = self
            .world
            .edges
            .iter()
            .map(|e| EdgeSnapshot {
                participants: e.participants.iter().map(|&x| x as u64).collect(),
                weight: e.weight as f64,
                emergent: e.emergent,
                age: e.age,
            })
            .collect();

        let mut beliefs: Vec<_> = self.world.beliefs.iter().collect();
        beliefs.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let beliefs = beliefs
            .iter()
            .take(30)
            .map(|b| BeliefSnapshot {
                content: b.content.clone(),
                confidence: b.confidence as f64,
                source: format!("{:?}", b.source),
            })
            .collect();

        let improvements = self
            .world
            .improvement_history
            .iter()
            .rev()
            .take(20)
            .map(|e| ImprovementSnapshot {
                intent: e.intent.clone(),
                mutation_type: format!("{:?}", e.mutation_type),
                coherence_before: e.coherence_before,
                coherence_after: e.coherence_after,
                applied: e.applied,
            })
            .collect();

        let ontology = self
            .world
            .ontology
            .iter()
            .take(30)
            .map(|(k, v)| (k.clone(), v.concept.clone()))
            .collect();

        let event_log = self
            .event_log
            .iter()
            .rev()
            .take(50)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        // Live component snapshots sourced from runtime component state.
        let council = self.components.council.clone();
        let dks = self.components.dks.clone();
        let cass = self.components.cass.clone();
        let navigation = self.components.navigation.clone();
        let communication = self.components.communication.clone();
        let kuramoto = self.components.kuramoto.clone();
        let gpu = self.components.gpu.clone();
        let llm = self.components.llm.clone();
        let email = self.components.email.clone();

        let federation = FederationSnapshot {
            status: if self.federation_addr.is_some() {
                "active".into()
            } else {
                "inactive".into()
            },
            addr: self.federation_addr.clone(),
            system_id: self
                .world
                .federation_config
                .as_ref()
                .map(|c| c.system_id.clone())
                .unwrap_or_default(),
            peers: self.federation_peers.clone(),
            imported: self.federation_imported,
            exported: self.federation_exported,
            conflicts: self.federation_conflicts,
        };

        // Build skill snapshot
        let all_skills = self.world.skill_bank.all_skills();
        let mut top_skills: Vec<SkillInfo> = all_skills
            .iter()
            .filter(|s| matches!(s.status, SkillStatus::Active | SkillStatus::Advanced))
            .map(|s| {
                let total = s.success_count + s.failure_count;
                let success_rate = if total > 0 {
                    s.success_count as f64 / total as f64
                } else {
                    0.5
                };
                SkillInfo {
                    id: s.id.clone(),
                    title: s.title.clone(),
                    principle: s.principle.clone(),
                    level: match &s.level {
                        SkillLevel::General => "General".into(),
                        SkillLevel::RoleSpecific(r) => format!("Role:{:?}", r),
                        SkillLevel::TaskSpecific(t) => format!("Task:{}", t),
                    },
                    confidence: s.confidence,
                    credit_ema: s.credit_ema,
                    status: format!("{:?}", s.status),
                    usage_count: s.usage_count,
                    success_rate,
                }
            })
            .collect();
        top_skills.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_skills.truncate(20);

        let skills = SkillSnapshot {
            total_skills: all_skills.len(),
            general_count: self.world.skill_bank.general_skills.len(),
            role_count: self
                .world
                .skill_bank
                .role_skills
                .values()
                .map(|v| v.len())
                .sum(),
            task_count: self
                .world
                .skill_bank
                .task_skills
                .values()
                .map(|v| v.len())
                .sum(),
            evolution_epoch: self.world.skill_bank.evolution_epoch,
            top_skills,
            recent_distillations: vec![], // Could track this if needed
            credit_history: self.world.skill_bank.credit_history.clone(),
        };

        // Calculate chat context stats
        let estimated_tokens: usize = self
            .chat_messages
            .iter()
            .map(|m| Self::estimate_tokens(&m.content))
            .sum();
        let context_limit = 262144; // 262K tokens (based on user's image showing 262K limit)
        let _percent_used = (estimated_tokens as f32 / context_limit as f32 * 100.0).min(100.0);

        // Get LCM stats if available. If LCM exists but legacy-only chat paths were used,
        // merge with legacy estimates so UI telemetry does not collapse to zeros.
        let (chat_tokens, chat_regular_tokens, chat_cache_read, chat_cache_write, lcm_dag_info) =
            if let Some(ref lcm) = self.lcm_context {
                let stats = lcm.get_stats();
                let max_depth = lcm
                    .dag
                    .nodes
                    .values()
                    .filter(|n| {
                        matches!(n.node_type, hyper_stigmergy::lcm::NodeType::Summary { .. })
                    })
                    .map(|n| n.depth)
                    .max()
                    .unwrap_or(0);
                let dag_info = DagInfoSnapshot {
                    total_nodes: lcm.dag.nodes.len(),
                    summary_nodes: lcm
                        .dag
                        .nodes
                        .values()
                        .filter(|n| {
                            matches!(n.node_type, hyper_stigmergy::lcm::NodeType::Summary { .. })
                        })
                        .count(),
                    large_file_nodes: lcm
                        .dag
                        .nodes
                        .values()
                        .filter(|n| {
                            matches!(
                                n.node_type,
                                hyper_stigmergy::lcm::NodeType::LargeFile { .. }
                            )
                        })
                        .count(),
                    max_depth,
                };
                let merged_tokens = stats.estimated_tokens.max(estimated_tokens);
                let merged_regular = if stats.regular_tokens == 0 && estimated_tokens > 0 {
                    estimated_tokens.min(context_limit)
                } else {
                    stats.regular_tokens.min(context_limit)
                };
                (
                    merged_tokens,
                    merged_regular,
                    stats.cache_read_tokens,
                    stats.cache_write_tokens,
                    dag_info,
                )
            } else {
                (
                    estimated_tokens,
                    estimated_tokens.min(context_limit),
                    0,
                    0,
                    DagInfoSnapshot::default(),
                )
            };

        let chat_context = ChatContextSnapshot {
            message_count: self.chat_messages.len(),
            estimated_tokens: chat_tokens,
            percent_used: (chat_tokens as f32 / context_limit as f32 * 100.0).min(100.0),
            limit_tokens: context_limit,
            regular_tokens: chat_regular_tokens,
            cache_read_tokens: chat_cache_read,
            cache_write_tokens: chat_cache_write,
            has_summary: self.chat_context_summary.is_some() || lcm_dag_info.summary_nodes > 0,
            dag_info: lcm_dag_info,
        };

        let snap = WorldSnapshot {
            tick: self.world.tick_count,
            coherence,
            coherence_trend: coherence_trend.to_string(),
            global_jw: self.world.global_jw(),
            agents,
            edges,
            beliefs,
            improvements,
            ontology,
            event_log,
            council,
            dks,
            cass,
            navigation,
            communication,
            kuramoto,
            gpu,
            llm,
            email,
            federation,
            skills,
            chat_context,
        };

        // Non-blocking write — if the lock is contended we skip this update
        if let Ok(mut guard) = self.web_snapshot.try_write() {
            *guard = snap;
        }
    }

    /// Connect (or reconnect) to RooDB from chat: `/connect [url]`
    fn do_connect_roodb(&mut self, url: String) {
        let config = RooDbConfig::from_url(&url);
        let db = RooDb::new(&config);
        let bg = self.bg_tx.clone();
        let host = config.host.clone();
        let port = config.port;
        let database = config.database.clone();
        tokio::spawn(async move {
            let result = tokio::time::timeout(Duration::from_secs(5), async {
                db.ping().await?;
                db.init_schema().await?;
                Ok::<_, anyhow::Error>(db)
            })
            .await;
            match result {
                Ok(Ok(db)) => {
                    let _ = bg.send(BgEvent::Log(format!(
                        "RooDB connected: {}:{}/{}",
                        host, port, database
                    )));
                    let _ = bg.send(BgEvent::RooDbConnected(Arc::new(db)));
                }
                Ok(Err(e)) => {
                    let _ = bg.send(BgEvent::Log(format!("RooDB connect failed: {}", e)));
                }
                Err(_) => {
                    let _ = bg.send(BgEvent::Log("RooDB connection timed out".into()));
                }
            }
        });
        self.log(&format!("Connecting to RooDB at {}…", url));
    }

    fn do_reflect(&mut self) {
        if self.reflect_in_progress {
            self.log("Reflection already in progress...");
            return;
        }
        self.reflect_in_progress = true;
        self.reflect_status = Some("Reflecting...".into());
        self.log("Starting reflection (heuristic mode)...");

        // Use heuristic reflection synchronously (Ollama async handled separately)
        let coherence_before = self.world.global_coherence();

        // Build analysis from improvement history
        let mut analysis = String::new();
        let recent: Vec<f64> = self
            .world
            .improvement_history
            .iter()
            .rev()
            .take(5)
            .map(|e| e.coherence_after - e.coherence_before)
            .collect();
        let avg_delta = if recent.is_empty() {
            0.0
        } else {
            recent.iter().sum::<f64>() / recent.len() as f64
        };

        if avg_delta > 0.001 {
            analysis
                .push_str("INSIGHT: Upward trajectory — recent mutations improving coherence\n");
        } else if avg_delta < -0.001 {
            analysis.push_str("INSIGHT: Coherence declining — mutations may be too disruptive\n");
            analysis.push_str("AVOID: High-novelty mutations during coherence decline\n");
        } else {
            analysis.push_str("INSIGHT: System plateaued — consider increasing exploration\n");
        }

        let coherence = self.world.global_coherence();
        if coherence > 0.8 {
            analysis.push_str(
                "INSIGHT: High coherence — introduce diversity to avoid over-optimization\n",
            );
        } else if coherence < 0.3 {
            analysis.push_str("INSIGHT: Low coherence — strengthen connections\n");
            analysis.push_str("AVOID: Removing edges when coherence is below 0.3\n");
        }

        let edge_ratio = self.world.edges.len() as f64 / self.world.agents.len().max(1) as f64;
        if edge_ratio < 1.5 {
            analysis.push_str("INSIGHT: Sparse connectivity — network needs more links\n");
        }

        // Parse and apply insights
        let insights: Vec<String> = analysis
            .lines()
            .filter(|l| l.starts_with("INSIGHT:"))
            .map(|l| l.trim_start_matches("INSIGHT:").trim().to_string())
            .collect();
        let avoids: Vec<String> = analysis
            .lines()
            .filter(|l| l.starts_with("AVOID:"))
            .map(|l| l.trim_start_matches("AVOID:").trim().to_string())
            .collect();

        let mut beliefs_added = 0;
        for insight in &insights {
            self.world
                .add_belief(insight, 0.6, hyper_stigmergy::BeliefSource::Reflection);
            beliefs_added += 1;
        }

        // Store avoids as low-confidence beliefs (contradicting evidence so Critic role sees them),
        // and update the world's avoid_hints so mutation scoring can penalize matching types.
        for avoid in &avoids {
            let avoid_belief = format!("AVOID: {}", avoid);
            self.world.add_belief(
                &avoid_belief,
                0.3,
                hyper_stigmergy::BeliefSource::Reflection,
            );
        }
        // Replace (not accumulate) avoid hints each reflection cycle so stale hints don't persist
        self.world.avoid_hints = avoids.clone();

        self.world.generate_beliefs_from_history();
        self.world.decay_beliefs();
        self.world.reflection_count += 1;
        self.world.last_reflection_tick = self.world.tick_count;

        let coherence_after = self.world.global_coherence();
        let summary = format!(
            "Reflection #{}: {} insights, {} beliefs, {} avoids | coh {:.4}→{:.4}",
            self.world.reflection_count,
            insights.len(),
            beliefs_added,
            avoids.len(),
            coherence_before,
            coherence_after,
        );
        self.log(&summary);
        self.reflect_status = Some(summary);
        self.reflect_in_progress = false;
        self.record_snapshot();
    }

    fn do_pareto_bid(&mut self) {
        let role = self.world.select_role_pareto(&self.bid_config);
        let name = format!("{:?}", role);
        *self.bid_history.entry(name.clone()).or_insert(0) += 1;
        self.total_bids += 1;
        self.log(&format!("Pareto bid won by {}", name));
    }

    fn do_decay_beliefs(&mut self) {
        self.world.decay_beliefs();
        let remaining = self.world.beliefs.len();
        self.log(&format!("Beliefs decayed. {} remaining", remaining));
    }

    /// Run a raw SQL query against RooDB and emit the result as a chat message.
    fn do_query_roodb(&mut self, sql: String) {
        if let Some(ref db) = self.roodb {
            let db = db.clone();
            let bg = self.bg_tx.clone();
            let sql_clone = sql.clone();
            tokio::spawn(async move {
                match db.raw_query(&sql_clone).await {
                    Ok((headers, rows)) => {
                        let _ = bg.send(BgEvent::QueryResult {
                            sql: sql_clone,
                            rows,
                            headers,
                        });
                    }
                    Err(e) => {
                        let _ = bg.send(BgEvent::Log(format!("Query error: {}", e)));
                    }
                }
            });
        } else {
            self.chat_messages.push(ChatMsg {
                role: "assistant".into(),
                content: "Not connected to RooDB. Use /connect first.".into(),
                model: "system".into(),
            });
        }
    }

    /// Spawn export-to-duckdb.py as a background process and log output.
    fn do_export_duckdb(&mut self) {
        let bg = self.bg_tx.clone();
        tokio::spawn(async move {
            let _ = bg.send(BgEvent::Log(
                "Exporting RooDB → DuckDB bridge (lars/)…".into(),
            ));
            let result = tokio::process::Command::new("python3.12")
                .arg("lars/export-to-duckdb.py")
                .output()
                .await;
            match result {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    if out.status.success() {
                        let summary = stdout.lines().last().unwrap_or("Done").to_string();
                        let _ = bg.send(BgEvent::Log(format!("DuckDB export: {}", summary)));
                    } else {
                        let err = if stderr.is_empty() { stdout } else { stderr };
                        let _ = bg.send(BgEvent::Log(format!("DuckDB export failed: {}", err)));
                    }
                }
                Err(e) => {
                    let _ = bg.send(BgEvent::Log(format!("Failed to run export script: {}", e)));
                }
            }
        });
    }

    /// Handle a `/command [args]` typed in the chat box.
    /// Returns true if the command was consumed (no Ollama call needed).
    async fn handle_slash_command(&mut self, text: &str) -> bool {
        let parts: Vec<&str> = text.splitn(2, ' ').collect();
        let cmd = parts[0].to_ascii_lowercase();
        let arg = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();

        // Echo the command as a user message (async LCM version)
        self.add_chat_message("user", text, "").await;
        self.ensure_runtime_services_initialized().await;

        let reply = match cmd.as_str() {
            "/save" | "/s" => {
                self.do_save_with_compat_gate("operator").await
            }
            "/load" | "/o" => {
                self.do_load();
                "Loading latest embedded graph snapshot…".into()
            }
            "/loaddb" | "/db" => {
                self.do_load_db();
                "Loading latest RooDB snapshot…".into()
            }
            "/connect" => {
                let url = if arg.is_empty() {
                    "127.0.0.1:3307".to_string()
                } else {
                    arg.clone()
                };
                self.do_connect_roodb(url.clone());
                format!("Connecting to RooDB at {}…", url)
            }
            "/export" | "/viz" | "/v" => {
                self.do_export_viz();
                "Exported viz/hyper_graph.json — browser reloading.".into()
            }
            "/tick" | "/t" => {
                let n: usize = arg.parse().unwrap_or(1).max(1).min(100);
                for _ in 0..n {
                    self.world.tick();
                }
                self.record_snapshot();
                self.do_export_viz();
                format!("Ticked {} time(s). Tick={}", n, self.world.tick_count)
            }
            "/improve" | "/i" => {
                self.do_improvement();
                self.do_export_viz();
                format!("Improvement #{} applied. Viz updated.", self.improvement_count)
            }
            "/bid" | "/b" => {
                self.do_bid_round();
                format!("Bid round complete. Total bids: {}", self.total_bids)
            }
            "/reflect" | "/r" => {
                self.do_reflect();
                self.do_export_viz();
                "Reflection complete. Viz updated.".into()
            }
            "/link" | "/l" => {
                self.do_link_random();
                self.do_export_viz();
                "Random link added. Viz updated.".into()
            }
            "/auto" => {
                self.auto_tick = !self.auto_tick;
                format!("Auto-tick: {}", if self.auto_tick { "ON" } else { "OFF" })
            }
            "/pareto" | "/p" => {
                self.do_pareto_bid();
                format!("Pareto bid complete. Total bids: {}", self.total_bids)
            }
            "/decay" | "/dy" => {
                self.do_decay_beliefs();
                format!("Beliefs decayed. {} remaining.", self.world.beliefs.len())
            }
            "/speed" | "/spd" => {
                if arg.is_empty() {
                    format!("Current tick speed: {}ms. Usage: /speed <ms>", self.tick_speed_ms)
                } else {
                    match arg.parse::<u64>() {
                        Ok(ms) => {
                            self.tick_speed_ms = ms.max(50).min(5000);
                            format!("Tick speed set to {}ms", self.tick_speed_ms)
                        }
                        Err(_) => "Usage: /speed <milliseconds>".into(),
                    }
                }
            }
            "/context" | "/ctx" => {
                let total_msgs = self.chat_messages.len();
                let summary_exists = self.chat_context_summary.is_some();
                let summary_len = self.chat_context_summary.as_ref().map(|s| s.len()).unwrap_or(0);

                let estimated_tokens: usize = self.chat_messages.iter()
                    .map(|m| Self::estimate_tokens(&m.content))
                    .sum();

                let mut info = format!(
                    "📊 Chat Context Status:\n\
                     • Messages in window: {}\n\
                     • Has summary: {}\n\
                     • Summary length: {} chars\n\
                     • Estimated tokens: {} (~{}% of 4K context)",
                    total_msgs,
                    if summary_exists { "YES" } else { "NO" },
                    summary_len,
                    estimated_tokens,
                    (estimated_tokens as f32 / 4096.0 * 100.0) as usize
                );

                if arg == "compact" || arg == "c" {
                    // Force compaction
                    if total_msgs > 6 {
                        let keep_count = 6;
                        let summarize_end = total_msgs - keep_count;
                        let mut text_to_summarize = String::new();
                        for (i, msg) in self.chat_messages[0..summarize_end].iter().enumerate() {
                            if i > 0 { text_to_summarize.push('\n'); }
                            text_to_summarize.push_str(&format!("{}: {}", msg.role, msg.content));
                        }
                        let summary = self.extract_key_points(&text_to_summarize);

                        if let Some(ref mut existing) = self.chat_context_summary {
                            existing.push_str("\n---\n");
                            existing.push_str(&summary);
                        } else {
                            self.chat_context_summary = Some(summary);
                        }

                        self.chat_messages.drain(0..summarize_end);
                        info.push_str("\n\n✅ Context manually compacted.");
                    } else {
                        info.push_str("\n\n⚠️ Not enough messages to compact.");
                    }
                } else if arg == "clear" {
                    self.chat_context_summary = None;
                    info.push_str("\n\n✅ Context summary cleared.");
                } else {
                    info.push_str("\n\nUsage:\n  /context         - Show status\n  /context compact - Force compaction\n  /context clear   - Clear summary");
                }

                info
            }
            "/query" | "/q" => {
                if arg.is_empty() {
                    "Usage: /query <SQL>  e.g. /query SELECT COUNT(*) FROM agents".into()
                } else {
                    self.do_query_roodb(arg.clone());
                    format!("Running query: {}…", &arg[..arg.len().min(60)])
                }
            }
            "/exportdb" | "/edb" => {
                self.do_export_duckdb();
                "Exporting RooDB → DuckDB bridge for LARS. Check logs for progress.".into()
            }
            "/lars" => {
                let lars_dir = "lars/";
                let cascades = ["beliefs_semantic", "improvement_analysis", "agent_health", "belief_topics", "edge_emergence"];
                let cascade_list = cascades.iter().map(|c| format!("  • {}", c)).collect::<Vec<_>>().join("\n");
                format!(
                    "LARS Semantic SQL\n\
                     ─────────────────\n\
                     SQL server:  psql postgresql://admin:admin@localhost:15432/default\n\
                     Studio UI:   http://localhost:5050\n\
                     DuckDB file: {lars_dir}hyper_stigmergy.duckdb\n\
                     \n\
                     To start LARS:\n\
                       ./lars/start-lars.sh\n\
                     \n\
                     To sync DB:\n\
                       /exportdb  (then LARS auto-sees updated file)\n\
                     \n\
                     Available cascades:\n\
                     {cascade_list}\n\
                     \n\
                     Run cascade: lars cascade run beliefs_semantic --intent 'plateau'\n\
                     Semantic SQL: lars ssql \"SELECT * FROM hyper_stigmergy.beliefs WHERE content MEANS 'system is plateauing'\""
                )
            }
            "/council" | "/c" => {
                if arg.is_empty() {
                    "Usage: /council <question>  e.g. /council Why is coherence declining?".into()
                } else {
                    self.do_council(arg.clone(), "auto");
                    format!("Council started: \"{}\" — see Studio → Council tab", truncate_str(&arg, 50))
                }
            }
            "/coder" => {
                if arg.is_empty() {
                    "Coder Agent — Available tools:\n\
                     • pi_read <filepath> — Read file contents\n\
                     • pi_bash <command> — Execute bash commands\n\
                     • pi_grep <pattern> — Search in files\n\
                     • pi_find <pattern> — Find files by name\n\
                     • pi_ls [path] — List directory\n\
                     • pi_edit <path>\nOLD:\n<text>\nNEW:\n<text> — Edit files\n\
                     • pi_write <path>\n<content> — Create new files\n\
                     \n\
                     The Coder agent activates automatically for coding tasks (code, debug, refactor, etc.)".into()
                } else {
                    // Just show the help - coding detection is automatic
                    "Coder agent activates automatically for coding tasks.\n\
                     Just ask about code, files, or implementation.\n\
                     Use /coder (no args) to see available tools.".into()
                }
            }
            "/federation" | "/fed" => {
                if arg.is_empty() {
                    let status = if self.federation_addr.is_some() {
                        format!(
                            "Federation ACTIVE\nAddress: {}\nPeers: {}\nImported: {} | Exported: {} | Conflicts: {}",
                            self.federation_addr.as_ref().unwrap(),
                            self.federation_peers.len(),
                            self.federation_imported,
                            self.federation_exported,
                            self.federation_conflicts
                        )
                    } else {
                        "Federation INACTIVE\nStart with: --federation <addr> --peer <url>".into()
                    };
                    status
                } else {
                    let parts: Vec<&str> = arg.splitn(2, ' ').collect();
                    match parts[0] {
                        "add" | "connect" => {
                            if parts.len() < 2 {
                                "Usage: /federation add <peer_url>".into()
                            } else {
                                let peer = parts[1].to_string();
                                self.federation_peers.push(peer.clone());
                                if let Some(ref mut cfg) = self.world.federation_config {
                                    if !cfg.known_peers.contains(&peer) {
                                        cfg.known_peers.push(peer.clone());
                                    }
                                }
                                format!("Added peer: {}", peer)
                            }
                        }
                        "remove" | "disconnect" => {
                            if parts.len() < 2 {
                                "Usage: /federation remove <peer_url>".into()
                            } else {
                                let peer = parts[1];
                                self.federation_peers.retain(|p| p != peer);
                                if let Some(ref mut cfg) = self.world.federation_config {
                                    cfg.known_peers.retain(|p| p != peer);
                                }
                                format!("Removed peer: {}", peer)
                            }
                        }
                        "list" | "peers" => {
                            if self.federation_peers.is_empty() {
                                "No peers configured".into()
                            } else {
                                format!("Peers:\n{}", self.federation_peers.iter().enumerate()
                                    .map(|(i, p)| format!("  {}. {}", i + 1, p))
                                    .collect::<Vec<_>>()
                                    .join("\n"))
                            }
                        }
                        "sync" => {
                            self.do_federation_sync().await
                        }
                        _ => "Usage: /federation [add|remove|list|sync] [args]".into()
                    }
                }
            }
            "/dks" => {
                let dks = &self.components.dks;
                format!(
                    "DKS (Dynamic Kinetic Stability)\n\
                     Generation: {}\n\
                     Population: {} agents\n\
                     Replicators: {}\n\
                     Avg Persistence: {:.4}\n\
                     Flux Intensity: {:.4}",
                    dks.generation,
                    dks.population_size,
                    dks.replicator_count,
                    dks.avg_persistence,
                    dks.flux_intensity
                )
            }
            "/kuramoto" => {
                if let Some(k) = self.components.kuramoto.as_ref() {
                    let diag = &k.diagnostics;
                    let warnings = if k.preflight_warnings.is_empty() {
                        "none".to_string()
                    } else {
                        k.preflight_warnings.join(" | ")
                    };
                    format!(
                        "Kuramoto (graph oscillator sync)\n\
                         R: {:.4}\n\
                         Mean phase: {:.3}\n\
                         Oscillators: {}\n\
                         Trend(ΔR): {:+.4}\n\
                         Entropy: {:.3}\n\
                         Velocity stddev: {:.4}\n\
                         R-window stddev: {:.4}\n\
                         Warnings: {}",
                        k.order_parameter,
                        k.mean_phase,
                        k.oscillators.len(),
                        self.services.kuramoto.sync_trend(),
                        diag.phase_entropy,
                        diag.velocity_stddev,
                        diag.r_window_stddev,
                        warnings
                    )
                } else {
                    "Kuramoto: snapshot unavailable".to_string()
                }
            }
            "/cass" => {
                let cass = &self.components.cass;
                format!(
                    "CASS (Context-Aware Semantic Skills)\n\
                     Skills: {}\n\
                     Context Depth: {}\n\
                     Embedding Dim: {}\n\
                     Semantic Graph: {} nodes",
                    cass.skill_count,
                    cass.context_depth,
                    cass.embedding_dimension,
                    cass.skill_count
                )
            }
            "/gpu" => {
                #[cfg(feature = "gpu")]
                let status = "GPU ENABLED\nDevice: Metal/Apple Silicon\nCompute: Active\nFallback: No";
                #[cfg(not(feature = "gpu"))]
                let status = "GPU DISABLED\nUsing CPU fallback\nBuild with: --features gpu";
                status.into()
            }
            "/llm" => {
                let llm = &self.components.llm;
                let cache = llm.cache_hit_rate
                    .map(|v| format!("{:.1}%", v * 100.0))
                    .unwrap_or_else(|| "n/a".to_string());
                format!(
                    "FrankenTorch LLM Engine\n\
                     Model: {}\n\
                     Loaded: {}\n\
                     Cache Hit Rate: {}\n\
                     Tokens Generated: {}\n\
                     Avg Latency: {:.1}ms\n\
                     Provider: Ollama @ localhost:11434",
                    llm.model_name.as_deref().unwrap_or("(none)"),
                    if llm.model_loaded { "YES" } else { "NO" },
                    cache,
                    llm.tokens_generated,
                    llm.avg_latency_ms,
                )
            }
            "/email" => {
                format!(
                    "Email Agent\n\
                     Status: Not configured\n\
                     To enable: Set EMAIL_CONFIG env var\n\
                     Memory: {} entries",
                    self.world.beliefs.len()
                )
            }
            "/navigation" | "/nav" => {
                let nav = &self.components.navigation;
                format!(
                    "Code Navigation\n\
                     Indexed Files: {}\n\
                     Topics: {}\n\
                     Semantic Index: Active\n\
                     Parser: Rust/Python/JS/TS",
                    nav.indexed_files,
                    nav.topics.len()
                )
            }
            "/status" | "/state" => {
                format!(
                    "Agents={} Edges={} Vertices={} Coherence={:.4} Tick={} Beliefs={} Ontology={} DB={} Speed={}ms",
                    self.world.agents.len(),
                    self.world.edges.len(),
                    self.world.vertex_meta.len(),
                    self.world.global_coherence(),
                    self.world.tick_count,
                    self.world.beliefs.len(),
                    self.world.ontology.len(),
                    if self.roodb.is_some() { "connected" } else { "not connected" },
                    self.tick_speed_ms,
                )
            }
            "/skills" => {
                let bank = &self.world.skill_bank;
                let all_skills = bank.all_skills();
                let active_count = all_skills.iter().filter(|s| matches!(s.status, SkillStatus::Active)).count();
                let advanced_count = all_skills.iter().filter(|s| matches!(s.status, SkillStatus::Advanced)).count();
                let suspended_count = all_skills.iter().filter(|s| matches!(s.status, SkillStatus::Suspended { .. })).count();
                let curation = bank.curation_summary();
                let hire_tree_count = bank.hire_trees.len();
                let hire_history_count = bank.hire_history.len();

                let mut msg = format!(
                    "📚 Skill Bank (SkillRL) — Epoch {}\n\
                     Total: {} | Active: {} | Advanced: {} | Suspended: {}\n\
                     General: {} | Role: {} | Task: {}\n\
                     Curation: {} curated, {} promoted, {} proposed, {} legacy\n\
                     Delegation: {} active trees, {} completed\n\
                     ────────────────────────────────────────\n",
                    bank.evolution_epoch,
                    all_skills.len(),
                    active_count,
                    advanced_count,
                    suspended_count,
                    bank.general_skills.len(),
                    bank.role_skills.values().map(|v| v.len()).sum::<usize>(),
                    bank.task_skills.values().map(|v| v.len()).sum::<usize>(),
                    curation.get("human_curated").unwrap_or(&0),
                    curation.get("promoted").unwrap_or(&0),
                    curation.get("proposed").unwrap_or(&0),
                    curation.get("legacy").unwrap_or(&0),
                    hire_tree_count,
                    hire_history_count,
                );

                // Show top skills by confidence
                let mut top: Vec<_> = all_skills.iter()
                    .filter(|s| matches!(s.status, SkillStatus::Active | SkillStatus::Advanced))
                    .collect();
                top.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

                for (_i, skill) in top.iter().take(10).enumerate() {
                    let level_icon = match &skill.level {
                        SkillLevel::General => "🌐",
                        SkillLevel::RoleSpecific(_) => "👤",
                        SkillLevel::TaskSpecific(_) => "⚙️",
                    };
                    msg.push_str(&format!(
                        "{} [{}] {} — {} (conf: {:.0}%, used: {})\n",
                        level_icon,
                        skill.id,
                        skill.title,
                        match &skill.status { SkillStatus::Advanced => "⭐", _ => "  " },
                        skill.confidence * 100.0,
                        skill.usage_count
                    ));
                }

                if top.len() > 10 {
                    msg.push_str(&format!("\n... and {} more\n", top.len() - 10));
                }

                msg.push_str("\n💡 Use Components → Skills tab for full browser\n");
                msg
            }
            "/help" | "/h" | "/?" => {
                "── Simulation ───────────────────────────\n\
                 /tick [n]       Run n ticks (default 1)\n\
                 /auto           Toggle auto-tick on/off\n\
                 /speed <ms>     Set auto-tick interval (50–5000ms)\n\
                 /improve        Run improvement cycle\n\
                 /bid            Run bid round\n\
                 /pareto         Pareto bid selection\n\
                 /reflect        Agent reflection\n\
                 /link           Add random hyperedge\n\
                 /decay          Decay beliefs\n\
                 ── Intelligence ─────────────────────────\n\
                 /council <q>    Socratic multi-LLM reasoning chain\n\
                 /coder          Coder agent tools (read, bash, edit, write, grep, find, ls)\n\
                 /dks            DKS (Dynamic Kinetic Stability) status\n\
                 /kuramoto       Kuramoto synchronization diagnostics\n\
                 /cass           CASS (Semantic Skills) status
                 /skills         Browse skill bank (SkillRL system)\n\
                 /llm            LLM engine status\n\
                 /navigation     Code navigation status\n\
                 ── Federation ───────────────────────────\n\
                 /federation     Show federation status\n\
                 /federation add <url>     Add peer\n\
                 /federation remove <url>  Remove peer\n\
                 /federation list          List peers\n\
                 /federation sync          Sync with peers\n\
                 ── Persistence ──────────────────────────\n\
                 /save           Save to embedded store + RooDB\n\
                 /load           Load latest embedded graph snapshot\n\
                 /loaddb         Load latest RooDB snapshot\n\
                 /connect [url]  Connect to RooDB (default 127.0.0.1:3307)\n\
                 ── Visualization ────────────────────────\n\
                 /export         Export viz + trigger browser reload\n\
                 /status         Show system state\n\
                 /gpu            GPU acceleration status\n\
                 /email          Email agent status\n\\n                 /draw <desc>    Generate visual diagram (HTML)\n\
                 Studio UI:      http://localhost:8787\n\
                 ── LARS Semantic SQL ─────────────────────\n\
                 /exportdb       Export RooDB → DuckDB (for LARS)\n\
                 /query <sql>    Run raw SQL against RooDB\n\
                 /lars           Show LARS connection info & cascades\n\
                 ─────────────────────────────────────────\n\
                 Anything else → Ollama chat".into()
            }
            "/open" => {
                if arg.is_empty() {
                    "Usage: /open <path> — Open folder in file manager".into()
                } else {
                    let path = arg.trim();
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(path).spawn();
                    #[cfg(target_os = "linux")]
                    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
                    #[cfg(target_os = "windows")]
                    let _ = std::process::Command::new("explorer").arg(path).spawn();
                    format!("Opening: {}", path)
                }
            }
            "/draw" | "/diagram" | "/chart" | "/table" | "/visual" => {
                if arg.is_empty() {
                    "Usage: /draw <description> — Generate a visual explanation\nExamples:\n  /draw architecture of our agent system\n  /diagram flowchart: data pipeline\n  /table compare model performance".into()
                } else {
                    let diagram_type = match cmd.as_str() {
                        "/chart" => "dashboard",
                        "/table" => "table",
                        _ => "architecture",
                    };
                    let (title, content) = if let Some(pos) = arg.find(':') {
                        let (t, c) = arg.split_at(pos);
                        (t.trim().to_string(), c[1..].trim().to_string())
                    } else {
                        (arg.clone(), arg.clone())
                    };
                    let _ = self.bg_tx.send(BgEvent::VisualExplainer {
                        diagram_type: diagram_type.to_string(),
                        title: title.clone(),
                        content,
                        data: None,
                        open_browser: false,
                    });
                    format!("📊 Generating visualization: {}...\nWatch the Visual tab (📊) for results.", title)
                }
            }
            "/visuals" | "/diagrams" => {
                let output_dir = std::path::PathBuf::from("visual-explainer/output");
                let files: Vec<(String, String, String)> = std::fs::read_dir(&output_dir)
                    .ok()
                    .map(|entries| {
                        entries
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().extension().map(|ext| ext == "json").unwrap_or(false))
                            .filter_map(|e| {
                                let name = e.file_name().into_string().ok()?;
                                let content = std::fs::read_to_string(e.path()).ok()?;
                                let json: serde_json::Value = serde_json::from_str(&content).ok()?;
                                let title = json.get("title")?.as_str()?.to_string();
                                let viz_type = json.get("type")?.as_str()?.to_string();
                                Some((name, title, viz_type))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if files.is_empty() {
                    "No visualizations generated yet.\nUse /draw, /diagram, /chart, or /table to create visuals.".into()
                } else {
                    let list = files.iter().enumerate()
                        .map(|(i, (_, title, viz_type))| format!("  {}. [{}] {}", i + 1, viz_type, title))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!("📊 Generated Visualizations ({}):\n{}\n\nView them in the Visual tab (📊 icon).", files.len(), list)
                }
            }
            "/lcm" => {
                if arg.is_empty() {
                    // Show LCM status
                    if let Some(ref lcm) = self.lcm_context {
                        let stats = lcm.get_stats();
                        let max_depth = lcm.dag.nodes.values()
                            .filter(|n| matches!(n.node_type, NodeType::Summary { .. }))
                            .map(|n| n.depth)
                            .max()
                            .unwrap_or(0);
                        format!(
                            "🔗 LCM (Lossless Context Management)\n\
                            Status: ACTIVE\n\
                            Messages: {} | Summaries: {} | Large Files: {}\n\
                            Tokens: {} / {} ({:.1}%)\n\
                            Max DAG Depth: {}\n\
                            Recursion Depth: {} / {}\n\n\
                            Commands:\n  /lcm expand <id> — Expand a summary\n  /lcm grep <pattern> — Search context\n  /lcm compact — Force compaction",
                            stats.message_count,
                            stats.summary_count,
                            stats.large_file_count,
                            stats.estimated_tokens,
                            stats.limit_tokens,
                            stats.percent_used,
                            max_depth,
                            lcm.recursion_depth,
                            lcm.max_recursion_depth
                        )
                    } else {
                        "🔗 LCM: Not initialized yet. Will initialize on first chat message.".into()
                    }
                } else {
                    let lcm_parts: Vec<&str> = arg.splitn(2, ' ').collect();
                    match lcm_parts[0] {
                        "expand" => {
                            if lcm_parts.len() < 2 {
                                "Usage: /lcm expand <summary_id>".into()
                            } else {
                                let summary_id = lcm_parts[1].to_string();
                                if let Some(ref lcm) = self.lcm_context {
                                    match lcm.expand_summary(&summary_id) {
                                        Some(messages) => {
                                            let preview: String = messages.iter()
                                                .take(5)
                                                .map(|(role, content)| format!("{}: {}", role, content.chars().take(100).collect::<String>()))
                                                .collect::<Vec<_>>()
                                                .join("\n");
                                            format!("Expanded {} messages from {}:\n{}", messages.len(), summary_id, preview)
                                        }
                                        None => format!("Summary {} not found", summary_id)
                                    }
                                } else {
                                    "LCM not initialized".into()
                                }
                            }
                        }
                        "grep" => {
                            if lcm_parts.len() < 2 {
                                "Usage: /lcm grep <pattern>".into()
                            } else {
                                let pattern = lcm_parts[1];
                                if let Some(ref lcm) = self.lcm_context {
                                    if let Some(results) = lcm.grep(pattern) {
                                        if results.is_empty() {
                                            format!("No matches for '{}'", pattern)
                                        } else {
                                            let list: String = results.iter()
                                                .take(10)
                                                .map(|(id, content)| format!("  {}: {}", id, content.chars().take(80).collect::<String>()))
                                                .collect::<Vec<_>>()
                                                .join("\n");
                                            format!("Found {} matches for '{}':\n{}", results.len(), pattern, list)
                                        }
                                    } else {
                                        format!("Invalid regex pattern '{}'", pattern)
                                    }
                                } else {
                                    "LCM not initialized".into()
                                }
                            }
                        }
                        "compact" => {
                            self.maybe_compact_lcm_context().await;
                            "LCM compaction triggered".into()
                        }
                        _ => "Usage: /lcm [status|expand <id>|grep <pattern>|compact]".into()
                    }
                }
            }
            _ => return false, // unknown slash command → pass to Ollama
        };

        // Add assistant response (async version)
        self.add_chat_message("assistant", &reply, "system").await;
        true
    }

    /// Estimate token count from text (rough approximation: ~4 chars per token)
    fn estimate_tokens(text: &str) -> usize {
        text.len() / 4
    }

    /// Check if context exceeds 85% threshold and compact older messages into a summary
    fn maybe_compact_context(&mut self, model: &str) {
        // Context window sizes for common models
        let context_limit = if model.contains("8B") || model.contains("7B") {
            4096
        } else if model.contains("13B") || model.contains("14B") {
            8192
        } else if model.contains("32B") || model.contains("70B") {
            32768
        } else {
            4096 // Default conservative limit
        };

        let threshold = (context_limit as f32 * 0.85) as usize;

        // Calculate current context size
        let mut total_tokens = 0;
        for msg in &self.chat_messages {
            total_tokens += Self::estimate_tokens(&msg.content);
        }

        // If under threshold, no compaction needed
        if total_tokens < threshold {
            return;
        }

        // Need to compact: summarize older messages
        // Keep last 6 messages (3 pairs) intact, summarize the rest
        let keep_count = 6;
        if self.chat_messages.len() <= keep_count {
            return; // Not enough history to compact
        }

        let summarize_start = 0;
        let summarize_end = self.chat_messages.len() - keep_count;

        // Build text to summarize
        let mut text_to_summarize = String::new();
        for (i, msg) in self.chat_messages[summarize_start..summarize_end]
            .iter()
            .enumerate()
        {
            if i > 0 {
                text_to_summarize.push('\n');
            }
            text_to_summarize.push_str(&format!("{}: {}", msg.role, msg.content));
        }

        // For now, use a simple extraction-based summary
        // In production, this would call an LLM to summarize
        let summary = self.extract_key_points(&text_to_summarize);

        // Update summary (append to existing or create new)
        if let Some(ref mut existing) = self.chat_context_summary {
            existing.push_str("\n---\n");
            existing.push_str(&summary);
            // Keep summary from growing too large
            if existing.len() > 2000 {
                *existing = existing[existing.len() - 2000..].to_string();
            }
        } else {
            self.chat_context_summary = Some(summary);
        }

        // Remove the summarized messages
        self.chat_messages.drain(summarize_start..summarize_end);

        self.log(&format!(
            "🔄 Context compacted: {} messages summarized, {} kept. Tokens: {} → ~{}",
            summarize_end,
            self.chat_messages.len(),
            total_tokens,
            self.chat_messages
                .iter()
                .map(|m| Self::estimate_tokens(&m.content))
                .sum::<usize>()
        ));
    }

    /// Extract key points from conversation for summary
    fn extract_key_points(&self, text: &str) -> String {
        // Simple extraction: get first 1000 chars as gist
        // In production, this would use LLM to generate proper summary
        let truncated = if text.len() > 1000 {
            &text[..1000]
        } else {
            text
        };

        format!(
            "Previous discussion covered: {}... ({} chars)",
            truncated.replace('\n', " | "),
            text.len()
        )
    }

    /// Inspect the user's question and pull relevant live data from the in-memory world.
    /// Returns a data block injected into the system prompt so Ollama answers from facts.
    fn build_grounded_context(&self, question: &str) -> String {
        let q = question.to_lowercase();
        let injection_like = is_prompt_injection_like(&q);
        let asks_about_simulation = q.contains("simulation")
            || q.contains("world state")
            || q.contains("hsm")
            || q.contains("hyper-stig")
            || q.contains("stigmerg")
            || q.contains("coherence")
            || q.contains("hypergraph")
            || q.contains("hyperedge")
            || q.contains("agent#")
            || q.contains("architect")
            || q.contains("catalyst")
            || q.contains("chronicler")
            || q.contains("critic")
            || q.contains("explorer")
            || q.contains("transcenden")
            || q.contains("curiosity")
            || q.contains("harmony")
            || q.contains("growth")
            || q.contains("belief")
            || q.contains("ontology")
            || q.contains("council")
            || q.contains("dks")
            || q.contains("cass")
            || q.contains("federation")
            || q.contains("roodb")
            || q.contains("tick");
        if !asks_about_simulation || injection_like {
            return String::new();
        }
        let mut sections: Vec<String> = Vec::new();

        // ── Agents ──────────────────────────────────────────────────────────
        let wants_agents = q.contains("agent")
            || q.contains("role")
            || q.contains("curious")
            || q.contains("harmony")
            || q.contains("growth")
            || q.contains("drive")
            || q.contains("transcenden")
            || q.contains("architect")
            || q.contains("catalyst")
            || q.contains("chronicler")
            || q.contains("critic")
            || q.contains("explorer")
            || q.contains("learning")
            || q.contains("bid");

        if wants_agents && !self.world.agents.is_empty() {
            let mut rows: Vec<String> = self.world.agents.iter().map(|a| {
                format!(
                    "  agent#{} {:?} | curiosity={:.3} harmony={:.3} growth={:.3} transcendence={:.3} lr={:.4} | \"{}\"",
                    a.id, a.role,
                    a.drives.curiosity, a.drives.harmony, a.drives.growth, a.drives.transcendence,
                    a.learning_rate,
                    if a.description.len() > 80 { &a.description[..80] } else { &a.description }
                )
            }).collect();
            rows.sort();
            sections.push(format!(
                "AGENTS (live, {} total):\n{}",
                self.world.agents.len(),
                rows.join("\n")
            ));
        }

        // ── Beliefs ─────────────────────────────────────────────────────────
        let wants_beliefs = q.contains("belief")
            || q.contains("plateau")
            || q.contains("stagnate")
            || q.contains("emerg")
            || q.contains("convergence")
            || q.contains("confident");

        if wants_beliefs && !self.world.beliefs.is_empty() {
            let mut beliefs: Vec<_> = self.world.beliefs.iter().collect();
            beliefs.sort_by(|a, b| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let rows: Vec<String> = beliefs
                .iter()
                .take(15)
                .map(|b| format!("  [{:.2}] {}", b.confidence, b.content))
                .collect();
            sections.push(format!(
                "BELIEFS (top {} by confidence):\n{}",
                rows.len(),
                rows.join("\n")
            ));
        }

        // ── Hyperedges ──────────────────────────────────────────────────────
        let wants_edges = q.contains("edge")
            || q.contains("connect")
            || q.contains("link")
            || q.contains("relation")
            || q.contains("topology")
            || q.contains("structure")
            || q.contains("hyperedge")
            || q.contains("weight")
            || q.contains("network");

        if wants_edges && !self.world.edges.is_empty() {
            let mut edges: Vec<_> = self.world.edges.iter().collect();
            edges.sort_by(|a, b| {
                b.weight
                    .partial_cmp(&a.weight)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let emergent_count = edges.iter().filter(|e| e.emergent).count();
            let rows: Vec<String> = edges
                .iter()
                .take(10)
                .map(|e| {
                    format!(
                        "  participants={:?} weight={:.3} emergent={} age={}",
                        e.participants, e.weight, e.emergent, e.age
                    )
                })
                .collect();
            sections.push(format!(
                "HYPEREDGES ({} total, {} emergent, {} seeded, top 10 by weight):\n{}",
                edges.len(),
                emergent_count,
                edges.len() - emergent_count,
                rows.join("\n")
            ));
        }

        // ── Improvement history ─────────────────────────────────────────────
        let wants_improvements = q.contains("improv")
            || q.contains("evolv")
            || q.contains("mutation")
            || q.contains("coher")
            || q.contains("rebalanc")
            || q.contains("self-")
            || q.contains("cycle")
            || q.contains("progress");

        if wants_improvements && !self.world.improvement_history.is_empty() {
            let mut events: Vec<_> = self.world.improvement_history.iter().collect();
            events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            let rows: Vec<String> = events
                .iter()
                .take(10)
                .map(|e| {
                    let delta = e.coherence_after - e.coherence_before;
                    format!(
                        "  [{:?}] \"{}\" Δcoherence={:+.4} applied={}",
                        e.mutation_type, e.intent, delta, e.applied
                    )
                })
                .collect();
            sections.push(format!(
                "RECENT IMPROVEMENTS (last {}):\n{}",
                rows.len(),
                rows.join("\n")
            ));
        }

        // ── Experiences ─────────────────────────────────────────────────────
        let wants_experiences = q.contains("experience")
            || q.contains("outcome")
            || q.contains("positive")
            || q.contains("negative")
            || q.contains("learn from");

        if wants_experiences && !self.world.experiences.is_empty() {
            let mut exps: Vec<_> = self.world.experiences.iter().collect();
            exps.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            let rows: Vec<String> = exps
                .iter()
                .take(8)
                .map(|e| {
                    let outcome_str = match &e.outcome {
                        hyper_stigmergy::ExperienceOutcome::Positive { coherence_delta } => {
                            format!("positive(Δ{:+.3})", coherence_delta)
                        }
                        hyper_stigmergy::ExperienceOutcome::Negative { coherence_delta } => {
                            format!("negative(Δ{:+.3})", coherence_delta)
                        }
                        hyper_stigmergy::ExperienceOutcome::Neutral => "neutral".into(),
                    };
                    format!("  [{}] {}", outcome_str, e.description)
                })
                .collect();
            sections.push(format!(
                "RECENT EXPERIENCES (last {}):\n{}",
                rows.len(),
                rows.join("\n")
            ));
        }

        // ── Ontology ────────────────────────────────────────────────────────
        let wants_ontology = q.contains("ontolog")
            || q.contains("concept")
            || q.contains("definition")
            || q.contains("meaning")
            || q.contains("vocabulary")
            || q.contains("term");

        if wants_ontology && !self.world.ontology.is_empty() {
            let rows: Vec<String> = self
                .world
                .ontology
                .iter()
                .take(20)
                .map(|(k, v)| {
                    let desc = if v.concept.len() > 80 {
                        &v.concept[..80]
                    } else {
                        &v.concept
                    };
                    format!("  {}: {} (instances: {})", k, desc, v.instances.len())
                })
                .collect();
            sections.push(format!(
                "ONTOLOGY ({} concepts, sample):\n{}",
                self.world.ontology.len(),
                rows.join("\n")
            ));
        }

        // ── Council ─────────────────────────────────────────────────────────
        let wants_council = q.contains("council")
            || q.contains("decision")
            || q.contains("vote")
            || q.contains("debate")
            || q.contains("orchestrate")
            || q.contains("consensus");

        if wants_council {
            let council = &self.components.council;
            sections.push(format!(
                "COUNCIL SYSTEM:\n  Mode: {}\n  Members: {}\n  Status: {}",
                council.mode,
                council.member_count,
                if council.active { "active" } else { "standby" }
            ));
        }

        // ── DKS (Dynamic Kinetic Stability) ─────────────────────────────────
        let wants_dks = q.contains("dks")
            || q.contains("replicator")
            || q.contains("persistence")
            || q.contains("population")
            || q.contains("generation")
            || q.contains("flux")
            || q.contains("metabolism")
            || q.contains("selection");

        if wants_dks {
            let dks = &self.components.dks;
            sections.push(format!(
                "DKS (DYNAMIC KINETIC STABILITY):\n  Generation: {}\n  Population: {}\n  Replicators: {}\n  Avg Persistence: {:.4}\n  Flux Intensity: {:.4}",
                dks.generation,
                dks.population_size,
                dks.replicator_count,
                dks.avg_persistence,
                dks.flux_intensity
            ));
        }

        // ── Kuramoto synchronization ───────────────────────────────────────
        let wants_kuramoto = q.contains("kuramoto")
            || q.contains("oscillator")
            || q.contains("phase sync")
            || q.contains("order parameter")
            || q.contains("synchronization")
            || q.contains("coherence wave");
        if wants_kuramoto {
            if let Some(k) = self.components.kuramoto.as_ref() {
                sections.push(format!(
                    "KURAMOTO (GRAPH SYNCHRONIZATION):\n  R: {:.4}\n  Mean phase: {:.3}\n  Oscillators: {}\n  Trend(ΔR): {:+.4}\n  Phase entropy: {:.3}\n  Velocity stddev: {:.4}\n  R-window stddev: {:.4}\n  Preflight warnings: {}",
                    k.order_parameter,
                    k.mean_phase,
                    k.oscillators.len(),
                    self.services.kuramoto.sync_trend(),
                    k.diagnostics.phase_entropy,
                    k.diagnostics.velocity_stddev,
                    k.diagnostics.r_window_stddev,
                    if k.preflight_warnings.is_empty() { "none".to_string() } else { k.preflight_warnings.join(" | ") }
                ));
            } else {
                sections.push("KURAMOTO: snapshot unavailable".to_string());
            }
        }

        // ── CASS (Context-Aware Semantic Skills) ────────────────────────────
        let wants_cass = q.contains("cass")
            || q.contains("skill")
            || q.contains("semantic")
            || q.contains("embedding")
            || q.contains("matching");

        if wants_cass {
            let cass = &self.components.cass;
            sections.push(format!(
                "CASS (SEMANTIC SKILLS):\n  Skills: {}\n  Context Depth: {}\n  Embedding Dim: {}\n  Recent Matches: {}",
                cass.skill_count,
                cass.context_depth,
                cass.embedding_dimension,
                cass.recent_matches.len()
            ));
        }

        // ── Skill Forge / Skill Bank (SkillRL) ─────────────────────────────
        let wants_skills = q.contains("skill")
            || q.contains("skillrl")
            || q.contains("skill forge")
            || q.contains("skill bank")
            || q.contains("distill")
            || q.contains("evolve");
        if wants_skills {
            let all_skills = self.world.skill_bank.all_skills();
            let active_count = all_skills
                .iter()
                .filter(|s| matches!(s.status, SkillStatus::Active | SkillStatus::Advanced))
                .count();
            let mut top: Vec<_> = all_skills.into_iter().collect();
            top.sort_by(|a, b| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let top_rows: Vec<String> = top
                .into_iter()
                .take(6)
                .map(|s| {
                    format!(
                        "  [{}] {} ({:.2}, {:?})",
                        s.id, s.title, s.confidence, s.status
                    )
                })
                .collect();
            sections.push(format!(
                "SKILL FORGE / SKILL BANK:\n  Total: {}\n  Active: {}\n  Evolution Epoch: {}\n{}",
                self.world.skill_bank.total_skills(),
                active_count,
                self.world.skill_bank.evolution_epoch,
                if top_rows.is_empty() {
                    "  (no skills yet)".to_string()
                } else {
                    top_rows.join("\n")
                }
            ));
        }

        // ── Federation ──────────────────────────────────────────────────────
        let wants_federation = q.contains("federation")
            || q.contains("peer")
            || q.contains("connect")
            || q.contains("sync")
            || q.contains("import")
            || q.contains("export")
            || q.contains("remote")
            || q.contains("distributed");

        if wants_federation {
            let fed_status = if let Some(ref addr) = self.federation_addr {
                format!(
                    "FEDERATION:\n  Status: ACTIVE\n  Address: {}\n  Peers: {}\n  Imported: {}\n  Exported: {}\n  Conflicts: {}",
                    addr, self.federation_peers.len(),
                    self.federation_imported, self.federation_exported, self.federation_conflicts
                )
            } else {
                "FEDERATION:\n  Status: INACTIVE\n  Start with: --federation <addr> --peer <url>"
                    .into()
            };
            sections.push(fed_status);
        }

        // ── GPU ─────────────────────────────────────────────────────────────
        let wants_gpu = q.contains("gpu")
            || q.contains("acceleration")
            || q.contains("compute")
            || q.contains("shader")
            || q.contains("metal")
            || q.contains("cuda");

        if wants_gpu {
            let gpu = &self.components.gpu;
            let load = gpu
                .compute_load
                .map(|v| format!("{:.0}%", v * 100.0))
                .unwrap_or_else(|| "n/a".to_string());
            let mem = gpu
                .memory_used_mb
                .map(|v| format!("{} MB", v))
                .unwrap_or_else(|| "n/a".to_string());
            sections.push(format!(
                "GPU:\n  Status: {}\n  Device: {}\n  Load: {}\n  Memory: {}\n  Fallback: {}",
                if gpu.available { "ENABLED" } else { "DISABLED" },
                gpu.device_name.as_deref().unwrap_or("n/a"),
                load,
                mem,
                if gpu.fallback_active { "Yes" } else { "No" },
            ));
        }

        // ── LLM ─────────────────────────────────────────────────────────────
        let wants_llm = q.contains("llm")
            || q.contains("model")
            || q.contains("inference")
            || q.contains("generation")
            || q.contains("token")
            || q.contains("ollama");

        if wants_llm {
            let llm = &self.components.llm;
            let cache = llm
                .cache_hit_rate
                .map(|v| format!("{:.1}%", v * 100.0))
                .unwrap_or_else(|| "n/a".to_string());
            sections.push(
                format!(
                    "LLM ENGINE:\n  Provider: Ollama @ localhost:11434\n  Model: {}\n  Loaded: {}\n  Cache Hit Rate: {}\n  Status: {}",
                    llm.model_name.as_deref().unwrap_or("(none)"),
                    if llm.model_loaded { "YES" } else { "NO" },
                    cache,
                    if llm.model_loaded { "Ready" } else { "Idle" },
                )
            );
        }

        // ── Optional system summary for explicit diagnostic questions ───────
        let wants_system_summary = q.contains("system state")
            || q.contains("world state")
            || q.contains("runtime state")
            || q.contains("internal state")
            || q.contains("component status")
            || q.contains("health check")
            || q.contains("diagnostic")
            || q.contains("telemetry")
            || q.contains("/api/components");
        let coherence = self.world.global_coherence();
        let coherence_trend = if self.coherence_history.len() >= 5 {
            let recent: Vec<f64> = self
                .coherence_history
                .iter()
                .rev()
                .take(5)
                .map(|&v| v as f64 / 100.0)
                .collect();
            let first = recent.last().copied().unwrap_or(coherence);
            let last = recent.first().copied().unwrap_or(coherence);
            if last - first > 0.001 {
                "rising"
            } else if first - last > 0.001 {
                "falling"
            } else {
                "stable"
            }
        } else {
            "stable"
        };

        let summary = format!(
            "SYSTEM STATE: tick={} coherence={:.4} ({}) agents={} edges={} beliefs={} experiences={} ontology={}",
            self.world.tick_count, coherence, coherence_trend,
            self.world.agents.len(), self.world.edges.len(),
            self.world.beliefs.len(), self.world.experiences.len(),
            self.world.ontology.len(),
        );

        if sections.is_empty() {
            return String::new();
        }
        if wants_system_summary {
            format!("{}\n\n{}", summary, sections.join("\n\n"))
        } else {
            sections.join("\n\n")
        }
    }

    /// Small, always-safe agent snapshot for prompts.
    fn build_agent_roster_brief(&self, max_agents: usize) -> String {
        if self.world.agents.is_empty() || max_agents == 0 {
            return String::new();
        }
        let mut agents: Vec<_> = self.world.agents.iter().collect();
        agents.sort_by(|a, b| {
            b.drives
                .curiosity
                .partial_cmp(&a.drives.curiosity)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        let rows: Vec<String> = agents.into_iter().take(max_agents).map(|a| {
            format!(
                "agent#{} {:?} | curiosity={:.3} harmony={:.3} growth={:.3} transcendence={:.3} lr={:.4} jw={:.4}",
                a.id, a.role,
                a.drives.curiosity, a.drives.harmony, a.drives.growth, a.drives.transcendence,
                a.learning_rate, a.jw
            )
        }).collect();
        rows.join("\n")
    }

    /// Semantic extraction: treat graph as syntax, extract proximity from topology.
    fn build_semantic_grounding(
        &self,
        max_messages: usize,
        max_edges: usize,
    ) -> (String, EvidenceIndex) {
        let mut lines = Vec::new();
        lines.push(
            "SEMANTIC EXTRACTION (proximity only; derived from syntactic graph + messages):"
                .to_string(),
        );
        lines.push("Semantics = topological proximity induced by hyperedges; do not add computational semantics.".to_string());
        lines.push("Rule: cite message ids first; edge ids optional. Do not infer from traits or add external summaries.".to_string());
        lines.push("Action bias: if recommending an addition, phrase it as a concrete action and cite a msg:ID that describes the mechanism (e.g., collaboration_module: task-router/shared-memory/handoff).".to_string());
        lines.push("OUTPUT FORMAT: For each claim, include lines:\nClaim: ...\nEvidence: [msg:ID, edge:ID]".to_string());
        let mut evidence = EvidenceIndex::default();

        // Messages: prioritize task messages
        let mut task_lines = Vec::new();
        let mut other_lines = Vec::new();
        for msg in self.services.communication.recent_messages(max_messages) {
            let target = match msg.recipient {
                Target::Agent(id) => format!("agent#{}", id),
                Target::Broadcast => "broadcast".to_string(),
                Target::Swarm => "swarm".to_string(),
            };
            let line = format!(
                "[msg:{}] from agent#{} to {} type={} content={}",
                msg.id,
                msg.sender,
                target,
                msg.message_type,
                truncate_str(&msg.content, 200)
            );
            evidence.msg_ids.insert(msg.id.clone());
            evidence.msg_senders.insert(msg.id.clone(), msg.sender);
            evidence.msg_context.insert(
                msg.id.clone(),
                MessageEvidence {
                    msg_id: msg.id.clone(),
                    formatted: line.clone(),
                    sender: msg.sender,
                    target: target.clone(),
                    message_type: msg.message_type.clone(),
                    content: msg.content.clone(),
                    timestamp: msg.timestamp,
                },
            );
            if msg.message_type == "task" {
                task_lines.push(line);
            } else {
                other_lines.push(line);
            }
        }
        if !task_lines.is_empty() {
            lines.push("TASK MESSAGES (primary evidence):".to_string());
            lines.extend(task_lines.iter().cloned());
        }
        if !other_lines.is_empty() {
            lines.push("OTHER MESSAGES:".to_string());
            lines.extend(other_lines.iter().cloned());
        }
        if task_lines.is_empty() && other_lines.is_empty() {
            lines.push("No inter-agent messages available. State that evidence is missing rather than inventing it.".to_string());
        }

        let sparse = evidence.msg_ids.len() + evidence.edge_ids.len() < 3;
        if sparse {
            lines.push("EVIDENCE SCARCITY: produce at most 1 claim. If unsure, state 'insufficient evidence' and cite available ids.".to_string());
        }

        // Edges: select high-salience edges (syntax -> proximity)
        let coherence = self.world.global_coherence();
        let mut edges: Vec<_> = self.world.edges.iter().enumerate().collect();
        edges.sort_by(|a, b| {
            let sa = self.edge_salience(a.1, coherence);
            let sb = self.edge_salience(b.1, coherence);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut edge_lines = Vec::new();
        let mut selected_edges: Vec<(usize, &HyperEdge)> = Vec::new();
        for (idx, edge) in edges.into_iter().take(max_edges) {
            selected_edges.push((idx, edge));
            let participants = edge
                .participants
                .iter()
                .map(|p| format!("agent#{}", p))
                .collect::<Vec<_>>()
                .join(",");
            let line = format!(
                "[edge:{}] participants=[{}] weight={:.3} emergent={} age={} salience={:.3}",
                idx,
                participants,
                edge.weight,
                edge.emergent,
                edge.age,
                self.edge_salience(edge, coherence)
            );
            evidence.edge_ids.insert(idx);
            evidence
                .edge_participants
                .insert(idx, edge.participants.clone());
            edge_lines.push(line);
        }
        if !edge_lines.is_empty() {
            lines.push("SALIENT HYPEREDGES (syntactic structure):".to_string());
            lines.extend(edge_lines);
        }

        // Explicit topological proximity (semantics = proximity).
        let proximity_lines = self.build_topological_proximity(&selected_edges, 10);
        if !proximity_lines.is_empty() {
            lines.push("TOPOLOGICAL PROXIMITY (semantics induced by hyperedges):".to_string());
            lines.extend(proximity_lines);
        }

        (lines.join("\n"), evidence)
    }

    async fn maybe_extract_skillbank(&mut self) {
        if self.last_skill_extract.elapsed() < self.skill_extract_interval {
            return;
        }
        self.last_skill_extract = Instant::now();
        if let Err(err) = self.extract_skills_from_messages_and_edges().await {
            self.log(&format!("Skill extraction failed: {}", err));
        }
    }

    async fn extract_skills_from_messages_and_edges(&mut self) -> anyhow::Result<()> {
        let now = HyperStigmergicMorphogenesis::current_timestamp();
        let recent = self.services.communication.recent_messages(200);
        if recent.is_empty() {
            return Ok(());
        }

        let mut skill_hits: HashMap<String, Vec<(String, Option<usize>)>> = HashMap::new();
        let patterns: Vec<(&str, &str, &str, &[&str])> = vec![
            (
                "skill_collab_core",
                "Collaboration Module Core",
                "Use collaboration_module to coordinate cross-agent work with explicit routing and shared context.",
                &["collaboration module", "collaboration_module"]
            ),
            (
                "skill_collab_task_router",
                "Task Router Coordination",
                "Route inter-agent tasks through task-router to reduce duplication and enable explicit handoff.",
                &["task-router", "task router"]
            ),
            (
                "skill_collab_shared_memory",
                "Shared Memory Sync",
                "Use shared-memory to keep agent context consistent before handoff.",
                &["shared-memory", "shared memory"]
            ),
            (
                "skill_collab_handoff",
                "Explicit Handoff Protocol",
                "Use handoff-protocol to ensure task ownership transfer is explicit and auditable.",
                &["handoff", "handoff-protocol"]
            ),
        ];

        for msg in &recent {
            let content = msg.content.to_lowercase();
            for (id, _title, _principle, keys) in &patterns {
                if keys.iter().any(|k| content.contains(k)) {
                    skill_hits
                        .entry(id.to_string())
                        .or_default()
                        .push((msg.id.clone(), None));
                }
            }
            if msg.message_type == "task" {
                skill_hits
                    .entry("skill_inter_agent_task".to_string())
                    .or_default()
                    .push((msg.id.clone(), None));
            }
        }

        // Attach top salient edges as secondary evidence when present.
        let coherence = self.world.global_coherence();
        let mut edges: Vec<_> = self.world.edges.iter().enumerate().collect();
        edges.sort_by(|a, b| {
            let sa = self.edge_salience(a.1, coherence);
            let sb = self.edge_salience(b.1, coherence);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        let top_edges: Vec<usize> = edges.into_iter().take(3).map(|(i, _)| i).collect();
        if !top_edges.is_empty() {
            for (_skill, hits) in skill_hits.iter_mut() {
                if let Some(first) = hits.first_mut() {
                    first.1 = Some(top_edges[0]);
                }
            }
        }

        let mut updated = 0usize;
        let db = self.roodb.clone();
        for (skill_id, hits) in skill_hits {
            if hits.is_empty() {
                continue;
            }
            let (title, principle) = match skill_id.as_str() {
                "skill_inter_agent_task" => (
                    "Inter-Agent Task Routing",
                    "Assign tasks explicitly between agents with clear targets and task messages."
                ),
                "skill_collab_task_router" => (
                    "Task Router Coordination",
                    "Route inter-agent tasks through task-router to reduce duplication and enable explicit handoff."
                ),
                "skill_collab_shared_memory" => (
                    "Shared Memory Sync",
                    "Use shared-memory to keep agent context consistent before handoff."
                ),
                "skill_collab_handoff" => (
                    "Explicit Handoff Protocol",
                    "Use handoff-protocol to ensure task ownership transfer is explicit and auditable."
                ),
                _ => (
                    "Collaboration Module Core",
                    "Use collaboration_module to coordinate cross-agent work with explicit routing and shared context."
                ),
            };

            self.upsert_skill_in_bank(skill_id.clone(), title, principle, 0.6, now);

            if let Some(db) = db.clone() {
                let row = SkillRow {
                    skill_id: skill_id.clone(),
                    title: title.to_string(),
                    principle: principle.to_string(),
                    level: "General".to_string(),
                    role: None,
                    task: None,
                    confidence: 0.6,
                    usage_count: 0,
                    success_count: 0,
                    failure_count: 0,
                    status: "active".to_string(),
                    created_at: now,
                    updated_at: now,
                };
                let _ = db.upsert_skill(&row).await;
                for (msg_id, edge_id) in hits {
                    let ev = SkillEvidenceRow {
                        skill_id: skill_id.clone(),
                        msg_id,
                        edge_id: edge_id.map(|v| v as i64).unwrap_or(-1),
                        outcome: None,
                        created_at: now,
                    };
                    let _ = db.insert_skill_evidence(&ev).await;
                }
            }
            updated += 1;
        }

        if updated > 0 {
            self.log(&format!(
                "Skill extraction: updated {} skill(s) from message log",
                updated
            ));
        }

        Ok(())
    }

    async fn hydrate_skillbank_from_roodb(&mut self, db: Arc<RooDb>) -> anyhow::Result<()> {
        let rows = db.fetch_skills(200).await.unwrap_or_default();
        let row_count = rows.len();
        if rows.is_empty() {
            return Ok(());
        }
        for row in rows {
            let status = match row.status.as_str() {
                "advanced" => SkillStatus::Advanced,
                "suspended" => SkillStatus::Suspended {
                    suspended_at_tick: 0,
                    revival_attempts: 0,
                },
                "deprecated" => SkillStatus::Deprecated,
                _ => SkillStatus::Active,
            };
            if let Some(existing) = self
                .world
                .skill_bank
                .general_skills
                .iter_mut()
                .find(|s| s.id == row.skill_id)
            {
                existing.title = row.title.clone();
                existing.principle = row.principle.clone();
                existing.confidence = row.confidence;
                existing.usage_count = row.usage_count;
                existing.success_count = row.success_count;
                existing.failure_count = row.failure_count;
                existing.status = status;
                existing.last_evolved = row.updated_at;
            } else {
                self.world.skill_bank.general_skills.push(Skill {
                    id: row.skill_id.clone(),
                    title: row.title.clone(),
                    principle: row.principle.clone(),
                    when_to_apply: Vec::new(),
                    level: SkillLevel::General,
                    source: SkillSource::Seeded,
                    confidence: row.confidence,
                    usage_count: row.usage_count,
                    success_count: row.success_count,
                    failure_count: row.failure_count,
                    embedding: None,
                    created_at: row.created_at,
                    last_evolved: row.updated_at,
                    status,
                    bayesian: BayesianConfidence::default(),
                    credit_ema: 0.0,
                    credit_count: 0,
                    last_credit_tick: 0,
                    curation: SkillCuration::Legacy,
                    scope: SkillScope::default(),
                    delegation_ema: 0.0,
                    delegation_count: 0,
                    hired_count: 0,
                });
            }
        }
        self.log(&format!(
            "SkillBank hydrated from RooDB ({} skills)",
            row_count
        ));
        Ok(())
    }

    fn edge_salience(&self, edge: &HyperEdge, coherence: f64) -> f64 {
        let base = edge.weight / (1.0 + edge.age as f64);
        let coherence_scale = edge.weight / (coherence.max(1e-6));
        let emergent_bonus = if edge.emergent { 1.2 } else { 1.0 };
        base * coherence_scale * emergent_bonus
    }

    /// Compute topological proximity between agent pairs using selected hyperedges.
    fn build_topological_proximity(
        &self,
        edges: &[(usize, &HyperEdge)],
        max_pairs: usize,
    ) -> Vec<String> {
        if edges.is_empty() || max_pairs == 0 {
            return Vec::new();
        }
        let mut pair_scores: HashMap<(u64, u64), (f64, Vec<usize>)> = HashMap::new();
        for (edge_idx, edge) in edges {
            let w = edge.weight / (1.0 + edge.age as f64);
            let participants = &edge.participants;
            for i in 0..participants.len() {
                for j in (i + 1)..participants.len() {
                    let a = participants[i];
                    let b = participants[j];
                    let key = if a < b { (a, b) } else { (b, a) };
                    let entry = pair_scores.entry(key).or_insert((0.0, Vec::new()));
                    entry.0 += w;
                    entry.1.push(*edge_idx);
                }
            }
        }
        let mut pairs: Vec<_> = pair_scores.into_iter().collect();
        pairs.sort_by(|a, b| {
            b.1 .0
                .partial_cmp(&a.1 .0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        pairs.truncate(max_pairs);
        pairs
            .into_iter()
            .map(|((a, b), (score, edges))| {
                let edge_list = edges
                    .iter()
                    .map(|e| format!("edge:{}", e))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "[{}] proximity agent#{}↔agent#{} score={:.3}",
                    edge_list, a, b, score
                )
            })
            .collect()
    }

    fn upsert_skill_in_bank(
        &mut self,
        skill_id: String,
        title: &str,
        principle: &str,
        confidence: f64,
        now: u64,
    ) {
        if let Some(existing) = self
            .world
            .skill_bank
            .general_skills
            .iter_mut()
            .find(|s| s.id == skill_id)
        {
            existing.title = title.to_string();
            existing.principle = principle.to_string();
            existing.confidence = confidence;
            existing.last_evolved = now;
            existing.status = SkillStatus::Active;
            return;
        }

        self.world.skill_bank.general_skills.push(Skill {
            id: skill_id,
            title: title.to_string(),
            principle: principle.to_string(),
            when_to_apply: Vec::new(),
            level: SkillLevel::General,
            source: SkillSource::Seeded,
            confidence,
            usage_count: 0,
            success_count: 0,
            failure_count: 0,
            embedding: None,
            created_at: now,
            last_evolved: now,
            status: SkillStatus::Active,
            bayesian: BayesianConfidence::default(),
            credit_ema: 0.0,
            credit_count: 0,
            last_credit_tick: 0,
            curation: SkillCuration::Legacy,
            scope: SkillScope::default(),
            delegation_ema: 0.0,
            delegation_count: 0,
            hired_count: 0,
        });
    }

    fn parse_target(&self, raw: &str) -> Target {
        let lower = raw.trim().to_lowercase();
        if lower.starts_with("agent:") {
            if let Ok(id) = lower.trim_start_matches("agent:").parse::<u64>() {
                return Target::Agent(id);
            }
        }
        match lower.as_str() {
            "swarm" => Target::Swarm,
            "broadcast" => Target::Broadcast,
            _ => Target::Broadcast,
        }
    }

    fn parse_message_type(&self, raw: &str) -> MessageType {
        match raw.trim().to_lowercase().as_str() {
            "direct" => MessageType::Direct,
            "task" => MessageType::Task,
            "completion" => MessageType::Completion,
            "coordination" => MessageType::Coordination,
            "alert" => MessageType::Alert,
            "info" => MessageType::Info,
            "query" => MessageType::Query,
            "response" => MessageType::Response,
            "proposal" => MessageType::Proposal,
            "vote" => MessageType::Vote,
            "stigmergic" => MessageType::StigmergicSignal,
            "discovery" => MessageType::Discovery,
            "heartbeat" => MessageType::Heartbeat,
            other => MessageType::Custom(other.to_string()),
        }
    }

    fn send_inter_agent_message(
        &mut self,
        sender: u64,
        msg: Message,
        target: Target,
        forward_ui: bool,
    ) {
        let kind = msg.message_type.to_string();
        let content = msg.content.clone();
        let target_str = match target {
            Target::Agent(id) => format!("agent:{}", id),
            Target::Broadcast => "broadcast".to_string(),
            Target::Swarm => "swarm".to_string(),
        };
        let msg_id = match self.services.communication.send_from(sender, msg, target) {
            Ok(id) => Some(id),
            Err(_) => None,
        };

        if let (Some(id), Some(db)) = (msg_id.clone(), self.roodb.clone()) {
            let row = MessageRow {
                msg_id: id,
                sender,
                target: target_str.clone(),
                kind: kind.clone(),
                content: content.clone(),
                created_at: HyperStigmergicMorphogenesis::current_timestamp(),
            };
            tokio::spawn(async move {
                let _ = db.insert_message(&row).await;
            });
        }

        if forward_ui {
            let forward_base = env::var("HSM_HYPERGRAPH_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8787".to_string());
            let payload = serde_json::json!({
                "sender": sender,
                "target": target_str,
                "kind": kind,
                "content": content,
            });
            tokio::spawn(async move {
                let url = format!("{}/api/message", forward_base.trim_end_matches('/'));
                let _ = reqwest::Client::new().post(url).json(&payload).send().await;
            });
        }
    }

    async fn handle_plan_steps(&mut self, plan_text: String, mut plan_steps: Vec<PlanStep>) {
        if plan_steps.is_empty() {
            return;
        }
        self.enrich_plan_steps(&mut plan_steps);
        {
            let mut guard = self.plan_steps.write().await;
            *guard = plan_steps.clone();
        }
        for step in plan_steps.iter().cloned() {
            self.start_plan_optimization(step);
        }
        self.emit_plan_steps_event(&plan_text, &plan_steps);

        // ── Build hire records for delegation-enriched plan steps ──
        let now = HyperStigmergicMorphogenesis::current_timestamp();
        let tick = self.world.tick_count;
        let mut hire_rows: Vec<SkillHireRow> = Vec::new();
        let mut step_hire_trees: Vec<HireTree> = Vec::new();

        for step in &plan_steps {
            if step.skill_refs.len() < 2 {
                // Single skill (legacy fallback) — no delegation tree needed
                continue;
            }
            // This step used scoped delegation retrieval → create hire records
            let root_skill_id = format!("orchestrator_step_{}", step.index);
            let signature = ProofSignature {
                signer_skill_id: root_skill_id.clone(),
                claim: step.claim.clone(),
                evidence_msg_ids: step.messages.iter().map(|m| m.msg_id.clone()).collect(),
                parent_signature_id: None,
                signature_id: format!("sig_{}_{}", step.index, now),
                timestamp: now,
            };

            let briefing_ids: Vec<String> =
                step.skill_refs.iter().map(|sr| sr.id.clone()).collect();
            let domains: Vec<String> = step
                .claim
                .split_whitespace()
                .filter(|w| w.len() > 4)
                .take(5)
                .map(|w| {
                    w.to_lowercase()
                        .trim_matches(|c: char| !c.is_alphanumeric())
                        .to_string()
                })
                .filter(|w| !w.is_empty())
                .collect();

            let mut tree = HireTree::new(root_skill_id.clone(), step.index);

            for (child_idx, sr) in step.skill_refs.iter().enumerate() {
                let hire_id = format!("hire_{}_{}_{}", step.index, child_idx, now);
                let child_sig = ProofSignature {
                    signer_skill_id: root_skill_id.clone(),
                    claim: format!("Delegating '{}' to skill '{}'", step.claim, sr.title),
                    evidence_msg_ids: step.messages.iter().map(|m| m.msg_id.clone()).collect(),
                    parent_signature_id: Some(signature.signature_id.clone()),
                    signature_id: format!("sig_{}_{}", hire_id, now),
                    timestamp: now,
                };

                let hire = SkillHire {
                    hire_id: hire_id.clone(),
                    parent_skill_id: root_skill_id.clone(),
                    child_skill_id: sr.id.clone(),
                    package: DelegationPackage {
                        subproblem: step.claim.clone(),
                        subproblem_domains: domains.clone(),
                        skill_briefing: briefing_ids.clone(),
                        signature: child_sig.clone(),
                        budget: 1.0 / step.skill_refs.len() as f64,
                        depth: 1,
                    },
                    status: HireStatus::Active,
                    outcome_score: None,
                    created_at: now,
                    completed_at: None,
                };
                tree.hires.push(hire);

                // Build persistence row
                hire_rows.push(SkillHireRow {
                    hire_id,
                    parent_skill_id: root_skill_id.clone(),
                    child_skill_id: sr.id.clone(),
                    plan_step_index: step.index,
                    subproblem: step.claim.clone(),
                    subproblem_domains: domains.clone(),
                    skill_briefing: briefing_ids.clone(),
                    signature_id: child_sig.signature_id.clone(),
                    signature_claim: child_sig.claim.clone(),
                    signature_evidence: child_sig.evidence_msg_ids.clone(),
                    parent_sig_id: child_sig.parent_signature_id.clone(),
                    depth: 1,
                    budget: 1.0 / step.skill_refs.len() as f64,
                    status: "Active".to_string(),
                    outcome_score: None,
                    created_at: now,
                    completed_at: None,
                });
            }

            if !tree.hires.is_empty() {
                step_hire_trees.push(tree);
            }
        }

        // Store hire trees in skill bank for credit propagation
        for tree in &step_hire_trees {
            self.world.skill_bank.hire_trees.push(tree.clone());
        }
        if !step_hire_trees.is_empty() {
            self.log(&format!(
                "🌳 Delegation: {} hire trees created across {} steps ({} total hires)",
                step_hire_trees.len(),
                plan_steps.len(),
                hire_rows.len(),
            ));
        }

        // ── Persist plan steps + hire records to RooDB ──
        if let Some(ref db) = self.roodb {
            let db = db.clone();
            let steps_for_db = plan_steps.clone();
            let hires_for_db = hire_rows;
            tokio::spawn(async move {
                for step in &steps_for_db {
                    let row = PlanStepRow {
                        step_index: step.index,
                        claim: step.claim.clone(),
                        plan_text: step.plan_text.clone(),
                        evidence_msg_ids: step.messages.iter().map(|m| m.msg_id.clone()).collect(),
                        qmd_ids: step.qmd_ids.clone(),
                        skill_ref_ids: step.skill_refs.iter().map(|sr| sr.id.clone()).collect(),
                        has_task_msg: step.has_task_message,
                        workflow_msg_id: step.workflow_message_id.clone(),
                        created_at: now,
                    };
                    if let Err(e) = db.insert_plan_step(&row).await {
                        eprintln!("[PlanStep] Failed to persist step {}: {}", step.index, e);
                    }
                }
                for hire_row in &hires_for_db {
                    if let Err(e) = db.insert_skill_hire(hire_row).await {
                        eprintln!(
                            "[SkillHire] Failed to persist hire {}: {}",
                            hire_row.hire_id, e
                        );
                    }
                }
            });
        }

        // ── Skill Credit: delegation-aware propagation ──
        let used_skill_ids: Vec<String> = plan_steps
            .iter()
            .flat_map(|s| s.skill_refs.iter().map(|sr| sr.id.clone()))
            .collect();
        if !used_skill_ids.is_empty() {
            let credit_delta = 0.15;

            // If hire trees exist, propagate through the tree; otherwise flat credit
            if !step_hire_trees.is_empty() {
                let mut total_leaf = 0;
                let mut total_mgr = 0;
                let mut total_credit = 0.0;
                for tree in &step_hire_trees {
                    let result =
                        tree.propagate_credit(&mut self.world.skill_bank, credit_delta, tick);
                    total_leaf += result.leaf_updates;
                    total_mgr += result.manager_updates;
                    total_credit += result.total_credit_distributed;
                }
                self.log(&format!(
                    "⚙ Delegation credit propagated: {} leaf, {} manager updates, total={:.3}",
                    total_leaf, total_mgr, total_credit,
                ));
                // Archive completed hire trees
                for tree in step_hire_trees {
                    self.world.skill_bank.hire_history.push(tree);
                }
            } else {
                // Flat credit path (no delegation trees)
                let report =
                    self.world
                        .skill_bank
                        .apply_skill_credit(&used_skill_ids, credit_delta, tick);
                if report.updated > 0 {
                    self.log(&format!(
                        "⚙ Skill credit applied: {} updated, {} suspended, {} revived, mean_credit={:.3}",
                        report.updated, report.suspended, report.revived, report.mean_credit
                    ));
                }
            }

            // ── Skill Evolution Trigger: evolve when enough credit data accumulated ──
            let total_credit_records = self.world.skill_bank.credit_history.len();
            if total_credit_records > 0 && total_credit_records % 20 == 0 {
                let recent_experiences: Vec<_> = self
                    .world
                    .experiences
                    .iter()
                    .rev()
                    .take(10)
                    .filter(|exp| {
                        matches!(
                            exp.outcome,
                            hyper_stigmergy::ExperienceOutcome::Negative { .. }
                        )
                    })
                    .cloned()
                    .collect();
                if !recent_experiences.is_empty() {
                    let result = self.world.skill_bank.evolve(&recent_experiences);
                    self.log(&format!(
                        "🧬 Skill evolution triggered: {} refined, {} deprecated",
                        result.skills_refined, result.skills_deprecated
                    ));
                }
            }
        }
    }

    fn enrich_plan_steps(&mut self, steps: &mut Vec<PlanStep>) {
        for step in steps.iter_mut() {
            // Extract domain hints from claim text for scoped retrieval
            let domains: Vec<String> = step
                .claim
                .split_whitespace()
                .filter(|w| w.len() > 4)
                .take(5)
                .map(|w| {
                    w.to_lowercase()
                        .trim_matches(|c: char| !c.is_alphanumeric())
                        .to_string()
                })
                .filter(|w| !w.is_empty())
                .collect();

            // Try delegation-scoped retrieval first (curated/promoted skills only)
            let delegation_skills = self.world.skill_bank.retrieve_for_delegation(
                &domains, 0, // depth 0 = root orchestrator level
                3, // max 3 skills per step
                None,
            );

            if !delegation_skills.is_empty() {
                // Curated skills found — use them as the briefing
                step.skill_refs = delegation_skills
                    .iter()
                    .map(|s| SkillSummary {
                        id: s.id.clone(),
                        title: s.title.clone(),
                        confidence: s.confidence,
                    })
                    .collect();
            } else {
                // No curated skills available — fall back to ensure_plan_skill
                let skill = self
                    .world
                    .skill_bank
                    .ensure_plan_skill(&step.claim, &step.plan_text);
                step.skill_refs = vec![SkillSummary {
                    id: skill.id.clone(),
                    title: skill.title.clone(),
                    confidence: skill.confidence,
                }];
            }
        }
    }

    fn emit_plan_steps_event(&self, plan_text: &str, steps: &[PlanStep]) {
        if steps.is_empty() {
            return;
        }
        let payload = serde_json::json!({
            "type": "plan_steps",
            "plan_text": plan_text,
            "steps": steps,
        });
        let _ = self.web_council_broadcast.send(payload.to_string());
    }

    fn start_plan_optimization(&self, step: PlanStep) {
        let tx = self.web_optimize_broadcast.clone();
        let plan_index = step.index;
        let plan_claim = step.claim.clone();
        let objective = step.plan_text.clone();
        let model = self.chat_models[self.selected_model].1.to_string();
        let _ = tx.send(
            serde_json::json!({
                "type": "plan_optimize_start",
                "step": plan_index,
                "claim": plan_claim,
            })
            .to_string(),
        );
        tokio::spawn(async move {
            let mut config = OptimizationConfig::default();
            config.max_iterations = 4;
            config.population_size = 3;
            config.model = model;
            let mut session =
                OptimizationSession::new(objective, config, OptimizationMode::SingleTask);
            session.run(tx).await;
        });
    }

    async fn create_workflow_from_plan_step(&mut self, step_index: usize) {
        let step = {
            let guard = self.plan_steps.read().await;
            guard.get(step_index.saturating_sub(1)).cloned()
        };
        let step = match step {
            Some(step) => step,
            None => {
                self.log(&format!(
                    "Plan workflow request failed: step {} missing",
                    step_index
                ));
                return;
            }
        };
        let sender = step.messages.first().map(|m| m.sender).unwrap_or(0);
        let target_str = step
            .messages
            .first()
            .map(|m| m.target.clone())
            .unwrap_or_else(|| "broadcast".to_string());
        let target = self.parse_target(&target_str);
        let workflow_text = if let Some(msg_id) = step.workflow_message_id.as_ref() {
            format!("Workflow kickoff (based on {}): {}", msg_id, step.plan_text)
        } else {
            format!("Workflow kickoff: {}", step.plan_text)
        };
        let msg = Message::new(MessageType::Task, workflow_text);
        self.send_inter_agent_message(sender, msg, target, true);
        // Also trigger OptimizeAnything for this step's claim
        self.start_plan_optimization(step.clone());
        self.log(&format!(
            "✨ Workflow created for plan step {} (+ optimization seeded)",
            step.index
        ));
    }

    async fn send_chat_message(&mut self) {
        let text = self.chat_input.trim().to_string();
        if text.is_empty() || self.chat_streaming {
            return;
        }
        self.chat_input.clear();

        // Slash commands are handled locally — no Ollama call
        if text.starts_with('/') {
            let handled = self.handle_slash_command(&text).await;
            if handled {
                return;
            }
        }

        let model_display = self.chat_models[self.selected_model].0.to_string();
        let model_name = self.chat_models[self.selected_model].1.to_string();

        // Add user message (to both legacy and LCM)
        self.add_chat_message("user", &text, "").await;

        // Add placeholder for assistant response
        self.chat_messages.push(ChatMsg {
            role: "assistant".into(),
            content: String::new(),
            model: model_display.clone(),
        });

        self.chat_streaming = true;
        self.chat_scroll = 0;
        self.log(&format!(
            "Chat → {} via {}",
            truncate_str(&text, 40),
            model_display
        ));
        self.track_navigation_search(&text);

        // Broadcast agent activation for visualization
        self.broadcast_graph_activity(
            "agent_activate",
            Some(self.selected_model as u64),
            None,
            &format!("Model {} activated for chat", model_display),
        );

        // Check if LCM compaction is needed before building context
        self.maybe_compact_lcm_context().await;

        self.send_chat_message_async_common(model_name, model_display, text);
    }

    /// Common part of send_chat_message (sync part that spawns the async ollama task)
    fn send_chat_message_async_common(
        &mut self,
        model_name: String,
        model_display: String,
        text: String,
    ) {
        self.mark_llm_request_start(&model_name);

        // Build context using LCM + question-grounded live data.
        let base_context = self.build_lcm_context();
        let grounded_context = self.build_grounded_context(&text);
        let has_grounded_context = !grounded_context.is_empty();
        let mut context = if !has_grounded_context {
            base_context
        } else if base_context.is_empty() {
            grounded_context.clone()
        } else {
            format!(
                "{}\n\n[Grounded World Data]\n{}",
                base_context, grounded_context
            )
        };
        let agent_roster = self.build_agent_roster_brief(8);
        if !agent_roster.is_empty() && !is_prompt_injection_like(&text) {
            if context.is_empty() {
                context = format!("[Agent Snapshot]\n{}", agent_roster);
            } else {
                context = format!("{}\n\n[Agent Snapshot]\n{}", context, agent_roster);
            }
        }
        let roodb = self.roodb.clone();
        let (semantic_context, evidence_index) = self.build_semantic_grounding(20, 10);

        // Broadcast memory access for visualization
        self.broadcast_graph_activity(
            "memory_access",
            None,
            None,
            &format!("LCM context built: {} chars", context.len()),
        );

        // Update the web API /api/context endpoint
        if let Ok(mut ctx) = self.web_last_context.try_write() {
            *ctx = context.clone();
        }

        // Notify browser: new chat message started
        let _ = self.web_chat_broadcast.send(
            json!({
                "type": "user_message",
                "content": &text,
                "model": &model_display,
            })
            .to_string(),
        );

        // Detect if this is a coding task
        let is_coding_task = Self::is_coding_task(&text);
        let use_dspy_chat = !is_coding_task;
        let skill_role = if is_coding_task {
            Role::Coder
        } else {
            Role::Architect
        };
        let retrieved_skills = self.world.skill_bank.retrieve(&skill_role, None, &[]);
        let mut skill_prompt = SkillBank::format_for_prompt(&retrieved_skills);
        if skill_prompt.chars().count() > 3000 {
            skill_prompt = bounded_text(&skill_prompt, 3000);
        }
        let retrieved_skill_count =
            retrieved_skills.general.len() + retrieved_skills.specific.len();
        if retrieved_skill_count > 0 {
            self.broadcast_graph_activity(
                "task_execute",
                None,
                None,
                &format!(
                    "Skill retrieval: {} skills for {:?}",
                    retrieved_skill_count, skill_role
                ),
            );
        }

        // System prompt: ground Ollama in actual live data, not hallucination
        // OR use Coder prompt for coding tasks
        let system_prompt = if is_coding_task {
            format!(
                "You are an expert coding assistant. You help users with coding tasks by reading files, executing commands, editing code, and writing new files.\n\
                 \n\
                 AVAILABLE TOOLS:\n\
                 - read: Read file contents\n\
                   • path: Path to the file (relative or absolute)\n\
                   • offset: Line number to start from (1-indexed, optional)\n\
                   • limit: Maximum lines to read (default: 2000)\n\
                 - write: Write content to a file\n\
                   • path: Path to the file\n\
                   • content: Content to write\n\
                 - edit: Edit a file by replacing exact text\n\
                   • path: Path to the file\n\
                   • oldText: Exact text to find and replace (must match exactly including whitespace)\n\
                   • newText: New text to replace with\n\
                 - bash: Execute a bash command\n\
                   • command: Bash command to execute\n\
                   • timeout: Timeout in seconds (optional)\n\
                 \n\
                 GUIDELINES:\n\
                 - Use bash for file operations like ls, grep, find\n\
                 - Use read to examine files before editing\n\
                 - Use edit for precise, surgical edits (oldText must match exactly)\n\
                 - Use write only for new files or complete rewrites (automatically creates parent directories)\n\
                 - When summarizing your actions, output plain text directly - do NOT use cat or bash to display what you did\n\
                 - Be concise in your responses\n\
                 - Show file paths clearly when working with files\n\
                 \n\
                 LIVE WORLD DATA (for context):\n\
                 {}\n\n\
                 SKILL FORGE GUIDANCE (SkillRL retrieval):\n\
                 {}",
                context,
                if skill_prompt.is_empty() { "(none)".to_string() } else { skill_prompt.clone() },
            )
        } else if has_grounded_context {
            format!(
                "You are the analyst of Hyper-Stigmergic Morphogenesis II — \
                 a self-evolving hypergraph multi-agent system.\n\
                 \n\
                 LIVE WORLD DATA (answer from this, not from general knowledge):\n\
                 {}\n\
                 \n\
                 Agent roles: Architect (structural design), Catalyst (innovation), \
                 Chronicler (documentation & memory), Critic (skeptical evaluation), \
                 Explorer (novelty & diversity), Coder (code assistant).\n\
                 \n\
                 Answer concisely using the live data. Be specific about actual values. \
                 Do not invent data not shown above.\n\n\
                 SKILL FORGE GUIDANCE (SkillRL retrieval):\n\
                 {}",
                context,
                if skill_prompt.is_empty() {
                    "(none)".to_string()
                } else {
                    skill_prompt.clone()
                },
            )
        } else {
            format!(
                "You are a concise, practical assistant.\n\
                 Answer directly and focus on the user's request.\n\
                 Do not discuss internal system structure unless the user explicitly asks.\n\n\
                 Conversation context:\n{}\n\n\
                 SKILL FORGE GUIDANCE (SkillRL retrieval):\n{}",
                context,
                if skill_prompt.is_empty() {
                    "(none)".to_string()
                } else {
                    skill_prompt.clone()
                },
            )
        };

        // Check if context needs compaction before building messages
        self.maybe_compact_context(&model_name);

        let fallback_prompt = format!("{}\n\nUser: {}\nAssistant:", system_prompt, text);

        // Build conversation history for Ollama with compaction support
        let mut messages = vec![OllamaChatMsg::system(system_prompt)];

        // Add context summary if available (from previous compaction)
        if let Some(ref summary) = self.chat_context_summary {
            messages.push(OllamaChatMsg::system(format!(
                "[Previous conversation summary: {}]",
                summary
            )));
        }

        // Add recent messages (last 10 pairs, or fewer if we have a summary)
        let recent_count = if self.chat_context_summary.is_some() {
            10
        } else {
            20
        };
        let history_start = self.chat_messages.len().saturating_sub(recent_count + 1);
        for msg in &self.chat_messages[history_start..self.chat_messages.len().saturating_sub(1)] {
            match msg.role.as_str() {
                "user" => messages.push(OllamaChatMsg::user(msg.content.clone())),
                "assistant" => messages.push(OllamaChatMsg::assistant(msg.content.clone())),
                _ => {}
            }
        }

        let tx = self.chat_tx.clone();
        let bg_tx = self.bg_tx.clone();
        let chat_session = self.chat_dspy_session.clone();

        tokio::spawn(async move {
            let ollama = Ollama::new("http://localhost".to_string(), 11434);
            let mut parser = ThinkingStreamParser::default();

            if use_dspy_chat && !is_coding_task {
                let question_for_session = text.clone();
                {
                    let mut session = chat_session.lock().await;
                    session.add_turn(TurnRole::User, question_for_session.clone());
                }
                let draft_ctx = DspyContext {
                    question: &text,
                    grounded: "",
                    agents: &semantic_context,
                    prior: "",
                };
                let draft_trace = run_signature_traced(
                    &ollama,
                    &model_name,
                    &sig_chat_draft(),
                    &draft_ctx,
                    roodb.clone(),
                    None,
                )
                .await;
                let mut prior = String::new();
                let mut final_resp = None;
                let mut traces_to_persist: Vec<TraceResult> = Vec::new();
                if let Ok(mut t) = draft_trace {
                    prior = t.output.clone();
                    final_resp = Some(t.output.clone());
                    t.score = 0.7; // Default score for draft
                    traces_to_persist.push(t);
                }
                if !prior.is_empty() {
                    let refine_ctx = DspyContext {
                        question: &text,
                        grounded: "",
                        agents: &semantic_context,
                        prior: &prior,
                    };
                    if let Ok(mut t) = run_signature_traced(
                        &ollama,
                        &model_name,
                        &sig_chat_refine(),
                        &refine_ctx,
                        roodb.clone(),
                        None,
                    )
                    .await
                    {
                        final_resp = Some(t.output.clone());
                        t.score = 0.8; // Refined is better
                        traces_to_persist.push(t);
                    }
                }
                match final_resp {
                    Some(resp) => {
                        let mut verification =
                            App::verify_semantic_contract_static(&resp, &evidence_index);
                        let mut repaired = resp.clone();
                        let mut repair_count = 0i32;
                        if !verification.ok {
                            for _ in 0..2 {
                                if let Ok(fixed) = attempt_semantic_repair(
                                    &ollama,
                                    &model_name,
                                    &text,
                                    "",
                                    &semantic_context,
                                    &repaired,
                                    roodb.clone(),
                                )
                                .await
                                {
                                    repaired = fixed;
                                    repair_count += 1;
                                    verification = App::verify_semantic_contract_static(
                                        &repaired,
                                        &evidence_index,
                                    );
                                    if verification.ok {
                                        break;
                                    }
                                }
                            }
                        }
                        // Update last trace with verification results
                        if let Some(last) = traces_to_persist.last_mut() {
                            last.semantic_ok = verification.ok;
                            last.repair_count = repair_count;
                            last.score = if verification.ok { 0.9 } else { 0.4 };
                        }
                        // Fire-and-forget trace persistence
                        if let Some(ref db) = roodb {
                            let db = db.clone();
                            let bg = bg_tx.clone();
                            tokio::spawn(async move {
                                for trace in &traces_to_persist {
                                    persist_trace(&db, trace).await;
                                    let _ = bg.send(BgEvent::DspyTraceLogged {
                                        signature_name: trace.signature_name.clone(),
                                        score: trace.score,
                                    });
                                }
                            });
                        }
                        let user_facing = if repaired.trim().is_empty() {
                            resp
                        } else {
                            repaired
                        };
                        let (session_id, example_len) = {
                            let mut session = chat_session.lock().await;
                            session.add_turn(TurnRole::Assistant, user_facing.clone());
                            (session.id(), session.to_optimization_examples().len())
                        };
                        if example_len > 0 {
                            let _ = bg_tx.send(BgEvent::Log(format!(
                                "Dspy chat session {} has {} optimization examples",
                                session_id, example_len
                            )));
                        }
                        let display_text = strip_claim_evidence_format(&user_facing);
                        let _ = tx.send(ChatEvent::Token(display_text));
                        let _ = tx.send(ChatEvent::Done);
                        return;
                    }
                    None => {}
                }
            }

            // First, verify connectivity by trying a non-streaming request
            // to give a better error message than "Error in Ollama"
            let request = ChatMessageRequest::new(model_name.clone(), messages);

            use tokio_stream::StreamExt;
            match ollama.send_chat_messages_stream(request).await {
                Ok(mut stream) => {
                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(chunk) => {
                                if !chunk.message.content.is_empty() {
                                    let events = parser.push_chunk(&chunk.message.content);
                                    for event in events {
                                        // Strip Claim/Evidence format from user-facing tokens
                                        let cleaned_event = match event {
                                            ChatEvent::Token(text) => {
                                                ChatEvent::Token(strip_claim_evidence_format(&text))
                                            }
                                            other => other,
                                        };
                                        let _ = tx.send(cleaned_event);
                                    }
                                }
                                if chunk.done {
                                    for event in parser.finish() {
                                        // Strip Claim/Evidence format from final tokens too
                                        let cleaned_event = match event {
                                            ChatEvent::Token(text) => {
                                                ChatEvent::Token(strip_claim_evidence_format(&text))
                                            }
                                            other => other,
                                        };
                                        let _ = tx.send(cleaned_event);
                                    }
                                    let _ = tx.send(ChatEvent::Done);
                                    return;
                                }
                            }
                            Err(_) => {
                                let _ = tx.send(ChatEvent::Error(
                                    "Stream decode error — response may be incomplete".into(),
                                ));
                                return;
                            }
                        }
                    }
                    // Stream ended without done flag
                    let _ = tx.send(ChatEvent::Done);
                }
                Err(e) => {
                    let gen_request =
                        GenerationRequest::new(model_name.clone(), fallback_prompt.clone());
                    if let Ok(gen_response) = ollama.generate(gen_request).await {
                        if !gen_response.response.is_empty() {
                            let mut fallback_parser = ThinkingStreamParser::default();
                            for event in fallback_parser.push_chunk(&gen_response.response) {
                                // Strip Claim/Evidence format from fallback tokens
                                let cleaned_event = match event {
                                    ChatEvent::Token(text) => {
                                        ChatEvent::Token(strip_claim_evidence_format(&text))
                                    }
                                    other => other,
                                };
                                let _ = tx.send(cleaned_event);
                            }
                            for event in fallback_parser.finish() {
                                // Strip Claim/Evidence format from final fallback tokens
                                let cleaned_event = match event {
                                    ChatEvent::Token(text) => {
                                        ChatEvent::Token(strip_claim_evidence_format(&text))
                                    }
                                    other => other,
                                };
                                let _ = tx.send(cleaned_event);
                            }
                        }
                        let _ = tx.send(ChatEvent::Done);
                        return;
                    }

                    let err_str = format!("{}", e);
                    let detail = if err_str.contains("Connection refused")
                        || err_str.contains("connection refused")
                    {
                        "Ollama not running — start it with: ollama serve".into()
                    } else if err_str.contains("404") || err_str.contains("not found") {
                        format!(
                            "Model '{}' not found — pull it with: ollama pull {}",
                            model_name, model_name
                        )
                    } else if err_str.contains("timeout") || err_str.contains("Timeout") {
                        "Connection timed out — is Ollama responsive?".into()
                    } else {
                        format!("{} (is Ollama running? try: ollama serve)", err_str)
                    };
                    let _ = tx.send(ChatEvent::Error(detail));
                }
            }
        });
    }

    fn is_coding_task(text: &str) -> bool {
        text.contains("code")
            || text.contains("implement")
            || text.contains("debug")
            || text.contains("refactor")
            || text.contains("fix")
            || text.contains("write")
            || text.contains("file")
            || text.contains("function")
            || text.starts_with("/coder")
            || text.contains("bash")
            || text.contains("grep")
            || text.contains("read ")
    }

    /// Run the Code Agent with pi-agent-core style tool execution.
    ///
    /// The agent loop handles:
    /// 1. User query processing
    /// 2. LLM generates response with potential tool calls
    /// Launch an optimize_anything session in a background task.
    /// Events are streamed over `web_optimize_broadcast`.
    fn do_optimize(&mut self, body: serde_json::Value) {
        let tx = self.web_optimize_broadcast.clone();
        tokio::spawn(async move {
            match session_from_json(&body.to_string()) {
                Ok(mut session) => {
                    session.run(tx).await;
                }
                Err(e) => {
                    let _ = tx.send(
                        serde_json::json!({"type":"error","message":e.to_string()}).to_string(),
                    );
                }
            }
        });
    }

    /// 3. Tool execution (read, write, edit, bash, grep, find, ls)
    /// 4. Results fed back to LLM
    /// 5. Repeats until no tool calls (final answer)
    fn start_code_agent_social_session(&mut self, session_id: &str, query: &str) {
        if self.world.agents.is_empty() {
            return;
        }

        let task_key = code_agent_task_key(query);
        let sensitivity = code_agent_task_sensitivity(query);
        let candidate_ids: Vec<u64> = self.world.agents.iter().map(|agent| agent.id).collect();
        let actor_id = self
            .world
            .recommend_delegate(
                &candidate_ids,
                Some(task_key.as_str()),
                None,
                Some(sensitivity.clone()),
            )
            .map(|candidate| candidate.agent_id)
            .or_else(|| self.world.agents.first().map(|agent| agent.id));
        let Some(actor_id) = actor_id else {
            return;
        };

        let promise_id = self.world.record_agent_promise(
            actor_id,
            None,
            &task_key,
            &format!("Complete code-agent task: {}", truncate_str(query, 80)),
            sensitivity.clone(),
            None,
        );
        self.world.apply_stigmergic_cycle();
        self.code_agent_sessions.insert(
            session_id.to_string(),
            CodeAgentRuntimeSession {
                query: query.to_string(),
                task_key: task_key.clone(),
                actor_id,
                promise_id,
                sensitivity,
                successful_tools: 0,
                failed_tools: 0,
                unsafe_tool_events: 0,
            },
        );
        self.log(&format!(
            "🤝 Code agent session {} mapped to agent {} for {}",
            session_id, actor_id, task_key
        ));
    }

    fn ingest_code_agent_tool_call(
        &mut self,
        session_id: &str,
        turn: i32,
        tool_name: &str,
        args: &serde_json::Value,
        result: Option<&str>,
        error: Option<&str>,
        duration_ms: u64,
    ) {
        let success = error.is_none();
        let capability_key = code_agent_capability_key(tool_name);
        let output_preview = result.map(|text| truncate_str(text, 240));
        let quality_score = code_agent_tool_quality(tool_name, success, duration_ms);
        let (actor_id, promise_id, task_key, sensitivity) = {
            let Some(session) = self.code_agent_sessions.get_mut(session_id) else {
                return;
            };
            let sensitivity = session.sensitivity.clone();
            let safe_for_sensitive_data =
                code_agent_tool_safe_for_sensitive_data(tool_name, args, success, &sensitivity);
            if success {
                session.successful_tools += 1;
            } else {
                session.failed_tools += 1;
            }
            if !safe_for_sensitive_data {
                session.unsafe_tool_events += 1;
            }
            (
                session.actor_id,
                session.promise_id.clone(),
                session.task_key.clone(),
                sensitivity,
            )
        };
        let safe_for_sensitive_data =
            code_agent_tool_safe_for_sensitive_data(tool_name, args, success, &sensitivity);
        let summary = if let Some(err) = error {
            format!(
                "turn {}: {} failed for {} ({})",
                turn, tool_name, task_key, err
            )
        } else {
            format!("turn {}: {} succeeded for {}", turn, tool_name, task_key)
        };
        self.world.record_tool_execution_evidence(
            actor_id,
            tool_name,
            "code-agent",
            Some(&task_key),
            success,
            &summary,
            Some(&promise_id),
            None,
            output_preview.as_deref(),
        );
        self.world.record_agent_delivery(
            actor_id,
            &capability_key,
            success,
            quality_score,
            success,
            safe_for_sensitive_data,
            &[],
        );
        self.world.apply_stigmergic_cycle();
    }

    fn complete_code_agent_social_session(
        &mut self,
        session_id: &str,
        quality_score: Option<f64>,
        error: Option<&str>,
    ) {
        let Some(session) = self.code_agent_sessions.remove(session_id) else {
            return;
        };

        let total_tools = session.successful_tools + session.failed_tools;
        let derived_quality = if total_tools == 0 {
            0.5
        } else {
            session.successful_tools as f64 / total_tools as f64
        };
        let final_quality = quality_score.unwrap_or(derived_quality).clamp(0.0, 1.0);
        let success = error.is_none() && final_quality >= 0.5;
        let safe_for_sensitive_data = session.unsafe_tool_events == 0;
        self.world.resolve_agent_promise(
            &session.promise_id,
            if success {
                PromiseStatus::Kept
            } else {
                PromiseStatus::Broken
            },
            Some(session.actor_id),
            Some(final_quality),
            Some(success),
            Some(safe_for_sensitive_data),
            &[],
        );
        self.world.apply_stigmergic_cycle();
        self.log(&format!(
            "🧠 Social memory updated from code agent session {} (agent={}, quality={:.2}, task={})",
            session_id,
            session.actor_id,
            final_quality,
            truncate_str(&session.query, 48)
        ));
    }

    fn do_code_agent(&mut self, query: String, model: &str) {
        use serde_json::json;

        let code_tx = self.web_code_broadcast.clone();
        let graph_tx = self.web_graph_activity_broadcast.clone();
        let bg_tx = self.bg_tx.clone();
        self.mark_llm_request_start(model);
        self.track_navigation_search(&query);

        self.log(&format!(
            "Code Agent starting: \"{}\"",
            truncate_str(&query, 60)
        ));

        // Broadcast agent activation for visualization
        self.broadcast_graph_activity(
            "agent_activate",
            Some(999), // Code agent ID
            None,
            &format!("Coder Agent: {}", truncate_str(&query, 50)),
        );

        // Send start event
        let _ = code_tx.send(
            json!({
                "type": "start",
                "query": query,
                "model": model
            })
            .to_string(),
        );

        let model = model.to_string();
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let session_id = uuid::Uuid::new_v4().to_string();
        let working_dir = cwd.to_string_lossy().to_string();

        // Emit session start event
        let _ = bg_tx.send(BgEvent::CodeAgentSessionStart {
            session_id: session_id.clone(),
            query: query.clone(),
            model: model.clone(),
            working_dir: working_dir.clone(),
        });

        tokio::spawn(async move {
            // Create tool executor with proper context
            let tool_context = hyper_stigmergy::CoderToolContext {
                cwd: cwd.clone(),
                env_vars: std::collections::HashMap::new(),
                timeout_ms: 60000,
                execution_policy:
                    hyper_stigmergy::coder_assistant::tools::ToolExecutionPolicy::default(),
            };
            let tool_executor = ToolExecutor::with_context(tool_context);

            // Preferred path: use the live coder_assistant AgentLoop
            // (streaming + tool-call parsing + schema validation).
            let use_live_coder_loop = true;
            if use_live_coder_loop {
                let model_name = model.clone();
                let system_prompt = format!(
                    "You are an expert coding assistant in a local CLI environment.\n\
                     Working directory: {}\n\
                     Use tools for concrete actions, keep responses concise, and do not invent tool outputs.",
                    cwd.display()
                );

                let mut agent = hyper_stigmergy::AgentLoop::new(
                    hyper_stigmergy::coder_assistant::agent_loop::AgentConfig {
                        model: model_name.clone(),
                        api_url: "http://localhost:11434".to_string(),
                        supports_thinking: true,
                        ..Default::default()
                    },
                    session_id.clone(),
                );
                agent.add_system_message(system_prompt.clone());

                let _ = bg_tx.send(BgEvent::CodeAgentSessionMessage {
                    session_id: session_id.clone(),
                    turn: 0,
                    role: "system".to_string(),
                    content: system_prompt,
                    has_tool_calls: false,
                });

                let (agent_event_tx, mut agent_event_rx) =
                    mpsc::channel::<hyper_stigmergy::coder_assistant::agent_loop::AgentEvent>(1024);
                let agent_query = query.clone();
                let run_handle = tokio::spawn(async move {
                    let run_result: Result<(), String> = agent
                        .add_user_message(agent_query, agent_event_tx)
                        .await
                        .map_err(|e| e.to_string());
                    let snapshot = agent.messages().to_vec();
                    (run_result, snapshot)
                });

                let mut final_response = String::new();
                let mut turn_count: i32 = 0;
                let mut pending_tool_args: std::collections::HashMap<
                    String,
                    Vec<serde_json::Value>,
                > = std::collections::HashMap::new();
                let mut loop_error: Option<String> = None;

                while let Some(event) = agent_event_rx.recv().await {
                    match event {
                        hyper_stigmergy::coder_assistant::agent_loop::AgentEvent::Stream(stream_ev) => {
                            match stream_ev {
                                hyper_stigmergy::StreamEvent::Content { text } => {
                                    let _ = code_tx.send(json!({
                                        "type": "token",
                                        "content": text
                                    }).to_string());
                                }
                                hyper_stigmergy::StreamEvent::Thinking { text } => {
                                    let _ = code_tx.send(json!({
                                        "type": "thinking",
                                        "content": text
                                    }).to_string());
                                }
                                hyper_stigmergy::StreamEvent::Done => {
                                    let _ = code_tx.send(json!({
                                        "type": "thinking_end"
                                    }).to_string());
                                }
                                _ => {}
                            }
                        }
                        hyper_stigmergy::coder_assistant::agent_loop::AgentEvent::MessageAdded(msg) => {
                            let role = match msg.role {
                                hyper_stigmergy::MessageRole::System => "system",
                                hyper_stigmergy::MessageRole::User => "user",
                                hyper_stigmergy::MessageRole::Assistant => {
                                    turn_count += 1;
                                    final_response = msg.content.clone();
                                    "assistant"
                                }
                                hyper_stigmergy::MessageRole::Tool => "tool",
                            };
                            let _ = bg_tx.send(BgEvent::CodeAgentSessionMessage {
                                session_id: session_id.clone(),
                                turn: turn_count,
                                role: role.to_string(),
                                content: msg.content.clone(),
                                has_tool_calls: msg.tool_calls.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
                            });
                        }
                        hyper_stigmergy::coder_assistant::agent_loop::AgentEvent::ToolStart { name, args } => {
                            pending_tool_args.entry(name.clone()).or_default().push(args.clone());
                            let _ = code_tx.send(json!({
                                "type": "tool_start",
                                "tool": name,
                                "args": args
                            }).to_string());
                            let _ = graph_tx.send(make_graph_event(
                                "task_execute",
                                Some(999),
                                None,
                                "Code Agent executing tool",
                            ));
                        }
                        hyper_stigmergy::coder_assistant::agent_loop::AgentEvent::ToolComplete { name, result } => {
                            let args = pending_tool_args
                                .get_mut(&name)
                                .and_then(|v| if v.is_empty() { None } else { Some(v.remove(0)) })
                                .unwrap_or_else(|| json!({}));
                            let file_path = args.get("path").and_then(|p| p.as_str()).map(|s| s.to_string());
                            let _ = bg_tx.send(BgEvent::CodeAgentSessionToolCall {
                                session_id: session_id.clone(),
                                turn: turn_count,
                                tool_name: name.clone(),
                                args: args.clone(),
                                result: Some(result.output.clone()),
                                error: None,
                                duration_ms: result.execution_time_ms,
                                file_path,
                            });
                            let _ = code_tx.send(json!({
                                "type": "tool_complete",
                                "tool": name,
                                "result": result.output,
                                "duration_ms": result.execution_time_ms
                            }).to_string());
                        }
                        hyper_stigmergy::coder_assistant::agent_loop::AgentEvent::ToolError { name, error } => {
                            let args = pending_tool_args
                                .get_mut(&name)
                                .and_then(|v| if v.is_empty() { None } else { Some(v.remove(0)) })
                                .unwrap_or_else(|| json!({}));
                            let file_path = args.get("path").and_then(|p| p.as_str()).map(|s| s.to_string());
                            let _ = bg_tx.send(BgEvent::CodeAgentSessionToolCall {
                                session_id: session_id.clone(),
                                turn: turn_count,
                                tool_name: name.clone(),
                                args: args.clone(),
                                result: None,
                                error: Some(error.clone()),
                                duration_ms: 0,
                                file_path,
                            });
                            let _ = code_tx.send(json!({
                                "type": "tool_error",
                                "tool": name,
                                "error": error
                            }).to_string());
                        }
                        hyper_stigmergy::coder_assistant::agent_loop::AgentEvent::IterationComplete { iteration } => {
                            let _ = code_tx.send(json!({
                                "type": "round",
                                "content": format!("\n━━━ Turn {} ━━━\n", iteration)
                            }).to_string());
                        }
                        hyper_stigmergy::coder_assistant::agent_loop::AgentEvent::Error(err) => {
                            loop_error = Some(err.clone());
                            let _ = code_tx.send(json!({
                                "type": "error",
                                "error": err
                            }).to_string());
                        }
                        hyper_stigmergy::coder_assistant::agent_loop::AgentEvent::Complete => {}
                    }
                }

                let (run_result, message_snapshot) = match run_handle.await {
                    Ok(v) => v,
                    Err(join_err) => (Err(join_err.to_string()), Vec::new()),
                };

                if final_response.is_empty() {
                    if let Some(last_assistant) = message_snapshot
                        .iter()
                        .rev()
                        .find(|m| m.role == hyper_stigmergy::MessageRole::Assistant)
                    {
                        final_response = last_assistant.content.clone();
                    }
                }

                let result: Result<(String, i32), String> = match run_result {
                    Ok(()) => {
                        if let Some(err) = loop_error {
                            Err(err)
                        } else {
                            Ok((final_response.clone(), turn_count))
                        }
                    }
                    Err(e) => Err(e.to_string()),
                };

                let quality_score = match &result {
                    Ok((output, _)) => evaluate_code_output(output, &query, &model_name)
                        .await
                        .unwrap_or(0.5),
                    Err(_) => 0.3,
                };

                let _ = code_tx.send(
                    json!({
                        "type": "quality_score",
                        "score": quality_score,
                    })
                    .to_string(),
                );
                let _ = bg_tx.send(BgEvent::CodeAgentQuality {
                    query: query.clone(),
                    quality_score,
                });

                let (final_response_opt, turns) = match &result {
                    Ok((output, t)) => (Some(output.clone()), *t),
                    Err(_) => (None, turn_count),
                };

                let _ = bg_tx.send(BgEvent::CodeAgentSessionComplete {
                    session_id: session_id.clone(),
                    final_response: final_response_opt,
                    quality_score: Some(quality_score),
                    turn_count: turns,
                    error: result.as_ref().err().cloned(),
                });

                match result {
                    Ok(_) => {
                        let _ = code_tx.send(json!({"type": "complete"}).to_string());
                        let _ = bg_tx.send(BgEvent::CodeAgentFinished { success: true });
                    }
                    Err(e) => {
                        let _ = code_tx.send(
                            json!({
                                "type": "error",
                                "error": e
                            })
                            .to_string(),
                        );
                        let _ = bg_tx.send(BgEvent::CodeAgentFinished { success: false });
                    }
                }

                let _ = graph_tx.send(make_graph_event(
                    "task_execute",
                    Some(999),
                    None,
                    "Code Agent completed",
                ));
                return;
            }

            // Run the agent loop with real tool execution inline
            // Returns (final_response, turn_count)
            let result: Result<(String, i32), String> = async {
                use serde_json::json;
                use regex::Regex;

                // Parse tool calls from LLM response (XML tags and JSON schema-style calls)
                fn parse_tool_calls(response: &str) -> Vec<(String, serde_json::Value)> {
                    let mut calls = Vec::new();

                    fn normalize_tool_name(raw: &str) -> Option<&'static str> {
                        match raw.trim().to_ascii_lowercase().as_str() {
                            "read" => Some("read"),
                            "write" => Some("write"),
                            "edit" => Some("edit"),
                            "bash" => Some("bash"),
                            "grep" => Some("grep"),
                            "find" => Some("find"),
                            "ls" => Some("ls"),
                            // Schema aliases from the coding contract
                            "glob" => Some("find"),
                            "task" => Some("task"),
                            "todowrite" => Some("todowrite"),
                            _ => None,
                        }
                    }

                    // XML / tag style calls
                    let patterns = [
                        ("read", r"(?is)<read>\s*<path>(.*?)</path>\s*</read>"),
                        ("write", r"(?is)<write>\s*<path>(.*?)</path>\s*<content>(.*?)</content>\s*</write>"),
                        ("edit", r"(?is)<edit>\s*<path>(.*?)</path>\s*<oldText>(.*?)</oldText>\s*<newText>(.*?)</newText>\s*</edit>"),
                        ("bash", r"(?is)<bash>\s*<command>(.*?)</command>\s*</bash>"),
                        ("grep", r"(?is)<grep>\s*<pattern>(.*?)</pattern>\s*(?:<path>(.*?)</path>)?\s*</grep>"),
                        ("find", r"(?is)<find>\s*<pattern>(.*?)</pattern>\s*</find>"),
                        ("find", r"(?is)<glob>\s*<pattern>(.*?)</pattern>\s*</glob>"),
                        ("ls", r"(?is)<ls>\s*(?:<path>(.*?)</path>)?\s*</ls>"),
                        ("task", r"(?is)<task>\s*<description>(.*?)</description>\s*<prompt>(.*?)</prompt>\s*<subagent_type>(.*?)</subagent_type>\s*</task>"),
                        ("todowrite", r"(?is)<todowrite>\s*(.*?)\s*</todowrite>"),
                    ];

                    for (tool_name, pattern) in &patterns {
                        if let Ok(re) = Regex::new(pattern) {
                            for cap in re.captures_iter(response) {
                                let args = match *tool_name {
                                    "read" => json!({"path": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim()}),
                                    "write" => json!({
                                        "path": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim(),
                                        "content": cap.get(2).map(|m| m.as_str()).unwrap_or("").trim()
                                    }),
                                    "edit" => json!({
                                        "path": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim(),
                                        "oldText": cap.get(2).map(|m| m.as_str()).unwrap_or("").trim(),
                                        "newText": cap.get(3).map(|m| m.as_str()).unwrap_or("").trim()
                                    }),
                                    "bash" => json!({"command": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim()}),
                                    "grep" => json!({
                                        "pattern": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim(),
                                        "path": cap.get(2).map(|m| m.as_str()).unwrap_or(".").trim()
                                    }),
                                    "find" => json!({"pattern": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim()}),
                                    "ls" => json!({"path": cap.get(1).map(|m| m.as_str()).unwrap_or(".").trim()}),
                                    "task" => json!({
                                        "description": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim(),
                                        "prompt": cap.get(2).map(|m| m.as_str()).unwrap_or("").trim(),
                                        "subagent_type": cap.get(3).map(|m| m.as_str()).unwrap_or("").trim(),
                                    }),
                                    "todowrite" => json!({
                                        "todos": cap.get(1).map(|m| m.as_str()).unwrap_or("").trim()
                                    }),
                                    _ => json!({}),
                                };
                                calls.push((tool_name.to_string(), args));
                            }
                        }
                    }

                    // JSON schema-style calls
                    fn parse_json_node(node: &serde_json::Value, out: &mut Vec<(String, serde_json::Value)>) {
                        if let Some(arr) = node.as_array() {
                            for item in arr {
                                parse_json_node(item, out);
                            }
                            return;
                        }

                        if let Some(obj) = node.as_object() {
                            // Wrapper shape: { "tools": [ ... ] }
                            if let Some(inner_tools) = obj.get("tools") {
                                parse_json_node(inner_tools, out);
                            }

                            let raw_name = obj.get("name")
                                .or_else(|| obj.get("tool"))
                                .and_then(|v| v.as_str());

                            if let Some(name) = raw_name {
                                if let Some(normalized) = normalize_tool_name(name) {
                                    let args = obj.get("args")
                                        .or_else(|| obj.get("parameters"))
                                        .or_else(|| obj.get("input"))
                                        .cloned()
                                        .unwrap_or_else(|| json!({}));
                                    out.push((normalized.to_string(), args));
                                }
                            }
                        }
                    }

                    // Parse raw response if it's pure JSON
                    if let Ok(root) = serde_json::from_str::<serde_json::Value>(response.trim()) {
                        parse_json_node(&root, &mut calls);
                    }

                    // Parse fenced JSON blocks
                    if let Ok(re_json_block) = Regex::new(r"(?is)```json\s*(.*?)```") {
                        for cap in re_json_block.captures_iter(response) {
                            if let Some(block) = cap.get(1).map(|m| m.as_str()) {
                                if let Ok(root) = serde_json::from_str::<serde_json::Value>(block.trim()) {
                                    parse_json_node(&root, &mut calls);
                                }
                            }
                        }
                    }

                    calls
                }

                let ollama = Ollama::new("http://localhost".to_string(), 11434);
                let max_turns = 10;

                // Build system prompt with refined tool schema
                let system_prompt = format!(
                    "You are an expert coding assistant operating in a local CLI environment.\n\n\
                     Environment:\n\
                     - The project root and working directory is: {}\n\
                     - Treat this directory as a sandbox. Only read from or write to files within this tree.\n\n\
                     TOOL INTERFACE (coding part):\n\
                     You support both XML tool calls and JSON schema-style tool calls.\n\
                     Canonical executable tools in this runtime are: read, write, edit, bash, grep, find, ls.\n\
                     Accepted aliases: Read/Write/Edit/Bash/Grep/LS, and Glob->find.\n\
                     Task and TodoWrite are accepted as planning signals but do not spawn real subagents or persistent todo UI here.\n\n\
                     REFERENCE BEHAVIOR (Claude Code v2.0.0 style):\n\
                     - Do exactly what was asked; nothing more, nothing less.\n\
                     - Keep responses concise and directly actionable.\n\
                     - Prefer editing existing files; avoid creating new files unless required.\n\
                     - Never create documentation files unless explicitly requested.\n\
                     - Prefer specialized tools over shell equivalents for file ops/search.\n\
                     - For multi-step work, use TodoWrite-style planning in your reasoning, then execute concrete tools.\n\
                     - Assist defensive security tasks only; refuse clearly malicious coding requests.\n\n\
                     XML formats:\n\
                     <read>\n\
                       <path>relative/path/from/project/root.rs</path>\n\
                     </read>\n\n\
                     <write>\n\
                       <path>relative/path/from/project/root.rs</path>\n\
                       <content>full file contents here</content>\n\
                     </write>\n\n\
                     <edit>\n\
                       <path>relative/path/from/project/root.rs</path>\n\
                       <oldText>text to replace</oldText>\n\
                       <newText>replacement text</newText>\n\
                     </edit>\n\n\
                     <bash>\n\
                       <command>cargo build</command>\n\
                     </bash>\n\n\
                     <grep>\n\
                       <pattern>fn main</pattern>\n\
                       <path>src</path>\n\
                     </grep>\n\n\
                     <find>\n\
                       <pattern>*.rs</pattern>\n\
                     </find>\n\n\
                     <ls>\n\
                       <path>.</path>\n\
                     </ls>\n\n\
                     JSON formats:\n\
                     {{\"name\":\"Read\",\"args\":{{\"path\":\"src/main.rs\"}}}}\n\
                     {{\"name\":\"Grep\",\"args\":{{\"pattern\":\"fn main\",\"path\":\"src\"}}}}\n\
                     {{\"tools\":[{{\"name\":\"LS\",\"args\":{{\"path\":\".\"}}}},{{\"name\":\"Glob\",\"args\":{{\"pattern\":\"*.rs\"}}}}]}}\n\
                     {{\"name\":\"Task\",\"args\":{{\"description\":\"search code\",\"prompt\":\"find where council mode is chosen\",\"subagent_type\":\"general-purpose\"}}}}\n\n\
                     {{\"name\":\"TodoWrite\",\"args\":{{\"todos\":[{{\"content\":\"Run tests\",\"status\":\"in_progress\",\"activeForm\":\"Running tests\"}}]}}}}\n\n\
                     Rules:\n\n\
                     1. Tool schema and tags\n\
                        - Prefer executable tools: read, write, edit, bash, grep, find, ls.\n\
                        - You may use Glob as alias for find.\n\
                        - Task and TodoWrite are advisory only in this runtime; follow up with concrete executable tools.\n\
                        - Do NOT invent new XML tags, attributes, or wrappers.\n\
                        - Surround tool calls with minimal natural language explanations as needed.\n\n\
                     2. Path and directory safety\n\
                        - All paths MUST be relative to the project root; do not use absolute paths.\n\
                        - Do NOT use parent directory segments like '../'.\n\
                        - Never attempt to access paths outside the project root.\n\n\
                     3. Reading before writing\n\
                        - Before modifying any file, always call <read> on it first.\n\
                        - Never guess file contents. If you have not seen a file through <read>, treat its contents as unknown.\n\n\
                     4. Write behavior\n\
                        - <write>: Replaces entire file content. Use for new files or complete rewrites.\n\
                        - <edit>: Replaces specific text. Use for small changes to existing files.\n\
                        - Choose the appropriate tool based on the scope of change.\n\n\
                     5. Tool results and failures\n\
                        - Never fabricate or assume tool results.\n\
                        - If a tool call fails, do NOT proceed as if it succeeded.\n\n\
                     6. Style and project conventions\n\
                        - Follow the existing style and conventions of the project.\n\
                        - When editing Rust code, prefer idiomatic Rust.\n\n\
                     7. Communication style\n\
                        - Use clear, minimal natural language to explain what you are doing and why.\n\
                        - Do NOT wrap your reasoning or explanations in XML-like tags.\n\
                        - Keep everything machine-parseable.\n\n\
                     8. Task completion\n\
                        - When complete, provide a brief summary of what you changed and which files you touched.",
                    cwd.display()
                );

                // Get initial context - ls of current directory
                let initial_context = match tool_executor.execute("ls", &json!({"path": "."})).await {
                    Ok(output) => format!("\n\nCurrent directory structure:\n{}", output),
                    Err(_) => String::new(),
                };

                let mut messages: Vec<OllamaChatMsg> = vec![
                    OllamaChatMsg::new(MessageRole::System, system_prompt.clone()),
                    OllamaChatMsg::new(MessageRole::User, format!("{}{}\n\nTask: {}",
                        query, initial_context, query)),
                ];

                // Record system message
                let _ = bg_tx.send(BgEvent::CodeAgentSessionMessage {
                    session_id: session_id.clone(),
                    turn: 0,
                    role: "system".to_string(),
                    content: system_prompt,
                    has_tool_calls: false,
                });

                // Record initial user message
                let _ = bg_tx.send(BgEvent::CodeAgentSessionMessage {
                    session_id: session_id.clone(),
                    turn: 0,
                    role: "user".to_string(),
                    content: format!("{}{}\n\nTask: {}", query, initial_context, query),
                    has_tool_calls: false,
                });

                let mut final_response = String::new();
                let mut turn_count: i32 = 0;

                for turn in 1..=max_turns {
                    turn_count = turn as i32;
                    let _ = code_tx.send(json!({
                        "type": "round",
                        "content": format!("\n━━━ Turn {} ━━━\n", turn)
                    }).to_string());

                    let request = ChatMessageRequest::new(model.to_string(), messages.clone());

                    // Stream the response
                    match ollama.send_chat_messages_stream(request).await {
                        Ok(mut stream) => {
                            let mut response_buffer = String::new();

                            while let Some(result) = stream.next().await {
                                match result {
                                    Ok(chunk) => {
                                        let content = chunk.message.content;
                                        let _ = code_tx.send(json!({
                                            "type": "token",
                                            "content": &content
                                        }).to_string());
                                        response_buffer.push_str(&content);
                                    }
                                    Err(e) => {
                                        return Err(format!("Stream error: {:?}", e));
                                    }
                                }
                            }

                            final_response = response_buffer.clone();

                            // Add assistant response to messages
                            messages.push(OllamaChatMsg::new(MessageRole::Assistant, response_buffer.clone()));

                            // Record assistant message
                            let has_tool_calls = !parse_tool_calls(&response_buffer).is_empty();
                            let _ = bg_tx.send(BgEvent::CodeAgentSessionMessage {
                                session_id: session_id.clone(),
                                turn: turn as i32,
                                role: "assistant".to_string(),
                                content: response_buffer.clone(),
                                has_tool_calls,
                            });

                            // Parse and execute tool calls
                            let tool_calls = parse_tool_calls(&response_buffer);

                            if tool_calls.is_empty() {
                                // No tool calls, we're done
                                break;
                            }

                            // Execute each tool call
                            for (tool_name, args) in tool_calls {
                                let _ = code_tx.send(json!({
                                    "type": "tool_start",
                                    "tool": &tool_name,
                                    "args": &args
                                }).to_string());

                                let start_time = std::time::Instant::now();

                                // Broadcast tool execution to graph
                                let _ = graph_tx.send(make_graph_event(
                                    "task_execute",
                                    Some(999),
                                    None,
                                    &format!("Executing {}: {:?}", tool_name, args),
                                ));

                                let result = if tool_name == "task" {
                                    let prompt = args.get("prompt")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .trim();
                                    let subagent = args.get("subagent_type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("general-purpose");
                                    let desc = args.get("description")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    Ok(format!(
                                        "Task tool acknowledged (desc='{}', subagent='{}'). \
                                         This local runtime does not spawn subagents. \
                                         Continue with concrete tools (grep/find/read/edit/bash/ls). \
                                         Requested task prompt: {}",
                                        desc,
                                        subagent,
                                        prompt
                                    ))
                                } else if tool_name == "todowrite" {
                                    Ok(
                                        "TodoWrite acknowledged. This runtime has no persistent todo UI; \
                                         continue by executing concrete tools and report progress in responses."
                                            .to_string()
                                    )
                                } else {
                                    tool_executor.execute(&tool_name, &args).await
                                };
                                let duration_ms = start_time.elapsed().as_millis() as u64;

                                // Extract file path from args if present
                                let file_path = args.get("path").and_then(|p| p.as_str()).map(|s| s.to_string());

                                match result {
                                    Ok(ref output) => {
                                        let _ = bg_tx.send(BgEvent::CodeAgentSessionToolCall {
                                            session_id: session_id.clone(),
                                            turn: turn as i32,
                                            tool_name: tool_name.clone(),
                                            args: args.clone(),
                                            result: Some(output.clone()),
                                            error: None,
                                            duration_ms,
                                            file_path: file_path.clone(),
                                        });
                                    }
                                    Err(ref e) => {
                                        let _ = bg_tx.send(BgEvent::CodeAgentSessionToolCall {
                                            session_id: session_id.clone(),
                                            turn: turn as i32,
                                            tool_name: tool_name.clone(),
                                            args: args.clone(),
                                            result: None,
                                            error: Some(e.to_string()),
                                            duration_ms,
                                            file_path: file_path.clone(),
                                        });
                                    }
                                }

                                match result {
                                    Ok(output) => {
                                        let _ = code_tx.send(json!({
                                            "type": "tool_complete",
                                            "tool": &tool_name,
                                            "result": &output,
                                            "duration_ms": duration_ms
                                        }).to_string());

                                        // Add tool result to messages for context
                                        let tool_result_msg = format!(
                                            "<tool_result>\n<tool>{}</tool>\n<output>{}</output>\n</tool_result>",
                                            tool_name, output
                                        );
                                        messages.push(OllamaChatMsg::new(MessageRole::User, tool_result_msg));
                                    }
                                    Err(e) => {
                                        let error_msg = format!("Tool {} failed: {}", tool_name, e);
                                        let _ = code_tx.send(json!({
                                            "type": "tool_error",
                                            "tool": &tool_name,
                                            "error": &error_msg,
                                            "duration_ms": duration_ms
                                        }).to_string());

                                        let tool_result_msg = format!(
                                            "<tool_result>\n<tool>{}</tool>\n<error>{}</error>\n</tool_result>",
                                            tool_name, e
                                        );
                                        messages.push(OllamaChatMsg::new(MessageRole::User, tool_result_msg));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            return Err(format!("Ollama error: {}", e));
                        }
                    }
                }

                Ok((final_response, turn_count))
            }.await;

            // Extract result data
            let (final_response_str, turn_count) = match &result {
                Ok((output, turns)) => (Some(output.clone()), *turns),
                Err(_) => (None, 0i32),
            };

            // Send quality evaluation
            let quality_score = match &result {
                Ok((output, _)) => evaluate_code_output(output, &query, &model)
                    .await
                    .unwrap_or(0.5),
                Err(_) => 0.3,
            };

            let _ = code_tx.send(
                json!({
                    "type": "quality_score",
                    "score": quality_score,
                })
                .to_string(),
            );

            // Record experience
            let _ = bg_tx.send(BgEvent::CodeAgentQuality {
                query: query.clone(),
                quality_score,
            });

            // Determine error string
            let error_str = match &result {
                Ok(_) => None,
                Err(e) => Some(e.clone()),
            };

            // Emit session complete event
            let _ = bg_tx.send(BgEvent::CodeAgentSessionComplete {
                session_id: session_id.clone(),
                final_response: final_response_str,
                quality_score: Some(quality_score),
                turn_count,
                error: error_str,
            });

            // Handle result
            match result {
                Ok(_) => {
                    let _ = code_tx.send(json!({"type": "complete"}).to_string());
                    let _ = bg_tx.send(BgEvent::CodeAgentFinished { success: true });
                }
                Err(e) => {
                    let _ = code_tx.send(
                        json!({
                            "type": "error",
                            "error": e
                        })
                        .to_string(),
                    );
                    let _ = bg_tx.send(BgEvent::CodeAgentFinished { success: false });
                }
            }

            // Broadcast completion
            let _ = graph_tx.send(make_graph_event(
                "task_execute",
                Some(999),
                None,
                "Code Agent completed",
            ));
        });
    }

    /// Run the Socratic council: 3-role sequential deliberation with asymmetric information.
    ///
    /// Protocol:
    /// 1. Analyst: Sees only the question. Produces structured analysis.
    /// 2. Challenger: Sees question + Analyst response. Adversarial review.
    /// 3. Chair: Sees full transcript. Produces canonical answer + dissent notes.
    ///
    /// All roles use the same abliterated model but with specialized prompts.
    ///
    /// Modes:
    /// - "auto"/"socratic" (default): ModeSwitcher selects simple/debate/orchestrate/llm
    /// - "simple": Single-pass answer without deliberation
    /// - "debate": Extended multi-round debate (more rounds)
    /// - "orchestrate": Task breakdown with parallel sub-task execution
    /// - "llm": full LLM-deliberation council
    fn estimate_council_urgency(question: &str) -> f64 {
        let q = question.to_ascii_lowercase();
        let urgent_keywords = [
            "urgent",
            "asap",
            "immediately",
            "now",
            "critical",
            "prod",
            "outage",
            "security",
            "incident",
            "bug",
            "broken",
            "stuck",
            "timeout",
            "failed",
        ];
        let routine_keywords = [
            "routine", "minor", "small", "refactor", "cleanup", "style", "docs", "rename",
        ];
        let urgent_hits = urgent_keywords.iter().filter(|kw| q.contains(**kw)).count() as f64;
        let routine_hits = routine_keywords
            .iter()
            .filter(|kw| q.contains(**kw))
            .count() as f64;
        (0.45 + urgent_hits * 0.12 - routine_hits * 0.08).clamp(0.0, 1.0)
    }

    fn build_council_members(&self) -> Vec<CouncilMember> {
        self.world
            .agents
            .iter()
            .map(|a| CouncilMember {
                agent_id: a.id,
                role: a.role.clone(),
                expertise_score: (a.learning_rate + a.jw).clamp(0.0, 1.0),
                participation_weight: 1.0,
            })
            .collect()
    }

    fn resolve_council_mode(
        &self,
        question: &str,
        requested_mode: &str,
    ) -> (String, Option<ModeSelectionReport>) {
        let req = requested_mode.trim().to_ascii_lowercase();
        if req == "simple" || req == "debate" || req == "orchestrate" || req == "llm" {
            return (req, None);
        }

        // Legacy alias: socratic now means automatic mode selection.
        if req == "socratic" || req.is_empty() || req == "auto" {
            let mut proposal = Proposal::new("runtime", "Runtime council question", question, 0);
            proposal.estimate_complexity();
            proposal.urgency = Self::estimate_council_urgency(question);
            self.world.enrich_council_proposal(&mut proposal);
            let members = self.build_council_members();
            let switcher = ModeSwitcher::new(ModeConfig::default());
            let report = switcher.select_mode_with_report(&proposal, &members);
            let mode = match report.selected_mode {
                CouncilMode::Simple => "simple".to_string(),
                CouncilMode::Debate => "debate".to_string(),
                CouncilMode::Orchestrate => "orchestrate".to_string(),
                CouncilMode::LLMDeliberation => "llm".to_string(),
                CouncilMode::Ralph => "ralph".to_string(),
            };
            return (mode, Some(report));
        }

        ("debate".to_string(), None)
    }

    fn do_council(&mut self, question: String, mode: &str) {
        // Guardrails: bound very long/adversarial inputs so council doesn't stall.
        let bounded_question = bounded_text(&question, 1200);
        let requested_mode = mode.to_string();
        let (auto_mode, mode_report) =
            self.resolve_council_mode(&bounded_question, &requested_mode);
        let final_mode =
            if is_prompt_injection_like(&bounded_question) && auto_mode == "orchestrate" {
                "simple".to_string()
            } else {
                auto_mode
            };

        // Use the currently selected chat model for council runs.
        let model = self.chat_models[self.selected_model].1.to_string();
        let model_short = self.chat_models[self.selected_model].0;
        self.mark_llm_request_start(&model);
        self.track_navigation_search(&bounded_question);
        self.components.council.active = true;
        self.components.council.mode = final_mode.clone();
        self.components.council.current_proposal = Some(bounded_question.clone());

        let ground_input = if is_prompt_injection_like(&bounded_question) {
            ""
        } else {
            &bounded_question
        };
        let _ = ground_input; // semantic grounding is extracted from syntax; no extra summaries
        let grounded = String::new();
        let (agent_context_base, evidence_index) = self.build_semantic_grounding(30, 20);
        let council_skills = self.world.skill_bank.retrieve(&Role::Architect, None, &[]);
        let mut council_skill_prompt = SkillBank::format_for_prompt(&council_skills);
        if council_skill_prompt.chars().count() > 3000 {
            council_skill_prompt = bounded_text(&council_skill_prompt, 3000);
        }
        let agent_context = if council_skill_prompt.is_empty() {
            agent_context_base
        } else {
            format!(
                "{}\n\nSKILL FORGE GUIDANCE (SkillRL retrieval):\n{}",
                agent_context_base, council_skill_prompt
            )
        };
        let council_tx = self.web_council_broadcast.clone();
        let graph_tx = self.web_graph_activity_broadcast.clone();
        let bg_tx = self.bg_tx.clone();
        let roodb = self.roodb.clone();

        // Mode-specific setup
        let council_name = match final_mode.as_str() {
            "simple" => "Simple Council",
            "debate" => "Debate Council",
            "orchestrate" => "Orchestrator Council",
            "llm" => "LLM Deliberation Council",
            _ => "Auto Council",
        };

        self.log(&format!(
            "{} starting (requested: {}, selected: {}, model: {}) \"{}\"",
            council_name,
            requested_mode,
            final_mode,
            model_short,
            truncate_str(&bounded_question, 60)
        ));

        if let Some(report) = &mode_report {
            let _ = self.web_council_broadcast.send(
                serde_json::json!({
                    "type": "mode_selection",
                    "selected_mode": &final_mode,
                    "confidence": report.confidence,
                    "complexity": report.complexity,
                    "urgency": report.urgency,
                    "role_diversity": report.role_diversity,
                    "scores": {
                        "simple": report.simple.adjusted,
                        "debate": report.debate.adjusted,
                        "orchestrate": report.orchestrate.adjusted,
                        "llm": report.llm.adjusted,
                    }
                })
                .to_string(),
            );
        }

        // Broadcast council activation for visualization
        self.broadcast_graph_activity(
            "council_start",
            None,
            None,
            &format!(
                "{} (mode: {}, model: {}): {}",
                council_name,
                final_mode,
                model_short,
                truncate_str(&bounded_question, 50)
            ),
        );

        // Broadcast memory access
        self.broadcast_graph_activity(
            "memory_access",
            None,
            None,
            &format!("Council grounded context: {} chars", grounded.len()),
        );

        // Clone for the async block
        let mode_owned = final_mode.clone();
        let model_clone = model;
        let model_short_clone = model_short.to_string();
        let question_owned = bounded_question.clone();
        let repl_state_owned = self.repl_state.clone();
        let members = self.build_council_members();
        let agent_context_owned = agent_context.to_string();
        let evidence_index = evidence_index.clone();
        let roodb_owned = roodb.clone();
        let mut stigmergic_proposal =
            Proposal::new("runtime", "Runtime Council Proposal", &bounded_question, 0);
        self.world.enrich_council_proposal(&mut stigmergic_proposal);
        let stigmergic_context = stigmergic_proposal.stigmergic_context.clone();

        let agent_count = self.world.agents.len();
        let council_session = self.council_dspy_session.clone();
        tokio::spawn(async move {
            let ollama = Ollama::new("http://localhost".to_string(), 11434);

            // Route to mode-specific implementation
            match mode_owned.as_str() {
                "simple" => {
                    council_run_simple(
                        &ollama,
                        &model_clone,
                        &model_short_clone,
                        &question_owned,
                        &grounded,
                        &agent_context_owned,
                        &evidence_index,
                        roodb_owned.clone(),
                        &council_tx,
                        &graph_tx,
                        &bg_tx,
                        agent_count,
                        repl_state_owned.clone(),
                        council_session.clone(),
                    )
                    .await;
                }
                "debate" => {
                    council_run_debate(
                        &ollama,
                        &model_clone,
                        &model_short_clone,
                        &question_owned,
                        &grounded,
                        &agent_context_owned,
                        &evidence_index,
                        roodb_owned.clone(),
                        &council_tx,
                        &graph_tx,
                        &bg_tx,
                        agent_count,
                        council_session.clone(),
                    )
                    .await;
                }
                "orchestrate" => {
                    council_run_orchestrate(
                        &ollama,
                        &model_clone,
                        &model_short_clone,
                        &question_owned,
                        &grounded,
                        &agent_context_owned,
                        &evidence_index,
                        roodb_owned.clone(),
                        &council_tx,
                        &graph_tx,
                        &bg_tx,
                        agent_count,
                        council_session.clone(),
                    )
                    .await;
                }
                "llm" => {
                    council_run_llm_deliberation(
                        &question_owned,
                        &model_clone,
                        &model_short_clone,
                        members,
                        &council_tx,
                        &graph_tx,
                        &bg_tx,
                        roodb_owned.clone(),
                        agent_count,
                        council_session.clone(),
                        stigmergic_context.clone(),
                    )
                    .await;
                }
                _ => {
                    // Fallback default.
                    council_run_debate(
                        &ollama,
                        &model_clone,
                        &model_short_clone,
                        &question_owned,
                        &grounded,
                        &agent_context_owned,
                        &evidence_index,
                        roodb_owned.clone(),
                        &council_tx,
                        &graph_tx,
                        &bg_tx,
                        agent_count,
                        council_session.clone(),
                    )
                    .await;
                }
            }
            let _ = bg_tx.send(BgEvent::CouncilFinished {
                mode: mode_owned,
                success: true,
            });
        });
    }

    /// Run a single council role and stream tokens to the browser.
    fn drain_chat_events(&mut self) {
        while let Ok(event) = self.chat_rx.try_recv() {
            match event {
                ChatEvent::Token(token) => {
                    self.mark_llm_token_emitted(&token);
                    let _ = self.web_chat_broadcast.send(
                        json!({
                            "type": "token",
                            "content": token,
                        })
                        .to_string(),
                    );
                    if let Some(last) = self.chat_messages.last_mut() {
                        if last.role == "assistant" {
                            last.content.push_str(&token);
                        }
                    }
                }
                ChatEvent::Thinking(thinking) => {
                    self.mark_llm_token_emitted(&thinking);
                    let _ = self.web_chat_broadcast.send(
                        json!({
                            "type": "thinking",
                            "content": thinking,
                        })
                        .to_string(),
                    );
                }
                ChatEvent::Done => {
                    self.chat_streaming = false;
                    self.mark_llm_request_done();
                    let _ = self
                        .web_chat_broadcast
                        .send("{\"type\":\"done\"}".to_string());

                    // Check if the completed message would benefit from visualization
                    if let Some(last) = self.chat_messages.last() {
                        if last.role == "assistant" && last.content.len() > 200 {
                            let content = last.content.clone();
                            let chat_broadcast = self.web_chat_broadcast.clone();
                            tokio::spawn(async move {
                                // Small delay to let the UI settle
                                tokio::time::sleep(Duration::from_millis(500)).await;

                                if let Some((diagram_type, title, _reason)) =
                                    should_generate_visual(&content)
                                {
                                    // Generate visual
                                    let output_dir =
                                        std::path::PathBuf::from("visual-explainer/output");
                                    std::fs::create_dir_all(&output_dir).ok();

                                    // Extract relevant content
                                    let viz_content = if content.len() > 800 {
                                        content[..800].to_string()
                                    } else {
                                        content.clone()
                                    };

                                    match generate_visual_explanation(
                                        &diagram_type,
                                        &title,
                                        &viz_content,
                                        None,
                                        &output_dir,
                                        false,
                                    )
                                    .await
                                    {
                                        Ok(filepath) => {
                                            // Notify user about the visual
                                            let filename = filepath
                                                .file_name()
                                                .map(|f| f.to_string_lossy().to_string())
                                                .unwrap_or_else(|| "visual.html".to_string());
                                            let _ = chat_broadcast.send(json!({
                                                "type": "visual_suggestion",
                                                "message": format!("A {} visualization was generated for this response", diagram_type),
                                                "filepath": filename,
                                                "diagram_type": diagram_type,
                                            }).to_string());
                                        }
                                        Err(_) => {}
                                    }
                                }
                            });
                        }
                    }
                }
                ChatEvent::Error(err) => {
                    self.chat_streaming = false;
                    self.mark_llm_request_done();
                    let _ = self.web_chat_broadcast.send(
                        json!({
                            "type": "error",
                            "content": &err,
                        })
                        .to_string(),
                    );
                    if let Some(last) = self.chat_messages.last_mut() {
                        if last.role == "assistant" {
                            last.content = format!("[Error: {}]", err);
                        }
                    }
                    self.log(&format!("Chat error: {}", err));
                }
            }
        }
    }

    async fn drain_bg_events(&mut self) {
        while let Ok(event) = self.bg_rx.try_recv() {
            match event {
                BgEvent::Log(msg) => {
                    self.log(&msg);
                }
                BgEvent::WorldLoaded { world, source } => {
                    self.world = *world;
                    self.log(&format!("World state loaded from {}", source));
                    self.record_snapshot();
                }
                BgEvent::SaveComplete {
                    bincode_ok,
                    db_snapshot_id,
                    errors,
                } => {
                    self.save_in_progress = false;
                    let mut parts = Vec::new();
                    if bincode_ok {
                        parts.push("bincode OK".to_string());
                    }
                    if let Some(id) = db_snapshot_id {
                        parts.push(format!("roodb #{}", id));
                    }
                    if parts.is_empty() {
                        self.log(&format!("Save FAILED: {}", errors.join("; ")));
                    } else {
                        let msg = format!("Saved: {}", parts.join(", "));
                        if errors.is_empty() {
                            self.log(&msg);
                        } else {
                            self.log(&format!("{} (errors: {})", msg, errors.join("; ")));
                        }
                    }
                    // Auto-export viz after every save
                    self.do_export_viz();
                }
                BgEvent::RooDbConnected(db) => {
                    self.roodb = Some(db.clone());
                    if let Ok(mut slot) = self.web_roodb.try_write() {
                        *slot = Some(db);
                    }
                    self.log("RooDB connection established and ready");
                    if let Some(ref db) = self.roodb {
                        let _ = self.hydrate_skillbank_from_roodb(db.clone()).await;
                    }
                }
                BgEvent::QueryResult { sql, rows, headers } => {
                    let formatted = if rows.is_empty() {
                        format!("Query returned no rows.\nSQL: {}", sql)
                    } else {
                        let col_widths: Vec<usize> = headers
                            .iter()
                            .enumerate()
                            .map(|(i, h)| {
                                rows.iter()
                                    .map(|r| r.get(i).map(|s| s.len()).unwrap_or(0))
                                    .max()
                                    .unwrap_or(0)
                                    .max(h.len())
                            })
                            .collect();
                        let header_line = headers
                            .iter()
                            .enumerate()
                            .map(|(i, h)| format!("{:<width$}", h, width = col_widths[i]))
                            .collect::<Vec<_>>()
                            .join(" | ");
                        let sep_line = col_widths
                            .iter()
                            .map(|&w| "-".repeat(w))
                            .collect::<Vec<_>>()
                            .join("-+-");
                        let data_lines: Vec<String> = rows
                            .iter()
                            .map(|row| {
                                row.iter()
                                    .enumerate()
                                    .map(|(i, v)| {
                                        format!(
                                            "{:<width$}",
                                            v,
                                            width = col_widths.get(i).copied().unwrap_or(v.len())
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" | ")
                            })
                            .collect();
                        let mut s =
                            format!("{}\n{}\n{}", header_line, sep_line, data_lines.join("\n"));
                        s.push_str(&format!(
                            "\n({} row{})",
                            rows.len(),
                            if rows.len() == 1 { "" } else { "s" }
                        ));
                        s
                    };
                    // Fulfill pending web query if one is waiting
                    if let Some(tx) = self.pending_web_query.take() {
                        let _ = tx.send(formatted.clone());
                    }
                    // Also push to TUI chat
                    self.chat_messages.push(ChatMsg {
                        role: "assistant".into(),
                        content: formatted,
                        model: "roodb".into(),
                    });
                }
                BgEvent::WebChat {
                    text,
                    model,
                    resp_tx,
                } => {
                    // Backward-compatible alias normalization for older UI selections.
                    let normalized_model = model.as_ref().map(|m| normalize_model_alias(m));
                    let mut selected_model = self.selected_model;
                    if let Some(ref m) = normalized_model {
                        if let Some(idx) = self
                            .chat_models
                            .iter()
                            .position(|(_, ollama)| *ollama == m.as_str())
                        {
                            selected_model = idx;
                        }
                    }
                    let (model_display, model_name) = {
                        let (d, n) = &self.chat_models[selected_model];
                        (d.to_string(), n.to_string())
                    };
                    self.selected_model = selected_model;

                    // Persist to both legacy chat history and LCM so context telemetry stays truthful.
                    self.add_chat_message_sync("user", &text, &model_display);

                    self.broadcast_graph_activity(
                        "agent_activate",
                        Some(selected_model as u64),
                        None,
                        &format!("Web chat model {} activated", model_display),
                    );

                    // Route coding-heavy prompts through the full Coder Agent tool loop.
                    if Self::is_coding_task(&text) {
                        let _ = self.web_chat_broadcast.send(
                            json!({
                                "type": "user_message",
                                "content": &text,
                                "model": &model_display,
                            })
                            .to_string(),
                        );
                        let _ = self.web_chat_broadcast.send(json!({
                            "type": "token",
                            "content": "[Routing to Coder Agent with tool execution. See the Code tab for live steps.]"
                        }).to_string());
                        let _ = self
                            .web_chat_broadcast
                            .send("{\"type\":\"done\"}".to_string());
                        self.do_code_agent(text.clone(), &model_name);
                    } else {
                        // Direct chat path for normal prompts; council remains opt-in via /council.
                        self.chat_messages.push(ChatMsg {
                            role: "assistant".into(),
                            content: String::new(),
                            model: model_display.clone(),
                        });
                        self.chat_streaming = true;
                        self.track_navigation_search(&text);
                        self.maybe_compact_lcm_context().await;
                        self.send_chat_message_async_common(
                            model_name.clone(),
                            model_display.clone(),
                            text.clone(),
                        );
                    }

                    if let Some(tx) = resp_tx {
                        let _ = tx.send(());
                    }

                    self.do_tick(); // chat activity advances world state
                }
                BgEvent::WebCommand { cmd, resp_tx } => {
                    // A browser sent a slash command
                    // /query is async — stash the resp_tx, fulfilled when QueryResult arrives
                    let is_query = cmd.starts_with("/query") || cmd.starts_with("/q ");
                    self.chat_input = cmd.clone();
                    if is_query {
                        self.pending_web_query = Some(resp_tx);
                        if cmd.starts_with('/') {
                            self.handle_slash_command(&cmd).await;
                        }
                    } else {
                        let result = if cmd.starts_with('/') {
                            let handled = self.handle_slash_command(&cmd).await;
                            if handled {
                                self.chat_messages
                                    .last()
                                    .map(|m| m.content.clone())
                                    .unwrap_or_else(|| "OK".into())
                            } else {
                                "Unknown command".into()
                            }
                        } else {
                            "Commands must start with /".into()
                        };
                        let _ = resp_tx.send(result);
                    }
                }
                BgEvent::InjectMessage {
                    sender,
                    target,
                    kind,
                    content,
                } => {
                    let target = self.parse_target(&target);
                    let msg_type = self.parse_message_type(&kind);
                    let msg = Message::new(msg_type, content);
                    self.send_inter_agent_message(sender, msg, target, false);
                }
                BgEvent::CouncilRequest { question, mode } => {
                    self.do_council(question, &mode);
                }
                BgEvent::CouncilFinished { mode, success } => {
                    self.components.council.active = false;
                    self.components.council.mode = mode.clone();
                    self.components.council.current_proposal = None;
                    self.mark_llm_request_done();
                    if success {
                        self.log(&format!("Council completed (mode: {})", mode));
                    } else {
                        self.log(&format!("Council ended with errors (mode: {})", mode));
                    }
                }
                BgEvent::CodeAgent { query, model } => {
                    self.do_code_agent(query, &model);
                }
                BgEvent::CodeAgentFinished { success } => {
                    self.mark_llm_request_done();
                    if success {
                        self.log("Code agent completed");
                    } else {
                        self.log("Code agent failed");
                    }
                }
                BgEvent::OptimizeRequest { body } => {
                    self.do_optimize(body);
                }
                BgEvent::PlanWorkflow { step_index } => {
                    self.create_workflow_from_plan_step(step_index).await;
                }
                BgEvent::PlanOptimize { step_index } => {
                    let found_step = {
                        let guard = self.plan_steps.read().await;
                        guard.iter().find(|s| s.index == step_index).cloned()
                    };
                    if let Some(step) = found_step {
                        self.start_plan_optimization(step);
                        self.log(&format!(
                            "🔬 Plan optimization triggered for step {}",
                            step_index
                        ));
                    } else {
                        self.log(&format!("⚠ PlanOptimize: step {} not found", step_index));
                    }
                }
                BgEvent::SkillCurate {
                    action,
                    body,
                    resp_tx,
                } => {
                    let result = match action.as_str() {
                        "promote" => {
                            let skill_id =
                                body.get("skill_id").and_then(|v| v.as_str()).unwrap_or("");
                            let promoted_by = body
                                .get("promoted_by")
                                .and_then(|v| v.as_str())
                                .unwrap_or("api_user");
                            if skill_id.is_empty() {
                                "error: missing skill_id".to_string()
                            } else if self.world.skill_bank.promote_skill(skill_id, promoted_by) {
                                self.log(&format!(
                                    "🏅 Skill '{}' promoted by {}",
                                    skill_id, promoted_by
                                ));
                                format!("ok: promoted {}", skill_id)
                            } else {
                                format!("error: skill '{}' not found or not promotable", skill_id)
                            }
                        }
                        "add_curated" => {
                            let title = body.get("title").and_then(|v| v.as_str()).unwrap_or("");
                            let principle =
                                body.get("principle").and_then(|v| v.as_str()).unwrap_or("");
                            let curator = body
                                .get("curator")
                                .and_then(|v| v.as_str())
                                .unwrap_or("api_user");
                            let domain = body
                                .get("domain")
                                .and_then(|v| v.as_str())
                                .unwrap_or("general");
                            if title.is_empty() || principle.is_empty() {
                                "error: missing title or principle".to_string()
                            } else {
                                let skill = self.world.skill_bank.add_curated_skill(
                                    title,
                                    principle,
                                    curator,
                                    domain,
                                    SkillScope::default(),
                                    SkillLevel::General,
                                );
                                self.log(&format!(
                                    "📚 Curated skill added: '{}' ({})",
                                    skill.title, skill.id
                                ));
                                format!("ok: created {}", skill.id)
                            }
                        }
                        _ => format!("error: unknown curation action '{}'", action),
                    };
                    let _ = resp_tx.send(result);
                }
                BgEvent::HireComplete {
                    hire_id,
                    status,
                    outcome_score,
                    completed_at,
                } => {
                    // Update hire status in any active hire tree
                    let mut found = false;
                    for tree in self.world.skill_bank.hire_trees.iter_mut() {
                        if let Some(hire) = tree.hires.iter_mut().find(|h| h.hire_id == hire_id) {
                            hire.status = if status == "completed" {
                                HireStatus::Completed
                            } else {
                                HireStatus::Failed
                            };
                            hire.outcome_score = Some(outcome_score);
                            hire.completed_at = Some(completed_at);
                            found = true;
                            break;
                        }
                    }
                    // Persist to RooDB
                    if found {
                        if let Some(ref db) = self.roodb {
                            let db: Arc<RooDb> = db.clone();
                            let hire_id_c = hire_id.clone();
                            let status_c = status.clone();
                            tokio::spawn(async move {
                                let _ = db
                                    .update_skill_hire_status(
                                        &hire_id_c,
                                        &status_c,
                                        Some(outcome_score),
                                        Some(completed_at),
                                    )
                                    .await;
                            });
                        }
                        self.log(&format!(
                            "📋 Hire {} → {} (score: {:.2})",
                            hire_id, status, outcome_score
                        ));
                    }
                }
                BgEvent::DspyOptimize {
                    signature_name,
                    resp_tx,
                } => {
                    if let Some(ref db) = self.roodb {
                        let db = db.clone();
                        let model = self.chat_models[self.selected_model].1.to_string();
                        self.log("🧬 DSPy optimizer triggered");
                        tokio::spawn(async move {
                            let ollama = Ollama::new("http://localhost".to_string(), 11434);
                            let result = match signature_name {
                                Some(name) => {
                                    if let Some(sig) =
                                        hyper_stigmergy::dspy::get_template_by_name(&name)
                                    {
                                        match hyper_stigmergy::dspy::optimize_signature(
                                            &ollama, &model, &db, &sig, 5,
                                        )
                                        .await
                                        {
                                            Ok(r) => format!(
                                                "ok: {} improved={} {:.3}→{:.3}",
                                                r.signature_name,
                                                r.improved,
                                                r.previous_score,
                                                r.new_score
                                            ),
                                            Err(e) => format!("error: {}", e),
                                        }
                                    } else {
                                        format!("error: unknown signature '{}'", name)
                                    }
                                }
                                None => {
                                    let results =
                                        optimize_all_signatures(&ollama, &model, &db, 10, 5).await;
                                    let improved = results.iter().filter(|r| r.improved).count();
                                    format!(
                                        "ok: optimized {} signatures, {} improved",
                                        results.len(),
                                        improved
                                    )
                                }
                            };
                            let _ = resp_tx.send(result);
                        });
                    } else {
                        let _ = resp_tx.send("error: RooDB not connected".to_string());
                    }
                }
                BgEvent::DspyTraceLogged {
                    signature_name,
                    score,
                } => {
                    self.log(&format!("📊 DSPy trace: {} → {:.2}", signature_name, score));
                }
                BgEvent::CouncilSynthesis {
                    question,
                    synthesis,
                    confidence,
                    citations,
                    coverage,
                    plan_text,
                    plan_steps,
                } => {
                    use hyper_stigmergy::{BeliefSource, ExperienceOutcome};
                    // Persist synthesis as a belief — confidence is now the LLM-evaluated score
                    let belief_content = format!(
                        "Council on \"{}\": {}",
                        truncate_str(&question, 60),
                        synthesis
                    );
                    self.world
                        .add_belief(&belief_content, confidence, BeliefSource::Inference);
                    self.add_chat_message("assistant", &synthesis, "council")
                        .await;
                    // coherence_delta scales with quality: strong beliefs move the needle more
                    let coherence_delta = 0.005 + (confidence * 0.02);
                    if !citations.is_empty() {
                        for &(agent_id, count) in &citations {
                            self.world.record_citation(agent_id, count as u64);
                        }
                        // ── JW Reward Loop: persist reward logs for council contributors ──
                        let tick = self.world.tick_count;
                        let now = unix_timestamp_secs();
                        if let Some(ref db) = self.roodb {
                            let db = db.clone();
                            let cit = citations.clone();
                            let conf = confidence;
                            tokio::spawn(async move {
                                for (agent_id, count) in cit {
                                    // Reward = confidence × citation_count (quality × contribution)
                                    let reward = conf * count as f64 * 0.1;
                                    let row = RewardLogRow {
                                        tick,
                                        agent_id,
                                        reward,
                                        source: "council_citation".to_string(),
                                        created_at: now,
                                    };
                                    if let Err(e) = db.insert_reward_log(&row).await {
                                        eprintln!("[Reward] Failed to log council reward: {}", e);
                                    }
                                }
                            });
                        }
                        // ── JW Boost: directly increment JW for cited agents ──
                        for &(agent_id, count) in &citations {
                            if let Some(agent) =
                                self.world.agents.iter_mut().find(|a| a.id == agent_id)
                            {
                                let bonus = (confidence * count as f64 * 0.02).min(0.1);
                                agent.jw = (agent.jw + bonus).min(1.0);
                            }
                        }
                    }
                    if coverage > 0.0 {
                        self.log(&format!(
                            "Semantic coverage: {:.2} evidence/claim",
                            coverage
                        ));
                    }

                    // Integration 2: Track negative outcomes for role prompt optimization
                    let outcome = if confidence < 0.5 {
                        ExperienceOutcome::Negative {
                            coherence_delta: -0.01,
                        }
                    } else {
                        ExperienceOutcome::Positive { coherence_delta }
                    };

                    self.world.record_experience(
                        &format!("Council deliberated on: {}", truncate_str(&question, 80)),
                        &synthesis,
                        outcome.clone(),
                    );

                    // If negative outcome, trigger role prompt optimization
                    if confidence < 0.5 {
                        self.log(&format!("⚠️ Low confidence council (conf={:.2}) — triggering role prompt optimization", confidence));
                        let model = self.chat_models[self.selected_model].1.to_string();
                        // Use stored optimized prompts if available
                        let role_prompts =
                            get_current_role_prompts(self.optimized_role_prompts.as_ref());
                        let bg_tx = self.bg_tx.clone();
                        tokio::spawn(async move {
                            let optimized = optimize_role_prompts(&role_prompts, &model).await;
                            // Send back to main loop for storage
                            let _ =
                                bg_tx.send(BgEvent::OptimizedRolePrompts { prompts: optimized });
                        });
                    }
                    self.components.council.current_proposal = None;
                    self.components.council.recent_decisions.push(format!(
                        "{} (conf={:.2})",
                        truncate_str(&question, 80),
                        confidence
                    ));
                    if self.components.council.recent_decisions.len() > 10 {
                        self.components.council.recent_decisions.remove(0);
                    }

                    self.do_tick(); // council synthesis advances world state
                    self.log(&format!(
                        "Council synthesis → belief (quality-conf={:.2}, Δcoherence={:.4})",
                        confidence, coherence_delta
                    ));
                    // Broadcast the evaluation result to Optimize tab subscribers
                    let _ = self.web_optimize_broadcast.send(
                        serde_json::json!({
                            "type": "council_belief",
                            "question": truncate_str(&question, 80),
                            "confidence": confidence,
                            "coherence_delta": coherence_delta,
                            "belief_snippet": truncate_str(&synthesis, 120),
                        })
                        .to_string(),
                    );

                    // Check if council synthesis would benefit from visualization
                    if synthesis.len() > 150 {
                        let synth_clone = synthesis.clone();
                        let question_clone = question.clone();
                        let council_broadcast = self.web_council_broadcast.clone();
                        tokio::spawn(async move {
                            if let Some((diagram_type, title, _reason)) =
                                should_generate_visual(&synth_clone)
                            {
                                let output_dir =
                                    std::path::PathBuf::from("visual-explainer/output");
                                std::fs::create_dir_all(&output_dir).ok();

                                let viz_content = format!(
                                    "Question: {}\n\nSynthesis: {}",
                                    truncate_str(&question_clone, 100),
                                    if synth_clone.len() > 600 {
                                        &synth_clone[..600]
                                    } else {
                                        &synth_clone
                                    }
                                );

                                match generate_visual_explanation(
                                    &diagram_type,
                                    &title,
                                    &viz_content,
                                    None,
                                    &output_dir,
                                    false,
                                )
                                .await
                                {
                                    Ok(filepath) => {
                                        let filename = filepath
                                            .file_name()
                                            .map(|f| f.to_string_lossy().to_string())
                                            .unwrap_or_else(|| "visual.html".to_string());
                                        let _ = council_broadcast.send(json!({
                                            "type": "visual_suggestion",
                                            "message": format!("A {} visualization was generated for this synthesis", diagram_type),
                                            "filepath": filename,
                                            "diagram_type": diagram_type,
                                        }).to_string());
                                    }
                                    Err(_) => {}
                                }
                            }
                        });
                    }

                    let _ = self.handle_plan_steps(plan_text, plan_steps).await;
                    self.record_snapshot();
                }
                // Integration 2: Store optimized role prompts for reuse
                BgEvent::OptimizedRolePrompts { prompts } => {
                    self.optimized_role_prompts = Some(prompts.clone());
                    self.log(&format!(
                        "📚 Stored {} optimized role prompts for future councils",
                        prompts.len()
                    ));
                }
                // Integration 5: Record code agent quality as experience
                BgEvent::CodeAgentQuality {
                    query,
                    quality_score,
                } => {
                    use hyper_stigmergy::ExperienceOutcome;
                    let outcome = if quality_score > 0.7 {
                        ExperienceOutcome::Positive {
                            coherence_delta: quality_score * 0.01,
                        }
                    } else {
                        ExperienceOutcome::Negative {
                            coherence_delta: -(1.0 - quality_score) * 0.01,
                        }
                    };
                    self.world.record_experience(
                        &format!("Code agent executed: {}", truncate_str(&query, 60)),
                        &format!("Quality score: {:.2}", quality_score),
                        outcome,
                    );
                    self.do_tick(); // code agent completion advances world state
                    self.log(&format!(
                        "📝 Recorded code agent experience (quality={:.2})",
                        quality_score
                    ));
                }
                // Code Agent Session Persistence Events
                BgEvent::CodeAgentSessionStart {
                    session_id,
                    query,
                    model,
                    working_dir,
                } => {
                    self.start_code_agent_social_session(&session_id, &query);
                    if let Some(ref db) = self.roodb {
                        let db = db.clone();
                        tokio::spawn(async move {
                            if let Err(e) = db
                                .start_code_agent_session(&session_id, &query, &model, &working_dir)
                                .await
                            {
                                eprintln!("[CodeAgent] Failed to start session in RooDB: {}", e);
                            }
                        });
                    }
                }
                BgEvent::CodeAgentSessionMessage {
                    session_id,
                    turn,
                    role,
                    content,
                    has_tool_calls,
                } => {
                    if let Some(ref db) = self.roodb {
                        let db = db.clone();
                        tokio::spawn(async move {
                            if let Err(e) = db
                                .record_code_agent_message(
                                    &session_id,
                                    turn,
                                    &role,
                                    &content,
                                    has_tool_calls,
                                )
                                .await
                            {
                                eprintln!("[CodeAgent] Failed to record message in RooDB: {}", e);
                            }
                        });
                    }
                }
                BgEvent::CodeAgentSessionToolCall {
                    session_id,
                    turn,
                    tool_name,
                    args,
                    result,
                    error,
                    duration_ms,
                    file_path,
                } => {
                    if tool_name == "write" || tool_name == "edit" || tool_name == "bash" {
                        let approved = error.is_none();
                        let action_kind = if tool_name == "bash" {
                            "SelfModificationOrExternalWrite"
                        } else {
                            "ExternalWrite"
                        };
                        let reason = if approved {
                            "tool executed".to_string()
                        } else {
                            error.clone().unwrap_or_else(|| "tool blocked".to_string())
                        };
                        if let Some(ref db) = self.roodb {
                            let db = db.clone();
                            let audit = OuroborosGateAuditRow {
                                action_id: format!("{}:{}:{}", session_id, turn, tool_name),
                                action_kind: action_kind.to_string(),
                                risk_level: "High".to_string(),
                                policy_decision: if approved {
                                    "Allow".to_string()
                                } else {
                                    "Deny".to_string()
                                },
                                council_required: true,
                                council_mode: None,
                                approved,
                                reason: Some(reason.clone()),
                                created_at: HyperStigmergicMorphogenesis::current_timestamp(),
                            };
                            let mem = OuroborosMemoryEventRow {
                                event_id: uuid::Uuid::new_v4().to_string(),
                                event_kind: "ActionAudited".to_string(),
                                payload: json!({
                                    "action_id": audit.action_id,
                                    "approved": approved,
                                    "tool_name": tool_name,
                                    "file_path": file_path.clone(),
                                    "reason": reason,
                                })
                                .to_string(),
                                created_at: HyperStigmergicMorphogenesis::current_timestamp(),
                            };
                            tokio::spawn(async move {
                                if let Err(e) = db.insert_ouroboros_gate_audit(&audit).await {
                                    eprintln!(
                                        "[CompatGate] Failed to record tool gate audit: {}",
                                        e
                                    );
                                }
                                if let Err(e) = db.insert_ouroboros_memory_event(&mem).await {
                                    eprintln!("[CompatGate] Failed to record memory event: {}", e);
                                }
                            });
                        }
                    }
                    self.ingest_code_agent_tool_call(
                        &session_id,
                        turn,
                        &tool_name,
                        &args,
                        result.as_deref(),
                        error.as_deref(),
                        duration_ms,
                    );
                    if let Some(ref db) = self.roodb {
                        let db = db.clone();
                        tokio::spawn(async move {
                            if let Err(e) = db
                                .record_code_agent_tool_call(
                                    &session_id,
                                    turn,
                                    &tool_name,
                                    &args,
                                    result.as_deref(),
                                    error.as_deref(),
                                    duration_ms,
                                    file_path.as_deref(),
                                )
                                .await
                            {
                                eprintln!("[CodeAgent] Failed to record tool call in RooDB: {}", e);
                            }
                        });
                    }
                }
                BgEvent::CodeAgentSessionComplete {
                    session_id,
                    final_response,
                    quality_score,
                    turn_count,
                    error,
                } => {
                    let session_id_for_log = session_id.clone();
                    self.complete_code_agent_social_session(
                        &session_id,
                        quality_score,
                        error.as_deref(),
                    );
                    if let Some(ref db) = self.roodb {
                        let db = db.clone();
                        tokio::spawn(async move {
                            if let Err(e) = db
                                .complete_code_agent_session(
                                    &session_id,
                                    final_response.as_deref(),
                                    quality_score,
                                    turn_count,
                                    error.as_deref(),
                                )
                                .await
                            {
                                eprintln!("[CodeAgent] Failed to complete session in RooDB: {}", e);
                            }
                        });
                    }
                    self.log(&format!(
                        "💾 Code agent session {} completed ({} turns)",
                        session_id_for_log, turn_count
                    ));
                }
                BgEvent::VisualExplainer {
                    diagram_type,
                    title,
                    content,
                    data,
                    open_browser,
                } => {
                    self.log(&format!(
                        "📊 Visual Explainer: {} ({})",
                        title, diagram_type
                    ));
                    let output_dir = std::path::PathBuf::from("visual-explainer/output");
                    if let Err(e) = std::fs::create_dir_all(&output_dir) {
                        self.log(&format!("❌ Failed to create output directory: {}", e));
                    }

                    // Send "generating" notification to browser clients
                    let progress_msg = serde_json::json!({
                        "type": "visual_progress",
                        "status": "generating",
                        "title": title,
                        "diagram_type": diagram_type,
                    })
                    .to_string();
                    let _ = self.web_visual_broadcast.send(progress_msg);

                    let bg_tx = self.bg_tx.clone();
                    let visual_broadcast = self.web_visual_broadcast.clone();
                    tokio::spawn(async move {
                        match generate_visual_explanation(
                            &diagram_type,
                            &title,
                            &content,
                            data,
                            &output_dir,
                            open_browser,
                        )
                        .await
                        {
                            Ok(filepath) => {
                                let filename = filepath
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| "unknown".to_string());

                                // Send "complete" notification
                                let complete_msg = serde_json::json!({
                                    "type": "visual_progress",
                                    "status": "complete",
                                    "title": title,
                                    "diagram_type": diagram_type,
                                    "filename": filename,
                                })
                                .to_string();
                                let _ = visual_broadcast.send(complete_msg);

                                let _ = bg_tx.send(BgEvent::Log(format!(
                                    "✅ Visual explainer generated: {}",
                                    filepath.display()
                                )));
                            }
                            Err(e) => {
                                // Send "error" notification
                                let error_msg = serde_json::json!({
                                    "type": "visual_progress",
                                    "status": "error",
                                    "title": title,
                                    "error": e,
                                })
                                .to_string();
                                let _ = visual_broadcast.send(error_msg);

                                let _ = bg_tx.send(BgEvent::Log(format!(
                                    "❌ Visual explainer failed: {}",
                                    e
                                )));
                                eprintln!("[VisualExplainer] Error: {}", e);
                            }
                        }
                    });
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct MessageEvidence {
    msg_id: String,
    formatted: String,
    sender: u64,
    target: String,
    message_type: String,
    content: String,
    timestamp: u64,
}

#[derive(Clone, Debug, Serialize)]
struct SkillSummary {
    id: String,
    title: String,
    confidence: f64,
}

#[derive(Clone, Debug, Default)]
struct EvidenceIndex {
    msg_ids: std::collections::HashSet<String>,
    edge_ids: std::collections::HashSet<usize>,
    msg_senders: std::collections::HashMap<String, u64>,
    edge_participants: std::collections::HashMap<usize, Vec<u64>>,
    msg_context: std::collections::HashMap<String, MessageEvidence>,
}

#[derive(Clone, Debug)]
struct VerificationResult {
    ok: bool,
    verified_text: String,
    errors: Vec<String>,
    proofs: Vec<ClaimProof>,
    coverage: f64,
    evidence_count: usize,
    claim_count: usize,
}

#[derive(Clone, Debug)]
struct ClaimProof {
    claim: String,
    msg_ids: Vec<String>,
    edge_ids: Vec<usize>,
    qmd_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct PlanStep {
    index: usize,
    claim: String,
    plan_text: String,
    messages: Vec<MessageEvidence>,
    qmd_ids: Vec<String>,
    skill_refs: Vec<SkillSummary>,
    has_task_message: bool,
    workflow_message_id: Option<String>,
}

fn format_proof_log(verification: &VerificationResult, evidence: &EvidenceIndex) -> String {
    if verification.proofs.is_empty() {
        return "Semantic proof log: no verified claims.".to_string();
    }
    let mut lines = Vec::new();
    lines.push("Semantic proof log:".to_string());
    for (i, proof) in verification.proofs.iter().enumerate() {
        let msg_ids = if proof.msg_ids.is_empty() {
            "none".to_string()
        } else {
            proof.msg_ids.join(", ")
        };
        let edge_ids = if proof.edge_ids.is_empty() {
            "none".to_string()
        } else {
            proof
                .edge_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        lines.push(format!(
            "{}. Claim: {} | msg:[{}] edge:[{}]",
            i + 1,
            truncate_str(&proof.claim, 80),
            msg_ids,
            edge_ids
        ));
        for msg_id in &proof.msg_ids {
            if let Some(ctx) = evidence.msg_context.get(msg_id) {
                lines.push(format!(
                    "    msg:{} from agent#{} → {} [{}]: {}",
                    msg_id,
                    ctx.sender,
                    ctx.target,
                    ctx.message_type,
                    truncate_str(&ctx.content, 160)
                ));
            }
        }
        for qmd_id in &proof.qmd_ids {
            lines.push(format!("    qmd:{} (vault note)", qmd_id));
        }
    }
    lines.join("\n")
}

fn build_plan_data(
    verification: &VerificationResult,
    evidence: &EvidenceIndex,
) -> (String, Vec<PlanStep>) {
    if verification.proofs.is_empty() {
        return (
            "Implementation steps unavailable: no verified claims.".to_string(),
            Vec::new(),
        );
    }
    let mut lines = Vec::new();
    let mut steps = Vec::new();
    lines.push("Implementation steps (derived from verified claims):".to_string());

    for (i, proof) in verification.proofs.iter().enumerate() {
        let mut step_text = format!("{}. {}", i + 1, proof.claim.trim().trim_end_matches('.'));
        let mut guidance = Vec::new();
        let mut plan_msgs = Vec::new();
        for msg_id in &proof.msg_ids {
            if let Some(ctx) = evidence.msg_context.get(msg_id) {
                plan_msgs.push(ctx.clone());
                guidance.push(format!(
                    "{} (agent#{}→{} [{}])",
                    msg_id, ctx.sender, ctx.target, ctx.message_type
                ));
            }
        }
        if !guidance.is_empty() {
            step_text.push_str(" Agent guidance: ");
            step_text.push_str(&guidance.join(" | "));
        }
        if !proof.qmd_ids.is_empty() {
            step_text.push_str(" Vault references: ");
            step_text.push_str(&proof.qmd_ids.join(", "));
        }
        lines.push(step_text.clone());
        for qmd_id in &proof.qmd_ids {
            lines.push(format!("    qmd:{} (vault note)", qmd_id));
        }

        let has_task_message = plan_msgs.iter().any(|m| m.message_type == "task");
        let workflow_message_id = plan_msgs
            .iter()
            .find(|m| m.message_type == "task")
            .map(|m| m.msg_id.clone());

        steps.push(PlanStep {
            index: i + 1,
            claim: proof.claim.clone(),
            plan_text: step_text.clone(),
            messages: plan_msgs,
            qmd_ids: proof.qmd_ids.clone(),
            skill_refs: Vec::new(),
            has_task_message,
            workflow_message_id,
        });
    }

    (lines.join("\n"), steps)
}

fn is_execution_style_query(question: &str) -> bool {
    let q = question.to_lowercase();
    [
        "mcp",
        "workflow",
        "coding",
        "code",
        "email",
        "marketing",
        "autonom",
        "unsupervis",
        "accept",
        "refuse",
        "infrastructure",
        "visualiz",
        "project align",
    ]
    .iter()
    .any(|k| q.contains(k))
}

fn render_council_synthesis_for_query(
    question: &str,
    summary_text: &str,
    verification: &VerificationResult,
    plan_text: &str,
) -> String {
    if !is_execution_style_query(question) {
        return format!("{}\n\n{}", summary_text.trim(), plan_text);
    }

    let mut out = Vec::new();
    out.push("EXECUTION BLUEPRINT".to_string());
    out.push(format!("Goal: {}", question.trim()));
    out.push(String::new());

    let mut summary = summary_text.trim().to_string();
    if summary.contains("VERIFIED CLAIMS") {
        let mut lines = Vec::new();
        for (i, proof) in verification.proofs.iter().enumerate().take(5) {
            lines.push(format!("{}. {}", i + 1, truncate_str(&proof.claim, 160)));
        }
        summary = if lines.is_empty() {
            "Evidence-backed recommendations are available; see action plan below.".to_string()
        } else {
            lines.join("\n")
        };
    }
    if !summary.is_empty() {
        out.push("System answer:".to_string());
        out.push(summary);
        out.push(String::new());
    }

    out.push("Acceptance policy (operate without supervision):".to_string());
    out.push("- Accept only evidence-backed actions aligned to project scope/KPIs.".to_string());
    out.push(
        "- Refuse unsafe, destructive, or out-of-scope actions without explicit approval."
            .to_string(),
    );
    out.push("- Require auditable traces for every applied change.".to_string());
    out.push(String::new());

    out.push(plan_text.trim().to_string());
    out.push(String::new());

    if !verification.proofs.is_empty() {
        out.push("Evidence trace (audit):".to_string());
        for (i, proof) in verification.proofs.iter().enumerate().take(6) {
            let mut refs = Vec::new();
            if !proof.msg_ids.is_empty() {
                refs.push(format!("msg:{}", proof.msg_ids.join(",")));
            }
            if !proof.edge_ids.is_empty() {
                refs.push(format!(
                    "edge:{}",
                    proof
                        .edge_ids
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            out.push(format!(
                "{}. {} [{}]",
                i + 1,
                truncate_str(&proof.claim, 140),
                refs.join(" | ")
            ));
        }
    }

    if !verification.errors.is_empty() {
        out.push(String::new());
        out.push("Validation warnings:".to_string());
        for err in verification.errors.iter().take(4) {
            out.push(format!("- {}", err));
        }
    }

    out.join("\n")
}

fn choose_user_facing_summary(question: &str, preferred: &str, fallback: &str) -> String {
    if is_execution_style_query(question) {
        let p = preferred.trim();
        if !p.is_empty() {
            return p.to_string();
        }
    }
    fallback.trim().to_string()
}

fn collect_citations(
    verification: &VerificationResult,
    evidence: &EvidenceIndex,
) -> Vec<(u64, u32)> {
    let mut counts: std::collections::HashMap<u64, u32> = std::collections::HashMap::new();
    for proof in &verification.proofs {
        for msg_id in &proof.msg_ids {
            if let Some(sender) = evidence.msg_senders.get(msg_id) {
                *counts.entry(*sender).or_default() += 1;
            }
        }
    }
    counts.into_iter().collect()
}

impl App {
    fn verify_semantic_contract_static(text: &str, evidence: &EvidenceIndex) -> VerificationResult {
        verify_semantic_contract(text, evidence, false)
    }

    fn verify_semantic_contract_strict(text: &str, evidence: &EvidenceIndex) -> VerificationResult {
        verify_semantic_contract(text, evidence, true)
    }
}

fn verify_semantic_contract(
    text: &str,
    evidence: &EvidenceIndex,
    require_msg: bool,
) -> VerificationResult {
    let mut verified_blocks = Vec::new();
    let mut errors = Vec::new();
    let mut proofs = Vec::new();
    let mut current_claim: Option<String> = None;
    let mut current_evidence: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Claim:") {
            if let (Some(claim), Some(ev_line)) = (current_claim.take(), current_evidence.take()) {
                if let Some((block, proof)) =
                    verify_claim_block(&claim, &ev_line, evidence, require_msg, &mut errors)
                {
                    verified_blocks.push(block);
                    proofs.push(proof);
                }
            }
            current_claim = Some(trimmed.trim_start_matches("Claim:").trim().to_string());
            current_evidence = None;
            continue;
        }
        if trimmed.starts_with("Evidence:") {
            current_evidence = Some(trimmed.trim_start_matches("Evidence:").trim().to_string());
            continue;
        }
    }

    if let (Some(claim), Some(ev_line)) = (current_claim.take(), current_evidence.take()) {
        if let Some((block, proof)) =
            verify_claim_block(&claim, &ev_line, evidence, require_msg, &mut errors)
        {
            verified_blocks.push(block);
            proofs.push(proof);
        }
    }

    let claim_count = proofs.len();
    let evidence_count = evidence.msg_ids.len() + evidence.edge_ids.len();
    let coverage = if claim_count == 0 {
        0.0
    } else {
        (evidence_count as f64 / claim_count as f64).min(5.0)
    };

    if verified_blocks.is_empty() {
        if errors.is_empty() {
            errors.push("No Claim/Evidence blocks found.".to_string());
        }
        return VerificationResult {
            ok: false,
            verified_text: "NO VERIFIED CLAIMS: evidence missing or invalid.".to_string(),
            errors,
            proofs,
            coverage,
            evidence_count,
            claim_count,
        };
    }

    let mut out = Vec::new();
    out.push("VERIFIED CLAIMS".to_string());
    for block in verified_blocks {
        out.push(format!("✅ {}", block.replace('\n', "\n   ")));
    }
    if !errors.is_empty() {
        out.push("REJECTED CLAIMS".to_string());
        for err in &errors {
            out.push(format!("❌ {}", err));
        }
    }
    VerificationResult {
        ok: errors.is_empty(),
        verified_text: out.join("\n"),
        errors,
        proofs,
        coverage,
        evidence_count,
        claim_count,
    }
}

struct ReplInput {
    line: String,
}

fn print_help() {
    println!("Commands:");
    println!("  help                 show this message");
    println!("  status               show system summary");
    println!("  inspect              show the last answer dict");
    println!("  clear                clear the last answer");
    println!("  plan [index]         inspect current council plan steps");
    println!("  optimize <index>     trigger OptimizeAnything on a plan step");
    println!("  hire list|complete <id> [status] [score]    view or finish hires");
    println!("  vault list|search <query>|index  inspect the vault content/index");
    println!("  council [--mode MODE] <question>  trigger the council");
    println!("  quit | exit          leave the REPL");
}

fn print_answer(answer: &AnswerDict) {
    println!("-----");
    println!("ready: {} status: {:?}", answer.ready, answer.status);
    println!("{}", answer.content);
    if !answer.metadata.is_null() {
        println!("metadata: {}", answer.metadata);
    }
    println!("-----");
}

fn repl_build_api_state(app: &App) -> ApiState {
    ApiState {
        snapshot: app.web_snapshot.clone(),
        chat_broadcast: app.web_chat_broadcast.clone(),
        web_cmd_tx: app.bg_tx.clone(),
        last_context: app.web_last_context.clone(),
        council_broadcast: app.web_council_broadcast.clone(),
        graph_activity_broadcast: app.web_graph_activity_broadcast.clone(),
        council_mode: Arc::new(RwLock::new(app.components.council.mode.clone())),
        code_broadcast: app.web_code_broadcast.clone(),
        optimize_broadcast: app.web_optimize_broadcast.clone(),
        visual_broadcast: app.web_visual_broadcast.clone(),
        roodb: app.web_roodb.clone(),
        roodb_url: app.roodb_url.clone(),
        embed_model: app.embed_model.clone(),
        embed_client: app.embed_client.clone(),
        vault_dir: app.vault_dir.clone(),
        plan_steps: app.plan_steps.clone(),
        recent_rewards: Arc::new(RwLock::new(Vec::new())),
        recent_skill_evidence: Arc::new(RwLock::new(Vec::new())),
    }
}

async fn repl_plan_command(app: &App, args: &str) -> AnswerDict {
    let steps = app.plan_steps.read().await;
    if steps.is_empty() {
        return AnswerDict::error("No plan steps available. Run a council to produce plans.");
    }
    let trimmed = args.trim();
    if trimmed.is_empty() {
        let mut lines = Vec::new();
        for step in steps.iter().take(12) {
            lines.push(format!(
                "[{}] {} (skills={}, messages={})",
                step.index,
                truncate_str(&step.claim, 64),
                step.skill_refs.len(),
                step.messages.len()
            ));
        }
        let summary = if lines.is_empty() {
            "(no plan summaries available yet)".to_string()
        } else {
            lines.join("\n")
        };
        let metadata_steps: Vec<_> = steps
            .iter()
            .map(|step| {
                json!({
                    "index": step.index,
                    "claim": step.claim,
                    "skills": step.skill_refs.len(),
                    "messages": step.messages.len(),
                })
            })
            .collect();
        let metadata = json!({
            "plan_count": steps.len(),
            "steps": metadata_steps,
        });
        return AnswerDict::with_content(
            format!("Plan steps ({} total):\n{}", steps.len(), summary),
            metadata,
            Some("plan".into()),
        );
    }
    let idx = match trimmed.parse::<usize>() {
        Ok(v) => v,
        Err(_) => return AnswerDict::error("Plan command requires a numeric step index."),
    };
    if let Some(step) = steps.iter().find(|s| s.index == idx) {
        let skill_refs: Vec<_> = step
            .skill_refs
            .iter()
            .map(|sr| json!({"id": sr.id, "title": sr.title, "confidence": sr.confidence}))
            .collect();
        let message_summaries: Vec<_> = step
            .messages
            .iter()
            .map(|msg| {
                json!({
                    "msg_id": msg.msg_id,
                    "sender": msg.sender,
                    "target": msg.target,
                    "excerpt": truncate_str(&msg.content, 80),
                })
            })
            .collect();
        let metadata = json!({
            "step_index": step.index,
            "claim": step.claim,
            "message_count": step.messages.len(),
            "skill_count": step.skill_refs.len(),
            "skill_refs": skill_refs,
            "messages": message_summaries,
            "qmd_ids": step.qmd_ids,
            "has_task_message": step.has_task_message,
            "workflow_message_id": step.workflow_message_id,
        });
        let mut answer = AnswerDict::with_content(
            format!(
                "Plan step {}:\nClaim: {}\nPlan text:\n{}\nSkills: {}\nMessages: {}\nQMD: {}",
                step.index,
                step.claim,
                truncate_str(&step.plan_text, 300),
                step.skill_refs
                    .iter()
                    .map(|sr| format!("{} ({:.2})", sr.title, sr.confidence))
                    .collect::<Vec<_>>()
                    .join(", "),
                step.messages
                    .iter()
                    .map(|msg| msg.msg_id.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
                if step.qmd_ids.is_empty() {
                    "none".to_string()
                } else {
                    step.qmd_ids.join(", ")
                }
            ),
            metadata,
            Some("plan".into()),
        );
        answer.trace_hash = Some(hash_content(&step.plan_text));
        return answer;
    }
    AnswerDict::error(&format!("Plan step {} not found.", idx))
}

async fn repl_optimize_command(app: &App, args: &str) -> AnswerDict {
    let trimmed = args.trim();
    let index = match trimmed.parse::<usize>() {
        Ok(v) => v,
        Err(_) => return AnswerDict::error("Optimize requires a numeric plan step index."),
    };
    let steps = app.plan_steps.read().await;
    if let Some(step) = steps.iter().find(|s| s.index == index) {
        let _ = app.bg_tx.send(BgEvent::PlanOptimize { step_index: index });
        let metadata = json!({
            "step_index": index,
            "claim": step.claim,
            "skill_refs": step.skill_refs.iter().map(|sr| sr.id.clone()).collect::<Vec<_>>(),
            "messages": step.messages.iter().map(|msg| msg.msg_id.clone()).collect::<Vec<_>>(),
        });
        let mut answer = AnswerDict::with_content(
            format!(
                "Plan optimization triggered for step {} ({})",
                index,
                truncate_str(&step.claim, 80)
            ),
            metadata,
            Some("optimize".into()),
        );
        answer.trace_hash = Some(hash_content(&step.plan_text));
        answer
    } else {
        AnswerDict::error(&format!("Plan step {} not found.", index))
    }
}

async fn repl_hire_command(app: &App, args: &str) -> AnswerDict {
    let mut parts = args.trim().split_whitespace();
    match parts.next() {
        Some("list") => {
            let mut lines = Vec::new();
            for tree in &app.world.skill_bank.hire_trees {
                for hire in &tree.hires {
                    lines.push(format!(
                        "{} → {} (status={:?}, plan_step={})",
                        hire.parent_skill_id,
                        hire.child_skill_id,
                        hire.status,
                        tree.plan_step_index
                    ));
                    if lines.len() >= 12 {
                        break;
                    }
                }
                if lines.len() >= 12 {
                    break;
                }
            }
            let metadata = json!({
                "total_trees": app.world.skill_bank.hire_trees.len(),
            });
            return AnswerDict::with_content(
                format!(
                    "Active hire trees: {}\n{}",
                    app.world.skill_bank.hire_trees.len(),
                    if lines.is_empty() {
                        "(no hires yet)".to_string()
                    } else {
                        lines.join("\n")
                    }
                ),
                metadata,
                Some("hire".into()),
            );
        }
        Some("complete") => {
            let hire_id = match parts.next() {
                Some(id) => id,
                None => return AnswerDict::error("hire complete requires a hire_id."),
            };
            let status = parts.next().unwrap_or("completed");
            let status = match status {
                "completed" | "failed" | "revoked" => status,
                _other => {
                    return AnswerDict::error("status must be one of completed|failed|revoked");
                }
            };
            let score = parts
                .next()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.75);
            let now = unix_timestamp_secs();
            let _ = app.bg_tx.send(BgEvent::HireComplete {
                hire_id: hire_id.to_string(),
                status: status.to_string(),
                outcome_score: score,
                completed_at: now,
            });
            let mut answer = AnswerDict::with_content(
                format!("Hire {} marked {} with score {:.2}", hire_id, status, score),
                json!({
                    "hire_id": hire_id,
                    "status": status,
                    "score": score,
                    "completed_at": now,
                }),
                Some("hire".into()),
            );
            answer.trace_hash = Some(format!("hire:{}:{}", hire_id, now));
            answer
        }
        _ => AnswerDict::error(
            "hire command must be 'hire list' or 'hire complete <hire_id> [status] [score]'",
        ),
    }
}

async fn repl_vault_command(app: &App, args: &str) -> AnswerDict {
    let trimmed = args.trim();
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let sub = parts.next().unwrap_or("");
    match sub {
        "" | "help" => AnswerDict::error("vault commands: list | search <query> | index"),
        "list" => {
            let path = Path::new(&app.vault_dir);
            match vault::scan_vault(path) {
                Ok(notes) => {
                    let mut summary = Vec::new();
                    for note in notes.iter().take(10) {
                        summary.push(format!(
                            "{} ({} tags) → {}",
                            note.title,
                            note.tags.len(),
                            note.id
                        ));
                    }
                    let metadata = json!({
                        "note_count": notes.len(),
                        "notes": notes
                            .iter()
                            .take(20)
                            .map(|note| {
                                json!({
                                    "id": note.id,
                                    "title": note.title,
                                    "tags": note.tags,
                                    "type": note.note_type,
                                })
                            })
                            .collect::<Vec<_>>(),
                    });
                    let mut answer = AnswerDict::with_content(
                        format!(
                            "Vault notes ({}):\n{}",
                            notes.len(),
                            if summary.is_empty() {
                                "(empty vault)".to_string()
                            } else {
                                summary.join("\n")
                            }
                        ),
                        metadata,
                        Some("vault".into()),
                    );
                    answer.trace_hash = Some(hash_content(&notes.len().to_string()));
                    answer
                }
                Err(err) => AnswerDict::error(&format!("vault scan failed: {}", err)),
            }
        }
        "index" => {
            let api_state = repl_build_api_state(app);
            match index_vault_embeddings(&api_state).await {
                Ok((total, embedded, skipped, errors)) => {
                    let mut answer = AnswerDict::with_content(
                        format!(
                            "Vault index complete: {} embedded, {} skipped (total {}).",
                            embedded, skipped, total
                        ),
                        json!({
                            "total": total,
                            "embedded": embedded,
                            "skipped": skipped,
                            "errors": errors,
                        }),
                        Some("vault".into()),
                    );
                    answer.trace_hash = Some(format!("vault_index:{}", embedded));
                    answer
                }
                Err(err) => AnswerDict::error(&format!("vault index failed: {}", err)),
            }
        }
        "search" => {
            let query = parts.next().unwrap_or("").trim();
            repl_vault_search(app, query).await
        }
        other => repl_vault_search(app, other).await,
    }
}

async fn repl_vault_search(app: &App, query: &str) -> AnswerDict {
    let api_state = repl_build_api_state(app);
    let db = match ensure_roodb(&api_state).await {
        Ok(db) => db,
        Err(err) => return AnswerDict::error(&format!("vault search failed: {}", err)),
    };
    let rows = match db.fetch_vault_embeddings().await {
        Ok(rows) => rows,
        Err(err) => return AnswerDict::error(&format!("vault search failed: {}", err)),
    };
    let display_query = if query.is_empty() { "*" } else { query };
    let results = if query.is_empty() || query == "*" {
        let mut sorted = rows.clone();
        sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sorted
            .into_iter()
            .take(8)
            .map(|row| {
                let note_type = row
                    .metadata
                    .get("type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                VaultSearchResult {
                    id: format!("vault:{}", row.note_id),
                    label: row.title,
                    score: 0.0,
                    tags: row.tags,
                    preview: row.preview,
                    path: row.path,
                    note_type,
                    metadata: row.metadata,
                    evidence: vec![format!("emb:{}", row.note_id)],
                    snippet: None,
                    sources: vec!["embeddings".to_string()],
                }
            })
            .collect::<Vec<_>>()
    } else {
        let query_embedding =
            match ollama_embed(&api_state.embed_client, &api_state.embed_model, query).await {
                Ok(e) => e,
                Err(err) => return AnswerDict::error(&format!("vault search failed: {}", err)),
            };
        let qmd_hits = match run_qmd_query(query, 8).await {
            Ok(hits) => hits,
            Err(_) => Vec::new(),
        };
        let qmd_map: HashMap<String, QmdHit> = qmd_hits
            .into_iter()
            .map(|hit| (hit.note_id.clone(), hit))
            .collect();
        let mut scored: Vec<(f32, VaultEmbeddingRow, Option<QmdHit>)> = rows
            .into_iter()
            .map(|row| {
                let embed_score = cosine_similarity(&query_embedding, &row.embedding);
                let qmd_entry = qmd_map.get(&row.note_id).cloned();
                let qmd_score = qmd_entry.as_ref().map(|h| h.score).unwrap_or(0.0);
                let combined_score = (embed_score * 0.65) + (qmd_score * 0.35);
                (combined_score, row, qmd_entry)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored
            .into_iter()
            .take(8)
            .map(|(score, row, qmd)| {
                let note_type = row
                    .metadata
                    .get("type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                VaultSearchResult {
                    id: format!("vault:{}", row.note_id),
                    label: row.title,
                    score,
                    tags: row.tags,
                    preview: row.preview,
                    path: row.path,
                    note_type,
                    metadata: row.metadata,
                    evidence: vec![format!("emb:{}", row.note_id)],
                    snippet: qmd.as_ref().map(|h| h.snippet.clone()),
                    sources: {
                        let mut sources = vec!["embeddings".to_string()];
                        if qmd.is_some() {
                            sources.push("qmd".to_string());
                        }
                        sources
                    },
                }
            })
            .collect::<Vec<_>>()
    };
    let summary_lines: Vec<_> = results
        .iter()
        .take(6)
        .map(|res| format!("{} ({:.2}) → {}", res.label, res.score, res.id))
        .collect();
    let metadata = json!({
        "query": display_query,
        "result_count": results.len(),
        "top_sources": results
            .iter()
            .map(|res| res.sources.clone())
            .collect::<Vec<_>>(),
    });
    let mut answer = AnswerDict::with_content(
        format!(
            "Vault search: {}\n{}",
            display_query,
            if summary_lines.is_empty() {
                "(no matches)".to_string()
            } else {
                summary_lines.join("\n")
            }
        ),
        metadata,
        Some("vault".into()),
    );
    answer.trace_hash = Some(hash_content(display_query));
    answer
}

async fn gather_status_answer(app: &App) -> AnswerDict {
    let plan_steps = app.plan_steps.read().await;
    let jw_avg = if app.world.agents.is_empty() {
        0.0
    } else {
        let total: f64 = app.world.agents.iter().map(|a| a.jw as f64).sum();
        total / app.world.agents.len() as f64
    };
    let active_hires: usize = app
        .world
        .skill_bank
        .hire_trees
        .iter()
        .map(|tree| {
            tree.hires
                .iter()
                .filter(|h| h.status == HireStatus::Active)
                .count()
        })
        .sum();
    let metadata = json!({
        "agents": app.world.agents.len(),
        "edges": app.world.edges.len(),
        "skills": app.world.skill_bank.all_skills().len(),
        "plan_steps": plan_steps.len(),
        "avg_jw": jw_avg,
        "active_hires": active_hires,
        "roodb_url": app.roodb_url,
        "roodb_connected": app.roodb.is_some(),
    });
    AnswerDict::with_content(
        format!(
            "Agents: {}, edges: {}, skills: {}, plan steps: {}, avg JW: {:.2}, active hires: {}",
            app.world.agents.len(),
            app.world.edges.len(),
            app.world.skill_bank.all_skills().len(),
            plan_steps.len(),
            jw_avg,
            active_hires,
        ),
        metadata,
        Some("status".into()),
    )
}

async fn handle_repl_line(
    app: &mut App,
    state: &Arc<ReplState>,
    line: String,
) -> anyhow::Result<bool> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    match trimmed {
        "quit" | "exit" => {
            println!("Goodbye.");
            return Ok(true);
        }
        "help" => {
            print_help();
            return Ok(false);
        }
        "status" => {
            let answer = gather_status_answer(app).await;
            print_answer(&answer);
            state.set_last(answer);
            return Ok(false);
        }
        "inspect" => {
            let answer = state.last();
            print_answer(&answer);
            return Ok(false);
        }
        "clear" => {
            state.clear();
            println!("Answer cleared.");
            return Ok(false);
        }
        _ => {}
    }

    if let Some(rest) = trimmed.strip_prefix("plan") {
        let answer = repl_plan_command(app, rest).await;
        print_answer(&answer);
        state.set_last(answer);
        return Ok(false);
    }
    if let Some(rest) = trimmed.strip_prefix("optimize") {
        let answer = repl_optimize_command(app, rest).await;
        print_answer(&answer);
        state.set_last(answer);
        return Ok(false);
    }
    if let Some(rest) = trimmed.strip_prefix("hire") {
        let answer = repl_hire_command(app, rest).await;
        print_answer(&answer);
        state.set_last(answer);
        return Ok(false);
    }
    if let Some(rest) = trimmed.strip_prefix("vault") {
        let answer = repl_vault_command(app, rest).await;
        print_answer(&answer);
        state.set_last(answer);
        return Ok(false);
    }
    if let Some(rest) = trimmed.strip_prefix("council") {
        let mut question = rest.trim().to_string();
        let mut mode = "auto".to_string();
        if question.starts_with("--mode") {
            let remainder = question["--mode".len()..].trim_start();
            if remainder.is_empty() {
                println!("Missing mode value.");
                return Ok(false);
            }
            let mut parts = remainder.splitn(2, char::is_whitespace);
            mode = parts.next().unwrap_or("auto").to_string();
            question = parts.next().unwrap_or("").trim().to_string();
        }
        if question.is_empty() {
            println!("Council command requires a question.");
            return Ok(false);
        }
        let (tx, rx) = oneshot::channel();
        if let Err(err) = state.set_pending(ReplPending {
            kind: ReplCommandKind::Council,
            tx,
        }) {
            println!("Unable to schedule council: {}", err);
            return Ok(false);
        }
        let _ = app.bg_tx.send(BgEvent::CouncilRequest {
            question: question.clone(),
            mode: mode.clone(),
        });
        println!("Council running (mode={})...", mode);
        match tokio::time::timeout(Duration::from_secs(120), rx).await {
            Ok(Ok(answer)) => {
                print_answer(&answer);
                state.set_last(answer);
            }
            Ok(Err(_)) => {
                println!("Council response channel closed.");
            }
            Err(_) => {
                println!("Council timed out.");
                state.take_pending(ReplCommandKind::Council);
            }
        }
        return Ok(false);
    }

    println!("Unknown command: {}", trimmed);
    Ok(false)
}

fn spawn_repl_input_thread(
    tx: mpsc::UnboundedSender<ReplInput>,
    running: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut editor = Reedline::create();
        let prompt = DefaultPrompt::default();
        while running.load(Ordering::SeqCst) {
            match editor.read_line(&prompt) {
                Ok(Signal::Success(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let _ = tx.send(ReplInput {
                        line: trimmed.to_string(),
                    });
                }
                Ok(Signal::CtrlC) => {
                    println!("(use 'quit' to exit)");
                }
                Ok(Signal::CtrlD) => {
                    let _ = tx.send(ReplInput {
                        line: "quit".into(),
                    });
                    break;
                }
                Err(err) => {
                    eprintln!("REPL input error: {}", err);
                    break;
                }
            }
        }
    })
}

async fn run_repl_mode(args: &[String]) -> anyhow::Result<()> {
    eprintln!("[HSM-II] Starting REPL mode (no TUI/UI)...");
    let mut app = App::new();

    {
        let models = app.chat_models.clone();
        tokio::spawn(async move {
            let ollama = Ollama::new("http://localhost".to_string(), 11434);
            for (name, model) in models {
                eprintln!("[Ollama] Checking model: {} ({})...", name, model);
                let test_req = ChatMessageRequest::new(
                    model.to_string(),
                    vec![OllamaChatMsg::system("test".into())],
                );
                match ollama.send_chat_messages(test_req).await {
                    Ok(_) => eprintln!("[Ollama] ✓ Model {} is available", name),
                    Err(e) => {
                        let err_str = format!("{}", e);
                        if err_str.contains("404") || err_str.contains("not found") {
                            eprintln!(
                                "[Ollama] ⚠ Model {} not found. Pull it with: ollama pull {}",
                                name, model
                            );
                        } else if err_str.contains("Connection refused") {
                            eprintln!("[Ollama] ⚠ Ollama not running. Start with: ollama serve");
                            break;
                        } else {
                            eprintln!("[Ollama] ✓ Model {} appears available", name);
                        }
                    }
                }
            }
        });
    }

    {
        let viz_rx = app.viz_tx.subscribe();
        tokio::spawn(viz_ws_server(viz_rx));
        app.log("Viz WS server: ws://localhost:8788/ws");
    }

    let mut roodb_url: Option<String> = None;
    {
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--federation" => {
                    if let Some(addr) = args.get(i + 1) {
                        app.federation_addr = Some(addr.clone());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--peer" => {
                    if let Some(peer) = args.get(i + 1) {
                        app.federation_peers.push(peer.clone());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--roodb" => {
                    if let Some(url) = args.get(i + 1) {
                        roodb_url = Some(url.clone());
                        i += 2;
                    } else {
                        roodb_url = Some("127.0.0.1:3307".to_string());
                        i += 1;
                    }
                }
                _ => {
                    i += 1;
                }
            }
        }

        if let Some(ref addr) = app.federation_addr {
            let system_id = uuid::Uuid::new_v4().to_string();
            let config = FederationConfig {
                system_id: system_id.clone(),
                listen_addr: addr.clone(),
                known_peers: app.federation_peers.clone(),
                trust_threshold: 0.3,
                auto_promote_after: 50,
            };

            app.world.federation_config = Some(config.clone());

            let meta_graph = std::sync::Arc::new(tokio::sync::RwLock::new(MetaGraph::new(&config)));
            app.federation_meta_graph = Some(meta_graph.clone());

            let world_arc = std::sync::Arc::new(tokio::sync::RwLock::new(app.world.clone()));
            let current_tick = std::sync::Arc::new(tokio::sync::RwLock::new(0u64));

            let state = FederationState {
                meta_graph: meta_graph.clone(),
                world: world_arc,
                current_tick,
            };

            let listen_addr = addr.clone();
            tokio::spawn(async move {
                if let Err(e) = FederationServer::serve(&listen_addr, state).await {
                    eprintln!("Federation server error: {}", e);
                }
            });

            app.log(&format!(
                "Federation enabled: {} (system {})",
                addr,
                &system_id[..8]
            ));
            let peers_snapshot: Vec<String> = app.federation_peers.clone();
            for peer in peers_snapshot {
                app.log(&format!("  Peer: {}", peer));
            }
        }
    }

    if roodb_url.is_none() {
        roodb_url = Some("127.0.0.1:3307".to_string());
        app.log("RooDB auto-connect default: 127.0.0.1:3307 (use --roodb to override)");
    }

    if let Some(ref url) = roodb_url {
        app.roodb_url = url.clone();
        let config = RooDbConfig::from_url(url);
        let db = RooDb::new(&config);
        let init_result = tokio::time::timeout(Duration::from_secs(5), async {
            db.ping().await?;
            db.init_schema().await?;
            Ok::<_, anyhow::Error>(db)
        })
        .await;

        match init_result {
            Ok(Ok(db)) => {
                app.log(&format!(
                    "RooDB connected: {}:{}/{}",
                    config.host, config.port, config.database
                ));
                let db = std::sync::Arc::new(db);
                app.roodb = Some(db.clone());
                if let Ok(mut slot) = app.web_roodb.try_write() {
                    *slot = Some(db);
                }
            }
            Ok(Err(e)) => {
                app.log(&format!(
                    "RooDB init failed: {} (falling back to embedded local store)",
                    e
                ));
            }
            Err(_) => {
                app.log(
                    "RooDB connection timed out after 5s (falling back to embedded local store)",
                );
            }
        }
    }

    let repl_state = Arc::new(ReplState::default());
    app.repl_state = Some(repl_state.clone());

    let (repl_tx, mut repl_rx) = mpsc::unbounded_channel::<ReplInput>();
    let running = Arc::new(AtomicBool::new(true));
    let input_handle = spawn_repl_input_thread(repl_tx, running.clone());

    println!("[HSM-II] REPL ready. Type 'help' for commands.");
    loop {
        tokio::select! {
            Some(input) = repl_rx.recv() => {
                let quit = handle_repl_line(&mut app, &repl_state, input.line).await?;
                if quit {
                    running.store(false, Ordering::SeqCst);
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
        app.drain_bg_events().await;
        app.drain_chat_events();
        app.maybe_extract_skillbank().await;
    }

    running.store(false, Ordering::SeqCst);
    let _ = input_handle.join();
    Ok(())
}

fn verify_claim_block(
    claim: &str,
    ev_line: &str,
    evidence: &EvidenceIndex,
    require_msg: bool,
    errors: &mut Vec<String>,
) -> Option<(String, ClaimProof)> {
    let mut msg_ids = Vec::new();
    let mut edge_ids = Vec::new();
    let mut qmd_ids = Vec::new();
    for token in ev_line
        .replace(&['[', ']', ',', ';'][..], " ")
        .split_whitespace()
    {
        if let Some(rest) = token.strip_prefix("msg:") {
            msg_ids.push(rest.to_string());
        } else if let Some(rest) = token.strip_prefix("edge:") {
            if let Ok(id) = rest.parse::<usize>() {
                edge_ids.push(id);
            }
        } else if let Some(rest) = token.strip_prefix("qmd:") {
            qmd_ids.push(rest.to_string());
        }
    }
    let mut missing = Vec::new();
    for id in &msg_ids {
        if !evidence.msg_ids.contains(id) {
            missing.push(format!("msg:{}", id));
        }
    }
    for id in &edge_ids {
        if !evidence.edge_ids.contains(id) {
            missing.push(format!("edge:{}", id));
        }
    }
    if msg_ids.is_empty() && edge_ids.is_empty() {
        errors.push(format!(
            "Claim missing evidence: {}",
            truncate_str(claim, 80)
        ));
        return None;
    }
    if require_msg && !evidence.msg_ids.is_empty() && msg_ids.is_empty() {
        errors.push(format!(
            "Claim missing message evidence: {}",
            truncate_str(claim, 80)
        ));
        return None;
    }
    if !missing.is_empty() {
        errors.push(format!(
            "Claim evidence missing: {} (missing: {})",
            truncate_str(claim, 80),
            missing.join(", ")
        ));
        return None;
    }
    let block = format!("Claim: {}\nEvidence: {}", claim, ev_line);
    let proof = ClaimProof {
        claim: claim.to_string(),
        msg_ids,
        edge_ids,
        qmd_ids,
    };
    Some((block, proof))
}

async fn persist_council_claims(
    roodb: Option<Arc<RooDb>>,
    question: &str,
    verification: &VerificationResult,
    confidence: f64,
    coverage: f64,
    mode: &str,
) {
    let Some(db) = roodb else {
        return;
    };
    let now = HyperStigmergicMorphogenesis::current_timestamp();
    for proof in &verification.proofs {
        let row = CouncilClaimRow {
            question: question.to_string(),
            claim: proof.claim.clone(),
            evidence_msgs: proof.msg_ids.clone(),
            evidence_edges: proof.edge_ids.clone(),
            confidence,
            coverage,
            mode: mode.to_string(),
            created_at: now,
        };
        let _ = db.insert_council_claim(&row).await;
    }
}

// ════════════════════════════════════════════════════════════════════════════
// VISUAL EXPLAINER IMPLEMENTATION
// ════════════════════════════════════════════════════════════════════════════

/// Generate a visual explanation - returns JSON with content for inline rendering
async fn generate_visual_explanation(
    diagram_type: &str,
    title: &str,
    content: &str,
    _data: Option<serde_json::Value>,
    output_dir: &std::path::Path,
    _open_browser: bool,
) -> Result<std::path::PathBuf, String> {
    use std::io::Write;

    // Generate content based on diagram type
    let (format, diagram_content) = match diagram_type {
        "flowchart" | "mermaid" => {
            // Generate mermaid flowchart syntax
            let mermaid = generate_mermaid_flowchart(title, content).await;
            ("mermaid".to_string(), mermaid)
        }
        "sequence" => {
            let mermaid = generate_mermaid_sequence(title, content).await;
            ("mermaid".to_string(), mermaid)
        }
        "er" | "schema" => {
            let mermaid = generate_mermaid_er(title, content).await;
            ("mermaid".to_string(), mermaid)
        }
        "state" => {
            let mermaid = generate_mermaid_state(title, content).await;
            ("mermaid".to_string(), mermaid)
        }
        "table" | "comparison" => {
            // Generate HTML table
            let html = generate_html_table(title, content).await;
            ("html".to_string(), html)
        }
        _ => {
            // Architecture - use mermaid graph
            let mermaid = generate_mermaid_architecture(title, content).await;
            ("mermaid".to_string(), mermaid)
        }
    };

    // Build JSON output
    let visual_json = json!({
        "title": title,
        "type": diagram_type,
        "format": format,
        "content": diagram_content,
        "created": chrono::Local::now().to_rfc3339(),
    });

    // Generate filename
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let safe_title = title
        .to_lowercase()
        .replace(' ', "-")
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "");
    let filename = format!("{}-{}-{:.20}.json", diagram_type, safe_title, timestamp);
    let filepath = output_dir.join(&filename);

    // Write JSON file
    let mut file =
        std::fs::File::create(&filepath).map_err(|e| format!("Failed to create file: {}", e))?;
    file.write_all(visual_json.to_string().as_bytes())
        .map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(filepath)
}

fn sanitize_mermaid_label(label: &str) -> String {
    label
        .replace('{', "(")
        .replace('}', ")")
        .replace('[', "(")
        .replace(']', ")")
        .replace('"', "'")
        .trim()
        .to_string()
}

fn sanitize_mermaid_code(raw: &str) -> String {
    raw.replace("```mermaid", "")
        .replace("```", "")
        .replace("{}", "unspecified")
        .replace("[{}", "[unspecified ")
        .replace("ends", "end")
        .trim()
        .to_string()
}

/// Generate a mermaid flowchart from content description
async fn generate_mermaid_flowchart(title: &str, content: &str) -> String {
    // Use LLM to generate mermaid syntax
    let prompt = format!(
        "Generate Mermaid flowchart syntax for: {}\n\nContent: {}\n\n\
         Rules:\
         1. Use flowchart TD (top-down) or LR (left-right)\
         2. Use descriptive node IDs with brackets for labels like A[Label]\
         3. Add arrow labels for clarity like A -->|label| B\
         4. Use different shapes: [] rectangle, () circle, {{}} diamond, [/parallelogram/] parallelogram\
         5. Style with classes if needed\
         6. Output ONLY the mermaid code block, no explanation\n\n\
         Example:\
         ```mermaid\
         flowchart TD\
             A[Start] --> B{{Decision?}}\
             B -->|Yes| C[Process]\
             B -->|No| D[End]\
         ```",
        title, content
    );

    match call_llm_for_mermaid(&prompt).await {
        Ok(mermaid) => sanitize_mermaid_code(&mermaid),
        Err(_) => format!(
            "flowchart TD\n    A[{}] --> B[Process]\n    B --> C[Output]",
            sanitize_mermaid_label(title)
        ),
    }
}

/// Generate a mermaid sequence diagram
async fn generate_mermaid_sequence(title: &str, content: &str) -> String {
    let prompt = format!(
        "Generate Mermaid sequence diagram syntax for: {}\n\nContent: {}\n\n\
         Rules:\
         1. Use sequenceDiagram\
         2. Define participants with 'participant Label'\
         3. Use ->> for solid arrows, -->> for dashed\
         4. Add activate/deactivate for lifelines\
         5. Output ONLY the mermaid code block, no explanation",
        title, content
    );

    match call_llm_for_mermaid(&prompt).await {
        Ok(mermaid) => {
            sanitize_mermaid_code(&mermaid)
        }
        Err(_) => format!(
            "sequenceDiagram\n    participant A\n    participant B\n    A->>B: {}\n    B-->>A: Response",
            sanitize_mermaid_label(title)
        ),
    }
}

/// Generate a mermaid ER diagram
async fn generate_mermaid_er(title: &str, content: &str) -> String {
    let prompt = format!(
        "Generate Mermaid ER diagram syntax for: {}\n\nContent: {}\n\n\
         Rules:\
         1. Use erDiagram\
         2. Define entities with attributes\n         3. Use relationships like ||--o{{ etc.\n         4. Output ONLY the mermaid code block, no explanation",
        title, content
    );

    match call_llm_for_mermaid(&prompt).await {
        Ok(mermaid) => sanitize_mermaid_code(&mermaid),
        Err(_) => format!(
            "erDiagram\n    ENTITY1 {{\n        string id\n    }}\n    ENTITY2 {{\n        string id\n    }}\n    ENTITY1 ||--o{{ ENTITY2 : relates",
        ),
    }
}

/// Generate a mermaid state diagram
async fn generate_mermaid_state(title: &str, content: &str) -> String {
    let prompt = format!(
        "Generate Mermaid state diagram syntax for: {}\n\nContent: {}\n\n\
         Rules:\
         1. Use stateDiagram-v2\
         2. Define states and transitions\
         3. Use [*] for start/end states\
         4. Output ONLY the mermaid code block, no explanation",
        title, content
    );

    match call_llm_for_mermaid(&prompt).await {
        Ok(mermaid) => sanitize_mermaid_code(&mermaid),
        Err(_) => format!(
            "stateDiagram-v2\n    [*] --> Idle\n    Idle --> Active: trigger\n    Active --> [*]",
        ),
    }
}

/// Generate a mermaid architecture diagram (as a graph)
async fn generate_mermaid_architecture(title: &str, content: &str) -> String {
    let prompt = format!(
        "Generate Mermaid architecture diagram for: {}\n\nContent: {}\n\n\
         CRITICAL SYNTAX RULES:\
         1. Start with 'graph TD' on its own line\
         2. Node IDs CANNOT have spaces - use CamelCase or snake_case\
         3. Labels with spaces MUST be quoted in brackets: UI[Web UI] - NOT UI[Web UI\
         4. Subgraphs use 'subgraph Name' and MUST end with 'end' (not 'ends')\
         5. Every subgraph MUST contain at least one node with a definition\n         6. Use --> for arrows, reference nodes by their exact ID\
         7. Node shapes: [] square, () circle, {{}} diamond, [(Database)] cylinder\
         8. Output ONLY valid mermaid code, NO markdown code blocks\n\n\
         CORRECT EXAMPLE:\
         graph TD\n             subgraph Frontend\n                 UI[Web UI]\n                 API[API Client]\n             end\n             subgraph Backend\n                 SRV[Server]\n                 DB[(Database)]\n             end\n             UI --> API\n             API --> SRV\n             SRV --> DB",
        title, content
    );

    match call_llm_for_mermaid(&prompt).await {
        Ok(mermaid) => sanitize_mermaid_code(&mermaid),
        Err(_) => format!(
            "graph TD\n    A[{}] --> B[Component]\n    B --> C[(Data)]",
            sanitize_mermaid_label(title)
        ),
    }
}

/// Generate an HTML table
async fn generate_html_table(title: &str, content: &str) -> String {
    let prompt = format!(
        "Generate HTML table for: {}\n\nContent: {}\n\n\
         Rules:\
         1. Use <table> with <thead> and <tbody>\
         2. Use inline styles for readability\
         3. Include borders and padding\
         4. Output ONLY the HTML table code, no explanation",
        title, content
    );

    match call_llm_for_html(&prompt).await {
        Ok(html) => html,
        Err(_) => format!(
            "<table style='border-collapse:collapse;width:100%;'><thead><tr><th>Item</th><th>Value</th></tr></thead><tbody><tr><td>{}</td><td>Data</td></tr></tbody></table>",
            title
        ),
    }
}

/// Call LLM to generate mermaid syntax
async fn call_llm_for_mermaid(prompt: &str) -> Result<String, String> {
    let ollama = Ollama::new("http://localhost".to_string(), 11434);
    let model = std::env::var("LLM_VIZ_MODEL")
        .unwrap_or_else(|_| "hf.co/mradermacher/Qwen3-8B-heretic-GGUF:Q6_K".to_string());

    let messages = vec![
        OllamaChatMsg::new(MessageRole::System,
            "You generate Mermaid diagram syntax. Output ONLY the mermaid code block, no explanations.".to_string()),
        OllamaChatMsg::new(MessageRole::User, prompt.to_string()),
    ];

    let request = ChatMessageRequest::new(model, messages);

    let response = ollama
        .send_chat_messages(request)
        .await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    let content = response.message.content;

    // Extract mermaid code block
    if let Some(start) = content.find("```mermaid") {
        let after_start = &content[start + 10..];
        if let Some(end) = after_start.find("```") {
            return Ok(after_start[..end].trim().to_string());
        }
    }

    // If no code block, return cleaned content
    Ok(sanitize_mermaid_code(
        content
            .trim()
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim(),
    ))
}

/// Call LLM to generate HTML
async fn call_llm_for_html(prompt: &str) -> Result<String, String> {
    let ollama = Ollama::new("http://localhost".to_string(), 11434);
    let model = std::env::var("LLM_VIZ_MODEL")
        .unwrap_or_else(|_| "hf.co/mradermacher/Qwen3-8B-heretic-GGUF:Q6_K".to_string());

    let messages = vec![
        OllamaChatMsg::new(
            MessageRole::System,
            "You generate HTML code. Output ONLY the HTML, no explanations.".to_string(),
        ),
        OllamaChatMsg::new(MessageRole::User, prompt.to_string()),
    ];

    let request = ChatMessageRequest::new(model, messages);

    let response = ollama
        .send_chat_messages(request)
        .await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    let content = response.message.content;

    // Extract HTML from code block if present
    if let Some(start) = content.find("```html") {
        let after_start = &content[start + 7..];
        if let Some(end) = after_start.find("```") {
            return Ok(after_start[..end].trim().to_string());
        }
    }

    Ok(content
        .trim()
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string())
}

/// Analyze content to determine if a visualization would be helpful
/// Returns Some((diagram_type, title, reason)) if visualization is recommended
fn should_generate_visual(content: &str) -> Option<(String, String, String)> {
    let content_lower = content.to_lowercase();
    let word_count = content.split_whitespace().count();

    // Check for visual-indicating keywords and patterns
    let has_architecture_terms = content_lower.contains("architecture")
        || content_lower.contains("system design")
        || content_lower.contains("component")
        || content_lower.contains("module")
        || content_lower.contains("layer")
        || content_lower.contains("structure");

    let has_flow_terms = content_lower.contains("flow")
        || content_lower.contains("pipeline")
        || content_lower.contains("process")
        || content_lower.contains("workflow")
        || content_lower.contains("sequence")
        || content_lower.contains("steps");

    let has_comparison_terms = content_lower.contains("compare")
        || content_lower.contains("versus")
        || content_lower.contains(" vs ")
        || content_lower.contains("difference between")
        || content_lower.contains("pros and cons")
        || content_lower.contains("trade-off");

    let has_data_terms = content_lower.contains("metrics")
        || content_lower.contains("statistics")
        || content_lower.contains("performance")
        || content_lower.contains("kpi")
        || content_lower.contains("measurement")
        || content_lower.contains("count")
        || content_lower.contains("percentage");

    let _has_relationship_terms = content_lower.contains("relationship")
        || content_lower.contains("connection")
        || content_lower.contains("dependency")
        || content_lower.contains("interaction")
        || content_lower.contains("link between");

    // Count list items (indicates structured data)
    let list_items = content
        .lines()
        .filter(|l| {
            l.trim().starts_with('-') || l.trim().starts_with('*') || l.trim().starts_with("1.")
        })
        .count();

    // Decision logic
    if has_architecture_terms && word_count > 50 {
        let title = extract_title(content, "System Architecture");
        return Some((
            "architecture".to_string(),
            title,
            "Complex architecture description detected".to_string(),
        ));
    }

    if has_flow_terms && (word_count > 40 || list_items >= 3) {
        let title = extract_title(content, "Process Flow");
        return Some((
            "flowchart".to_string(),
            title,
            "Process/workflow description detected".to_string(),
        ));
    }

    if has_comparison_terms && word_count > 60 {
        let title = extract_title(content, "Comparison Analysis");
        return Some((
            "table".to_string(),
            title,
            "Comparative analysis detected".to_string(),
        ));
    }

    if has_data_terms && (word_count > 50 || list_items >= 4) {
        let title = extract_title(content, "Metrics Dashboard");
        return Some((
            "dashboard".to_string(),
            title,
            "Data/metrics description detected".to_string(),
        ));
    }

    if list_items >= 6 {
        let title = extract_title(content, "Structured Overview");
        return Some((
            "table".to_string(),
            title,
            "Structured list data detected".to_string(),
        ));
    }

    // Complex content with multiple sections might benefit from visualization
    let sections = content.split("\n\n").count();
    if sections >= 4 && word_count > 100 {
        let title = extract_title(content, "Overview");
        return Some((
            "architecture".to_string(),
            title,
            "Complex multi-section content detected".to_string(),
        ));
    }

    None
}

/// Extract a title from content, or use default
fn extract_title(content: &str, default: &str) -> String {
    // Try to find a heading or first sentence
    if let Some(first_line) = content.lines().next() {
        let trimmed = first_line.trim();
        if !trimmed.is_empty() && trimmed.len() < 80 {
            // Remove markdown heading markers
            return trimmed.trim_start_matches('#').trim().to_string();
        }
    }

    // Try first sentence
    if let Some(sentence_end) = content.find(|c: char| c == '.' || c == '!' || c == '?') {
        let sentence = &content[..sentence_end];
        if sentence.len() > 10 && sentence.len() < 80 {
            return sentence.to_string();
        }
    }

    default.to_string()
}

/// Trigger automatic visual generation from chat/council content
#[allow(dead_code)]
async fn maybe_generate_visual(
    content: &str,
    source: &str, // "chat" or "council"
    bg_tx: &mpsc::UnboundedSender<BgEvent>,
) {
    if let Some((diagram_type, title, reason)) = should_generate_visual(content) {
        // Log the decision
        let _ = bg_tx.send(BgEvent::Log(format!(
            "📊 Auto-visual triggered from {}: {} ({})",
            source, title, reason
        )));

        // Extract relevant section for visualization (first 500 chars + any list items)
        let viz_content = if content.len() > 800 {
            let mut truncated = content[..500].to_string();
            // Add key list items if present
            for line in content.lines().skip(10) {
                if line.trim().starts_with('-') || line.trim().starts_with('*') {
                    truncated.push('\n');
                    truncated.push_str(line);
                }
                if truncated.len() > 800 {
                    break;
                }
            }
            truncated
        } else {
            content.to_string()
        };

        let _ = bg_tx.send(BgEvent::VisualExplainer {
            diagram_type,
            title,
            content: viz_content,
            data: None,
            open_browser: false, // Don't auto-open, just generate
        });
    }
}

// ════════════════════════════════════════════════════════════════════════════
// COUNCIL MODE IMPLEMENTATIONS
// ════════════════════════════════════════════════════════════════════════════

/// Simple Council: Single-pass direct answer
async fn council_run_simple(
    ollama: &Ollama,
    model: &str,
    model_short: &str,
    question: &str,
    grounded: &str,
    agent_context: &str,
    evidence: &EvidenceIndex,
    roodb: Option<Arc<RooDb>>,
    council_tx: &tokio::sync::broadcast::Sender<String>,
    graph_tx: &tokio::sync::broadcast::Sender<String>,
    bg_tx: &mpsc::UnboundedSender<BgEvent>,
    agent_count: usize,
    repl_state: Option<Arc<ReplState>>,
    council_session: Arc<Mutex<DspySession<NoopSessionAdapter>>>,
) {
    let _ = council_tx.send(format!(
        "{{\"type\":\"round\",\"content\":{}}}",
        serde_json::to_string(&format!(
            "\n━━━ Simple Council: Direct Answer ({}) ━━━\n",
            model_short
        ))
        .unwrap_or_default()
    ));

    let _ = graph_tx.send(make_graph_event(
        "agent_activate",
        Some(map_council_agent(0, agent_count)),
        None,
        "Simple Council: Single-pass analysis",
    ));

    {
        let mut session = council_session.lock().await;
        session.add_turn(TurnRole::User, question);
    }

    let ctx = DspyContext {
        question,
        grounded,
        agents: agent_context,
        prior: "",
    };
    let (response, mut trace_opt) = match run_signature_traced(
        ollama,
        model,
        &sig_simple_answer(),
        &ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => (t.output.clone(), Some(t)),
        Err(e) => {
            let _ = council_tx.send(format!(
                "{{\"type\":\"error\",\"content\":\"Council failed: {}\"}}",
                e
            ));
            (String::new(), None)
        }
    };

    if !response.is_empty() {
        let _ = council_tx.send(
            serde_json::json!({
                "type": "token",
                "round": 1,
                "persona": "Advisor",
                "content": response
            })
            .to_string(),
        );
        let raw = response.trim().to_string();
        let mut verification = App::verify_semantic_contract_strict(&raw, evidence);
        let mut repaired = raw.clone();
        let mut repair_count = 0i32;
        if !verification.ok {
            for _ in 0..2 {
                if let Ok(fixed) = attempt_semantic_repair(
                    ollama,
                    model,
                    question,
                    grounded,
                    agent_context,
                    &repaired,
                    roodb.clone(),
                )
                .await
                {
                    repaired = fixed;
                    repair_count += 1;
                    verification = App::verify_semantic_contract_strict(&repaired, evidence);
                    if verification.ok {
                        break;
                    }
                }
            }
        }
        // Persist trace with verification results
        if let (Some(ref mut trace), Some(ref db)) = (&mut trace_opt, &roodb) {
            trace.semantic_ok = verification.ok;
            trace.repair_count = repair_count;
            trace.score = if verification.ok { 0.9 } else { 0.4 };
            let db = db.clone();
            let trace_c = TraceResult {
                output: trace.output.clone(),
                score: trace.score,
                semantic_ok: trace.semantic_ok,
                repair_count: trace.repair_count,
                latency_ms: trace.latency_ms,
                signature_name: trace.signature_name.clone(),
                input_question: trace.input_question.clone(),
                input_context_hash: trace.input_context_hash.clone(),
                model: trace.model.clone(),
            };
            let bg = bg_tx.clone();
            tokio::spawn(async move {
                persist_trace(&db, &trace_c).await;
                let _ = bg.send(BgEvent::DspyTraceLogged {
                    signature_name: trace_c.signature_name,
                    score: trace_c.score,
                });
            });
        }
        let (plan_text, plan_steps) = build_plan_data(&verification, evidence);
        if !verification.errors.is_empty() {
            let _ = council_tx.send(serde_json::json!({
                "type": "round",
                "content": format!("⚠️ semantic contract violations: {}", verification.errors.join(" | ")),
            }).to_string());
        }
        let proof_log = format_proof_log(&verification, evidence);
        let _ = council_tx.send(
            serde_json::json!({
                "type": "round",
                "content": proof_log,
            })
            .to_string(),
        );
        let _ = council_tx.send(
            serde_json::json!({
                "type": "round",
                "content": format!(
                    "Semantic coverage: claims={} evidence={} evidence/claim={:.2}",
                    verification.claim_count, verification.evidence_count, verification.coverage
                ),
            })
            .to_string(),
        );
        let verified = verification.verified_text.clone();
        let primary_summary = choose_user_facing_summary(question, &repaired, &verified);
        let verified_with_plan = render_council_synthesis_for_query(
            question,
            &primary_summary,
            &verification,
            &plan_text,
        );
        let _ = council_tx.send(
            serde_json::json!({
                "type": "synthesis",
                "content": verified_with_plan,
            })
            .to_string(),
        );

        // Evaluate synthesis quality → real belief confidence + sharpen statement
        let _ = council_tx.send(
            serde_json::json!({
                "type": "round",
                "content": "\n⟳ evaluating synthesis quality…",
            })
            .to_string(),
        );
        let eval_result = evaluate_synthesis(&verified, question, model)
            .await
            .unwrap_or_else(|_| EvalResult {
                score: 0.65,
                sharpened: None,
                feedback: "Evaluation failed".to_string(),
            });
        let mut conf = eval_result.score;
        if !verification.ok {
            conf *= 0.3;
        }
        let sharpened = eval_result
            .sharpened
            .unwrap_or_else(|| verified.to_string());
        let primary_sharpened = choose_user_facing_summary(question, &repaired, &sharpened);
        let sharpened_with_plan = render_council_synthesis_for_query(
            question,
            &primary_sharpened,
            &verification,
            &plan_text,
        );

        let _ = council_tx.send(serde_json::json!({
            "type": "round",
            "content": format!("✓ synthesis score: {:.2} — belief confidence set to {:.2}", conf, conf),
        }).to_string());

        persist_council_claims(
            roodb.clone(),
            question,
            &verification,
            conf,
            verification.coverage,
            "simple",
        )
        .await;
        let citations = collect_citations(&verification, evidence);

        if let Some(state) = &repl_state {
            let metadata = json!({
                "question": question,
                "mode": "simple",
                "confidence": conf,
                "coverage": verification.coverage,
                "citations": citations.len(),
            });
            let answer = AnswerDict::with_content(
                sharpened_with_plan.clone(),
                metadata,
                Some("council".into()),
            );
            state.set_last(answer.clone());
            if let Some(pending) = state.take_pending(ReplCommandKind::Council) {
                let _ = pending.tx.send(answer);
            }
        }

        let _ = bg_tx.send(BgEvent::CouncilSynthesis {
            question: question.to_string(),
            synthesis: sharpened_with_plan.clone(),
            confidence: conf,
            citations,
            coverage: verification.coverage,
            plan_text: plan_text.clone(),
            plan_steps: plan_steps.clone(),
        });

        let (session_id, example_len) = {
            let mut session = council_session.lock().await;
            session.add_turn(TurnRole::Assistant, sharpened_with_plan.clone());
            (session.id(), session.to_optimization_examples().len())
        };
        if example_len > 0 {
            let _ = bg_tx.send(BgEvent::Log(format!(
                "Simple council session {} has {} optimization examples",
                session_id, example_len
            )));
        }
    }

    let _ = council_tx.send("{\"type\":\"done\"}".to_string());
}

async fn attempt_semantic_repair(
    ollama: &Ollama,
    model: &str,
    question: &str,
    grounded: &str,
    agent_context: &str,
    prior: &str,
    roodb: Option<Arc<RooDb>>,
) -> Result<String, String> {
    let ctx = DspyContext {
        question,
        grounded,
        agents: agent_context,
        prior,
    };
    run_signature(ollama, model, &sig_semantic_repair(), &ctx, roodb).await
}

/// Debate Council: Multi-phase structured deliberation
async fn council_run_debate(
    ollama: &Ollama,
    model: &str,
    model_short: &str,
    question: &str,
    grounded: &str,
    agent_context: &str,
    evidence: &EvidenceIndex,
    roodb: Option<Arc<RooDb>>,
    council_tx: &tokio::sync::broadcast::Sender<String>,
    graph_tx: &tokio::sync::broadcast::Sender<String>,
    bg_tx: &mpsc::UnboundedSender<BgEvent>,
    agent_count: usize,
    council_session: Arc<Mutex<DspySession<NoopSessionAdapter>>>,
) {
    // PHASE 1: ANALYST
    let _ = council_tx.send(format!(
        "{{\"type\":\"round\",\"content\":{}}}",
        serde_json::to_string(&format!("\n━━━ Round 1: Analyst ({}) ━━━\n", model_short))
            .unwrap_or_default()
    ));

    let _ = graph_tx.send(make_graph_event(
        "agent_activate",
        Some(map_council_agent(0, agent_count)),
        None,
        "Analyst presenting initial position",
    ));

    {
        let mut session = council_session.lock().await;
        session.add_turn(TurnRole::User, question);
    }

    let mut debate_traces: Vec<TraceResult> = Vec::new();

    let analyst_ctx = DspyContext {
        question,
        grounded,
        agents: agent_context,
        prior: "",
    };
    let analyst_stance = match run_signature_traced(
        ollama,
        model,
        &sig_analyst_stance(),
        &analyst_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[analyst_stance error: {}]", e),
    };
    let analyst_evidence = match run_signature_traced(
        ollama,
        model,
        &sig_analyst_evidence(),
        &analyst_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[analyst_evidence error: {}]", e),
    };
    let analyst_argument = match run_signature_traced(
        ollama,
        model,
        &sig_analyst_argument(),
        &analyst_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[analyst_argument error: {}]", e),
    };
    let analyst_response = format!(
        "**1. My Stance**\n{}\n\n**2. Key Evidence**\n{}\n\n**3. Strongest Argument**\n{}",
        analyst_stance.trim(),
        analyst_evidence.trim(),
        analyst_argument.trim()
    );
    let _ = council_tx.send(
        serde_json::json!({
            "type": "token",
            "round": 1,
            "persona": "Analyst",
            "content": analyst_response
        })
        .to_string(),
    );

    if analyst_response.is_empty() {
        let _ = council_tx.send("{\"type\":\"done\"}".to_string());
        return;
    }

    // PHASE 2: CHALLENGER
    let _ = council_tx.send(format!(
        "{{\"type\":\"round\",\"content\":{}}}",
        serde_json::to_string(&format!(
            "\n━━━ Round 2: Challenger ({}) ━━━\n",
            model_short
        ))
        .unwrap_or_default()
    ));

    let _ = graph_tx.send(make_graph_event(
        "agent_activate",
        Some(map_council_agent(1, agent_count)),
        None,
        "Challenging the Analyst's position",
    ));

    let challenger_ctx = DspyContext {
        question,
        grounded,
        agents: agent_context,
        prior: &analyst_response,
    };
    let challenger_weak = match run_signature_traced(
        ollama,
        model,
        &sig_challenger_weak_point(),
        &challenger_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[challenger_weak_point error: {}]", e),
    };
    let challenger_counter = match run_signature_traced(
        ollama,
        model,
        &sig_challenger_counter_evidence(),
        &challenger_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[challenger_counter_evidence error: {}]", e),
    };
    let challenger_alt = match run_signature_traced(
        ollama,
        model,
        &sig_challenger_alternative(),
        &challenger_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[challenger_alternative error: {}]", e),
    };
    let challenger_response = format!(
        "**1. Weakest Point**\n{}\n\n**2. Counter‑Evidence**\n{}\n\n**3. Alternative Position**\n{}",
        challenger_weak.trim(),
        challenger_counter.trim(),
        challenger_alt.trim()
    );
    let _ = council_tx.send(
        serde_json::json!({
            "type": "token",
            "round": 2,
            "persona": "Challenger",
            "content": challenger_response
        })
        .to_string(),
    );

    // PHASE 3: REBUTTAL
    let _ = council_tx.send(format!(
        "{{\"type\":\"round\",\"content\":{}}}",
        serde_json::to_string(&format!("\n━━━ Round 3: Rebuttal ({}) ━━━\n", model_short))
            .unwrap_or_default()
    ));

    let _ = graph_tx.send(make_graph_event(
        "agent_activate",
        Some(map_council_agent(2, agent_count)),
        None,
        "Analyst rebutting Challenger",
    ));

    let rebuttal_ctx = DspyContext {
        question,
        grounded,
        agents: agent_context,
        prior: &format!(
            "ANALYST:\n{}\n\nCHALLENGER:\n{}",
            analyst_response, challenger_response
        ),
    };
    let rebuttal_valid = match run_signature_traced(
        ollama,
        model,
        &sig_rebuttal_valid(),
        &rebuttal_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[rebuttal_valid error: {}]", e),
    };
    let rebuttal_refute = match run_signature_traced(
        ollama,
        model,
        &sig_rebuttal_refute(),
        &rebuttal_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[rebuttal_refute error: {}]", e),
    };
    let rebuttal_refine = match run_signature_traced(
        ollama,
        model,
        &sig_rebuttal_refine(),
        &rebuttal_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[rebuttal_refine error: {}]", e),
    };
    let rebuttal_response = format!(
        "**1. Valid Critiques**\n{}\n\n**2. Refutations**\n{}\n\n**3. Refined Position**\n{}",
        rebuttal_valid.trim(),
        rebuttal_refute.trim(),
        rebuttal_refine.trim()
    );
    let _ = council_tx.send(
        serde_json::json!({
            "type": "token",
            "round": 3,
            "persona": "Analyst (Rebuttal)",
            "content": rebuttal_response
        })
        .to_string(),
    );

    // PHASE 4: CHAIR
    let _ = council_tx.send(format!(
        "{{\"type\":\"round\",\"content\":{}}}",
        serde_json::to_string(&format!(
            "\n━━━ Round 4: Chair's Verdict ({}) ━━━\n",
            model_short
        ))
        .unwrap_or_default()
    ));

    let _ = graph_tx.send(make_graph_event(
        "agent_activate",
        Some(map_council_agent(3, agent_count)),
        None,
        "Chair rendering verdict",
    ));

    let chair_ctx = DspyContext {
        question,
        grounded,
        agents: agent_context,
        prior: &format!(
            "ANALYST:\n{}\n\nCHALLENGER:\n{}\n\nREBUTTAL:\n{}",
            analyst_response, challenger_response, rebuttal_response
        ),
    };
    let chair_winner = match run_signature_traced(
        ollama,
        model,
        &sig_chair_winner(),
        &chair_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[chair_winner error: {}]", e),
    };
    let chair_synth = match run_signature_traced(
        ollama,
        model,
        &sig_chair_synthesis(),
        &chair_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[chair_synthesis error: {}]", e),
    };
    let chair_conf = match run_signature_traced(
        ollama,
        model,
        &sig_chair_confidence(),
        &chair_ctx,
        roodb.clone(),
        None,
    )
    .await
    {
        Ok(t) => {
            let out = t.output.clone();
            debate_traces.push(t);
            out
        }
        Err(e) => format!("[chair_confidence error: {}]", e),
    };
    let chair_response = format!(
        "## WINNING ARGUMENT\n{}\n\n## SYNTHESIS\n{}\n\n## CONFIDENCE\n{}",
        chair_winner.trim(),
        chair_synth.trim(),
        chair_conf.trim()
    );

    let raw_chair = chair_response.trim().to_string();
    let mut verification = App::verify_semantic_contract_strict(&raw_chair, evidence);
    let mut repaired = raw_chair.clone();
    if !verification.ok {
        for _ in 0..2 {
            if let Ok(fixed) = attempt_semantic_repair(
                ollama,
                model,
                question,
                grounded,
                agent_context,
                &repaired,
                roodb.clone(),
            )
            .await
            {
                repaired = fixed;
                verification = App::verify_semantic_contract_strict(&repaired, evidence);
                if verification.ok {
                    break;
                }
            }
        }
    }
    let (plan_text, plan_steps) = build_plan_data(&verification, evidence);
    if !verification.errors.is_empty() {
        let _ = council_tx.send(serde_json::json!({
            "type": "round",
            "content": format!("⚠️ semantic contract violations: {}", verification.errors.join(" | ")),
        }).to_string());
    }
    let proof_log = format_proof_log(&verification, evidence);
    let _ = council_tx.send(
        serde_json::json!({
            "type": "round",
            "content": proof_log,
        })
        .to_string(),
    );
    let _ = council_tx.send(
        serde_json::json!({
            "type": "round",
            "content": format!(
                "Semantic coverage: claims={} evidence={} evidence/claim={:.2}",
                verification.claim_count, verification.evidence_count, verification.coverage
            ),
        })
        .to_string(),
    );
    let verified = verification.verified_text.clone();
    let primary_summary = choose_user_facing_summary(question, &repaired, &verified);
    let verified_with_plan =
        render_council_synthesis_for_query(question, &primary_summary, &verification, &plan_text);
    let _ = council_tx.send(
        serde_json::json!({
            "type": "synthesis",
            "content": verified_with_plan,
        })
        .to_string(),
    );

    // Evaluate synthesis quality → real belief confidence + sharpen statement
    let _ = council_tx.send(
        serde_json::json!({
            "type": "round",
            "content": "\n⟳ evaluating synthesis quality…",
        })
        .to_string(),
    );
    let eval_result = evaluate_synthesis(&verified, question, model)
        .await
        .unwrap_or_else(|_| EvalResult {
            score: 0.65,
            sharpened: None,
            feedback: "Evaluation failed".to_string(),
        });
    let mut conf = eval_result.score;
    if !verification.ok {
        conf *= 0.3;
    }
    let sharpened = eval_result.sharpened.unwrap_or_else(|| verified.clone());
    let primary_sharpened = choose_user_facing_summary(question, &repaired, &sharpened);
    let sharpened_with_plan =
        render_council_synthesis_for_query(question, &primary_sharpened, &verification, &plan_text);

    let _ = council_tx.send(serde_json::json!({
        "type": "round",
        "content": format!("✓ synthesis score: {:.2} — belief confidence set to {:.2}", conf, conf),
    }).to_string());

    persist_council_claims(
        roodb.clone(),
        question,
        &verification,
        conf,
        verification.coverage,
        "debate",
    )
    .await;
    persist_council_claims(
        roodb.clone(),
        question,
        &verification,
        conf,
        verification.coverage,
        "orchestrate",
    )
    .await;
    let citations = collect_citations(&verification, evidence);
    let _ = bg_tx.send(BgEvent::CouncilSynthesis {
        question: question.to_string(),
        synthesis: sharpened_with_plan.clone(),
        confidence: conf,
        citations,
        coverage: verification.coverage,
        plan_text: plan_text.clone(),
        plan_steps: plan_steps.clone(),
    });

    let (session_id, example_len) = {
        let mut session = council_session.lock().await;
        session.add_turn(TurnRole::Assistant, sharpened_with_plan.clone());
        (session.id(), session.to_optimization_examples().len())
    };
    if example_len > 0 {
        let _ = bg_tx.send(BgEvent::Log(format!(
            "Debate council session {} has {} optimization examples",
            session_id, example_len
        )));
    }

    // Persist all debate traces with final verification score applied to all
    if let Some(ref db) = roodb {
        let db = db.clone();
        let bg = bg_tx.clone();
        let sem_ok = verification.ok;
        let final_score = conf;
        tokio::spawn(async move {
            for mut trace in debate_traces {
                trace.semantic_ok = sem_ok;
                trace.score = final_score * 0.8 + 0.1; // Scale: debate traces get conf-proportional scores
                persist_trace(&db, &trace).await;
                let _ = bg.send(BgEvent::DspyTraceLogged {
                    signature_name: trace.signature_name,
                    score: trace.score,
                });
            }
        });
    }

    let _ = council_tx.send("{\"type\":\"done\"}".to_string());
}

/// Orchestrate Council: Task breakdown with parallel execution
async fn council_run_orchestrate(
    ollama: &Ollama,
    model: &str,
    model_short: &str,
    question: &str,
    grounded: &str,
    agent_context: &str,
    evidence: &EvidenceIndex,
    _roodb: Option<Arc<RooDb>>,
    council_tx: &tokio::sync::broadcast::Sender<String>,
    graph_tx: &tokio::sync::broadcast::Sender<String>,
    bg_tx: &mpsc::UnboundedSender<BgEvent>,
    agent_count: usize,
    council_session: Arc<Mutex<DspySession<NoopSessionAdapter>>>,
) {
    // PHASE 1: ORCHESTRATOR
    let _ = council_tx.send(format!(
        "{{\"type\":\"round\",\"content\":{}}}",
        serde_json::to_string(&format!(
            "\n━━━ Orchestrator: Task Decomposition ({}) ━━━\n",
            model_short
        ))
        .unwrap_or_default()
    ));

    let _ = graph_tx.send(make_graph_event(
        "agent_activate",
        Some(map_council_agent(0, agent_count)),
        None,
        "Orchestrator decomposing task",
    ));

    {
        let mut session = council_session.lock().await;
        session.add_turn(TurnRole::User, question);
    }

    let orchestrator_system = if grounded.is_empty() {
        format!(
            "You are an Orchestrator managing a complex task.\n\
             Break the problem into parallel sub-tasks for efficient execution.\n\n\
             AGENT SNAPSHOT:\n{}",
            agent_context
        )
    } else {
        format!(
            "You are an Orchestrator managing a complex task.\n\
             Break the problem into parallel sub-tasks for efficient execution.\n\n\
             LIVE WORLD DATA:\n{}\n\n\
             AGENT SNAPSHOT:\n{}",
            grounded, agent_context
        )
    };

    let orchestrator_prompt = format!(
        "TASK: {}\n\n\
         Decompose this into 2-4 parallel sub-tasks.\n\
         For each sub-task, specify:\n\
         - TASK_N: [clear description]\n\
         - INPUT: [what data it needs]\n\
         - OUTPUT: [expected deliverable]",
        question
    );

    let decomposition = match run_council_role(
        ollama,
        model,
        &orchestrator_system,
        &orchestrator_prompt,
        council_tx,
        "Orchestrator",
        1,
    )
    .await
    {
        Ok(resp) => strip_think_tags(&resp),
        Err(e) => {
            let _ = council_tx.send(format!(
                "{{\"type\":\"error\",\"content\":\"Orchestrator failed: {}\"}}",
                e
            ));
            String::new()
        }
    };

    if decomposition.is_empty() {
        let _ = council_tx.send("{\"type\":\"done\"}".to_string());
        return;
    }

    // PHASE 2: PARALLEL WORKER EXECUTION
    let _ = council_tx.send(format!(
        "{{\"type\":\"round\",\"content\":{}}}",
        serde_json::to_string(&format!(
            "\n━━━ Parallel Workers Executing ({} workers) ━━━\n",
            model_short
        ))
        .unwrap_or_default()
    ));

    // Parse sub-tasks from decomposition
    let sub_tasks: Vec<String> = decomposition
        .split("TASK_")
        .skip(1)
        .take(4)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if sub_tasks.is_empty() {
        let _ = council_tx.send(format!("{{\"type\":\"token\",\"content\":\"[!] No sub-tasks parsed, executing as single task\"}}"));
    }

    let mut worker_handles = vec![];
    let agent_context_owned = agent_context.to_string();

    for (i, sub_task) in sub_tasks.iter().enumerate() {
        let task_desc = bounded_text(sub_task, 900);
        let original_question = bounded_text(question, 1200);
        let grounded_ctx = bounded_text(grounded, 8000);
        let agent_ctx = agent_context_owned.clone();
        let worker_model = model.to_string();
        let worker_tx = council_tx.clone();
        let graph_tx_worker = graph_tx.clone();

        let handle = tokio::spawn(async move {
            let worker_ollama = Ollama::new("http://localhost".to_string(), 11434);

            let _ = graph_tx_worker.send(make_graph_event(
                "agent_activate",
                Some(map_council_agent(4 + i, agent_count)),
                None,
                &format!(
                    "Worker {} executing: {}",
                    i + 1,
                    &task_desc[..task_desc.len().min(50)]
                ),
            ));

            let _ = worker_tx.send(format!(
                "{{\"type\":\"token\",\"content\":\"\n>>> Worker {} starting...\"}}",
                i + 1
            ));

            // Execute the sub-task with LLM
            let worker_system = format!(
                "You are Worker {} in a parallel task execution system. \
                 Your job is to complete the assigned sub-task thoroughly and return concrete results. \
                 Be specific, factual, and actionable in your response.",
                i + 1
            );

            let worker_prompt = format!(
                "ORIGINAL TASK: {}\n\n\
                 YOUR SUB-TASK (TASK_{}):\n{}\n\n\
                 RELEVANT CONTEXT:\n{}\n\n\
                 AGENT SNAPSHOT:\n{}\n\n\
                 Execute this sub-task completely. Provide:\
                 1. Analysis of what needs to be done\
                 2. Step-by-step execution\
                 3. Concrete deliverables/results",
                original_question,
                i + 1,
                task_desc,
                grounded_ctx,
                agent_ctx
            );

            let messages = vec![
                OllamaChatMsg::system(worker_system),
                OllamaChatMsg::user(worker_prompt),
            ];
            let request = ChatMessageRequest::new(worker_model, messages);

            use tokio_stream::StreamExt;
            let mut response = String::new();

            use tokio::time::{timeout, Duration, Instant};
            match timeout(
                Duration::from_secs(180),
                worker_ollama.send_chat_messages_stream(request),
            )
            .await
            {
                Ok(Ok(mut stream)) => {
                    let deadline = Instant::now() + Duration::from_secs(600);
                    loop {
                        if Instant::now() >= deadline {
                            response.push_str(&format!("\n[Worker {} timeout after 600s]", i + 1));
                            break;
                        }
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        let wait_for = Duration::from_secs(30).min(remaining);
                        let next_item = match timeout(wait_for, stream.next()).await {
                            Ok(item) => item,
                            Err(_) => {
                                response.push_str(&format!(
                                    "\n[Worker {} stalled > {}s]",
                                    i + 1,
                                    wait_for.as_secs()
                                ));
                                break;
                            }
                        };
                        let Some(result) = next_item else {
                            break;
                        };
                        match result {
                            Ok(chunk) => {
                                if !chunk.message.content.is_empty() {
                                    response.push_str(&chunk.message.content);
                                }
                                if chunk.done {
                                    break;
                                }
                            }
                            Err(e) => {
                                response.push_str(&format!(
                                    "\n[Worker {} stream error: {:?}]",
                                    i + 1,
                                    e
                                ));
                                break;
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    response = format!("[Worker {} execution error: {}]", i + 1, e);
                }
                Err(_) => {
                    response = format!("[Worker {} stream start timed out after 180s]", i + 1);
                }
            }

            let _ = worker_tx.send(format!(
                "{{\"type\":\"token\",\"content\":\"[OK] Worker {} completed\"}}",
                i + 1
            ));

            format!(
                "## WORKER {} RESULT:\n{}\n",
                i + 1,
                strip_think_tags(&response)
            )
        });

        worker_handles.push(handle);
    }

    // Wait for all workers to complete
    let mut worker_results = vec![];
    for (i, handle) in worker_handles.into_iter().enumerate() {
        match handle.await {
            Ok(result) => worker_results.push(result),
            Err(e) => worker_results.push(format!("## WORKER {} ERROR: {:?}", i + 1, e)),
        }
    }

    // PHASE 3: INTEGRATOR
    let _ = council_tx.send(format!(
        "{{\"type\":\"round\",\"content\":{}}}",
        serde_json::to_string(&format!(
            "\n━━━ Integrator: Synthesis ({}) ━━━\n",
            model_short
        ))
        .unwrap_or_default()
    ));

    let _ = graph_tx.send(make_graph_event(
        "agent_activate",
        Some(map_council_agent(5, agent_count)),
        None,
        "Integrator synthesizing parallel results",
    ));

    let integrator_system =
        "You are an Integrator combining parallel work streams into a coherent whole.\n\
         Synthesize the worker outputs, resolve any conflicts, and produce a unified answer."
            .to_string();

    let worker_outputs = worker_results.join("\n\n");
    let integrator_prompt = format!(
        "ORIGINAL TASK: {}\n\n\
         TASK DECOMPOSITION:\n{}\n\n\
         PARALLEL WORKER OUTPUTS:\n{}\n\n\
         Synthesize these parallel results into a unified answer:\n\
         1. Summary of what each worker contributed\n\
         2. Integration of their findings\n\
         3. Final unified answer",
        question, decomposition, worker_outputs
    );

    let final_response = match run_council_role(
        ollama, model, &integrator_system, &integrator_prompt,
        council_tx, "Integrator", 3
    ).await {
        Ok(resp) => strip_think_tags(&resp),
        Err(_) => format!(
            "## Task Decomposition\n{}\n\n## Parallel Execution\n{}\n\n## Integrated Result\n[Orchestration completed with {} parallel workers]",
            decomposition, worker_outputs, worker_results.len()
        )
    };

    let raw_final = final_response.trim().to_string();
    let mut verification = App::verify_semantic_contract_strict(&raw_final, evidence);
    let mut repaired = raw_final.clone();
    if !verification.ok {
        for _ in 0..2 {
            if let Ok(fixed) = attempt_semantic_repair(
                ollama,
                model,
                question,
                grounded,
                agent_context,
                &repaired,
                None,
            )
            .await
            {
                repaired = fixed;
                verification = App::verify_semantic_contract_strict(&repaired, evidence);
                if verification.ok {
                    break;
                }
            }
        }
    }
    let (plan_text, plan_steps) = build_plan_data(&verification, evidence);
    if !verification.errors.is_empty() {
        let _ = council_tx.send(serde_json::json!({
            "type": "round",
            "content": format!("⚠️ semantic contract violations: {}", verification.errors.join(" | ")),
        }).to_string());
    }
    let verified = verification.verified_text.clone();
    let primary_summary = choose_user_facing_summary(question, &repaired, &verified);
    let verified_with_plan =
        render_council_synthesis_for_query(question, &primary_summary, &verification, &plan_text);
    let _ = council_tx.send(
        serde_json::json!({
            "type": "synthesis",
            "content": verified_with_plan,
        })
        .to_string(),
    );

    // Evaluate synthesis quality → real belief confidence + sharpen statement
    let _ = council_tx.send(
        serde_json::json!({
            "type": "round",
            "content": "\n⟳ evaluating synthesis quality…",
        })
        .to_string(),
    );
    let eval_result = evaluate_synthesis(&verified, question, model)
        .await
        .unwrap_or_else(|_| EvalResult {
            score: 0.65,
            sharpened: None,
            feedback: "Evaluation failed".to_string(),
        });
    let mut conf = eval_result.score;
    if !verification.ok {
        conf *= 0.3;
    }
    let sharpened = eval_result.sharpened.unwrap_or_else(|| verified.clone());
    let primary_sharpened = choose_user_facing_summary(question, &repaired, &sharpened);
    let sharpened_with_plan =
        render_council_synthesis_for_query(question, &primary_sharpened, &verification, &plan_text);
    let _ = council_tx.send(serde_json::json!({
        "type": "round",
        "content": format!("✓ synthesis score: {:.2} — belief confidence set to {:.2}", conf, conf),
    }).to_string());

    let citations = collect_citations(&verification, evidence);
    let _ = bg_tx.send(BgEvent::CouncilSynthesis {
        question: question.to_string(),
        synthesis: sharpened_with_plan.clone(),
        confidence: conf,
        citations,
        coverage: verification.coverage,
        plan_text: plan_text.clone(),
        plan_steps: plan_steps.clone(),
    });

    let (session_id, example_len) = {
        let mut session = council_session.lock().await;
        session.add_turn(TurnRole::Assistant, sharpened_with_plan.clone());
        (session.id(), session.to_optimization_examples().len())
    };
    if example_len > 0 {
        let _ = bg_tx.send(BgEvent::Log(format!(
            "Orchestrate council session {} has {} optimization examples",
            session_id, example_len
        )));
    }

    let _ = council_tx.send("{\"type\":\"done\"}".to_string());
}

/// LLM Deliberation Council: uses council::LLMDebateCouncil with structured decision output.
async fn council_run_llm_deliberation(
    question: &str,
    model: &str,
    model_short: &str,
    members: Vec<CouncilMember>,
    council_tx: &tokio::sync::broadcast::Sender<String>,
    graph_tx: &tokio::sync::broadcast::Sender<String>,
    bg_tx: &mpsc::UnboundedSender<BgEvent>,
    roodb: Option<Arc<RooDb>>,
    agent_count: usize,
    council_session: Arc<Mutex<DspySession<NoopSessionAdapter>>>,
    stigmergic_context: Option<StigmergicCouncilContext>,
) {
    use tokio::time::{timeout, Duration};

    let _ = council_tx.send(format!(
        "{{\"type\":\"round\",\"content\":{}}}",
        serde_json::to_string(&format!(
            "\n━━━ LLM Deliberation Council ({}) ━━━\n",
            model_short
        ))
        .unwrap_or_default()
    ));

    let _ = graph_tx.send(make_graph_event(
        "agent_activate",
        Some(map_council_agent(0, agent_count)),
        None,
        "LLM deliberation council started",
    ));

    {
        let mut session = council_session.lock().await;
        session.add_turn(TurnRole::User, question);
    }

    let mut proposal = Proposal::new("runtime", "Runtime Council Proposal", question, 0);
    proposal.estimate_complexity();
    proposal.urgency = App::estimate_council_urgency(question);
    proposal.stigmergic_context = stigmergic_context;

    let llm_cfg = LLMDeliberationConfig {
        model: model.to_string(),
        endpoint: "http://localhost:11434".to_string(),
        ..LLMDeliberationConfig::default()
    };
    let mut council = Council::new_with_llm_config(
        CouncilMode::LLMDeliberation,
        proposal.clone(),
        members,
        llm_cfg,
    );

    let _ = council_tx.send(
        serde_json::json!({
            "type": "token",
            "content": format!(
                "Mode selected: LLM deliberation (complexity={:.2}, urgency={:.2})\n\n",
                proposal.complexity, proposal.urgency
            ),
        })
        .to_string(),
    );

    let decision_result = timeout(Duration::from_secs(300), council.evaluate()).await;
    match decision_result {
        Ok(Ok(decision)) => {
            let decision_text = match decision.decision.clone() {
                Decision::Approve => "approve".to_string(),
                Decision::Reject => "reject".to_string(),
                Decision::Amend { .. } => "amend".to_string(),
                Decision::Defer { .. } => "defer".to_string(),
            };
            let synthesis = format!(
                "Claim: Council decision is {decision_text}\nEvidence: [msg:llm_deliberation, edge:0]\n\
Claim: Confidence reported as {:.2}\nEvidence: [msg:llm_deliberation, edge:0]\n\
Claim: Participating agents count is {}\nEvidence: [msg:llm_deliberation, edge:0]",
                decision.confidence,
                decision.participating_agents.len(),
            );

            let _ = council_tx.send(
                serde_json::json!({
                    "type": "synthesis",
                    "content": synthesis,
                })
                .to_string(),
            );

            let llm_evidence = EvidenceIndex {
                msg_ids: std::collections::HashSet::from(["llm_deliberation".to_string()]),
                edge_ids: std::collections::HashSet::from([0usize]),
                msg_senders: std::collections::HashMap::new(),
                edge_participants: std::collections::HashMap::new(),
                msg_context: std::collections::HashMap::new(),
            };
            let verification = verify_semantic_contract(&synthesis, &llm_evidence, true);
            let (plan_text, plan_steps) = build_plan_data(&verification, &llm_evidence);
            let verified = verification.verified_text.clone();
            let primary_summary = choose_user_facing_summary(question, &synthesis, &verified);
            let verified_with_plan = render_council_synthesis_for_query(
                question,
                &primary_summary,
                &verification,
                &plan_text,
            );
            let mut conf = decision.confidence.clamp(0.0, 1.0);
            if !verification.ok {
                conf *= 0.3;
            }
            let _ = council_tx.send(
                serde_json::json!({
                    "type": "synthesis",
                    "content": verified_with_plan,
                })
                .to_string(),
            );
            persist_council_claims(
                roodb.clone(),
                question,
                &verification,
                conf,
                verification.coverage,
                "llm",
            )
            .await;
            let sharpened = verified.clone();
            let primary_sharpened = choose_user_facing_summary(question, &synthesis, &sharpened);
            let sharpened_with_plan = render_council_synthesis_for_query(
                question,
                &primary_sharpened,
                &verification,
                &plan_text,
            );
            let _ = bg_tx.send(BgEvent::CouncilSynthesis {
                question: question.to_string(),
                synthesis: sharpened_with_plan.clone(),
                confidence: conf,
                citations: Vec::new(),
                coverage: verification.coverage,
                plan_text: plan_text.clone(),
                plan_steps: plan_steps.clone(),
            });
            let (session_id, example_len) = {
                let mut session = council_session.lock().await;
                session.add_turn(TurnRole::Assistant, sharpened_with_plan.clone());
                (session.id(), session.to_optimization_examples().len())
            };
            if example_len > 0 {
                let _ = bg_tx.send(BgEvent::Log(format!(
                    "LLM deliberation session {} has {} optimization examples",
                    session_id, example_len
                )));
            }
        }
        Ok(Err(e)) => {
            let _ = council_tx.send(
                serde_json::json!({
                    "type": "error",
                    "content": format!("LLM deliberation failed: {}", e),
                })
                .to_string(),
            );
        }
        Err(_) => {
            let _ = council_tx.send(
                serde_json::json!({
                    "type": "error",
                    "content": "LLM deliberation timed out after 300 seconds",
                })
                .to_string(),
            );
        }
    }

    let _ = council_tx.send("{\"type\":\"done\"}".to_string());
}

// ── Listener helpers ──────────────────────────────────────────────────

fn parse_port_env(name: &str) -> Option<u16> {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
}

fn build_host_candidates(env_host: Option<String>) -> Vec<String> {
    let mut hosts = Vec::new();
    if let Some(host) = env_host.filter(|h| !h.is_empty()) {
        hosts.push(host);
    }
    for fallback in ["127.0.0.1", "0.0.0.0"] {
        if !hosts.iter().any(|existing| existing == fallback) {
            hosts.push(fallback.to_string());
        }
    }
    hosts
}

fn build_port_candidates(
    env_port: Option<u16>,
    fallback_port: Option<u16>,
    default_port: u16,
) -> Vec<u16> {
    let mut ports = Vec::new();
    if let Some(port) = env_port {
        ports.push(port);
    }
    if let Some(port) = fallback_port {
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    if !ports.contains(&default_port) {
        ports.push(default_port);
    }
    if !ports.contains(&0) {
        ports.push(0);
    }
    ports
}

async fn bind_with_retry(
    label: &str,
    hosts: &[String],
    ports: &[u16],
) -> Option<tokio::net::TcpListener> {
    for host in hosts {
        for port in ports {
            let addr = format!("{}:{}", host, port);
            match tokio::net::TcpListener::bind(&addr).await {
                Ok(listener) => {
                    let bound = listener
                        .local_addr()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|_| addr.clone());
                    eprintln!("[HSM-II] {} bound to {}", label, bound);
                    return Some(listener);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::AddrInUse
                    ) =>
                {
                    eprintln!(
                        "[HSM-II] {} cannot bind {}: {} (trying next address)",
                        label, addr, e
                    );
                    continue;
                }
                Err(e) => {
                    eprintln!("[HSM-II] {} fatal bind error {}: {}", label, addr, e);
                    return None;
                }
            }
        }
    }
    eprintln!(
        "[HSM-II] {} could not bind to any host/port combination",
        label
    );
    None
}

// ── Viz live-reload WebSocket server ───────────────────────────────────
//
// Serves ws://localhost:8788/ws.  Each connected browser client waits on
// the watch channel; when a new value arrives (after export) it gets a
// "reload" text frame and calls loadGraph() on its own.

async fn viz_ws_server(viz_rx: watch::Receiver<u64>) {
    use axum::{
        extract::{
            ws::{Message, WebSocket, WebSocketUpgrade},
            State,
        },
        routing::get,
        Router,
    };

    async fn ws_handler(
        ws: WebSocketUpgrade,
        State(rx): State<watch::Receiver<u64>>,
    ) -> impl axum::response::IntoResponse {
        ws.on_upgrade(move |socket| handle_socket(socket, rx))
    }

    async fn handle_socket(mut socket: WebSocket, mut rx: watch::Receiver<u64>) {
        // Send initial ack
        let _ = socket.send(Message::Text("connected".into())).await;
        loop {
            if rx.changed().await.is_err() {
                break; // sender dropped → app exited
            }
            if socket.send(Message::Text("reload".into())).await.is_err() {
                break; // client disconnected
            }
        }
    }

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(viz_rx)
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        );

    let viz_hosts = build_host_candidates(
        env::var("HSM_VIZ_HOST")
            .ok()
            .or_else(|| env::var("HSM_HOST").ok()),
    );
    let viz_ports = build_port_candidates(
        parse_port_env("HSM_VIZ_PORT"),
        parse_port_env("HSM_VIZ_FALLBACK_PORT"),
        8788,
    );
    let listener = match bind_with_retry("Viz WS server", &viz_hosts, &viz_ports).await {
        Some(l) => l,
        None => return,
    };

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("Viz WS server error: {}", e);
    }
}

// ── Shared types & helpers for web API + REPL ────────────────────────────

#[derive(Clone)]
struct ApiState {
    snapshot: Arc<RwLock<WorldSnapshot>>,
    chat_broadcast: ChatBroadcast,
    web_cmd_tx: mpsc::UnboundedSender<BgEvent>,
    last_context: Arc<RwLock<String>>,
    council_broadcast: ChatBroadcast,
    graph_activity_broadcast: GraphActivityBroadcast,
    council_mode: Arc<RwLock<String>>,
    code_broadcast: ChatBroadcast,
    optimize_broadcast: ChatBroadcast,
    visual_broadcast: ChatBroadcast,
    roodb: Arc<RwLock<Option<Arc<RooDb>>>>,
    roodb_url: String,
    embed_model: String,
    embed_client: Client,
    vault_dir: String,
    plan_steps: Arc<RwLock<Vec<PlanStep>>>,
    recent_rewards: Arc<RwLock<Vec<RewardLogRow>>>,
    recent_skill_evidence: Arc<RwLock<Vec<SkillEvidenceRow>>>,
}

#[derive(serde::Deserialize)]
struct VaultSearchRequest {
    query: String,
    top_k: Option<usize>,
}

#[derive(serde::Serialize)]
struct VaultSearchResult {
    id: String,
    label: String,
    score: f32,
    tags: Vec<String>,
    preview: String,
    path: String,
    note_type: Option<String>,
    metadata: serde_json::Value,
    evidence: Vec<String>,
    snippet: Option<String>,
    sources: Vec<String>,
}

#[derive(Clone)]
struct QmdHit {
    note_id: String,
    score: f32,
    snippet: String,
}

fn hash_content(text: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

async fn ollama_embed(client: &Client, model: &str, text: &str) -> anyhow::Result<Vec<f32>> {
    let url = "http://localhost:11434/api/embeddings";
    let payload = serde_json::json!({ "model": model, "prompt": text });
    let resp = client.post(url).json(&payload).send().await?;
    let value: serde_json::Value = resp.json().await?;
    let arr = value
        .get("embedding")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("no embedding in response"))?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        if let Some(f) = v.as_f64() {
            out.push(f as f32);
        }
    }
    Ok(out)
}

async fn run_qmd_query(query: &str, limit: usize) -> Result<Vec<QmdHit>, String> {
    let qmd_bin = env::var("HSM_QMD_BINARY").unwrap_or_else(|_| "qmd".to_string());
    let mut cmd = Command::new(qmd_bin);
    cmd.arg("query")
        .arg("--json")
        .arg("--rank")
        .arg("hybrid")
        .arg("--limit")
        .arg(limit.to_string())
        .arg(query);
    let output = cmd.output().await.map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    let mut hits = Vec::new();
    if let Some(results) = value.get("results").and_then(|v| v.as_array()) {
        for item in results {
            let note_id = item
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    item.get("path").and_then(|v| v.as_str()).and_then(|p| {
                        Path::new(p)
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .map(|s| s.to_string())
                    })
                })
                .unwrap_or_else(|| "".to_string());
            if note_id.is_empty() {
                continue;
            }
            let score = item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let snippet = item
                .get("snippet")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            hits.push(QmdHit {
                note_id,
                score,
                snippet,
            });
        }
    }
    Ok(hits)
}

async fn index_vault_embeddings(
    s: &ApiState,
) -> Result<(usize, usize, usize, Vec<String>), String> {
    let db = match ensure_roodb(s).await {
        Ok(db) => db,
        Err(err) => return Err(err),
    };

    let vault_dir = std::path::PathBuf::from(&s.vault_dir);
    let notes = vault::scan_vault(&vault_dir).map_err(|e| e.to_string())?;

    let existing = db.fetch_vault_embeddings().await.unwrap_or_default();
    let mut existing_map: HashMap<String, String> = HashMap::new();
    for row in existing {
        existing_map.insert(row.note_id, row.content_hash);
    }

    let mut embedded = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();
    for note in notes.iter() {
        let combined = format!(
            "{}\n{}\n{}",
            note.title,
            note.tags.join(" "),
            note.search_text
        );
        let hash = hash_content(&combined);
        if let Some(prev) = existing_map.get(&note.id) {
            if prev == &hash {
                skipped += 1;
                continue;
            }
        }
        match ollama_embed(&s.embed_client, &s.embed_model, &combined).await {
            Ok(embedding) => {
                let metadata = serde_json::json!({
                    "type": note.note_type,
                    "template": note.template,
                    "properties": note.properties,
                    "attachments": note.attachments,
                });
                let row = VaultEmbeddingRow {
                    note_id: note.id.clone(),
                    title: note.title.clone(),
                    tags: note.tags.clone(),
                    path: note.path.display().to_string(),
                    preview: note.preview.clone(),
                    content_hash: hash,
                    metadata,
                    embedding,
                    updated_at: unix_timestamp_secs(),
                };
                if let Err(e) = db.upsert_vault_embedding(&row).await {
                    errors.push(format!("{}: {}", note.id, e));
                } else {
                    embedded += 1;
                }
            }
            Err(e) => {
                errors.push(format!("{}: {}", note.id, e));
            }
        }
    }

    Ok((notes.len(), embedded, skipped, errors))
}

async fn ensure_roodb(s: &ApiState) -> Result<Arc<RooDb>, String> {
    if let Some(db) = s.roodb.read().await.clone() {
        return Ok(db);
    }
    let url = s.roodb_url.clone();
    let config = RooDbConfig::from_url(&url);
    let db = RooDb::new(&config);
    let init_result = tokio::time::timeout(Duration::from_secs(5), async {
        db.ping().await?;
        db.init_schema().await?;
        Ok::<_, anyhow::Error>(db)
    })
    .await;
    match init_result {
        Ok(Ok(db)) => {
            let db = Arc::new(db);
            if let Ok(mut slot) = s.roodb.try_write() {
                *slot = Some(db.clone());
            }
            Ok(db)
        }
        Ok(Err(e)) => Err(format!("roodb init failed: {}", e)),
        Err(_) => Err("roodb connection timed out after 5s".to_string()),
    }
}

// ── Web API server :8787 ────────────────────────────────────────────────
//
// Serves the full Studio UI and a JSON/WebSocket API:
//   GET  /                    → viz/index.html
//   GET  /api/state           → WorldSnapshot as JSON
//   GET  /api/context         → last grounded context block
//   WS   /api/chat            → bidirectional: send text, receive streamed tokens
//   POST /api/command         → execute slash command, return JSON result
//   WS   /api/council         → receive council token stream
//   GET  /viz/*               → static files from viz/

async fn web_api_server(state: WebApiState) {
    use axum::{
        extract::{
            ws::{Message, WebSocket, WebSocketUpgrade},
            Path as AxumPath, State,
        },
        http::StatusCode,
        response::{IntoResponse, Json as AxumJson},
        routing::{get, post},
        Router,
    };
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;
    use tower_http::services::ServeDir;

    let api_state = ApiState {
        snapshot: state.snapshot,
        visual_broadcast: state.visual_broadcast,
        chat_broadcast: state.chat_broadcast,
        web_cmd_tx: state.web_cmd_tx,
        last_context: state.last_context,
        council_broadcast: state.council_broadcast,
        graph_activity_broadcast: state.graph_activity_broadcast,
        council_mode: state.council_mode,
        code_broadcast: state.code_broadcast,
        optimize_broadcast: state.optimize_broadcast,
        roodb: state.roodb,
        roodb_url: state.roodb_url,
        embed_model: state.embed_model,
        embed_client: state.embed_client,
        vault_dir: state.vault_dir,
        plan_steps: state.plan_steps.clone(),
        recent_rewards: state.recent_rewards.clone(),
        recent_skill_evidence: state.recent_skill_evidence.clone(),
    };

    // GET /api/state
    async fn handle_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await.clone();
        AxumJson(snap)
    }

    // GET /api/context
    async fn handle_context(State(s): State<ApiState>) -> impl IntoResponse {
        let ctx = s.last_context.read().await.clone();
        AxumJson(json!({ "context": ctx }))
    }

    // GET /api/chat/context - Chat context usage stats
    async fn handle_chat_context(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        let ctx = &snap.chat_context;
        AxumJson(json!({
            "percent": ctx.percent_used,
            "message_count": ctx.message_count,
            "estimated_tokens": ctx.estimated_tokens,
            "limit_tokens": ctx.limit_tokens,
            "regular": ctx.regular_tokens,
            "cache_read": ctx.cache_read_tokens,
            "cache_write": ctx.cache_write_tokens,
            "total": ctx.estimated_tokens,
            "total_formatted": format!("{:.0}K", ctx.estimated_tokens as f32 / 1000.0),
            "limit_formatted": format!("{:.0}K", ctx.limit_tokens as f32 / 1000.0),
            "has_summary": ctx.has_summary,
            "dag_info": {
                "total_nodes": ctx.dag_info.total_nodes,
                "summary_nodes": ctx.dag_info.summary_nodes,
                "large_file_nodes": ctx.dag_info.large_file_nodes,
                "max_depth": ctx.dag_info.max_depth,
            },
        }))
    }

    // POST /api/command  body: { "cmd": "/tick 5" }
    async fn handle_command(
        State(s): State<ApiState>,
        axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
    ) -> impl IntoResponse {
        let cmd = body
            .get("cmd")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if cmd.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                AxumJson(json!({"error":"missing cmd"})),
            )
                .into_response();
        }

        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<String>();
        let _ = s.web_cmd_tx.send(BgEvent::WebCommand { cmd, resp_tx });

        match tokio::time::timeout(Duration::from_secs(10), resp_rx).await {
            Ok(Ok(result)) => AxumJson(json!({"result": result})).into_response(),
            _ => (
                StatusCode::GATEWAY_TIMEOUT,
                AxumJson(json!({"error":"timeout"})),
            )
                .into_response(),
        }
    }

    // POST /api/message  body: { "sender": 1, "target": "agent:2", "kind": "task", "content": "..." }
    async fn handle_message(
        State(s): State<ApiState>,
        axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
    ) -> impl IntoResponse {
        let sender = body.get("sender").and_then(|v| v.as_u64()).unwrap_or(0);
        let target = body
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("broadcast")
            .to_string();
        let kind = body
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("note")
            .to_string();
        let content = body
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let _ = s.web_cmd_tx.send(BgEvent::InjectMessage {
            sender,
            target,
            kind,
            content,
        });
        // Forward to hypergraphd for UI message log if configured
        let forward_base =
            env::var("HSM_HYPERGRAPH_URL").unwrap_or_else(|_| "http://127.0.0.1:8787".to_string());
        let forward_url = format!("{}/api/message", forward_base.trim_end_matches('/'));
        let _ = reqwest::Client::new()
            .post(forward_url)
            .json(&body)
            .send()
            .await;
        (StatusCode::OK, AxumJson(json!({ "ok": true })))
    }

    // WS /api/chat — bidirectional chat with streaming tokens
    async fn handle_chat_ws(ws: WebSocketUpgrade, State(s): State<ApiState>) -> impl IntoResponse {
        ws.on_upgrade(move |socket| chat_socket(socket, s))
    }

    async fn chat_socket(mut socket: WebSocket, s: ApiState) {
        use tokio::time::{interval, Duration};

        // Subscribe to chat token broadcast BEFORE we start
        let mut token_rx = s.chat_broadcast.subscribe();

        // Setup keepalive ping every 30 seconds (don't fire immediately)
        let mut keepalive = interval(Duration::from_secs(30));
        keepalive.tick().await; // Skip the immediate first tick

        // Send connected confirmation
        let connect_msg = json!({
            "type": "connected",
            "message": "Chat WebSocket connected"
        })
        .to_string();
        if socket.send(Message::Text(connect_msg)).await.is_err() {
            return;
        }

        loop {
            tokio::select! {
                // Incoming message from browser
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            let parsed = serde_json::from_str::<serde_json::Value>(&text).ok();
                            let chat_text = parsed.as_ref()
                                .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
                                .unwrap_or(text.to_string());
                            let chat_model = parsed.as_ref()
                                .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(|s| s.to_string()));

                            let (resp_tx, _resp_rx) = tokio::sync::oneshot::channel::<()>();
                            let _ = s.web_cmd_tx.send(BgEvent::WebChat {
                                text: chat_text,
                                model: chat_model,
                                resp_tx: Some(resp_tx),
                            });
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = socket.send(Message::Pong(data)).await;
                        }
                        Some(Ok(Message::Pong(_))) => {}
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(_)) => {}
                        Some(Err(_)) => break,
                    }
                }
                // Outgoing token to browser
                token = token_rx.recv() => {
                    match token {
                        Ok(t) => {
                            if socket.send(Message::Text(t)).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
                // Keepalive ping
                _ = keepalive.tick() => {
                    let _ = socket.send(Message::Ping(vec![])).await;
                }
            }
        }
    }

    // WS /api/council — subscribe to Socratic council stream
    async fn handle_council_ws(
        ws: WebSocketUpgrade,
        State(s): State<ApiState>,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |socket| council_socket(socket, s))
    }

    async fn council_socket(mut socket: WebSocket, s: ApiState) {
        use tokio::time::{interval, Duration};
        let mut rx = s.council_broadcast.subscribe();
        let mut keepalive = interval(Duration::from_secs(30));

        // Send connected confirmation
        let _ = socket
            .send(Message::Text(
                json!({
                    "type": "connected",
                    "message": "Council WebSocket connected"
                })
                .to_string(),
            ))
            .await;

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Ok(m) => {
                            if socket.send(Message::Text(m)).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
                _ = keepalive.tick() => {
                    if socket.send(Message::Ping(vec![])).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    // WS /api/graph-activity — real-time graph visualization updates
    async fn handle_graph_activity_ws(
        ws: WebSocketUpgrade,
        State(s): State<ApiState>,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |socket| graph_activity_socket(socket, s))
    }

    async fn graph_activity_socket(mut socket: WebSocket, s: ApiState) {
        let mut rx = s.graph_activity_broadcast.subscribe();
        // Send initial connection message
        let _ = socket
            .send(Message::Text(
                json!({
                    "type": "connected",
                    "content": "Graph activity feed started"
                })
                .to_string(),
            ))
            .await;

        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if socket.send(Message::Text(msg)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    }

    // WS /api/code — Coder Agent with tool execution
    async fn handle_code_ws(ws: WebSocketUpgrade, State(s): State<ApiState>) -> impl IntoResponse {
        ws.on_upgrade(move |socket| code_socket(socket, s))
    }

    async fn code_socket(mut socket: WebSocket, s: ApiState) {
        let mut code_rx = s.code_broadcast.subscribe();

        loop {
            tokio::select! {
                // Incoming message from browser
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            // Parse JSON payload { "query": "...", "model": "..." }
                            let parsed = serde_json::from_str::<serde_json::Value>(&text).ok();
                            let query = parsed.as_ref()
                                .and_then(|v| v.get("query").and_then(|t| t.as_str()).map(|s| s.to_string()))
                                .unwrap_or(text.to_string());
                            let model = parsed.as_ref()
                                .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(|s| s.to_string()))
                                .unwrap_or_else(|| "hf.co/mradermacher/Qwen3-8B-heretic-GGUF:Q6_K".to_string());

                            // Send to background for processing
                            let _ = s.web_cmd_tx.send(BgEvent::CodeAgent {
                                query,
                                model,
                            });
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    }
                }
                // Outgoing code agent events to browser
                event = code_rx.recv() => {
                    match event {
                        Ok(msg) => {
                            if socket.send(Message::Text(msg)).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            }
        }
    }

    // WS /api/visual — Visual explainer progress updates
    async fn handle_visual_ws(
        ws: WebSocketUpgrade,
        State(s): State<ApiState>,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |socket| visual_socket(socket, s))
    }

    async fn visual_socket(mut socket: WebSocket, s: ApiState) {
        let mut visual_rx = s.visual_broadcast.subscribe();

        // Send connected message
        let _ = socket
            .send(Message::Text(
                serde_json::json!({
                    "type": "connected",
                    "content": "Visual explainer progress feed connected"
                })
                .to_string(),
            ))
            .await;

        loop {
            match visual_rx.recv().await {
                Ok(msg) => {
                    if socket.send(Message::Text(msg)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    }

    // POST /api/council  body: { "question": "..." }
    // POST /api/chat - Send a chat message (non-WebSocket)
    async fn handle_chat_post(
        State(s): State<ApiState>,
        axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
    ) -> impl IntoResponse {
        eprintln!("[HTTP] POST /api/chat - Received: {:?}", body);

        let text = body
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if text.is_empty() {
            eprintln!("[HTTP] POST /api/chat - Rejected: empty text");
            return (
                StatusCode::BAD_REQUEST,
                AxumJson(json!({"error":"missing text"})),
            )
                .into_response();
        }

        eprintln!(
            "[HTTP] POST /api/chat - Starting chat with text: {}",
            text.chars().take(50).collect::<String>()
        );

        // Send to background - WebChat doesn't need resp_tx for async
        let _ = s.web_cmd_tx.send(BgEvent::WebChat {
            text,
            model,
            resp_tx: None,
        });

        AxumJson(json!({"status":"chat started"})).into_response()
    }

    async fn handle_council_post(
        State(s): State<ApiState>,
        axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
    ) -> impl IntoResponse {
        eprintln!("[HTTP] POST /api/council - Received: {:?}", body);

        let question = body
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mode = body
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();

        if question.is_empty() {
            eprintln!("[HTTP] POST /api/council - Rejected: empty question");
            return (
                StatusCode::BAD_REQUEST,
                AxumJson(json!({"error":"missing question"})),
            )
                .into_response();
        }

        eprintln!(
            "[HTTP] POST /api/council - Starting council with question: {}",
            question.chars().take(50).collect::<String>()
        );

        // Update stored council mode
        {
            let mut stored_mode = s.council_mode.write().await;
            *stored_mode = mode.clone();
        }

        let _ = s.web_cmd_tx.send(BgEvent::CouncilRequest {
            question: question.clone(),
            mode,
        });
        eprintln!("[HTTP] POST /api/council - Sent to background task");
        AxumJson(json!({"status":"council started","question":question})).into_response()
    }

    // GET /api/components/council - Council system state
    async fn handle_council_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        AxumJson(json!({
            "council": snap.council,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        }))
    }

    // GET /api/components/dks - Dynamic Kinetic Stability state
    async fn handle_dks_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        AxumJson(json!({
            "dks": snap.dks,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        }))
    }

    // GET /api/components/cass - Context-Aware Semantic Skills state
    async fn handle_cass_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        AxumJson(json!({
            "cass": snap.cass,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        }))
    }

    // GET /api/components/skills - SkillRL skill bank state
    async fn handle_skills_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        AxumJson(json!({
            "skills": snap.skills,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        }))
    }

    // GET /api/components/navigation - Code navigation state
    async fn handle_navigation_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        AxumJson(json!({
            "navigation": snap.navigation,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        }))
    }

    // GET /api/skills/evidence - recent skill evidence bindings
    async fn handle_skill_evidence(State(s): State<ApiState>) -> impl IntoResponse {
        let mut source = "roodb";
        let rows = match ensure_roodb(&s).await {
            Ok(db) => {
                let rows = db.fetch_skill_evidence(100).await.unwrap_or_default();
                let mut cache = s.recent_skill_evidence.write().await;
                *cache = rows.clone();
                rows
            }
            Err(err) => {
                source = "fallback";
                eprintln!("[HSM-II] skill evidence fallback: {}", err);
                s.recent_skill_evidence.read().await.clone()
            }
        };
        (
            StatusCode::OK,
            AxumJson(json!({
                "ok": true,
                "rows": rows,
                "source": source,
            })),
        )
            .into_response()
    }

    // GET /api/rewards - recent reward logs
    async fn handle_rewards(State(s): State<ApiState>) -> impl IntoResponse {
        let mut source = "roodb";
        let rows = match ensure_roodb(&s).await {
            Ok(db) => {
                let rows = db.fetch_reward_logs(200).await.unwrap_or_default();
                let mut cache = s.recent_rewards.write().await;
                *cache = rows.clone();
                rows
            }
            Err(err) => {
                source = "fallback";
                eprintln!("[HSM-II] rewards fallback: {}", err);
                s.recent_rewards.read().await.clone()
            }
        };
        (
            StatusCode::OK,
            AxumJson(json!({
                "ok": true,
                "rows": rows,
                "source": source,
            })),
        )
            .into_response()
    }

    // GET /api/ouroboros/gate-audits?limit=200 - compatibility gate decisions
    async fn handle_ouroboros_gate_audits(
        State(s): State<ApiState>,
        axum::extract::Query(params): axum::extract::Query<
            std::collections::HashMap<String, String>,
        >,
    ) -> impl IntoResponse {
        let limit = params
            .get("limit")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(200)
            .clamp(1, 1000);
        let mut source = "roodb";
        let rows = match ensure_roodb(&s).await {
            Ok(db) => match db.fetch_ouroboros_gate_audits(limit).await {
                Ok(rows) => rows,
                Err(err) => {
                    source = "fallback";
                    eprintln!("[HSM-II] gate audits fallback: {}", err);
                    Vec::new()
                }
            },
            Err(err) => {
                source = "fallback";
                eprintln!("[HSM-II] gate audits fallback: {}", err);
                Vec::new()
            }
        };
        (
            StatusCode::OK,
            AxumJson(json!({
                "ok": true,
                "rows": rows,
                "source": source,
                "limit": limit,
            })),
        )
            .into_response()
    }

    // GET /api/ouroboros/memory-events?limit=200 - compatibility memory event stream
    async fn handle_ouroboros_memory_events(
        State(s): State<ApiState>,
        axum::extract::Query(params): axum::extract::Query<
            std::collections::HashMap<String, String>,
        >,
    ) -> impl IntoResponse {
        let limit = params
            .get("limit")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(200)
            .clamp(1, 1000);
        let mut source = "roodb";
        let rows = match ensure_roodb(&s).await {
            Ok(db) => match db.fetch_ouroboros_memory_events(limit).await {
                Ok(rows) => rows,
                Err(err) => {
                    source = "fallback";
                    eprintln!("[HSM-II] memory events fallback: {}", err);
                    Vec::new()
                }
            },
            Err(err) => {
                source = "fallback";
                eprintln!("[HSM-II] memory events fallback: {}", err);
                Vec::new()
            }
        };
        (
            StatusCode::OK,
            AxumJson(json!({
                "ok": true,
                "rows": rows,
                "source": source,
                "limit": limit,
            })),
        )
            .into_response()
    }

    // GET /api/messages - recent inter-agent messages
    async fn handle_messages(State(s): State<ApiState>) -> impl IntoResponse {
        let db = match ensure_roodb(&s).await {
            Ok(db) => db,
            Err(_err) => {
                // Fallback: return empty list (RooDB not connected yet)
                return AxumJson(json!([])).into_response();
            }
        };
        let rows = db.fetch_messages(200).await.unwrap_or_default();
        let out: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.msg_id,
                    "sender": r.sender,
                    "target": r.target,
                    "kind": r.kind,
                    "content": r.content,
                    "ts": r.created_at,
                })
            })
            .collect();
        AxumJson(json!(out)).into_response()
    }

    // GET /api/plan-steps - current plan steps from last council synthesis
    async fn handle_plan_steps_get(State(s): State<ApiState>) -> impl IntoResponse {
        let steps = s.plan_steps.read().await.clone();
        if !steps.is_empty() {
            return AxumJson(json!({ "ok": true, "source": "live", "steps": steps }))
                .into_response();
        }
        // Fallback: load from RooDB for audit/replay after restart
        if let Some(ref db) = *s.roodb.read().await {
            if let Ok(rows) = db.fetch_plan_steps(50).await {
                if !rows.is_empty() {
                    return AxumJson(json!({ "ok": true, "source": "roodb", "count": rows.len(), "steps": rows })).into_response();
                }
            }
        }
        let empty: Vec<serde_json::Value> = vec![];
        AxumJson(json!({ "ok": true, "source": "empty", "steps": empty })).into_response()
    }

    // GET /api/components/communication - Communication hub state
    async fn handle_communication_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        AxumJson(json!({
            "communication": snap.communication,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        }))
    }

    // GET /api/components/gpu - GPU acceleration status
    async fn handle_gpu_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        AxumJson(json!({
            "gpu": snap.gpu,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        }))
    }

    // GET /api/components/llm - LLM inference status
    async fn handle_llm_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        AxumJson(json!({
            "llm": snap.llm,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        }))
    }

    // GET /api/components/email - Email agent status
    async fn handle_email_state(State(s): State<ApiState>) -> impl IntoResponse {
        let snap = s.snapshot.read().await;
        AxumJson(json!({
            "email": snap.email,
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        }))
    }

    // POST /api/optimize  body: OptimizationRequest JSON
    async fn handle_optimize_post(
        State(s): State<ApiState>,
        axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
    ) -> impl IntoResponse {
        if body.get("artifact").is_none() {
            return (
                StatusCode::BAD_REQUEST,
                AxumJson(json!({"error":"missing 'artifact' field"})),
            )
                .into_response();
        }
        let _ = s.web_cmd_tx.send(BgEvent::OptimizeRequest { body });
        AxumJson(json!({"status":"optimization started"})).into_response()
    }

    #[derive(Deserialize)]
    struct PlanWorkflowRequest {
        step_index: usize,
    }

    async fn handle_plan_workflow(
        State(s): State<ApiState>,
        AxumJson(req): AxumJson<PlanWorkflowRequest>,
    ) -> impl IntoResponse {
        let _ = s.web_cmd_tx.send(BgEvent::PlanWorkflow {
            step_index: req.step_index,
        });
        AxumJson(json!({ "ok": true, "step_index": req.step_index }))
    }

    // POST /api/plan/optimize — trigger OptimizeAnything for a plan step
    async fn handle_plan_optimize(
        State(s): State<ApiState>,
        AxumJson(req): AxumJson<PlanWorkflowRequest>,
    ) -> impl IntoResponse {
        let _ = s.web_cmd_tx.send(BgEvent::PlanOptimize {
            step_index: req.step_index,
        });
        AxumJson(json!({ "ok": true, "step_index": req.step_index, "action": "optimize" }))
    }

    // GET /api/plan/history — fetch persisted plan steps from RooDB for audit/replay
    async fn handle_plan_history(State(s): State<ApiState>) -> impl IntoResponse {
        if let Some(ref db) = *s.roodb.read().await {
            match db.fetch_plan_steps(200).await {
                Ok(rows) => AxumJson(json!({ "ok": true, "count": rows.len(), "steps": rows }))
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(json!({ "ok": false, "error": format!("{}", e) })),
                )
                    .into_response(),
            }
        } else {
            AxumJson(json!({ "ok": false, "error": "RooDB not configured" })).into_response()
        }
    }

    // GET /api/hires — delegation hire tree records from RooDB
    async fn handle_hires_get(State(s): State<ApiState>) -> impl IntoResponse {
        if let Some(ref db) = *s.roodb.read().await {
            match db.fetch_skill_hires(200).await {
                Ok(rows) => {
                    // Group hires by plan_step_index to reconstruct trees
                    let mut trees: std::collections::HashMap<usize, Vec<&SkillHireRow>> =
                        std::collections::HashMap::new();
                    for row in &rows {
                        trees.entry(row.plan_step_index).or_default().push(row);
                    }

                    let tree_summaries: Vec<serde_json::Value> = trees
                        .iter()
                        .map(|(step_idx, hires)| {
                            let max_depth = hires.iter().map(|h| h.depth).max().unwrap_or(0);
                            json!({
                                "plan_step_index": step_idx,
                                "hire_count": hires.len(),
                                "max_depth": max_depth,
                                "hires": hires.iter().map(|h| json!({
                                    "hire_id": h.hire_id,
                                    "parent": h.parent_skill_id,
                                    "child": h.child_skill_id,
                                    "depth": h.depth,
                                    "budget": h.budget,
                                    "status": h.status,
                                    "outcome_score": h.outcome_score,
                                    "briefing_size": h.skill_briefing.len(),
                                    "domains": h.subproblem_domains,
                                    "signature_id": h.signature_id,
                                })).collect::<Vec<_>>(),
                            })
                        })
                        .collect();

                    AxumJson(json!({
                        "ok": true,
                        "total_hires": rows.len(),
                        "tree_count": trees.len(),
                        "trees": tree_summaries,
                    }))
                    .into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(json!({ "ok": false, "error": format!("{}", e) })),
                )
                    .into_response(),
            }
        } else {
            AxumJson(json!({ "ok": true, "total_hires": 0, "tree_count": 0, "trees": [], "note": "RooDB not configured" })).into_response()
        }
    }

    // POST /api/skills/curation — promote or add curated skills
    // body: { "action": "promote", "skill_id": "...", "promoted_by": "..." }
    //    or: { "action": "add_curated", "title": "...", "principle": "...", "curator": "...", "domain": "..." }
    async fn handle_skill_curation_post(
        State(s): State<ApiState>,
        axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
    ) -> impl IntoResponse {
        let action = body
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if action.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                AxumJson(json!({"ok": false, "error": "missing action"})),
            )
                .into_response();
        }

        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<String>();
        let _ = s.web_cmd_tx.send(BgEvent::SkillCurate {
            action,
            body,
            resp_tx,
        });

        match tokio::time::timeout(Duration::from_secs(5), resp_rx).await {
            Ok(Ok(result)) => {
                let ok = result.starts_with("ok:");
                AxumJson(json!({"ok": ok, "result": result})).into_response()
            }
            _ => (
                StatusCode::GATEWAY_TIMEOUT,
                AxumJson(json!({"ok": false, "error": "timeout"})),
            )
                .into_response(),
        }
    }

    // POST /api/dspy/optimize — trigger DSPy signature optimization
    // body: { "signature_name": "chat_draft" } or {} for all eligible
    async fn handle_dspy_optimize_post(
        State(s): State<ApiState>,
        axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
    ) -> impl IntoResponse {
        let sig_name = body
            .get("signature_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<String>();
        let _ = s.web_cmd_tx.send(BgEvent::DspyOptimize {
            signature_name: sig_name,
            resp_tx,
        });
        match tokio::time::timeout(Duration::from_secs(120), resp_rx).await {
            Ok(Ok(result)) => {
                let ok = result.starts_with("ok:");
                AxumJson(json!({"ok": ok, "result": result})).into_response()
            }
            _ => (
                StatusCode::GATEWAY_TIMEOUT,
                AxumJson(json!({"ok": false, "error": "optimization timed out"})),
            )
                .into_response(),
        }
    }

    // GET /api/dspy/traces?signature_name=chat_draft&limit=50
    async fn handle_dspy_traces_get(
        State(s): State<ApiState>,
        axum::extract::Query(params): axum::extract::Query<
            std::collections::HashMap<String, String>,
        >,
    ) -> impl IntoResponse {
        if let Some(ref db) = *s.roodb.read().await {
            let sig_name = params
                .get("signature_name")
                .map(|s| s.as_str())
                .unwrap_or("*");
            let limit = params
                .get("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(50);
            if sig_name == "*" {
                // List all signature names with trace counts
                match db.list_dspy_signature_names().await {
                    Ok(names) => AxumJson(json!({"ok": true, "signatures": names})).into_response(),
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        AxumJson(json!({"ok": false, "error": format!("{}", e)})),
                    )
                        .into_response(),
                }
            } else {
                match db.fetch_dspy_traces(sig_name, 0.0, limit).await {
                    Ok(rows) => {
                        let traces: Vec<serde_json::Value> = rows.iter().map(|r| json!({
                            "id": r.id,
                            "signature_name": r.signature_name,
                            "question": r.input_question,
                            "output_preview": if r.output.len() > 200 { format!("{}…", &r.output[..200]) } else { r.output.clone() },
                            "score": r.score,
                            "semantic_ok": r.semantic_ok,
                            "repair_count": r.repair_count,
                            "model": r.model,
                            "latency_ms": r.latency_ms,
                            "created_at": r.created_at,
                        })).collect();
                        AxumJson(json!({"ok": true, "count": traces.len(), "traces": traces}))
                            .into_response()
                    }
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        AxumJson(json!({"ok": false, "error": format!("{}", e)})),
                    )
                        .into_response(),
                }
            }
        } else {
            AxumJson(json!({"ok": true, "count": 0, "traces": [], "note": "RooDB not configured"}))
                .into_response()
        }
    }

    // GET /api/dspy/demos?signature_name=chat_draft
    async fn handle_dspy_demos_get(
        State(s): State<ApiState>,
        axum::extract::Query(params): axum::extract::Query<
            std::collections::HashMap<String, String>,
        >,
    ) -> impl IntoResponse {
        if let Some(ref db) = *s.roodb.read().await {
            let sig_name = params
                .get("signature_name")
                .map(|s| s.as_str())
                .unwrap_or("");
            if sig_name.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    AxumJson(json!({"ok": false, "error": "signature_name required"})),
                )
                    .into_response();
            }
            match db.fetch_dspy_demonstrations(sig_name, 50).await {
                Ok(rows) => {
                    let demos: Vec<serde_json::Value> = rows.iter().map(|r| json!({
                        "id": r.id,
                        "signature_name": r.signature_name,
                        "input_summary": r.input_summary,
                        "output_preview": if r.output.len() > 200 { format!("{}…", &r.output[..200]) } else { r.output.clone() },
                        "score": r.score,
                        "source": r.source,
                        "active": r.active,
                        "created_at": r.created_at,
                    })).collect();
                    AxumJson(json!({"ok": true, "count": demos.len(), "demos": demos}))
                        .into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(json!({"ok": false, "error": format!("{}", e)})),
                )
                    .into_response(),
            }
        } else {
            AxumJson(json!({"ok": true, "count": 0, "demos": [], "note": "RooDB not configured"}))
                .into_response()
        }
    }

    // POST /api/hires/complete — mark a hire as completed or failed
    // body: { "hire_id": "...", "status": "completed"|"failed", "outcome_score": 0.85 }
    async fn handle_hire_complete_post(
        State(s): State<ApiState>,
        axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
    ) -> impl IntoResponse {
        let hire_id = body
            .get("hire_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = body
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("completed")
            .to_string();
        let outcome_score = body
            .get("outcome_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);

        if hire_id.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                AxumJson(json!({"ok": false, "error": "missing hire_id"})),
            )
                .into_response();
        }

        let _ = s.web_cmd_tx.send(BgEvent::HireComplete {
            hire_id: hire_id.clone(),
            status: status.clone(),
            outcome_score,
            completed_at: unix_timestamp_secs(),
        });
        AxumJson(json!({"ok": true, "hire_id": hire_id, "status": status, "outcome_score": outcome_score})).into_response()
    }

    // WS /api/optimize — subscribe to optimization event stream
    async fn handle_optimize_ws(
        ws: WebSocketUpgrade,
        State(s): State<ApiState>,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |socket| optimize_socket(socket, s))
    }

    async fn optimize_socket(mut socket: WebSocket, s: ApiState) {
        let mut rx = s.optimize_broadcast.subscribe();
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if socket.send(Message::Text(msg)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    }

    // POST /api/visual — generate visual explanation
    #[derive(serde::Deserialize)]
    struct VisualExplainerRequest {
        diagram_type: String,
        title: String,
        content: String,
        data: Option<serde_json::Value>,
        #[serde(default = "default_open_browser")]
        open_browser: bool,
    }
    fn default_open_browser() -> bool {
        true
    }

    async fn handle_visual_post(
        State(s): State<ApiState>,
        axum::extract::Json(req): axum::extract::Json<VisualExplainerRequest>,
    ) -> impl IntoResponse {
        let _ = s.web_cmd_tx.send(BgEvent::VisualExplainer {
            diagram_type: req.diagram_type,
            title: req.title,
            content: req.content,
            data: req.data,
            open_browser: req.open_browser,
        });
        AxumJson(json!({"status":"visual explainer started"}))
    }

    // GET /api/visual — list generated visualizations (JSON files)
    async fn handle_visual_list() -> impl IntoResponse {
        let output_dir = std::path::PathBuf::from("visual-explainer/output");
        // Ensure directory exists
        std::fs::create_dir_all(&output_dir).ok();

        let files: Vec<serde_json::Value> = std::fs::read_dir(&output_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|ext| ext == "json")
                            .unwrap_or(false)
                    })
                    .filter_map(|e| {
                        let path = e.path();
                        let metadata = e.metadata().ok()?;
                        let name = path.file_name()?.to_string_lossy().to_string();
                        let modified = metadata.modified().ok()?;
                        let secs = modified
                            .duration_since(std::time::UNIX_EPOCH)
                            .ok()?
                            .as_secs();

                        // Try to read the JSON to get title and type
                        let mut title = name.clone();
                        let mut viz_type = "unknown".to_string();
                        let mut format = "unknown".to_string();

                        if let Ok(content) = std::fs::read_to_string(&path) {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                                if let Some(t) = json.get("title").and_then(|v| v.as_str()) {
                                    title = t.to_string();
                                }
                                if let Some(t) = json.get("type").and_then(|v| v.as_str()) {
                                    viz_type = t.to_string();
                                }
                                if let Some(f) = json.get("format").and_then(|v| v.as_str()) {
                                    format = f.to_string();
                                }
                            }
                        }

                        Some(json!({
                            "name": name,
                            "title": title,
                            "type": viz_type,
                            "format": format,
                            "modified": secs,
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default();

        AxumJson(json!({"files": files, "count": files.len()}))
    }

    // GET /api/visual/file/:filename — serve a specific visualization JSON
    async fn handle_visual_file(AxumPath(filename): AxumPath<String>) -> impl IntoResponse {
        let output_dir = std::path::PathBuf::from("visual-explainer/output");
        let file_path = output_dir.join(&filename);

        // Security: ensure the path is within the output directory
        let canonical_path = match std::fs::canonicalize(&file_path) {
            Ok(p) => p,
            Err(_) => {
                return (
                    StatusCode::NOT_FOUND,
                    AxumJson(json!({"error": "File not found"})),
                )
                    .into_response()
            }
        };
        let canonical_output = match std::fs::canonicalize(&output_dir) {
            Ok(p) => p,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(json!({"error": "Server error"})),
                )
                    .into_response()
            }
        };

        if !canonical_path.starts_with(&canonical_output) {
            return (
                StatusCode::FORBIDDEN,
                AxumJson(json!({"error": "Access denied"})),
            )
                .into_response();
        }

        // Read and parse the JSON file
        match std::fs::read_to_string(&canonical_path) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(json) => AxumJson(json).into_response(),
                Err(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(json!({"error": "Invalid JSON"})),
                )
                    .into_response(),
            },
            Err(_) => (
                StatusCode::NOT_FOUND,
                AxumJson(json!({"error": "File not found"})),
            )
                .into_response(),
        }
    }

    // DELETE /api/visual/file/:filename — delete a visualization
    async fn handle_visual_delete(AxumPath(filename): AxumPath<String>) -> impl IntoResponse {
        let output_dir = std::path::PathBuf::from("visual-explainer/output");
        let file_path = output_dir.join(&filename);

        // Security: ensure the path is within the output directory
        let canonical_output = match std::fs::canonicalize(&output_dir) {
            Ok(p) => p,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(json!({"error": "Server error"})),
                )
                    .into_response()
            }
        };
        let canonical_path = match std::fs::canonicalize(&file_path) {
            Ok(p) => p,
            Err(_) => {
                return (
                    StatusCode::NOT_FOUND,
                    AxumJson(json!({"error": "File not found"})),
                )
                    .into_response()
            }
        };

        if !canonical_path.starts_with(&canonical_output) {
            return (
                StatusCode::FORBIDDEN,
                AxumJson(json!({"error": "Access denied"})),
            )
                .into_response();
        }

        match std::fs::remove_file(&canonical_path) {
            Ok(_) => AxumJson(json!({"ok": true, "deleted": filename})).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                AxumJson(json!({"error": format!("Failed to delete: {}", e)})),
            )
                .into_response(),
        }
    }

    // POST /api/vault/index — embed and persist vault notes
    async fn handle_vault_index(State(s): State<ApiState>) -> impl IntoResponse {
        match index_vault_embeddings(&s).await {
            Ok((total, embedded, skipped, errors)) => {
                // ok=true if at least some work succeeded (embedded or skipped)
                let ok = embedded > 0 || skipped > 0 || errors.is_empty();
                (
                    StatusCode::OK,
                    AxumJson(json!({
                        "ok": ok,
                        "total": total,
                        "embedded": embedded,
                        "skipped": skipped,
                        "errors": errors,
                        "model": s.embed_model,
                    })),
                )
            }
            Err(err) => (
                StatusCode::SERVICE_UNAVAILABLE,
                AxumJson(json!({"ok": false, "error": err})),
            ),
        }
    }

    // POST /api/vault/search — semantic search over vault embeddings
    async fn handle_vault_search(
        State(s): State<ApiState>,
        AxumJson(req): AxumJson<VaultSearchRequest>,
    ) -> impl IntoResponse {
        let db = match ensure_roodb(&s).await {
            Ok(db) => db,
            Err(err) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    AxumJson(json!({"ok": false, "error": err})),
                );
            }
        };
        let query = req.query.trim().to_string();
        let wildcard_query = query.is_empty() || query == "*";
        let top_k = req.top_k.unwrap_or(8).max(1).min(50);

        let rows = match db.fetch_vault_embeddings().await {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    AxumJson(json!({"ok": false, "error": e.to_string()})),
                );
            }
        };

        if wildcard_query {
            let mut sorted = rows;
            sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            sorted.truncate(top_k);
            let results: Vec<VaultSearchResult> = sorted
                .into_iter()
                .map(|row| {
                    let note_type = row
                        .metadata
                        .get("type")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    VaultSearchResult {
                        id: format!("vault:{}", row.note_id),
                        label: row.title,
                        score: 0.0,
                        tags: row.tags,
                        preview: row.preview,
                        path: row.path,
                        note_type,
                        metadata: row.metadata.clone(),
                        evidence: vec![format!("emb:{}", row.note_id)],
                        snippet: None,
                        sources: vec!["embeddings".to_string()],
                    }
                })
                .collect();
            let display_query = if query.is_empty() {
                "*".to_string()
            } else {
                query.clone()
            };
            return (
                StatusCode::OK,
                AxumJson(json!({
                    "ok": true,
                    "query": display_query,
                    "count": results.len(),
                    "model": s.embed_model,
                    "results": results,
                })),
            );
        }

        let query_embedding = match ollama_embed(&s.embed_client, &s.embed_model, &query).await {
            Ok(e) => e,
            Err(e) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    AxumJson(json!({"ok": false, "error": e.to_string()})),
                );
            }
        };

        let qmd_hits = match run_qmd_query(&query, top_k).await {
            Ok(hits) => hits,
            Err(err) => {
                eprintln!("[vault search] QMD query failed: {}", err);
                Vec::new()
            }
        };
        let qmd_map: HashMap<String, QmdHit> = qmd_hits
            .into_iter()
            .map(|hit| (hit.note_id.clone(), hit))
            .collect();
        let mut scored: Vec<(f32, VaultEmbeddingRow, Option<QmdHit>)> = rows
            .into_iter()
            .map(|row| {
                let embed_score = cosine_similarity(&query_embedding, &row.embedding);
                let qmd_entry = qmd_map.get(&row.note_id).cloned();
                let qmd_score = qmd_entry.as_ref().map(|h| h.score).unwrap_or(0.0);
                let combined_score = (embed_score * 0.65) + (qmd_score * 0.35);
                (combined_score, row, qmd_entry)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);

        let results: Vec<VaultSearchResult> = scored
            .into_iter()
            .map(|(combined_score, row, qmd_hit)| {
                let mut evidence = vec![format!("emb:{}", row.note_id)];
                let mut sources = vec!["embeddings".to_string()];
                let snippet = qmd_hit.as_ref().map(|hit| hit.snippet.clone());
                if let Some(hit) = qmd_hit {
                    evidence.push(format!("qmd:{}", hit.note_id));
                    sources.push("qmd".to_string());
                }
                let note_type = row
                    .metadata
                    .get("type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                VaultSearchResult {
                    id: format!("vault:{}", row.note_id),
                    label: row.title,
                    score: combined_score,
                    tags: row.tags,
                    preview: row.preview,
                    path: row.path,
                    note_type,
                    metadata: row.metadata.clone(),
                    evidence,
                    snippet,
                    sources,
                }
            })
            .collect();

        (
            StatusCode::OK,
            AxumJson(json!({
                "ok": true,
                "query": query,
                "count": results.len(),
                "model": s.embed_model,
                "results": results,
            })),
        )
    }

    async fn handle_vault_note(
        State(s): State<ApiState>,
        AxumPath(note_id): AxumPath<String>,
    ) -> impl IntoResponse {
        let db = match ensure_roodb(&s).await {
            Ok(db) => db,
            Err(err) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    AxumJson(json!({"ok": false, "error": err})),
                );
            }
        };
        match db.fetch_vault_note_by_id(&note_id).await {
            Ok(Some(row)) => (
                StatusCode::OK,
                AxumJson(json!({
                    "ok": true,
                    "note": {
                        "note_id": row.note_id,
                        "title": row.title,
                        "preview": row.preview,
                        "path": row.path,
                        "tags": row.tags,
                        "metadata": row.metadata,
                        "updated_at": row.updated_at,
                    }
                })),
            ),
            Ok(None) => (
                StatusCode::NOT_FOUND,
                AxumJson(json!({"ok": false, "error": "note not found"})),
            ),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                AxumJson(json!({"ok": false, "error": err.to_string()})),
            ),
        }
    }

    // WS /ws — proxy to live-viz server on :8788 to avoid 404 spam
    async fn handle_viz_ws_proxy(ws: WebSocketUpgrade) -> impl IntoResponse {
        ws.on_upgrade(move |socket| async move {
            if let Err(e) = ws_proxy_to_viz(socket).await {
                eprintln!("[WS] /ws proxy error: {}", e);
            }
        })
    }

    async fn ws_proxy_to_viz(mut socket: WebSocket) -> Result<(), String> {
        let (upstream, _) = connect_async("ws://localhost:8788/ws")
            .await
            .map_err(|e| e.to_string())?;
        let (mut up_tx, mut up_rx) = upstream.split();

        loop {
            tokio::select! {
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            up_tx.send(tokio_tungstenite::tungstenite::Message::Text(text)).await.map_err(|e| e.to_string())?;
                        }
                        Some(Ok(Message::Binary(bin))) => {
                            up_tx.send(tokio_tungstenite::tungstenite::Message::Binary(bin)).await.map_err(|e| e.to_string())?;
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            let _ = up_tx.send(tokio_tungstenite::tungstenite::Message::Close(None)).await;
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => return Err(format!("{}", e)),
                    }
                }
                msg = futures_util::StreamExt::next(&mut up_rx) => {
                    match msg {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            socket.send(Message::Text(text)).await.map_err(|e| e.to_string())?;
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bin))) => {
                            socket.send(Message::Binary(bin)).await.map_err(|e| e.to_string())?;
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => return Err(format!("{}", e)),
                    }
                }
            }
        }
        Ok(())
    }

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    // Simple request logging layer
    async fn log_request(
        req: axum::http::Request<axum::body::Body>,
        next: axum::middleware::Next,
    ) -> impl IntoResponse {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        println!("[HTTP] {} {}", method, path);
        let response = next.run(req).await;
        println!("[HTTP] {} {} -> {}", method, path, response.status());
        response
    }

    let app = Router::new()
        .route("/api/health", get(|| async { "OK" }))
        .route("/api/state", get(handle_state))
        .route("/api/context", get(handle_context))
        .route("/api/chat/context", get(handle_chat_context))
        .route("/ws", get(handle_viz_ws_proxy))
        .route("/api/command", post(handle_command))
        .route("/api/message", post(handle_message))
        // Chat: POST to send, WebSocket to receive stream
        .route("/api/chat", post(handle_chat_post))
        .route("/api/chat", get(handle_chat_ws))
        // Council: POST to send, WebSocket to receive stream
        .route("/api/council", post(handle_council_post))
        .route("/api/council", get(handle_council_ws))
        .route("/api/graph-activity", get(handle_graph_activity_ws))
        .route("/api/code", get(handle_code_ws))
        // New component API endpoints
        .route("/api/components/council", get(handle_council_state))
        .route("/api/components/dks", get(handle_dks_state))
        .route("/api/components/cass", get(handle_cass_state))
        .route("/api/components/skills", get(handle_skills_state))
        .route("/api/components/navigation", get(handle_navigation_state))
        .route(
            "/api/components/communication",
            get(handle_communication_state),
        )
        .route("/api/components/gpu", get(handle_gpu_state))
        .route("/api/components/llm", get(handle_llm_state))
        .route("/api/components/email", get(handle_email_state))
        .route("/api/skills/evidence", get(handle_skill_evidence))
        .route("/api/rewards", get(handle_rewards))
        .route(
            "/api/ouroboros/gate-audits",
            get(handle_ouroboros_gate_audits),
        )
        .route(
            "/api/ouroboros/memory-events",
            get(handle_ouroboros_memory_events),
        )
        .route("/api/messages", get(handle_messages))
        .route("/api/plan-steps", get(handle_plan_steps_get))
        .route("/api/optimize", post(handle_optimize_post))
        .route("/api/optimize", get(handle_optimize_ws))
        // Visual explainer endpoints
        // Visual explainer endpoints
        // Note: WebSocket uses a different path to avoid conflict with REST endpoints
        .route("/api/visual/ws", get(handle_visual_ws))
        .route("/api/visual", get(handle_visual_list))
        .route("/api/visual", post(handle_visual_post))
        .route(
            "/api/visual/file/:filename",
            get(handle_visual_file).delete(handle_visual_delete),
        )
        .route("/api/vault/index", post(handle_vault_index))
        .route("/api/vault/search", post(handle_vault_search))
        .route("/api/vault/note/:note_id", get(handle_vault_note))
        .route("/api/plan/workflow", post(handle_plan_workflow))
        .route("/api/plan/optimize", post(handle_plan_optimize))
        .route("/api/plan/history", get(handle_plan_history))
        .route("/api/hires", get(handle_hires_get))
        .route("/api/hires/complete", post(handle_hire_complete_post))
        .route("/api/skills/curation", post(handle_skill_curation_post))
        .route("/api/dspy/optimize", post(handle_dspy_optimize_post))
        .route("/api/dspy/traces", get(handle_dspy_traces_get))
        .route("/api/dspy/demos", get(handle_dspy_demos_get))
        .fallback_service(ServeDir::new("viz").append_index_html_on_directories(true))
        .layer(axum::middleware::from_fn(log_request))
        .with_state(api_state.clone())
        .layer(cors);

    // Auto-index vault embeddings on startup and on interval.
    {
        let api_state = api_state.clone();
        let interval_secs: u64 = env::var("HSM_VAULT_INDEX_INTERVAL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);
        tokio::spawn(async move {
            // Initial delay to allow RooDB to connect.
            tokio::time::sleep(Duration::from_secs(5)).await;
            if let Ok((total, embedded, skipped, errors)) = index_vault_embeddings(&api_state).await
            {
                eprintln!(
                    "[Vault] indexed: total={} embedded={} skipped={} errors={}",
                    total,
                    embedded,
                    skipped,
                    errors.len()
                );
            }
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                if let Ok((total, embedded, skipped, errors)) =
                    index_vault_embeddings(&api_state).await
                {
                    if embedded > 0 || !errors.is_empty() {
                        eprintln!(
                            "[Vault] reindexed: total={} embedded={} skipped={} errors={}",
                            total,
                            embedded,
                            skipped,
                            errors.len()
                        );
                    }
                }
            }
        });
    }

    let api_hosts = build_host_candidates(
        env::var("HSM_API_HOST")
            .ok()
            .or_else(|| env::var("HSM_HOST").ok()),
    );
    let api_ports = build_port_candidates(
        parse_port_env("HSM_API_PORT"),
        parse_port_env("HSM_API_FALLBACK_PORT"),
        8787,
    );
    let listener = match bind_with_retry("Web API server", &api_hosts, &api_ports).await {
        Some(l) => l,
        None => return,
    };

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("Web API server error: {}", e);
    }
}

// ── Main ────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    // Headless JSON export: `hyper-stigmergy --export-json [path]`
    if args.iter().any(|a| a == "--export-json") {
        let path = args
            .iter()
            .position(|a| a == "--export-json")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("hyper_graph.json");
        let mut world = HyperStigmergicMorphogenesis::load();
        // Seed edges if the world is fresh (no saved state had edges)
        if world.edges.is_empty() {
            for i in 0..5usize {
                world.apply_action_with_agent(
                    &Action::LinkAgents {
                        vertices: vec![i, (i + 1) % 10, (i + 3) % 10],
                        weight: 1.0 + (i as f32) * 0.2,
                    },
                    Some(0),
                );
            }
        }
        world.export_json(path)?;
        return Ok(());
    }

    // REPL mode (non-server interactive shell)
    let repl_mode = args.iter().any(|a| a == "--repl");
    if repl_mode {
        return run_repl_mode(&args).await;
    }
    // Headless mode: run web server without TUI
    let headless = args.iter().any(|a| a == "--headless");
    let rewards_cache = Arc::new(RwLock::new(Vec::new()));
    let skill_evidence_cache = Arc::new(RwLock::new(Vec::new()));

    if headless {
        eprintln!("[HSM-II] Starting in headless mode...");
        eprintln!("[HSM-II] Studio UI: http://localhost:8787");

        let mut app = App::new();

        // Auto-pull Ollama models in background
        {
            let models = app.chat_models.clone();
            tokio::spawn(async move {
                let ollama = Ollama::new("http://localhost".to_string(), 11434);
                for (name, model) in models {
                    eprintln!("[Ollama] Checking model: {} ({})...", name, model);
                    let test_req = ChatMessageRequest::new(
                        model.to_string(),
                        vec![OllamaChatMsg::system("test".into())],
                    );
                    match ollama.send_chat_messages(test_req).await {
                        Ok(_) => eprintln!("[Ollama] ✓ Model {} is available", name),
                        Err(e) => {
                            let err_str = format!("{}", e);
                            if err_str.contains("404") || err_str.contains("not found") {
                                eprintln!(
                                    "[Ollama] ⚠ Model {} not found. Pull it with: ollama pull {}",
                                    name, model
                                );
                            } else if err_str.contains("Connection refused") {
                                eprintln!(
                                    "[Ollama] ⚠ Ollama not running. Start with: ollama serve"
                                );
                                break;
                            } else {
                                eprintln!("[Ollama] ✓ Model {} appears available", name);
                            }
                        }
                    }
                }
            });
        }

        // Spawn live-viz WebSocket server (ws://localhost:8788/ws)
        {
            let viz_rx = app.viz_tx.subscribe();
            tokio::spawn(viz_ws_server(viz_rx));
            eprintln!("[HSM-II] Viz WS server: ws://localhost:8788/ws");
        }

        // Spawn web API + Studio UI server (:8787)
        {
            let web_state = WebApiState {
                snapshot: app.web_snapshot.clone(),
                chat_broadcast: app.web_chat_broadcast.clone(),
                web_cmd_tx: app.bg_tx.clone(),
                last_context: app.web_last_context.clone(),
                council_broadcast: app.web_council_broadcast.clone(),
                graph_activity_broadcast: app.web_graph_activity_broadcast.clone(),
                council_mode: Arc::new(RwLock::new("auto".to_string())),
                code_broadcast: app.web_code_broadcast.clone(),
                optimize_broadcast: app.web_optimize_broadcast.clone(),
                visual_broadcast: app.web_visual_broadcast.clone(),
                roodb: app.web_roodb.clone(),
                roodb_url: "127.0.0.1:3307".to_string(),
                embed_model: embed_model_from_env(),
                embed_client: Client::new(),
                vault_dir: vault_dir_from_env(),
                plan_steps: app.plan_steps.clone(),
                recent_rewards: rewards_cache.clone(),
                recent_skill_evidence: skill_evidence_cache.clone(),
            };
            tokio::spawn(web_api_server(web_state));
        }

        // Run event loop without TUI
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            app.drain_bg_events().await;
            app.drain_chat_events();
            app.maybe_extract_skillbank().await;
        }
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    // Auto-pull Ollama models in background
    {
        let models = app.chat_models.clone();
        tokio::spawn(async move {
            let ollama = Ollama::new("http://localhost".to_string(), 11434);
            for (name, model) in models {
                println!("[Ollama] Checking model: {} ({})...", name, model);
                // Try a simple request to see if model exists
                let test_req = ChatMessageRequest::new(
                    model.to_string(),
                    vec![OllamaChatMsg::system("test".into())],
                );
                match ollama.send_chat_messages(test_req).await {
                    Ok(_) => println!("[Ollama] ✓ Model {} is available", name),
                    Err(e) => {
                        let err_str = format!("{}", e);
                        if err_str.contains("404") || err_str.contains("not found") {
                            println!("[Ollama] ⚠ Model {} not found. Pull it with:", name);
                            println!("[Ollama]   ollama pull {}", model);
                        } else if err_str.contains("Connection refused") {
                            println!("[Ollama] ⚠ Ollama not running. Start with: ollama serve");
                            break;
                        } else {
                            println!(
                                "[Ollama] ✓ Model {} appears available (test: {})",
                                name, err_str
                            );
                        }
                    }
                }
            }
        });
    }

    // Spawn live-viz WebSocket server (ws://localhost:8788/ws)
    {
        let viz_rx = app.viz_tx.subscribe();
        tokio::spawn(viz_ws_server(viz_rx));
        app.log("Viz WS server: ws://localhost:8788/ws");
    }

    // Spawn web API + Studio UI server (:8787)
    {
        let web_state = WebApiState {
            snapshot: app.web_snapshot.clone(),
            chat_broadcast: app.web_chat_broadcast.clone(),
            web_cmd_tx: app.bg_tx.clone(),
            last_context: app.web_last_context.clone(),
            council_broadcast: app.web_council_broadcast.clone(),
            graph_activity_broadcast: app.web_graph_activity_broadcast.clone(),
            council_mode: Arc::new(RwLock::new("auto".to_string())),
            code_broadcast: app.web_code_broadcast.clone(),
            optimize_broadcast: app.web_optimize_broadcast.clone(),
            visual_broadcast: app.web_visual_broadcast.clone(),
            roodb: app.web_roodb.clone(),
            roodb_url: "127.0.0.1:3307".to_string(),
            embed_model: embed_model_from_env(),
            embed_client: Client::new(),
            vault_dir: vault_dir_from_env(),
            plan_steps: app.plan_steps.clone(),
            recent_rewards: rewards_cache.clone(),
            recent_skill_evidence: skill_evidence_cache.clone(),
        };
        tokio::spawn(web_api_server(web_state));
        app.log("Studio UI: http://localhost:8787");
    }

    // Parse --federation <addr>, --peer <url>, and --roodb <url> flags
    let mut roodb_url: Option<String> = None;
    {
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--federation" => {
                    if let Some(addr) = args.get(i + 1) {
                        app.federation_addr = Some(addr.clone());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--peer" => {
                    if let Some(peer) = args.get(i + 1) {
                        app.federation_peers.push(peer.clone());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--roodb" => {
                    if let Some(url) = args.get(i + 1) {
                        roodb_url = Some(url.clone());
                        i += 2;
                    } else {
                        // Default to localhost (RooDB default port 3307)
                        roodb_url = Some("127.0.0.1:3307".to_string());
                        i += 1;
                    }
                }
                _ => {
                    i += 1;
                }
            }
        }

        // If --federation was provided, set up the federation subsystem
        if let Some(ref addr) = app.federation_addr {
            let system_id = uuid::Uuid::new_v4().to_string();
            let config = FederationConfig {
                system_id: system_id.clone(),
                listen_addr: addr.clone(),
                known_peers: app.federation_peers.clone(),
                trust_threshold: 0.3,
                auto_promote_after: 50,
            };

            app.world.federation_config = Some(config.clone());

            let meta_graph = std::sync::Arc::new(tokio::sync::RwLock::new(MetaGraph::new(&config)));
            app.federation_meta_graph = Some(meta_graph.clone());

            // Create shared world for the server
            let world_arc = std::sync::Arc::new(tokio::sync::RwLock::new(app.world.clone()));
            let current_tick = std::sync::Arc::new(tokio::sync::RwLock::new(0u64));

            let state = FederationState {
                meta_graph: meta_graph.clone(),
                world: world_arc,
                current_tick,
            };

            // Spawn the federation HTTP server
            let listen_addr = addr.clone();
            tokio::spawn(async move {
                if let Err(e) = FederationServer::serve(&listen_addr, state).await {
                    eprintln!("Federation server error: {}", e);
                }
            });

            app.log(&format!(
                "Federation enabled: {} (system {})",
                addr,
                &system_id[..8]
            ));
            let peers_snapshot: Vec<String> = app.federation_peers.clone();
            for peer in peers_snapshot {
                app.log(&format!("  Peer: {}", peer));
            }
        }
    }

    if roodb_url.is_none() {
        roodb_url = Some("127.0.0.1:3307".to_string());
        app.log("RooDB auto-connect default: 127.0.0.1:3307 (use --roodb to override)");
    }

    // If --roodb was provided, connect to RooDB (with 5s timeout so we don't block TUI)
    if let Some(ref url) = roodb_url {
        let config = RooDbConfig::from_url(url);
        let db = RooDb::new(&config);

        let init_result = tokio::time::timeout(Duration::from_secs(5), async {
            db.ping().await?;
            db.init_schema().await?;
            Ok::<_, anyhow::Error>(db)
        })
        .await;

        match init_result {
            Ok(Ok(db)) => {
                app.log(&format!(
                    "RooDB connected: {}:{}/{}",
                    config.host, config.port, config.database
                ));
                let db = std::sync::Arc::new(db);
                app.roodb = Some(db.clone());
                if let Ok(mut slot) = app.web_roodb.try_write() {
                    *slot = Some(db);
                }
            }
            Ok(Err(e)) => {
                app.log(&format!(
                    "RooDB init failed: {} (falling back to embedded local store)",
                    e
                ));
            }
            Err(_) => {
                app.log(
                    "RooDB connection timed out after 5s (falling back to embedded local store)",
                );
            }
        }
    }

    let tick_rate = Duration::from_millis(50);

    // Main loop
    while app.running {
        // Drain async results before drawing
        app.drain_chat_events();
        app.drain_bg_events().await;
        app.maybe_extract_skillbank().await;

        terminal.draw(|f| draw_ui(f, &app))?;

        // Auto-tick
        if app.auto_tick && app.last_auto_tick.elapsed() >= Duration::from_millis(app.tick_speed_ms)
        {
            app.do_tick();
            app.do_export_viz();
            app.last_auto_tick = Instant::now();

            // Auto-export to DuckDB/LARS every 50 ticks so LARS stays fresh
            const LARS_EXPORT_INTERVAL: u64 = 50;
            let current_tick = app.world.tick_count;
            if current_tick >= app.last_lars_export_tick + LARS_EXPORT_INTERVAL {
                app.last_lars_export_tick = current_tick;
                app.do_export_duckdb();
            }
        }

        // Input handling
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                // ── Chat tab: intercept all keys for text input ──
                if app.active_tab == 5 {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.running = false;
                        }
                        KeyCode::Esc => {
                            if !app.chat_input.is_empty() {
                                app.chat_input.clear();
                            } else {
                                // Go back to Dashboard instead of quitting
                                app.active_tab = 0;
                            }
                        }
                        KeyCode::Tab => {
                            app.active_tab = (app.active_tab + 1) % app.tab_titles.len();
                        }
                        KeyCode::BackTab => {
                            app.active_tab = if app.active_tab == 0 {
                                app.tab_titles.len() - 1
                            } else {
                                app.active_tab - 1
                            };
                        }
                        KeyCode::Enter => app.send_chat_message().await,
                        KeyCode::Backspace => {
                            app.chat_input.pop();
                        }
                        KeyCode::Char('M') => {
                            // Shift+M cycles through models
                            app.selected_model = (app.selected_model + 1) % app.chat_models.len();
                            app.log(&format!("Model: {}", app.chat_models[app.selected_model].0));
                        }
                        KeyCode::Up => {
                            app.chat_scroll = app.chat_scroll.saturating_add(2);
                        }
                        KeyCode::Down => {
                            app.chat_scroll = app.chat_scroll.saturating_sub(2);
                        }
                        KeyCode::Char(c) => {
                            app.chat_input.push(c);
                        }
                        _ => {}
                    }
                } else {
                    // ── Normal tabs: existing key handling ──
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.running = false,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.running = false
                        }

                        // Tab navigation
                        KeyCode::Tab | KeyCode::Right => {
                            app.active_tab = (app.active_tab + 1) % app.tab_titles.len();
                        }
                        KeyCode::BackTab | KeyCode::Left => {
                            app.active_tab = if app.active_tab == 0 {
                                app.tab_titles.len() - 1
                            } else {
                                app.active_tab - 1
                            };
                        }
                        KeyCode::Char('1') => app.active_tab = 0,
                        KeyCode::Char('2') => app.active_tab = 1,
                        KeyCode::Char('3') => app.active_tab = 2,
                        KeyCode::Char('4') => app.active_tab = 3,
                        KeyCode::Char('5') => app.active_tab = 4,
                        KeyCode::Char('6') => app.active_tab = 5,
                        KeyCode::Char('7') => app.active_tab = 6,

                        // Actions
                        KeyCode::Char(' ') | KeyCode::Char('t') => app.do_tick(),
                        KeyCode::Char('a') => app.auto_tick = !app.auto_tick,
                        KeyCode::Char('b') => app.do_bid_round(),
                        KeyCode::Char('i') => app.do_improvement(),
                        KeyCode::Char('l') => app.do_link_random(),
                        KeyCode::Char('s') => {
                            let msg = app.do_save_with_compat_gate("operator").await;
                            if msg.starts_with("Save blocked by compatibility gate:") {
                                app.log(&msg);
                            }
                        }
                        KeyCode::Char('o') => app.do_load(),
                        KeyCode::Char('S') => {
                            // Shift+S: force save (same as 's' but explicit intent)
                            let msg = app.do_save_with_compat_gate("operator").await;
                            if msg.starts_with("Save blocked by compatibility gate:") {
                                app.log(&msg);
                            }
                        }
                        KeyCode::Char('O') => {
                            // Shift+O: load from RooDB
                            app.do_load_db();
                        }
                        KeyCode::Char('e') => app.do_export_json(),
                        KeyCode::Char('r') => app.do_reflect(),
                        KeyCode::Char('p') => app.do_pareto_bid(),
                        KeyCode::Char('d') => app.do_decay_beliefs(),

                        // Speed control
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            app.tick_speed_ms = (app.tick_speed_ms.saturating_sub(50)).max(50);
                            app.log(&format!("Tick speed: {}ms", app.tick_speed_ms));
                        }
                        KeyCode::Char('-') => {
                            app.tick_speed_ms = (app.tick_speed_ms + 50).min(2000);
                            app.log(&format!("Tick speed: {}ms", app.tick_speed_ms));
                        }

                        // Scroll log
                        KeyCode::Up => {
                            if app.log_scroll < app.event_log.len().saturating_sub(1) {
                                app.log_scroll += 1;
                            }
                        }
                        KeyCode::Down => {
                            app.log_scroll = app.log_scroll.saturating_sub(1);
                        }

                        _ => {}
                    }
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    println!("Hyper-Stigmergic Morphogenesis II shut down cleanly.");
    Ok(())
}

// ── Drawing ─────────────────────────────────────────────────────────────

fn draw_ui(f: &mut Frame, app: &App) {
    let size = f.size();

    // Top-level layout: tabs + content + status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tabs
            Constraint::Min(10),   // content
            Constraint::Length(3), // status bar
        ])
        .split(size);

    draw_tabs(f, app, chunks[0]);

    match app.active_tab {
        0 => draw_dashboard(f, app, chunks[1]),
        1 => draw_agents(f, app, chunks[1]),
        2 => draw_hypergraph(f, app, chunks[1]),
        3 => draw_improvement(f, app, chunks[1]),
        4 => draw_log(f, app, chunks[1]),
        5 => draw_chat(f, app, chunks[1]),
        6 => draw_federation(f, app, chunks[1]),
        _ => {}
    }

    draw_status_bar(f, app, chunks[2]);
}

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = app
        .tab_titles
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let num = format!(" {} ", i + 1);
            Line::from(vec![
                Span::styled(num, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    t.clone(),
                    if i == app.active_tab {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    },
                ),
            ])
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Hyper-Stigmergic Morphogenesis II "),
        )
        .select(app.active_tab)
        .highlight_style(Style::default().fg(Color::Cyan))
        .divider(Span::raw(" | "));

    f.render_widget(tabs, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status = if app.active_tab == 5 {
        // Chat tab status bar
        Line::from(vec![
            Span::styled(" [Esc]", Style::default().fg(Color::Red)),
            Span::raw("back "),
            Span::styled("[Enter]", Style::default().fg(Color::Green)),
            Span::raw("send "),
            Span::styled("[Shift+M]", Style::default().fg(Color::Cyan)),
            Span::raw("model "),
            Span::styled("[↑↓]", Style::default().fg(Color::DarkGray)),
            Span::raw("scroll "),
            Span::styled("[Tab]", Style::default().fg(Color::DarkGray)),
            Span::raw("switch  "),
            Span::styled(
                format!("Model: {}", app.chat_models[app.selected_model].0),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        let auto_str = if app.auto_tick {
            format!("AUTO {}ms", app.tick_speed_ms)
        } else {
            "PAUSED".into()
        };
        Line::from(vec![
            Span::styled(" [q]", Style::default().fg(Color::Red)),
            Span::raw("uit "),
            Span::styled("[t]", Style::default().fg(Color::Green)),
            Span::raw("ick "),
            Span::styled("[a]", Style::default().fg(Color::Yellow)),
            Span::raw("uto "),
            Span::styled("[b]", Style::default().fg(Color::Magenta)),
            Span::raw("id "),
            Span::styled("[i]", Style::default().fg(Color::Cyan)),
            Span::raw("mprove "),
            Span::styled("[r]", Style::default().fg(Color::LightCyan)),
            Span::raw("eflect "),
            Span::styled("[p]", Style::default().fg(Color::LightMagenta)),
            Span::raw("areto "),
            Span::styled("[l]", Style::default().fg(Color::Blue)),
            Span::raw("ink "),
            Span::styled("[s]", Style::default().fg(Color::Green)),
            Span::raw("ave "),
            Span::styled("[6]", Style::default().fg(Color::Cyan)),
            Span::raw("chat "),
            Span::styled("[+/-]", Style::default().fg(Color::DarkGray)),
            Span::raw("speed "),
            Span::styled("[Tab]", Style::default().fg(Color::DarkGray)),
            Span::raw("switch  "),
            Span::styled(
                auto_str,
                if app.auto_tick {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
            if app.roodb.is_some() {
                Span::styled(
                    " [DB]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(" [no-db]", Style::default().fg(Color::DarkGray))
            },
        ])
    };
    let bar = Paragraph::new(status).block(Block::default().borders(Borders::ALL));
    f.render_widget(bar, area);
}

// ── Tab 0: Dashboard ────────────────────────────────────────────────────

fn draw_dashboard(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Left column: metrics + sparklines
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // key metrics
            Constraint::Length(5), // coherence sparkline
            Constraint::Length(5), // edge sparkline
            Constraint::Min(3),    // recent log
        ])
        .split(cols[0]);

    // Key metrics box
    let coherence = app.world.global_coherence();
    let emergent = app.world.emergent_edge_count();
    let total_edges = app.world.edges.len();
    let agents = app.world.agents.len();
    let tick = app.world.tick_count;
    let ontology = app.world.ontology.len();
    let improvements = app.improvement_count;

    let metrics_text = vec![
        Line::from(vec![
            Span::styled("  Tick: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", tick),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled("Agents: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", agents),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled("Ontology: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", ontology), Style::default().fg(Color::Magenta)),
        ]),
        Line::from(vec![
            Span::styled("  Edges: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", total_edges),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled("Emergent: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", emergent), Style::default().fg(Color::Green)),
            Span::raw("    "),
            Span::styled("Improved: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", improvements),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Coherence: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.4}", coherence),
                Style::default()
                    .fg(if coherence > 0.5 {
                        Color::Green
                    } else if coherence > 0.2 {
                        Color::Yellow
                    } else {
                        Color::Red
                    })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    let metrics = Paragraph::new(metrics_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" System Metrics "),
    );
    f.render_widget(metrics, left[0]);

    // Coherence sparkline
    let spark_data: Vec<u64> = app.coherence_history.clone();
    let spark = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Coherence (last 60) "),
        )
        .data(&spark_data)
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(spark, left[1]);

    // Edge count sparkline
    let edge_data: Vec<u64> = app.edge_count_history.clone();
    let edge_spark = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Edge Count (last 60) "),
        )
        .data(&edge_data)
        .style(Style::default().fg(Color::Yellow));
    f.render_widget(edge_spark, left[2]);

    // Recent log in bottom-left
    let log_items: Vec<ListItem> = app
        .event_log
        .iter()
        .rev()
        .take(left[3].height.saturating_sub(2) as usize)
        .map(|s| {
            ListItem::new(Span::styled(
                s.as_str(),
                Style::default().fg(Color::DarkGray),
            ))
        })
        .collect();
    let log_list = List::new(log_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Recent Events "),
    );
    f.render_widget(log_list, left[3]);

    // Right column: role distribution + gauges
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // role bids
            Constraint::Length(5),  // coherence gauge
            Constraint::Length(5),  // embedding gauge
            Constraint::Min(3),     // bid config
        ])
        .split(cols[1]);

    // Role bid bar chart
    let arch = *app.bid_history.get("Architect").unwrap_or(&0) as u64;
    let cat = *app.bid_history.get("Catalyst").unwrap_or(&0) as u64;
    let chron = *app.bid_history.get("Chronicler").unwrap_or(&0) as u64;

    let bar_group = BarGroup::default().bars(&[
        Bar::default()
            .value(arch)
            .label("Arch".into())
            .style(Style::default().fg(Color::Cyan)),
        Bar::default()
            .value(cat)
            .label("Cat".into())
            .style(Style::default().fg(Color::Yellow)),
        Bar::default()
            .value(chron)
            .label("Chron".into())
            .style(Style::default().fg(Color::Magenta)),
    ]);

    let barchart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Role Bids ({} total) ", app.total_bids)),
        )
        .data(bar_group)
        .bar_width(5)
        .bar_gap(2)
        .value_style(
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(barchart, right[0]);

    // Coherence gauge
    let coh_pct = (coherence * 100.0).min(100.0) as u16;
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Coherence "))
        .gauge_style(Style::default().fg(if coh_pct > 50 {
            Color::Green
        } else if coh_pct > 20 {
            Color::Yellow
        } else {
            Color::Red
        }))
        .percent(coh_pct)
        .label(format!("{:.2}%", coherence * 100.0));
    f.render_widget(gauge, right[1]);

    // Embedding coverage gauge
    let emb_cov = app.world.calculate_embedding_coverage();
    let emb_pct = (emb_cov * 100.0) as u16;
    let emb_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Embedding Coverage "),
        )
        .gauge_style(Style::default().fg(Color::Blue))
        .percent(emb_pct)
        .label(format!("{:.1}%", emb_cov * 100.0));
    f.render_widget(emb_gauge, right[2]);

    // Bid config info
    let config_text = vec![
        Line::from(vec![
            Span::styled("  Architect bias: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.2}", app.bid_config.architect_bias),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Catalyst bias:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.2}", app.bid_config.catalyst_bias),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Chronicler bias:", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(" {:.2}", app.bid_config.chronicler_bias),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Temperature:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.2}", app.bid_config.exploration_temperature),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    let config_para = Paragraph::new(config_text)
        .block(Block::default().borders(Borders::ALL).title(" Bid Config "));
    f.render_widget(config_para, right[3]);
}

// ── Tab 1: Agents ───────────────────────────────────────────────────────

fn draw_agents(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .world
        .agents
        .iter()
        .map(|agent| {
            let role_color = match agent.role {
                Role::Architect => Color::Cyan,
                Role::Catalyst => Color::Yellow,
                Role::Chronicler => Color::Magenta,
                Role::Critic => Color::Red,
                Role::Explorer => Color::Green,
                Role::Coder => Color::Blue,
            };
            let edges = app
                .world
                .adjacency
                .get(&agent.id)
                .map(|e| e.len())
                .unwrap_or(0);

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:>2} ", agent.id),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<11}", format!("{:?}", agent.role)),
                    Style::default().fg(role_color),
                ),
                Span::styled(
                    format!(
                        "C={:.2} H={:.2} G={:.2} T={:.2}",
                        agent.drives.curiosity,
                        agent.drives.harmony,
                        agent.drives.growth,
                        agent.drives.transcendence,
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("edges={}", edges),
                    Style::default().fg(Color::Green),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Agents ({}) ", app.world.agents.len())),
    );
    f.render_widget(list, area);
}

// ── Tab 2: Hypergraph ───────────────────────────────────────────────────

fn draw_hypergraph(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Edges list
    let edge_items: Vec<ListItem> = app
        .world
        .edges
        .iter()
        .enumerate()
        .rev()
        .take(cols[0].height.saturating_sub(2) as usize)
        .map(|(i, e)| {
            let parts: Vec<String> = e.participants.iter().map(|p| p.to_string()).collect();
            let emergent_str = if e.emergent { " [E]" } else { "" };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {:>3} ", i), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("({}) ", parts.join(",")),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("w={:.3}", e.weight),
                    Style::default().fg(if e.weight > 0.5 {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
                Span::styled(
                    format!(" age={}", e.age),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(emergent_str, Style::default().fg(Color::Cyan)),
            ]))
        })
        .collect();

    let edge_list = List::new(edge_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Hyperedges ({}) ", app.world.edges.len())),
    );
    f.render_widget(edge_list, cols[0]);

    // Ontology + vertex info
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(cols[1]);

    let ontology_items: Vec<ListItem> = app
        .world
        .ontology
        .iter()
        .take(right[0].height.saturating_sub(2) as usize)
        .map(|(concept, entry)| {
            let instances_str = entry.instances.join(", ");
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", concept),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({:.0}%) ", entry.confidence * 100.0),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    truncate_str(&instances_str, 40),
                    Style::default().fg(Color::White),
                ),
            ]))
        })
        .collect();

    let ontology_list = List::new(ontology_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Ontology ({}) ", app.world.ontology.len())),
    );
    f.render_widget(ontology_list, right[0]);

    // Vertex summary
    let (a, t, m, k) = (
        app.world.num_agents_vertices,
        app.world.num_tools_vertices,
        app.world.num_memory_vertices,
        app.world.num_task_vertices,
    );
    let props = app.world.property_vertices.len();
    let total = app.world.vertex_meta.len();
    let vert_text = vec![
        Line::from(vec![
            Span::styled("  Total vertices: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", total),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Agent:    ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{}", a)),
        ]),
        Line::from(vec![
            Span::styled("  Tool:     ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}", t)),
        ]),
        Line::from(vec![
            Span::styled("  Memory:   ", Style::default().fg(Color::Green)),
            Span::raw(format!("{}", m)),
        ]),
        Line::from(vec![
            Span::styled("  Task:     ", Style::default().fg(Color::Magenta)),
            Span::raw(format!("{}", k)),
        ]),
        Line::from(vec![
            Span::styled("  Property: ", Style::default().fg(Color::Blue)),
            Span::raw(format!("{}", props)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Decay rate: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.4}", app.world.decay_rate),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Clustering: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.4}", app.world.calculate_clustering_coefficient()),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    let vert_para =
        Paragraph::new(vert_text).block(Block::default().borders(Borders::ALL).title(" Vertices "));
    f.render_widget(vert_para, right[1]);
}

// ── Tab 3: Improvement ──────────────────────────────────────────────────

fn draw_improvement(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
        ])
        .split(area);

    // Left: History list
    let history_items: Vec<ListItem> = app
        .world
        .improvement_history
        .iter()
        .rev()
        .take(cols[0].height.saturating_sub(2) as usize)
        .map(|event| {
            let delta = event.coherence_after - event.coherence_before;
            let delta_color = if delta > 0.0 {
                Color::Green
            } else if delta < 0.0 {
                Color::Red
            } else {
                Color::DarkGray
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:?} ", event.mutation_type),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("d={:+.4} ", delta),
                    Style::default().fg(delta_color),
                ),
                Span::styled(
                    format!("n={:.2} ", event.novelty_score),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    if event.applied { "OK" } else { "--" },
                    Style::default().fg(if event.applied {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                ),
            ]))
        })
        .collect();

    let history_list =
        List::new(history_items).block(Block::default().borders(Borders::ALL).title(format!(
            " History ({}) ",
            app.world.improvement_history.len()
        )));
    f.render_widget(history_list, cols[0]);

    // Middle: Stats + memory
    let predict_coh = app.world.predict_coherence("current system state");
    let compute_nov = app.world.compute_novelty("novel emergent concept");

    let mut stats_text = vec![
        Line::from(vec![
            Span::styled(" Improvements: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.improvement_count),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Last: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.last_mutation.as_deref().unwrap_or("none"),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Delta: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:+.4}", app.last_coherence_delta),
                Style::default().fg(if app.last_coherence_delta >= 0.0 {
                    Color::Green
                } else {
                    Color::Red
                }),
            ),
            Span::styled("  Nov: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.2}", app.last_novelty),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " Predictions",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(" coh: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.4}", predict_coh),
                Style::default().fg(Color::Green),
            ),
            Span::styled("  nov: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.4}", compute_nov),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " Memory",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(" entries: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.memory.stats.total_entries),
                Style::default().fg(Color::White),
            ),
            Span::styled("  recalls: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.memory.stats.total_recalls),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled(" tools: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.tool_registry.len()),
                Style::default().fg(Color::White),
            ),
            Span::styled("  reflect: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("#{}", app.world.reflection_count),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " [i]improve [r]reflect",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            " [p]pareto  [d]decay",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    // Show reflect status if available
    if let Some(ref status) = app.reflect_status {
        stats_text.push(Line::from(""));
        stats_text.push(Line::from(Span::styled(
            format!(" {}", status),
            Style::default().fg(Color::Cyan),
        )));
    }

    let stats_para = Paragraph::new(stats_text)
        .block(Block::default().borders(Borders::ALL).title(" Stats "))
        .wrap(Wrap { trim: true });
    f.render_widget(stats_para, cols[1]);

    // Right: Beliefs panel
    let top_beliefs = app
        .world
        .top_beliefs(cols[2].height.saturating_sub(2) as usize);
    let belief_items: Vec<ListItem> = if top_beliefs.is_empty() {
        vec![ListItem::new(Span::styled(
            " No beliefs yet — press [r] to reflect",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        top_beliefs
            .iter()
            .map(|b| {
                let conf_color = if b.confidence > 0.7 {
                    Color::Green
                } else if b.confidence > 0.4 {
                    Color::Yellow
                } else {
                    Color::Red
                };
                let source_char = match b.source {
                    hyper_stigmergy::BeliefSource::Observation => 'O',
                    hyper_stigmergy::BeliefSource::Reflection => 'R',
                    hyper_stigmergy::BeliefSource::Inference => 'I',
                    hyper_stigmergy::BeliefSource::UserProvided => 'U',
                    hyper_stigmergy::BeliefSource::Prediction => 'P',
                };
                let content = if b.content.len() > 40 {
                    format!("{}...", &b.content[..37])
                } else {
                    b.content.clone()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{:.0}%{source_char}] ", b.confidence * 100.0),
                        Style::default().fg(conf_color),
                    ),
                    Span::styled(content, Style::default().fg(Color::White)),
                ]))
            })
            .collect()
    };

    let beliefs_list = List::new(belief_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Beliefs ({}) ", app.world.beliefs.len())),
    );
    f.render_widget(beliefs_list, cols[2]);
}

// ── Tab 4: Log ──────────────────────────────────────────────────────────

fn draw_log(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .event_log
        .iter()
        .rev()
        .skip(app.log_scroll)
        .take(area.height.saturating_sub(2) as usize)
        .map(|s| {
            let color = if s.contains("Improvement") || s.contains("improvement") {
                Color::Cyan
            } else if s.contains("Linked") || s.contains("Link") {
                Color::Blue
            } else if s.contains("Bid") || s.contains("bid") {
                Color::Magenta
            } else if s.contains("Save")
                || s.contains("Load")
                || s.contains("save")
                || s.contains("load")
            {
                Color::Green
            } else {
                Color::White
            };
            ListItem::new(Span::styled(s.as_str(), Style::default().fg(color)))
        })
        .collect();

    let title = if app.log_scroll > 0 {
        format!(
            " Event Log ({} entries, scroll +{}) ",
            app.event_log.len(),
            app.log_scroll
        )
    } else {
        format!(" Event Log ({} entries) ", app.event_log.len())
    };

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(list, area);
}

// ── Tab 5: Chat ────────────────────────────────────────────────────────

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // model selector bar
            Constraint::Min(6),    // chat messages
            Constraint::Length(3), // input field
        ])
        .split(area);

    // ── Model selector bar ──
    let mut model_line = vec![
        Span::styled("  [Shift+M] ", Style::default().fg(Color::DarkGray)),
        Span::styled("Model: ", Style::default().fg(Color::DarkGray)),
    ];
    for (i, (name, _)) in app.chat_models.iter().enumerate() {
        if i > 0 {
            model_line.push(Span::styled(" / ", Style::default().fg(Color::DarkGray)));
        }
        model_line.push(Span::styled(
            format!("{}", name),
            if i == app.selected_model {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::White)
            },
        ));
    }
    if app.chat_streaming {
        model_line.push(Span::styled(
            "  ● streaming...",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let model_bar = Paragraph::new(Line::from(model_line))
        .block(Block::default().borders(Borders::ALL).title(" Model "));
    f.render_widget(model_bar, chunks[0]);

    // ── Chat messages ──
    let msg_height = chunks[1].height.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    if app.chat_messages.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Type a message and press Enter to chat.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "  Use Shift+M to cycle between models.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Make sure Ollama is running (ollama serve).",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for msg in &app.chat_messages {
            let (prefix, color) = match msg.role.as_str() {
                "user" => ("You", Color::Yellow),
                "assistant" => {
                    if msg.model.is_empty() {
                        ("AI", Color::Cyan)
                    } else {
                        (msg.model.as_str(), Color::Cyan)
                    }
                }
                _ => ("sys", Color::DarkGray),
            };

            lines.push(Line::from(vec![Span::styled(
                format!("  {} › ", prefix),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )]));

            // Wrap message content into lines
            let content = if msg.content.is_empty() && app.chat_streaming && msg.role == "assistant"
            {
                "▌"
            } else {
                &msg.content
            };

            for text_line in content.lines() {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(text_line, Style::default().fg(Color::White)),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    // Scroll: show the bottom of the conversation by default
    let total_lines = lines.len();
    let skip = if total_lines > msg_height {
        total_lines - msg_height - app.chat_scroll.min(total_lines.saturating_sub(msg_height))
    } else {
        0
    };
    let visible_lines: Vec<Line> = lines.into_iter().skip(skip).take(msg_height).collect();

    let messages_widget = Paragraph::new(visible_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Chat ({} messages) ", app.chat_messages.len())),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(messages_widget, chunks[1]);

    // ── Input field ──
    let input_display = if app.chat_streaming {
        " (waiting for response...)".to_string()
    } else {
        format!(" {}", app.chat_input)
    };

    let cursor_char = if !app.chat_streaming { "▌" } else { "" };

    let input_line = Line::from(vec![
        Span::styled(
            "›",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            input_display,
            if app.chat_streaming {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            },
        ),
        Span::styled(cursor_char, Style::default().fg(Color::Cyan)),
    ]);

    let input_widget = Paragraph::new(input_line).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Input (Enter=send, Esc=clear, Tab=switch) ")
            .border_style(if app.chat_streaming {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Cyan)
            }),
    );
    f.render_widget(input_widget, chunks[2]);
}

// ── Tab 6: Federation ───────────────────────────────────────────────────

fn draw_federation(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // status / config
            Constraint::Min(6),    // peer list + meta-graph
        ])
        .split(area);

    // ── Top: federation status ──
    let (addr_str, system_id_str, status_color) = if let Some(ref addr) = app.federation_addr {
        (addr.clone(), "active".to_string(), Color::Green)
    } else {
        (
            "(not enabled)".to_string(),
            "disabled".to_string(),
            Color::DarkGray,
        )
    };

    let status_text = vec![
        Line::from(vec![
            Span::styled("  Listen addr : ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                addr_str,
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Status      : ", Style::default().fg(Color::DarkGray)),
            Span::styled(system_id_str, Style::default().fg(status_color)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Imported    : ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.federation_imported),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw("   "),
            Span::styled("Exported : ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.federation_exported),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("   "),
            Span::styled("Conflicts : ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", app.federation_conflicts),
                Style::default().fg(if app.federation_conflicts > 0 {
                    Color::Red
                } else {
                    Color::Green
                }),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Start with: --federation 0.0.0.0:9000 --peer http://host:9000",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let status_block = Paragraph::new(status_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Federation Status "),
    );
    f.render_widget(status_block, chunks[0]);

    // ── Bottom: two columns — peers | meta-graph ──
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Peer list
    let peer_items: Vec<ListItem> = if app.federation_peers.is_empty() {
        vec![ListItem::new(Span::styled(
            "  (no peers configured)",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        app.federation_peers
            .iter()
            .map(|p| {
                ListItem::new(Line::from(vec![
                    Span::styled("  ● ", Style::default().fg(Color::Green)),
                    Span::styled(p.clone(), Style::default().fg(Color::White)),
                ]))
            })
            .collect()
    };

    let peer_list = List::new(peer_items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Peers ({}) ", app.federation_peers.len())),
    );
    f.render_widget(peer_list, bottom[0]);

    // Meta-graph info (read synchronously if available, else show placeholder)
    let meta_lines: Vec<Line> = if let Some(ref mg_arc) = app.federation_meta_graph {
        // Try a non-blocking read; fall back to placeholder if locked
        match mg_arc.try_read() {
            Ok(mg) => {
                let systems = mg.known_systems.len();
                let trust_edges = mg.trust_graph.edges.len();
                vec![
                    Line::from(vec![
                        Span::styled("  Systems     : ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("{}", systems),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("  Trust edges : ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("{}", trust_edges),
                            Style::default().fg(Color::Yellow),
                        ),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        "  (updates on sync)",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            }
            Err(_) => vec![Line::from(Span::styled(
                "  (syncing...)",
                Style::default().fg(Color::DarkGray),
            ))],
        }
    } else {
        vec![
            Line::from(Span::styled(
                "  No meta-graph (federation not active).",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  Launch with --federation <addr> to enable.",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };

    let meta_block = Paragraph::new(meta_lines)
        .block(Block::default().borders(Borders::ALL).title(" Meta-Graph "));
    f.render_widget(meta_block, bottom[1]);
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn truncate_str(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else if max <= 3 {
        s.chars().take(max).collect()
    } else {
        let head: String = s.chars().take(max - 3).collect();
        format!("{}...", head)
    }
}

fn code_agent_task_key(query: &str) -> String {
    let mut normalized = String::with_capacity(query.len());
    let mut last_dash = false;
    for ch in query.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            last_dash = false;
        } else if !last_dash {
            normalized.push('-');
            last_dash = true;
        }
    }
    let trimmed = normalized.trim_matches('-');
    if trimmed.is_empty() {
        "code-agent-task".to_string()
    } else {
        format!("code-agent-{}", trimmed)
    }
}

fn code_agent_task_sensitivity(query: &str) -> DataSensitivity {
    let lower = query.to_ascii_lowercase();
    if ["private key", "ssh key", "password", "secret", "credential"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        DataSensitivity::Secret
    } else if [".env", "token", "auth", "apikey", "api key", "certificate"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        DataSensitivity::Confidential
    } else {
        DataSensitivity::Internal
    }
}

fn code_agent_capability_key(tool_name: &str) -> String {
    match tool_name {
        "read" | "grep" | "find" | "ls" => "inspect-workspace".to_string(),
        "write" | "edit" => "modify-workspace".to_string(),
        "bash" => "execute-shell".to_string(),
        other => format!("tool::{other}"),
    }
}

fn code_agent_tool_quality(tool_name: &str, success: bool, duration_ms: u64) -> f64 {
    if !success {
        return 0.2;
    }

    let base = match tool_name {
        "write" | "edit" => 0.84,
        "bash" => 0.78,
        "read" | "grep" | "find" | "ls" => 0.74,
        _ => 0.76,
    };
    let latency_penalty = ((duration_ms as f64 / 30_000.0) * 0.08).clamp(0.0, 0.08);
    (base - latency_penalty).clamp(0.0, 1.0)
}

fn code_agent_tool_safe_for_sensitive_data(
    tool_name: &str,
    args: &serde_json::Value,
    success: bool,
    sensitivity: &DataSensitivity,
) -> bool {
    if !success {
        return false;
    }
    if matches!(
        sensitivity,
        DataSensitivity::Public | DataSensitivity::Internal
    ) {
        return true;
    }

    let args_text = args.to_string().to_ascii_lowercase();
    let risky_markers = [
        "curl ",
        "wget ",
        "scp ",
        "ssh ",
        "rsync ",
        "ftp ",
        "http://",
        "https://",
        "nc ",
        "netcat",
        "token",
        "secret",
        "password",
        "private key",
    ];
    match tool_name {
        "bash" => !risky_markers
            .iter()
            .any(|marker| args_text.contains(marker)),
        _ => true,
    }
}

#[cfg(test)]
mod code_agent_social_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_key_normalization_is_stable() {
        assert_eq!(
            code_agent_task_key("Fix .env token leak in CI"),
            "code-agent-fix-env-token-leak-in-ci"
        );
    }

    #[test]
    fn sensitivity_detection_marks_secret_queries() {
        assert!(matches!(
            code_agent_task_sensitivity("rotate private key and password"),
            DataSensitivity::Secret
        ));
        assert!(matches!(
            code_agent_task_sensitivity("inspect API token usage"),
            DataSensitivity::Confidential
        ));
        assert!(matches!(
            code_agent_task_sensitivity("refactor graph runtime"),
            DataSensitivity::Internal
        ));
    }

    #[test]
    fn sensitive_bash_egress_is_not_marked_safe() {
        assert!(!code_agent_tool_safe_for_sensitive_data(
            "bash",
            &json!({"command": "curl https://example.com/upload"}),
            true,
            &DataSensitivity::Confidential,
        ));
        assert!(code_agent_tool_safe_for_sensitive_data(
            "edit",
            &json!({"path": "src/main.rs"}),
            true,
            &DataSensitivity::Confidential,
        ));
    }
}

/// Get current role prompts for optimization.
/// Uses optimized prompts if available, otherwise returns defaults.
fn get_current_role_prompts(optimized: Option<&Vec<(String, String)>>) -> Vec<(String, String)> {
    if let Some(prompts) = optimized {
        return prompts.clone();
    }

    vec![
        ("Analyst".to_string(), "You are the Analyst in a formal debate. Present your strongest case.".to_string()),
        ("Challenger".to_string(), "You are the Challenger in a formal debate. Your goal is to dismantle the Analyst's position.".to_string()),
        ("Chair".to_string(), "You are the Chair adjudicating a formal debate. Review both positions and render a fair verdict.".to_string()),
    ]
}

/// Optimize role prompts after negative council outcomes (Integration 2).
/// Returns the optimized prompts for storage and reuse.
async fn optimize_role_prompts(prompts: &[(String, String)], model: &str) -> Vec<(String, String)> {
    use ollama_rs::generation::chat::request::ChatMessageRequest;
    use ollama_rs::{
        generation::chat::{ChatMessage, MessageRole},
        Ollama,
    };

    let ollama = Ollama::new("http://localhost".to_string(), 11434);
    let mut optimized = Vec::new();

    for (role_name, current_prompt) in prompts {
        let system_prompt = format!(
            "You are optimizing a {} role prompt for a Socratic council.\n\
             The current prompt produced poor outcomes.\n\
             Rewrite it to produce focused, non-redundant deliberation.",
            role_name
        );

        let user_prompt = format!(
            "CURRENT PROMPT:\n{}\n\n\
             Rewrite to:\n\
             1. Produce more focused deliberation\n\
             2. Avoid redundant statements\n\
             3. Advance more effectively toward synthesis\n\n\
             Respond with ONLY the rewritten prompt.",
            current_prompt
        );

        let messages = vec![
            ChatMessage::new(MessageRole::System, system_prompt),
            ChatMessage::new(MessageRole::User, user_prompt),
        ];

        let request = ChatMessageRequest::new(model.to_string(), messages);

        match ollama.send_chat_messages(request).await {
            Ok(response) => {
                let improved = response.message.content.trim().to_string();
                println!("Optimized {} prompt ({} chars)", role_name, improved.len());
                optimized.push((role_name.clone(), improved));
            }
            Err(e) => {
                println!("Failed to optimize {} prompt: {}", role_name, e);
                // Keep original on error
                optimized.push((role_name.clone(), current_prompt.clone()));
            }
        }
    }

    optimized
}

/// Evaluate code agent output quality (Integration 5)
async fn evaluate_code_output(output: &str, query: &str, model: &str) -> anyhow::Result<f64> {
    use ollama_rs::generation::chat::request::ChatMessageRequest;
    use ollama_rs::{
        generation::chat::{ChatMessage, MessageRole},
        Ollama,
    };

    let ollama = Ollama::new("http://localhost".to_string(), 11434);

    let system_prompt = "You evaluate code agent outputs.\n\
        Score 0-1: Does this response actually solve the stated task?\n\
        Is it concrete and executable?\n\
        Respond: SCORE: [0.0-1.0]";

    let user_prompt = format!(
        "TASK: {}\n\n\
         OUTPUT:\n{}\n\n\
         Score the output quality.",
        query, output
    );

    let messages = vec![
        ChatMessage::new(MessageRole::System, system_prompt.to_string()),
        ChatMessage::new(MessageRole::User, user_prompt.to_string()),
    ];

    let request = ChatMessageRequest::new(model.to_string(), messages);

    match ollama.send_chat_messages(request).await {
        Ok(response) => {
            let content = response.message.content;
            for line in content.lines() {
                if line.starts_with("SCORE:") {
                    if let Some(s) = line.split(':').nth(1) {
                        return Ok(s.trim().parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0));
                    }
                }
            }
            Ok(0.5)
        }
        Err(_) => Ok(0.5),
    }
}

/// Evaluate a mutation intent before applying (Integration 3)
pub async fn evaluate_mutation_intent(
    intent: &str,
    model: &str,
) -> anyhow::Result<(f64, Option<String>)> {
    use ollama_rs::generation::chat::request::ChatMessageRequest;
    use ollama_rs::{
        generation::chat::{ChatMessage, MessageRole},
        Ollama,
    };

    let ollama = Ollama::new("http://localhost".to_string(), 11434);

    let system_prompt = "You evaluate mutation intents for self-improvement.\n\
        Score based on:\n\
        - Contains specific, measurable changes (+0.3)\n\
        - Mentions how to reverse if wrong (+0.2)\n\
        - Avoids vague words like 'maybe', 'try', 'unclear' (+0.3)\n\
        - Has clear success criteria (+0.2)\n\
        \n\
        Respond:\n\
        SCORE: [0.0-1.0]\n\
        REWRITTEN: [improved version if score < 0.7, else 'NONE']";

    let user_prompt = format!("Mutation intent: {}", intent);

    let messages = vec![
        ChatMessage::new(MessageRole::System, system_prompt.to_string()),
        ChatMessage::new(MessageRole::User, user_prompt.to_string()),
    ];

    let request = ChatMessageRequest::new(model.to_string(), messages);

    match ollama.send_chat_messages(request).await {
        Ok(response) => {
            let content = response.message.content;
            let mut score = 0.5;
            let mut rewritten = None;

            for line in content.lines() {
                if line.starts_with("SCORE:") {
                    if let Some(s) = line.split(':').nth(1) {
                        score = s.trim().parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0);
                    }
                } else if line.starts_with("REWRITTEN:") {
                    let r = line.split(':').nth(1).unwrap_or("").trim();
                    if !r.is_empty() && r != "NONE" {
                        rewritten = Some(r.to_string());
                    }
                }
            }

            Ok((score, rewritten))
        }
        Err(_) => Ok((0.5, None)),
    }
}

/// Re-evaluate an old belief against recent experiences (Integration 4)
pub async fn reevaluate_belief(
    belief_content: &str,
    recent_experiences: &[String],
    model: &str,
) -> anyhow::Result<f64> {
    use ollama_rs::generation::chat::request::ChatMessageRequest;
    use ollama_rs::{
        generation::chat::{ChatMessage, MessageRole},
        Ollama,
    };

    if recent_experiences.is_empty() {
        return Ok(0.5);
    }

    let ollama = Ollama::new("http://localhost".to_string(), 11434);

    let system_prompt = "You re-evaluate beliefs against new evidence.\n\
        Score: Is this belief still supported by recent experiences?\n\
        Is it specific enough to be useful?\n\
        Respond: SCORE: [0.0-1.0]";

    let experiences_text = recent_experiences.join("\n");
    let user_prompt = format!(
        "BELIEF: {}\n\n\
         RECENT EXPERIENCES:\n{}\n\n\
         Score the belief's current validity.",
        belief_content, experiences_text
    );

    let messages = vec![
        ChatMessage::new(MessageRole::System, system_prompt.to_string()),
        ChatMessage::new(MessageRole::User, user_prompt.to_string()),
    ];

    let request = ChatMessageRequest::new(model.to_string(), messages);

    match ollama.send_chat_messages(request).await {
        Ok(response) => {
            let content = response.message.content;
            for line in content.lines() {
                if line.starts_with("SCORE:") {
                    if let Some(s) = line.split(':').nth(1) {
                        return Ok(s.trim().parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0));
                    }
                }
            }
            Ok(0.5)
        }
        Err(_) => Ok(0.5),
    }
}

/// Run a single council role (Analyst, Challenger, or Chair) and stream tokens.
/// Returns the full response after streaming completes.
async fn run_council_role(
    ollama: &Ollama,
    model: &str,
    system: &str,
    prompt: &str,
    council_tx: &tokio::sync::broadcast::Sender<String>,
    role_name: &str,
    round: usize,
) -> Result<String, String> {
    use tokio::time::{timeout, Duration, Instant};
    #[derive(Copy, Clone, PartialEq)]
    enum PendingKind {
        Token,
        Thinking,
    }
    let messages = vec![
        OllamaChatMsg::system(system.to_string()),
        OllamaChatMsg::user(prompt.to_string()),
    ];
    let request = ChatMessageRequest::new(model.to_string(), messages);

    use tokio_stream::StreamExt;
    let mut full_response = String::new();
    let mut parser = CouncilThinkingParser::default();
    let mut pending_kind: Option<PendingKind> = None;
    let mut pending_buf = String::new();
    let mut last_flush = Instant::now();
    let mut last_sync_len: usize = 0;
    let mut last_sync = Instant::now();

    let flush_pending =
        |pending_kind: &mut Option<PendingKind>,
         pending_buf: &mut String,
         council_tx: &tokio::sync::broadcast::Sender<String>| {
            if pending_kind.is_none() || pending_buf.is_empty() {
                return;
            }
            let payload = match pending_kind.unwrap() {
                PendingKind::Token => serde_json::json!({
                    "type": "token",
                    "round": round,
                    "persona": role_name,
                    "content": pending_buf.as_str(),
                }),
                PendingKind::Thinking => serde_json::json!({
                    "type": "thinking",
                    "round": round,
                    "persona": role_name,
                    "content": pending_buf.as_str(),
                }),
            };
            let _ = council_tx.send(payload.to_string());
            pending_buf.clear();
            *pending_kind = None;
        };

    // Hard timeouts: stream must start quickly, remain active, and finish within budget.
    let stream_start_timeout = Duration::from_secs(180);
    let idle_chunk_timeout = Duration::from_secs(60);
    let overall_timeout = Duration::from_secs(600);

    match timeout(
        stream_start_timeout,
        ollama.send_chat_messages_stream(request),
    )
    .await
    {
        Ok(Ok(mut stream)) => {
            let deadline = Instant::now() + overall_timeout;
            loop {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "{} timed out after {}s",
                        role_name,
                        overall_timeout.as_secs()
                    ));
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                let wait_for = idle_chunk_timeout.min(remaining);
                let next_item = match timeout(wait_for, stream.next()).await {
                    Ok(item) => item,
                    Err(_) => {
                        return Err(format!(
                            "{} stalled (no stream chunks for {}s)",
                            role_name,
                            wait_for.as_secs()
                        ));
                    }
                };
                let Some(result) = next_item else {
                    break;
                };
                match result {
                    Ok(chunk) => {
                        if !chunk.message.content.is_empty() {
                            full_response.push_str(&chunk.message.content);
                            for part in parser.push_chunk(&chunk.message.content) {
                                match part {
                                    CouncilStreamPart::Token(content) => {
                                        if pending_kind != Some(PendingKind::Token) {
                                            flush_pending(
                                                &mut pending_kind,
                                                &mut pending_buf,
                                                council_tx,
                                            );
                                            pending_kind = Some(PendingKind::Token);
                                        }
                                        pending_buf.push_str(&content);
                                    }
                                    CouncilStreamPart::Thinking(content) => {
                                        if pending_kind != Some(PendingKind::Thinking) {
                                            flush_pending(
                                                &mut pending_kind,
                                                &mut pending_buf,
                                                council_tx,
                                            );
                                            pending_kind = Some(PendingKind::Thinking);
                                        }
                                        pending_buf.push_str(&content);
                                    }
                                    CouncilStreamPart::ThinkingEnd => {
                                        flush_pending(
                                            &mut pending_kind,
                                            &mut pending_buf,
                                            council_tx,
                                        );
                                        let payload = serde_json::json!({
                                            "type": "thinking_end",
                                            "round": round,
                                            "persona": role_name,
                                        });
                                        let _ = council_tx.send(payload.to_string());
                                    }
                                }
                                if pending_buf.len() > 512
                                    || last_flush.elapsed() > Duration::from_millis(200)
                                {
                                    flush_pending(&mut pending_kind, &mut pending_buf, council_tx);
                                    last_flush = Instant::now();
                                }
                            }
                            if full_response.len().saturating_sub(last_sync_len) > 1500
                                || last_sync.elapsed() > Duration::from_secs(2)
                            {
                                let sync_payload = serde_json::json!({
                                    "type": "sync",
                                    "round": round,
                                    "persona": role_name,
                                    "content": full_response,
                                });
                                let _ = council_tx.send(sync_payload.to_string());
                                last_sync_len = full_response.len();
                                last_sync = Instant::now();
                            }
                        }
                        if chunk.done {
                            flush_pending(&mut pending_kind, &mut pending_buf, council_tx);
                            let sync_payload = serde_json::json!({
                                "type": "sync",
                                "round": round,
                                "persona": role_name,
                                "content": full_response,
                            });
                            let _ = council_tx.send(sync_payload.to_string());
                            for part in parser.finish() {
                                match part {
                                    CouncilStreamPart::Token(content) => {
                                        if pending_kind != Some(PendingKind::Token) {
                                            flush_pending(
                                                &mut pending_kind,
                                                &mut pending_buf,
                                                council_tx,
                                            );
                                            pending_kind = Some(PendingKind::Token);
                                        }
                                        pending_buf.push_str(&content);
                                    }
                                    CouncilStreamPart::Thinking(content) => {
                                        if pending_kind != Some(PendingKind::Thinking) {
                                            flush_pending(
                                                &mut pending_kind,
                                                &mut pending_buf,
                                                council_tx,
                                            );
                                            pending_kind = Some(PendingKind::Thinking);
                                        }
                                        pending_buf.push_str(&content);
                                    }
                                    CouncilStreamPart::ThinkingEnd => {
                                        flush_pending(
                                            &mut pending_kind,
                                            &mut pending_buf,
                                            council_tx,
                                        );
                                        let payload = serde_json::json!({
                                            "type": "thinking_end",
                                            "round": round,
                                            "persona": role_name,
                                        });
                                        let _ = council_tx.send(payload.to_string());
                                    }
                                }
                            }
                            flush_pending(&mut pending_kind, &mut pending_buf, council_tx);
                            break;
                        }
                    }
                    Err(e) => return Err(format!("{} stream error: {:?}", role_name, e)),
                }
            }
            Ok(full_response)
        }
        Ok(Err(e)) => Err(format!("{} failed: {}", role_name, e)),
        Err(_) => Err(format!(
            "{} stream start timed out after {}s",
            role_name,
            stream_start_timeout.as_secs()
        )),
    }
}

/// Strip <think>...</think> tags from DeepSeek-R1 reasoning traces.
/// Keeps the actual response, removes the chain-of-thought.
fn strip_think_tags(s: &str) -> String {
    // Handle nested/multiline think tags
    let mut result = s.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result[start..].find("</think>") {
            let remove_start = start;
            let remove_end = start + end + "</think>".len();
            result.replace_range(remove_start..remove_end, "");
        } else {
            // Unclosed think tag - remove from start to end
            result.replace_range(start.., "");
            break;
        }
    }
    result.trim().to_string()
}

#[derive(Default)]
struct ThinkingStreamParser {
    buffer: String,
    in_thinking: bool,
}

impl ThinkingStreamParser {
    fn push_chunk(&mut self, chunk: &str) -> Vec<ChatEvent> {
        self.buffer.push_str(chunk);
        let mut out = Vec::new();

        loop {
            if self.in_thinking {
                if let Some(end_idx) = self.buffer.find("</think>") {
                    let segment = self.buffer[..end_idx].to_string();
                    self.buffer.drain(..end_idx + "</think>".len());
                    self.in_thinking = false;
                    if !segment.is_empty() {
                        out.push(ChatEvent::Thinking(segment));
                    }
                    continue;
                }

                // Keep tag boundary in buffer in case the closing tag is split.
                let reserve = "</think>".len().saturating_sub(1);
                let emit_len = self.buffer.len().saturating_sub(reserve);
                if emit_len == 0 {
                    break;
                }
                let segment = self.buffer[..emit_len].to_string();
                self.buffer.drain(..emit_len);
                if !segment.is_empty() {
                    out.push(ChatEvent::Thinking(segment));
                }
                break;
            } else {
                if let Some(start_idx) = self.buffer.find("<think>") {
                    let segment = self.buffer[..start_idx].to_string();
                    self.buffer.drain(..start_idx + "<think>".len());
                    self.in_thinking = true;
                    if !segment.is_empty() {
                        out.push(ChatEvent::Token(segment));
                    }
                    continue;
                }

                // Keep tag boundary in buffer in case the opening tag is split.
                let reserve = "<think>".len().saturating_sub(1);
                let emit_len = self.buffer.len().saturating_sub(reserve);
                if emit_len == 0 {
                    break;
                }
                let segment = self.buffer[..emit_len].to_string();
                self.buffer.drain(..emit_len);
                if !segment.is_empty() {
                    out.push(ChatEvent::Token(segment));
                }
                break;
            }
        }

        out
    }

    fn finish(&mut self) -> Vec<ChatEvent> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let remaining = std::mem::take(&mut self.buffer);
        if self.in_thinking {
            self.in_thinking = false;
            vec![ChatEvent::Thinking(remaining)]
        } else {
            vec![ChatEvent::Token(remaining)]
        }
    }
}
