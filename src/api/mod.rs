//! REST API module for HSM-II.
//!
//! Provides an axum-based JSON API over the hyper-stigmergic world state,
//! exposing beliefs, skills, context ranking, predictions, trust, council
//! decisions, world snapshots, governance text, and health checks.
//!
//! **Guardrails:** `POST /api/beliefs` is rate-limited (`HSM_API_BELIEFS_PER_SEC`, default 25/s).
//! High-confidence automated beliefs are capped unless evidence / `human_committed` is supplied
//! (see `world_guardrails` + `add_belief_with_extras`).
//!
//! **Architecture:** `GET /api/architecture` returns the embedded blueprint (`architecture/hsm-ii-blueprint.ron`)
//! plus optional runtime counts when a world is mounted.
//!
//! **Appliance:** `GET|POST /api/appliance/workspace/:thread_id`, uploads/artifacts listing, multipart upload,
//! and `POST /api/agent-skills/install` (`.skill` zip → `<appliance_home>/skills`).

mod appliance;
mod llm_stream;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::architecture_blueprint::{
    embedded_blueprint, ArchitectureBlueprint, WorldArchitectureRuntime,
};
use crate::council::CouncilDecision;
use crate::council::Proposal;
use crate::federation::trust::TrustEdge;
use crate::governance;
use crate::harness::{ApprovalOutcome, ApprovalService, PendingApproval};
use crate::hyper_stigmergy::{AddBeliefExtras, Belief, BeliefSource, HyperStigmergicMorphogenesis};
use crate::meta_graph::MetaGraph;
use crate::real::WorldSnapshot;
use crate::scenario_simulator::{PredictionReport, ScenarioSimulator, ScenarioSimulatorConfig};
use crate::skill::Skill;

// ── Shared State ───────────────────────────────────────────────────────────

/// Inner shared state protected by RwLock.
pub struct SharedState {
    pub world: Option<HyperStigmergicMorphogenesis>,
    pub meta_graph: Option<MetaGraph>,
    pub council_decisions: Vec<CouncilDecision>,
    pub prediction_reports: Vec<PredictionReport>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            world: None,
            meta_graph: None,
            council_decisions: Vec::new(),
            prediction_reports: Vec::new(),
        }
    }

    pub fn with_world(world: HyperStigmergicMorphogenesis) -> Self {
        Self {
            world: Some(world),
            meta_graph: None,
            council_decisions: Vec::new(),
            prediction_reports: Vec::new(),
        }
    }
}

#[derive(Default)]
struct BeliefPostLimiter {
    times: VecDeque<Instant>,
}

impl BeliefPostLimiter {
    fn check(&mut self, max_per_sec: u64) -> bool {
        let now = Instant::now();
        let window = Duration::from_secs(1);
        while self
            .times
            .front()
            .map_or(false, |t| now.duration_since(*t) > window)
        {
            self.times.pop_front();
        }
        if self.times.len() >= max_per_sec as usize {
            return false;
        }
        self.times.push_back(now);
        true
    }
}

/// Cheaply-cloneable handle passed to every handler via `axum::extract::State`.
#[derive(Clone)]
pub struct ApiState {
    pub inner: Arc<RwLock<SharedState>>,
    belief_post_limiter: Arc<tokio::sync::Mutex<BeliefPostLimiter>>,
    max_belief_posts_per_sec: u64,
    /// Optional Honcho state — populated when the personal agent is running.
    #[allow(dead_code)]
    pub honcho: Option<HonchoApiState>,
    /// Paperclip Intelligence Layer: **runtime** goals, DRIs, and capability tracking for every
    /// company/tenant served by this process. Populated by the unified `personal_agent` binary, **not**
    /// by importing a company pack (packs add agent/skill **content** elsewhere).
    pub intelligence: Option<Arc<tokio::sync::Mutex<crate::paperclip::IntelligenceLayer>>>,
    /// Data root for appliance routes (`HSM_APPLIANCE_HOME`, default `~/.hsmii`).
    pub appliance_home: std::path::PathBuf,
}

impl ApiState {
    pub fn new(shared: SharedState) -> Self {
        Self::from_shared(Arc::new(RwLock::new(shared)))
    }

    /// Build an `ApiState` from an already-wrapped `Arc<RwLock<SharedState>>`.
    ///
    /// Used by the unified runtime so the personal agent and API server share the
    /// exact same `SharedState` instance without an extra allocation.
    pub fn from_shared(inner: Arc<RwLock<SharedState>>) -> Self {
        let max_belief_posts_per_sec = std::env::var("HSM_API_BELIEFS_PER_SEC")
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|n: &u64| *n > 0)
            .unwrap_or(25);
        Self {
            inner,
            belief_post_limiter: Arc::new(tokio::sync::Mutex::new(BeliefPostLimiter::default())),
            max_belief_posts_per_sec,
            honcho: None,
            intelligence: None,
            appliance_home: crate::harness::appliance_home(),
        }
    }

    /// Attach Honcho state so the `/api/honcho/*` routes become active.
    pub fn with_honcho(mut self, honcho: HonchoApiState) -> Self {
        self.honcho = Some(honcho);
        self
    }

    /// Attach the shared Intelligence Layer so `/api/paperclip/*` routes reflect the live runtime
    /// (same `Arc` as the process that owns goals / DRIs / capabilities).
    pub fn with_intelligence(
        mut self,
        layer: Arc<tokio::sync::Mutex<crate::paperclip::IntelligenceLayer>>,
    ) -> Self {
        self.intelligence = Some(layer);
        self
    }
}

// ── Honcho shared state ────────────────────────────────────────────────────

/// Axum-extractable state for the `/api/honcho/*` routes.
#[derive(Clone)]
pub struct HonchoApiState {
    /// Path to `~/.hsmii/honcho/` directory.
    pub honcho_home: std::path::PathBuf,
    /// Shared HybridMemory (same instance as in the personal agent).
    pub hybrid_memory: Arc<RwLock<crate::memory::HybridMemory>>,
}

// ── Request / Response DTOs ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateBeliefRequest {
    pub content: String,
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    #[serde(default)]
    pub supporting_evidence: Vec<String>,
    #[serde(default)]
    pub owner_namespace: Option<String>,
    #[serde(default)]
    pub supersedes_belief_id: Option<usize>,
    #[serde(default)]
    pub evidence_belief_ids: Vec<usize>,
    /// Dual-control / procedural anchor for high-trust API ingests.
    #[serde(default)]
    pub human_committed: bool,
}

fn default_confidence() -> f64 {
    0.5
}

#[derive(Deserialize)]
pub struct RankRequest {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
}

fn default_top_k() -> usize {
    5
}

#[derive(Serialize)]
pub struct RankedItem {
    pub id: usize,
    pub content: String,
    pub relevance: f64,
}

#[derive(Deserialize)]
pub struct SimulateRequest {
    pub topic: String,
    pub seeds: Vec<String>,
    #[serde(default)]
    pub variables: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpsertTrustRequest {
    pub from_system: String,
    pub to_system: String,
    pub score: f64,
}

#[derive(Serialize)]
pub struct TrustEdgeResponse {
    pub from_system: String,
    pub to_system: String,
    pub score: f64,
    pub successful_imports: u64,
    pub failed_imports: u64,
    pub last_interaction: u64,
}

#[derive(Deserialize)]
pub struct ProposeRequest {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub proposer_id: u64,
    #[serde(default = "default_complexity")]
    pub complexity: f64,
    #[serde(default = "default_urgency")]
    pub urgency: f64,
}

fn default_complexity() -> f64 {
    0.5
}
fn default_urgency() -> f64 {
    0.5
}

#[derive(Serialize)]
pub struct ProposeResponse {
    pub proposal_id: String,
    pub accepted: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct CreateBeliefResponse {
    pub id: usize,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct GovernanceBundle {
    pub raci_summary: String,
    pub incident_playbook: String,
    pub federation_operations: String,
}

#[derive(Deserialize)]
pub struct ApprovalDecisionRequest {
    pub key: String,
    pub outcome: String,
    #[serde(default)]
    pub actor: Option<String>,
}

#[derive(Serialize)]
pub struct ApprovalDecisionResponse {
    pub ok: bool,
    pub key: String,
    pub outcome: String,
}

#[derive(Serialize)]
pub struct SkillSummary {
    pub id: String,
    pub title: String,
    pub principle: String,
    pub confidence: f64,
    pub usage_count: u64,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

// ── Helpers ────────────────────────────────────────────────────────────────

fn require_world(
    world: &Option<HyperStigmergicMorphogenesis>,
) -> Result<&HyperStigmergicMorphogenesis, (StatusCode, String)> {
    world.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "World not initialized".to_string(),
    ))
}

fn require_world_mut(
    world: &mut Option<HyperStigmergicMorphogenesis>,
) -> Result<&mut HyperStigmergicMorphogenesis, (StatusCode, String)> {
    world.as_mut().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "World not initialized".to_string(),
    ))
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Router Factory ─────────────────────────────────────────────────────────

/// Build the complete API router with shared state.
pub fn api_router(state: ApiState) -> Router {
    // Honcho sub-router (active only when HonchoApiState is provided)
    let honcho_routes = if let Some(ref hs) = state.honcho {
        Router::new()
            .route("/api/honcho/peers", get(honcho_list_peers))
            .route("/api/honcho/peers/:id", get(honcho_get_peer))
            .route("/api/honcho/peers/:id/context", get(honcho_packed_context))
            .route("/api/honcho/memory", get(honcho_memory_stats))
            .route("/api/honcho/memory/entries", get(honcho_memory_entries))
            .route(
                "/api/honcho/memory/entry/:id",
                delete(honcho_memory_delete_entry),
            )
            .route(
                "/api/honcho/sessions/:id/visibility",
                get(honcho_get_visibility).post(honcho_set_visibility),
            )
            .with_state(hs.clone())
    } else {
        Router::new()
    };

    // Paperclip Intelligence Layer routes (active only when intelligence is provided)
    let paperclip_routes = if let Some(ref il) = state.intelligence {
        Router::new()
            .route("/api/paperclip/summary", get(paperclip_summary))
            .route(
                "/api/paperclip/goals",
                get(paperclip_list_goals).post(paperclip_create_goal),
            )
            .route("/api/paperclip/goals/:id", get(paperclip_get_goal))
            .route(
                "/api/paperclip/goals/:id/complete",
                post(paperclip_complete_goal),
            )
            .route(
                "/api/paperclip/capabilities",
                get(paperclip_list_capabilities),
            )
            .route(
                "/api/paperclip/dris",
                get(paperclip_list_dris).post(paperclip_register_dri),
            )
            .route("/api/paperclip/signals", post(paperclip_emit_signal))
            .route(
                "/api/paperclip/template",
                get(paperclip_export_template).post(paperclip_import_template),
            )
            .with_state(il.clone())
    } else {
        Router::new()
    };

    let appliance_routes = appliance::routes().with_state(state.clone());

    Router::new()
        // Health
        .route("/api/health", get(health))
        .route("/api/llm/chat/stream", post(llm_stream::llm_chat_stream))
        .route("/api/architecture", get(get_architecture))
        .route("/api/governance", get(get_governance))
        .route("/api/approvals/pending", get(list_pending_approvals))
        .route("/api/approvals/decide", post(decide_approval))
        // Beliefs
        .route("/api/beliefs", get(list_beliefs).post(create_belief))
        .route("/api/beliefs/:id", get(get_belief).delete(delete_belief))
        // Skills
        .route("/api/skills", get(list_skills).post(create_skill))
        .route("/api/skills/:id", get(get_skill))
        // Context
        .route("/api/context/rank", post(rank_context))
        // Predictions
        .route("/api/predictions", get(list_predictions))
        .route("/api/predictions/simulate", post(simulate_prediction))
        // Trust
        .route("/api/trust", get(list_trust).put(upsert_trust))
        // Council
        .route("/api/council/decisions", get(list_council_decisions))
        .route("/api/council/propose", post(council_propose))
        // World
        .route("/api/world", get(get_world))
        .route("/api/world/:tick", get(get_world_at_tick))
        .route("/api/world/tick", post(advance_tick))
        .with_state(state)
        .merge(honcho_routes)
        .merge(paperclip_routes)
        .merge(appliance_routes)
}

// ── Health ─────────────────────────────────────────────────────────────────

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: "0.1.0".to_string(),
    })
}

#[derive(Serialize)]
pub struct ArchitectureApiResponse {
    pub blueprint: ArchitectureBlueprint,
    pub runtime: Option<WorldArchitectureRuntime>,
}

async fn get_architecture(State(state): State<ApiState>) -> Json<ArchitectureApiResponse> {
    let guard = state.inner.read().await;
    let runtime = guard.world.as_ref().map(|w| w.architecture_stats());
    Json(ArchitectureApiResponse {
        blueprint: embedded_blueprint(),
        runtime,
    })
}

async fn get_governance() -> Json<GovernanceBundle> {
    Json(GovernanceBundle {
        raci_summary: governance::RACI_SUMMARY.to_string(),
        incident_playbook: governance::INCIDENT_PLAYBOOK.to_string(),
        federation_operations: governance::FEDERATION_OPERATIONS.to_string(),
    })
}

async fn list_pending_approvals() -> ApiResult<Vec<PendingApproval>> {
    let svc = ApprovalService::from_env();
    let pending = svc
        .list_pending()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(pending))
}

async fn decide_approval(
    Json(req): Json<ApprovalDecisionRequest>,
) -> ApiResult<ApprovalDecisionResponse> {
    let outcome = match req.outcome.to_ascii_lowercase().as_str() {
        "allow" => ApprovalOutcome::Allow,
        "deny" => ApprovalOutcome::Deny,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "outcome must be `allow` or `deny`".to_string(),
            ))
        }
    };
    let actor = req.actor.unwrap_or_else(|| "api".to_string());
    let svc = ApprovalService::from_env();
    svc.decide(&req.key, outcome.clone(), &actor)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ApprovalDecisionResponse {
        ok: true,
        key: req.key,
        outcome: match outcome {
            ApprovalOutcome::Allow => "allow".to_string(),
            ApprovalOutcome::Deny => "deny".to_string(),
        },
    }))
}

// ── Beliefs ────────────────────────────────────────────────────────────────

async fn list_beliefs(State(state): State<ApiState>) -> ApiResult<Vec<Belief>> {
    let guard = state.inner.read().await;
    let world = require_world(&guard.world)?;
    Ok(Json(world.beliefs.clone()))
}

async fn get_belief(State(state): State<ApiState>, Path(id): Path<usize>) -> ApiResult<Belief> {
    let guard = state.inner.read().await;
    let world = require_world(&guard.world)?;
    world
        .beliefs
        .iter()
        .find(|b| b.id == id)
        .cloned()
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, format!("Belief {} not found", id)))
}

async fn create_belief(
    State(state): State<ApiState>,
    Json(req): Json<CreateBeliefRequest>,
) -> ApiResult<CreateBeliefResponse> {
    {
        let mut lim = state.belief_post_limiter.lock().await;
        if !lim.check(state.max_belief_posts_per_sec) {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "Belief create rate limit (adjust HSM_API_BELIEFS_PER_SEC)".to_string(),
            ));
        }
    }

    let mut guard = state.inner.write().await;
    let world = require_world_mut(&mut guard.world)?;

    let id = world.add_belief_with_extras(
        &req.content,
        req.confidence.clamp(0.0, 1.0),
        BeliefSource::UserProvided,
        AddBeliefExtras {
            owner_namespace: req.owner_namespace,
            supersedes_belief_id: req.supersedes_belief_id,
            evidence_belief_ids: req.evidence_belief_ids,
            human_committed: req.human_committed,
            supporting_evidence: req.supporting_evidence,
        },
    );
    Ok(Json(CreateBeliefResponse { id }))
}

async fn delete_belief(
    State(state): State<ApiState>,
    Path(id): Path<usize>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut guard = state.inner.write().await;
    let world = require_world_mut(&mut guard.world)?;

    let before = world.beliefs.len();
    world.beliefs.retain(|b| b.id != id);
    if world.beliefs.len() < before {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, format!("Belief {} not found", id)))
    }
}

// ── Skills ─────────────────────────────────────────────────────────────────

async fn list_skills(State(state): State<ApiState>) -> ApiResult<Vec<SkillSummary>> {
    let guard = state.inner.read().await;
    let world = require_world(&guard.world)?;
    let summaries: Vec<SkillSummary> = world
        .skill_bank
        .general_skills
        .iter()
        .map(|s| SkillSummary {
            id: s.id.clone(),
            title: s.title.clone(),
            principle: s.principle.clone(),
            confidence: s.confidence,
            usage_count: s.usage_count,
        })
        .collect();
    Ok(Json(summaries))
}

async fn get_skill(State(state): State<ApiState>, Path(id): Path<String>) -> ApiResult<Skill> {
    let guard = state.inner.read().await;
    let world = require_world(&guard.world)?;
    world
        .skill_bank
        .general_skills
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, format!("Skill {} not found", id)))
}

async fn create_skill(State(state): State<ApiState>, Json(skill): Json<Skill>) -> ApiResult<Skill> {
    let mut guard = state.inner.write().await;
    let world = require_world_mut(&mut guard.world)?;
    let returned = skill.clone();
    world.skill_bank.general_skills.push(skill);
    Ok(Json(returned))
}

// ── Context Ranking ────────────────────────────────────────────────────────

async fn rank_context(
    State(state): State<ApiState>,
    Json(req): Json<RankRequest>,
) -> ApiResult<Vec<RankedItem>> {
    let guard = state.inner.read().await;
    let world = require_world(&guard.world)?;

    // Simple text-similarity ranking over beliefs using Jaccard keyword overlap.
    let query_lower = req.query.to_lowercase();
    let query_words: std::collections::HashSet<&str> = query_lower.split_whitespace().collect();

    let mut scored: Vec<RankedItem> = world
        .beliefs
        .iter()
        .map(|b| {
            let content_lower = b.content.to_lowercase();
            let content_words: std::collections::HashSet<_> =
                content_lower.split_whitespace().collect();
            let intersection = query_words
                .iter()
                .filter(|w| content_words.contains(*w))
                .count() as f64;
            let union = (query_words.len() + content_words.len()) as f64 - intersection;
            let relevance = if union > 0.0 {
                (intersection / union) * b.confidence
            } else {
                0.0
            };
            RankedItem {
                id: b.id,
                content: b.content.clone(),
                relevance,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.relevance
            .partial_cmp(&a.relevance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(req.top_k);
    Ok(Json(scored))
}

// ── Predictions ────────────────────────────────────────────────────────────

async fn list_predictions(State(state): State<ApiState>) -> ApiResult<Vec<PredictionReport>> {
    let guard = state.inner.read().await;
    Ok(Json(guard.prediction_reports.clone()))
}

async fn simulate_prediction(
    State(state): State<ApiState>,
    Json(req): Json<SimulateRequest>,
) -> ApiResult<PredictionReport> {
    // Verify the world is available before running the simulation.
    {
        let guard = state.inner.read().await;
        require_world(&guard.world)?;
    }

    let simulator = ScenarioSimulator::new(ScenarioSimulatorConfig::default());
    let variables = if req.variables.is_empty() {
        None
    } else {
        Some(req.variables.as_slice())
    };

    let report = simulator
        .simulate(&req.topic, &req.seeds, variables)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // Persist the report in memory.
    {
        let mut guard = state.inner.write().await;
        guard.prediction_reports.push(report.clone());
    }

    Ok(Json(report))
}

// ── Trust ──────────────────────────────────────────────────────────────────

async fn list_trust(State(state): State<ApiState>) -> ApiResult<Vec<TrustEdgeResponse>> {
    let guard = state.inner.read().await;

    let edges: Vec<TrustEdgeResponse> = match &guard.meta_graph {
        Some(mg) => mg
            .trust_graph
            .edges
            .iter()
            .map(|((from, to), edge)| TrustEdgeResponse {
                from_system: from.clone(),
                to_system: to.clone(),
                score: edge.score,
                successful_imports: edge.successful_imports,
                failed_imports: edge.failed_imports,
                last_interaction: edge.last_interaction,
            })
            .collect(),
        None => Vec::new(),
    };

    Ok(Json(edges))
}

async fn upsert_trust(
    State(state): State<ApiState>,
    Json(req): Json<UpsertTrustRequest>,
) -> ApiResult<TrustEdgeResponse> {
    let mut guard = state.inner.write().await;

    let mg = guard.meta_graph.as_mut().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Federation meta-graph not initialized".to_string(),
    ))?;

    let from = req.from_system.clone();
    let to = req.to_system.clone();
    let ts = now_ts();

    let edge = mg
        .trust_graph
        .edges
        .entry((from.clone(), to.clone()))
        .or_insert_with(|| TrustEdge {
            score: 0.0,
            successful_imports: 0,
            failed_imports: 0,
            last_interaction: ts,
        });

    edge.score = req.score.clamp(0.0, 1.0);
    edge.last_interaction = ts;

    let resp = TrustEdgeResponse {
        from_system: from,
        to_system: to,
        score: edge.score,
        successful_imports: edge.successful_imports,
        failed_imports: edge.failed_imports,
        last_interaction: edge.last_interaction,
    };

    Ok(Json(resp))
}

// ── Council ────────────────────────────────────────────────────────────────

async fn list_council_decisions(State(state): State<ApiState>) -> ApiResult<Vec<CouncilDecision>> {
    let guard = state.inner.read().await;
    Ok(Json(guard.council_decisions.clone()))
}

async fn council_propose(
    State(state): State<ApiState>,
    Json(req): Json<ProposeRequest>,
) -> ApiResult<ProposeResponse> {
    let guard = state.inner.read().await;
    let _world = require_world(&guard.world)?;

    let proposal_id = uuid::Uuid::new_v4().to_string();
    let mut proposal = Proposal::new(&proposal_id, &req.title, &req.description, req.proposer_id);
    proposal.complexity = req.complexity.clamp(0.0, 1.0);
    proposal.urgency = req.urgency.clamp(0.0, 1.0);

    // Accept the proposal for asynchronous council deliberation.
    // Full deliberation would be driven by the council engine in the main loop.
    Ok(Json(ProposeResponse {
        proposal_id,
        accepted: true,
        message: "Proposal submitted for council deliberation".to_string(),
    }))
}

// ── World ──────────────────────────────────────────────────────────────────

async fn get_world(State(state): State<ApiState>) -> ApiResult<WorldSnapshot> {
    let guard = state.inner.read().await;
    let world = require_world(&guard.world)?;
    Ok(Json(WorldSnapshot::from(world)))
}

async fn get_world_at_tick(
    State(state): State<ApiState>,
    Path(tick): Path<u64>,
) -> ApiResult<WorldSnapshot> {
    let guard = state.inner.read().await;
    let world = require_world(&guard.world)?;

    // Only the current snapshot is held in memory.
    if world.tick_count == tick {
        Ok(Json(WorldSnapshot::from(world)))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            format!(
                "Snapshot at tick {} not available (current tick: {})",
                tick, world.tick_count
            ),
        ))
    }
}

async fn advance_tick(State(state): State<ApiState>) -> ApiResult<WorldSnapshot> {
    let mut guard = state.inner.write().await;
    let world = require_world_mut(&mut guard.world)?;

    world.tick();

    Ok(Json(WorldSnapshot::from(&*world)))
}

// ── Honcho handlers ────────────────────────────────────────────────────────

use axum::extract::Query as AxumQuery;

#[derive(Serialize)]
struct PeerListResponse {
    peers: Vec<serde_json::Value>,
    total: usize,
}

/// `GET /api/honcho/peers` — list all peers for which a representation exists.
async fn honcho_list_peers(
    State(hs): State<HonchoApiState>,
) -> Result<Json<PeerListResponse>, StatusCode> {
    let peers_dir = hs.honcho_home.join("peers");
    let mut peers = Vec::new();

    if let Ok(mut dir) = tokio::fs::read_dir(&peers_dir).await {
        while let Ok(Some(entry)) = dir.next_entry().await {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(raw) = tokio::fs::read_to_string(entry.path()).await {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                        peers.push(v);
                    }
                }
            }
        }
    }

    let total = peers.len();
    Ok(Json(PeerListResponse { peers, total }))
}

/// `GET /api/honcho/peers/:id` — get the full `UserRepresentation` for a peer.
async fn honcho_get_peer(
    State(hs): State<HonchoApiState>,
    Path(peer_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::honcho::user_representation::UserRepresentation;
    let repr = UserRepresentation::load(&hs.honcho_home, &peer_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::to_value(repr).unwrap_or_default()))
}

#[derive(Deserialize)]
struct PackedContextQuery {
    #[serde(default = "default_context_budget")]
    budget: usize,
}

fn default_context_budget() -> usize {
    4096
}

/// `GET /api/honcho/peers/:id/context?budget=N` — token-budget-aware packed context.
async fn honcho_packed_context(
    State(hs): State<HonchoApiState>,
    Path(peer_id): Path<String>,
    AxumQuery(q): AxumQuery<PackedContextQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::honcho::context_packer::{ContextBudget, PackedContextBuilder};
    use crate::honcho::user_representation::UserRepresentation;

    let repr = UserRepresentation::load(&hs.honcho_home, &peer_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let packed = PackedContextBuilder::new(ContextBudget::from_total(q.budget))
        .with_peer_representation(repr)
        .build();

    Ok(Json(serde_json::to_value(packed).unwrap_or_default()))
}

#[derive(Serialize)]
struct HonchoMemoryStatsResponse {
    entity_summaries: usize,
    total_entries: usize,
    beliefs: usize,
    experiences: usize,
}

/// `GET /api/honcho/memory` — stats on the Honcho HybridMemory store.
async fn honcho_memory_stats(State(hs): State<HonchoApiState>) -> Json<HonchoMemoryStatsResponse> {
    let mem = hs.hybrid_memory.read().await;
    let stats = &mem.stats;
    Json(HonchoMemoryStatsResponse {
        entity_summaries: stats.entity_summaries,
        total_entries: stats.total_entries,
        beliefs: stats.beliefs,
        experiences: stats.experiences,
    })
}

#[derive(Deserialize)]
struct HonchoMemoryEntriesQuery {
    #[serde(default = "default_honcho_memory_limit")]
    limit: usize,
}

fn default_honcho_memory_limit() -> usize {
    200
}

/// `GET /api/honcho/memory/entries?limit=N` — recent hybrid-memory rows (Honcho + agent), no embeddings.
async fn honcho_memory_entries(
    State(hs): State<HonchoApiState>,
    AxumQuery(q): AxumQuery<HonchoMemoryEntriesQuery>,
) -> Json<Vec<crate::memory::MemoryEntryPreview>> {
    let mem = hs.hybrid_memory.read().await;
    let lim = q.limit.clamp(1, 2000);
    Json(mem.list_entries_preview(lim))
}

/// `DELETE /api/honcho/memory/entry/:id` — remove one memory row by id.
async fn honcho_memory_delete_entry(
    State(hs): State<HonchoApiState>,
    Path(id): Path<usize>,
) -> StatusCode {
    let mut mem = hs.hybrid_memory.write().await;
    if mem.remove_entry_by_id(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// `GET /api/honcho/sessions/:id/visibility` — get the session visibility config.
async fn honcho_get_visibility(
    State(hs): State<HonchoApiState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::honcho::session_visibility::SessionVisibility;
    match SessionVisibility::load(&hs.honcho_home, &session_id).await {
        Ok(Some(vis)) => Ok(Json(serde_json::to_value(vis).unwrap_or_default())),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[derive(Deserialize)]
struct SetVisibilityRequest {
    participants: Vec<String>,
    /// Map of observer_id → list of sender_ids they can see.
    #[serde(default)]
    restrictions: std::collections::HashMap<String, Vec<String>>,
}

/// `POST /api/honcho/sessions/:id/visibility` — create/update session visibility.
async fn honcho_set_visibility(
    State(hs): State<HonchoApiState>,
    Path(session_id): Path<String>,
    Json(req): Json<SetVisibilityRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::honcho::session_visibility::SessionVisibility;

    let mut vis = SessionVisibility::new_open(session_id, req.participants);
    for (observer, senders) in req.restrictions {
        vis.matrix.restrict(observer, senders);
    }
    vis.save(&hs.honcho_home)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::to_value(&vis).unwrap_or_default()))
}

// ── Paperclip Intelligence Layer handlers ─────────────────────────────────

type IntelligenceState = Arc<tokio::sync::Mutex<crate::paperclip::IntelligenceLayer>>;

/// `GET /api/paperclip/summary` — dashboard overview.
async fn paperclip_summary(State(il): State<IntelligenceState>) -> Json<serde_json::Value> {
    let layer = il.lock().await;
    Json(layer.summary())
}

/// `GET /api/paperclip/goals` — list all goals.
async fn paperclip_list_goals(State(il): State<IntelligenceState>) -> Json<serde_json::Value> {
    let layer = il.lock().await;
    let goals: Vec<_> = layer.list_goals();
    Json(serde_json::to_value(goals).unwrap_or_default())
}

#[derive(Deserialize)]
struct CreateGoalRequest {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    required_capabilities: Vec<String>,
    #[serde(default)]
    assignee_ref: Option<String>,
    #[serde(default)]
    assignee_type: Option<String>, // "ic", "dri", "player_coach"
    #[serde(default)]
    assignee_domain: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// `POST /api/paperclip/goals` — create a new goal.
async fn paperclip_create_goal(
    State(il): State<IntelligenceState>,
    Json(req): Json<CreateGoalRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use crate::paperclip::goal::*;

    let priority = match req.priority.as_deref() {
        Some("low") => GoalPriority::Low,
        Some("high") => GoalPriority::High,
        Some("critical") => GoalPriority::Critical,
        _ => GoalPriority::Medium,
    };

    let assignee = match (req.assignee_ref.as_deref(), req.assignee_type.as_deref()) {
        (Some(r), Some("ic")) => GoalAssignee::Ic {
            agent_ref: r.to_string(),
            capability_id: req
                .required_capabilities
                .first()
                .cloned()
                .unwrap_or_default(),
        },
        (Some(r), Some("dri")) => GoalAssignee::Dri {
            agent_ref: r.to_string(),
            domain: req.assignee_domain.clone().unwrap_or_default(),
        },
        (Some(r), Some("player_coach")) => GoalAssignee::PlayerCoach {
            agent_ref: r.to_string(),
            mentee_refs: Vec::new(),
        },
        _ => GoalAssignee::Unassigned,
    };

    let mut goal = Goal::new(uuid::Uuid::new_v4().to_string(), req.title)
        .with_description(req.description)
        .with_priority(priority)
        .with_assignee(assignee)
        .with_capabilities(req.required_capabilities);
    goal.tags = req.tags;

    let mut layer = il.lock().await;
    let id = layer.add_goal(goal);

    Ok(Json(serde_json::json!({ "id": id })))
}

/// `GET /api/paperclip/goals/:id` — get a single goal.
async fn paperclip_get_goal(
    State(il): State<IntelligenceState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let layer = il.lock().await;
    layer
        .get_goal(&id)
        .map(|g| Json(serde_json::to_value(g).unwrap_or_default()))
        .ok_or(StatusCode::NOT_FOUND)
}

/// `POST /api/paperclip/goals/:id/complete` — mark a goal as done.
async fn paperclip_complete_goal(
    State(il): State<IntelligenceState>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let mut layer = il.lock().await;
    if layer.get_goal(&id).is_some() {
        layer.complete_goal(&id);
        Ok(StatusCode::OK)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// `GET /api/paperclip/capabilities` — list all capabilities with health.
async fn paperclip_list_capabilities(
    State(il): State<IntelligenceState>,
) -> Json<serde_json::Value> {
    let layer = il.lock().await;
    let caps: Vec<_> = layer.capabilities.all().collect();
    Json(serde_json::json!({
        "capabilities": serde_json::to_value(caps).unwrap_or_default(),
        "overall_health": layer.capabilities.health(),
    }))
}

/// `GET /api/paperclip/dris` — list all DRIs.
async fn paperclip_list_dris(State(il): State<IntelligenceState>) -> Json<serde_json::Value> {
    let layer = il.lock().await;
    let dris: Vec<_> = layer.dri_registry.all().collect();
    Json(serde_json::to_value(dris).unwrap_or_default())
}

#[derive(Deserialize)]
struct RegisterDriRequest {
    id: String,
    name: String,
    agent_ref: String,
    domains: Vec<String>,
}

/// `POST /api/paperclip/dris` — register a new DRI.
async fn paperclip_register_dri(
    State(il): State<IntelligenceState>,
    Json(req): Json<RegisterDriRequest>,
) -> Json<serde_json::Value> {
    use crate::paperclip::dri::DriEntry;

    let entry = DriEntry::new(req.id.clone(), req.name, req.agent_ref).with_domains(req.domains);

    let mut layer = il.lock().await;
    layer.dri_registry.register(entry);

    Json(serde_json::json!({ "id": req.id, "registered": true }))
}

#[derive(Deserialize)]
struct EmitSignalRequest {
    kind: String,
    description: String,
    #[serde(default = "default_severity")]
    severity: f64,
    #[serde(default)]
    metadata: std::collections::HashMap<String, serde_json::Value>,
}

fn default_severity() -> f64 {
    0.5
}

/// `POST /api/paperclip/signals` — emit a signal into the Intelligence Layer.
async fn paperclip_emit_signal(
    State(il): State<IntelligenceState>,
    Json(req): Json<EmitSignalRequest>,
) -> Json<serde_json::Value> {
    use crate::paperclip::intelligence::{Signal, SignalKind};

    let kind = match req.kind.as_str() {
        "external" => SignalKind::ExternalSignal {
            category: req
                .metadata
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
        },
        "custom" => SignalKind::Custom {
            label: req.description.clone(),
        },
        other => SignalKind::Custom {
            label: other.to_string(),
        },
    };

    let signal = Signal {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        source: "api".into(),
        description: req.description,
        severity: req.severity,
        timestamp: now_ts(),
        metadata: req.metadata,
    };

    let signal_id = signal.id.clone();
    let mut layer = il.lock().await;
    layer.emit_signal(signal);

    Json(serde_json::json!({ "signal_id": signal_id, "queued": true }))
}

/// `GET /api/paperclip/template` — export the live Intelligence Layer as a company template.
async fn paperclip_export_template(State(il): State<IntelligenceState>) -> Json<serde_json::Value> {
    let layer = il.lock().await;
    let template = crate::paperclip::template::CompanyTemplate::from_intelligence_layer(&layer);
    Json(serde_json::to_value(template).unwrap_or_default())
}

/// `POST /api/paperclip/template` — import a company template.
async fn paperclip_import_template(
    State(il): State<IntelligenceState>,
    Json(template): Json<crate::paperclip::template::CompanyTemplate>,
) -> Json<serde_json::Value> {
    let mut layer = il.lock().await;
    template.apply_to(&mut layer);
    Json(serde_json::json!({
        "imported": true,
        "capabilities": layer.capabilities.len(),
        "dris": layer.dri_registry.len(),
        "goals": layer.list_goals().len(),
    }))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response_serialization() {
        let resp = HealthResponse {
            status: "ok".to_string(),
            version: "0.1.0".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"version\":\"0.1.0\""));
    }

    #[test]
    fn test_shared_state_default() {
        let state = SharedState::new();
        assert!(state.world.is_none());
        assert!(state.meta_graph.is_none());
        assert!(state.council_decisions.is_empty());
        assert!(state.prediction_reports.is_empty());
    }

    #[test]
    fn test_api_state_clone() {
        let state = ApiState::new(SharedState::new());
        let cloned = state.clone();
        assert!(Arc::ptr_eq(&state.inner, &cloned.inner));
    }

    #[test]
    fn test_belief_post_limiter_enforces_window() {
        let mut limiter = BeliefPostLimiter::default();
        assert!(limiter.check(2));
        assert!(limiter.check(2));
        assert!(!limiter.check(2));
    }

    #[test]
    fn test_governance_bundle_serialization() {
        let g = GovernanceBundle {
            raci_summary: "r".into(),
            incident_playbook: "i".into(),
            federation_operations: "f".into(),
        };
        let json = serde_json::to_string(&g).unwrap();
        assert!(json.contains("\"raci_summary\""));
        assert!(json.contains("\"incident_playbook\""));
        assert!(json.contains("\"federation_operations\""));
    }
}
