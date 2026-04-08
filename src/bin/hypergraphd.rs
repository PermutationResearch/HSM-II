use std::collections::{HashMap, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path as AxumPath, State,
    },
    http::{header::CACHE_CONTROL, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json as AxumJson, Response},
    routing::{get, post},
    Router,
};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::json;
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::connect_async;
use tower_http::services::{ServeDir, ServeFile};
use url::Url;

use hyper_stigmergy::database::{RewardLogRow, SkillEvidenceRow};
use hyper_stigmergy::hyper_stigmergy::{
    Belief, BeliefSource, ExperienceOutcome, HyperEdge, ImprovementEvent,
};
use hyper_stigmergy::vault;
use hyper_stigmergy::{
    Action, ApplyActionRequest, ApplyActionResponse, GrpoUpdateRequest,
    HyperStigmergicMorphogenesis, RooDb, RooDbConfig, TickResponse, WorldSnapshot,
};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "hypergraphd", about = "HSM-II Hypergraph Service")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: String,
    #[arg(long, default_value_t = 10)]
    agent_count: usize,
    #[arg(long, default_value = "data/real")]
    data_dir: String,
    #[arg(long, default_value = "http://127.0.0.1:9000")]
    monolith_url: String,
    #[arg(long, default_value = "vault")]
    vault_dir: String,
}

#[derive(Clone)]
struct AppState {
    world: Arc<RwLock<HyperStigmergicMorphogenesis>>,
    persistence: Arc<Persistence>,
    event_log: Arc<RwLock<VecDeque<String>>>,
    role_prompts: Arc<RwLock<HashMap<String, String>>>,
    message_log: Arc<RwLock<VecDeque<MessageRecord>>>,
    reload_tx: broadcast::Sender<String>,
    monolith_url: Option<String>,
    http_client: Client,
    vault_dir: Option<PathBuf>,
    roodb: Option<Arc<RooDb>>,
}

#[derive(Clone)]
struct Persistence {
    data_dir: PathBuf,
    world_path: PathBuf,
    events_path: PathBuf,
    export_path: PathBuf,
    role_prompts_path: PathBuf,
    messages_path: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let persistence = Persistence::new(args.data_dir);
    fs::create_dir_all(&persistence.data_dir)?;

    let (world, log_lines, role_prompts, message_log) = load_world(&persistence, args.agent_count)
        .unwrap_or_else(|_| {
            (
                HyperStigmergicMorphogenesis::new(args.agent_count),
                VecDeque::new(),
                HashMap::new(),
                VecDeque::new(),
            )
        });

    let (reload_tx, _rx) = broadcast::channel(32);
    let roodb_url = std::env::var("HSM_ROODB_URL")
        .or_else(|_| std::env::var("HSM_ROODB"))
        .ok();
    let roodb = if let Some(url) = roodb_url {
        let config = RooDbConfig::from_url(&url);
        let db = RooDb::new(&config);
        let init_result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            db.ping().await?;
            db.init_schema().await?;
            Ok::<_, anyhow::Error>(db)
        })
        .await;
        match init_result {
            Ok(Ok(db)) => {
                eprintln!(
                    "[hypergraphd] RooDB connected: {}:{}/{}",
                    config.host, config.port, config.database
                );
                Some(Arc::new(db))
            }
            Ok(Err(e)) => {
                eprintln!("[hypergraphd] RooDB init failed: {}", e);
                None
            }
            Err(_) => {
                eprintln!("[hypergraphd] RooDB connection timeout");
                None
            }
        }
    } else {
        None
    };
    let monolith_url = if args.monolith_url.trim().is_empty() {
        None
    } else {
        let base = args.monolith_url.trim().trim_end_matches('/').to_string();
        let health_url = format!("{}/api/health", base);
        let probe = tokio::time::timeout(std::time::Duration::from_millis(1200), async {
            Client::new()
                .get(&health_url)
                .send()
                .await
                .ok()
                .and_then(|r| {
                    if r.status().is_success() {
                        Some(())
                    } else {
                        None
                    }
                })
        })
        .await
        .ok()
        .flatten()
        .is_some();
        if probe {
            Some(base)
        } else {
            eprintln!(
                "[hypergraphd] monolith unreachable at {}; using local fallback mode",
                args.monolith_url
            );
            None
        }
    };

    let state = AppState {
        world: Arc::new(RwLock::new(world)),
        persistence: Arc::new(persistence),
        event_log: Arc::new(RwLock::new(log_lines)),
        role_prompts: Arc::new(RwLock::new(role_prompts)),
        message_log: Arc::new(RwLock::new(message_log)),
        reload_tx,
        monolith_url,
        http_client: Client::new(),
        vault_dir: Some(PathBuf::from(args.vault_dir)),
        roodb,
    };

    let viz_dir = PathBuf::from("viz");

    let app = Router::new()
        .route("/snapshot", get(snapshot))
        .route("/tick", post(tick))
        .route("/apply_action", post(apply_action))
        .route("/grpo_update", post(grpo_update))
        .route("/api/state", get(api_state))
        .route("/api/context", get(api_context))
        .route(
            "/api/role_prompts",
            get(get_role_prompts).post(update_role_prompt),
        )
        .route("/api/message", post(post_message))
        .route("/api/messages", get(get_messages))
        .route("/api/components/skills", get(components_skills))
        .route("/api/skills/evidence", get(skill_evidence))
        .route("/api/rewards", get(rewards))
        .route("/api/code", get(ws_code_stub))
        .route("/api/chat", get(ws_chat_stub).post(chat_post_stub))
        .route("/api/council", get(ws_council_stub).post(council_post_stub))
        .route("/api/command", post(command_stub))
        .route("/api/optimize", post(optimize_stub))
        .route("/api/vault/index", post(vault_index_stub))
        .route("/api/vault/search", post(vault_search_stub))
        .route("/api/visual", get(visual_stub).post(visual_stub))
        .route("/api/visual/ws", get(ws_visual_stub))
        .route("/api/graph-activity", get(ws_graph_activity_stub))
        .route(
            "/api/visual/file/:name",
            get(visual_file_stub).delete(visual_file_delete_stub),
        )
        .route("/api/chat/context", get(chat_context_stub))
        .route("/api/health", get(health))
        .route("/ws", get(ws_handler))
        .route_service("/", ServeFile::new(viz_dir.join("index.html")))
        .route_service(
            "/hyper_graph.json",
            ServeFile::new(state.persistence.export_path.clone()),
        )
        .nest_service("/viz", ServeDir::new(viz_dir))
        .layer(middleware::from_fn(no_cache_middleware))
        .with_state(state);

    let addr: SocketAddr = args.bind.parse()?;
    println!("hypergraphd listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

impl Persistence {
    fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        let world_path = data_dir.join("world_state.bincode");
        let events_path = data_dir.join("events.jsonl");
        let export_path = data_dir.join("hyper_graph.json");
        let role_prompts_path = data_dir.join("role_prompts.json");
        let messages_path = data_dir.join("messages.jsonl");
        Self {
            data_dir,
            world_path,
            events_path,
            export_path,
            role_prompts_path,
            messages_path,
        }
    }
}

async fn no_cache_middleware(req: axum::http::Request<axum::body::Body>, next: Next) -> Response {
    let mut resp = next.run(req).await;
    resp.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate, max-age=0"),
    );
    resp
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn load_world(
    persistence: &Persistence,
    agent_count: usize,
) -> anyhow::Result<(
    HyperStigmergicMorphogenesis,
    VecDeque<String>,
    HashMap<String, String>,
    VecDeque<MessageRecord>,
)> {
    let world = if persistence.world_path.exists() {
        let bytes = fs::read(&persistence.world_path)?;
        let mut world: HyperStigmergicMorphogenesis = bincode::deserialize(&bytes)?;
        world.rebuild_adjacency();
        world
    } else {
        HyperStigmergicMorphogenesis::new(agent_count)
    };

    let mut log_lines = VecDeque::new();
    if persistence.events_path.exists() {
        if let Ok(contents) = fs::read_to_string(&persistence.events_path) {
            let lines: Vec<&str> = contents.lines().collect();
            let start = lines.len().saturating_sub(200);
            for line in &lines[start..] {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(msg) = value.get("line").and_then(|v| v.as_str()) {
                        log_lines.push_back(msg.to_string());
                        continue;
                    }
                }
                log_lines.push_back(line.to_string());
            }
        }
    }

    let mut role_prompts = HashMap::new();
    if persistence.role_prompts_path.exists() {
        if let Ok(contents) = fs::read_to_string(&persistence.role_prompts_path) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&contents) {
                role_prompts = map;
            }
        }
    }

    let mut message_log = VecDeque::new();
    if persistence.messages_path.exists() {
        if let Ok(contents) = fs::read_to_string(&persistence.messages_path) {
            let lines: Vec<&str> = contents.lines().collect();
            let start = lines.len().saturating_sub(200);
            for line in &lines[start..] {
                if let Ok(value) = serde_json::from_str::<MessageRecord>(line) {
                    message_log.push_back(value);
                }
            }
        }
    }

    Ok((world, log_lines, role_prompts, message_log))
}

fn append_event(persistence: &Persistence, line: &str) {
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&persistence.events_path)
    {
        use std::io::Write;
        let payload = serde_json::json!({
            "ts": now_ts(),
            "line": line,
        });
        let _ = writeln!(file, "{}", payload.to_string());
    }
}

async fn persist_world_with_vault(state: &AppState) {
    let world = state.world.read().await;
    if let Ok(bytes) = bincode::serialize(&*world) {
        let _ = fs::write(&state.persistence.world_path, bytes);
    }
    let _ = world.export_json(
        state
            .persistence
            .export_path
            .to_str()
            .unwrap_or("hyper_graph.json"),
    );
    drop(world);

    if let Some(vault_dir) = state.vault_dir.as_ref() {
        let export_path = state.persistence.export_path.clone();
        let vault_dir = vault_dir.clone();
        tokio::task::spawn_blocking(move || {
            let _ = vault::merge_vault_into_export(&export_path, &vault_dir);
        })
        .await
        .ok();
    }
}

fn persist_role_prompts(persistence: &Persistence, prompts: &HashMap<String, String>) {
    if let Ok(payload) = serde_json::to_string_pretty(prompts) {
        let _ = fs::write(&persistence.role_prompts_path, payload);
    }
}

fn append_message(persistence: &Persistence, message: &MessageRecord) {
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&persistence.messages_path)
    {
        use std::io::Write;
        if let Ok(line) = serde_json::to_string(message) {
            let _ = writeln!(file, "{}", line);
        }
    }
}

async fn snapshot(State(state): State<AppState>) -> AxumJson<WorldSnapshot> {
    let world = state.world.read().await;
    AxumJson(WorldSnapshot::from(&*world))
}

async fn tick(State(state): State<AppState>) -> AxumJson<TickResponse> {
    let mut world = state.world.write().await;
    world.tick();
    let line = format!("[t={}] tick", world.tick_count);
    drop(world);
    persist_world_with_vault(&state).await;
    push_event(&state, line).await;
    let _ = state.reload_tx.send("reload".to_string());
    AxumJson(TickResponse {
        snapshot: WorldSnapshot::from(&*state.world.read().await),
    })
}

async fn apply_action(
    State(state): State<AppState>,
    AxumJson(req): AxumJson<ApplyActionRequest>,
) -> Result<AxumJson<ApplyActionResponse>, StatusCode> {
    let mut world = state.world.write().await;
    world.apply_action_with_agent(&req.action, req.agent_id);
    let line = format!("[t={}] action {:?}", world.tick_count, req.action);
    drop(world);
    persist_world_with_vault(&state).await;
    push_event(&state, line).await;
    let _ = state.reload_tx.send("reload".to_string());
    Ok(AxumJson(ApplyActionResponse {
        snapshot: WorldSnapshot::from(&*state.world.read().await),
    }))
}

async fn grpo_update(
    State(state): State<AppState>,
    AxumJson(req): AxumJson<GrpoUpdateRequest>,
) -> StatusCode {
    let mut world = state.world.write().await;
    if req.rewards.is_empty() {
        return StatusCode::OK;
    }
    let rewards: Vec<f64> = req.rewards.iter().map(|r| r.reward).collect();
    for entry in &req.rewards {
        if let Some(agent) = world.agents.iter_mut().find(|a| a.id == entry.agent_id) {
            let lr = agent.learning_rate;
            agent.grpo_update_bid(&rewards, entry.reward, lr);
        }
    }
    let tick = world.tick_count;
    let line = format!("[t={}] grpo_update {} rewards", tick, req.rewards.len());
    drop(world);

    if let Some(db) = state.roodb.clone() {
        let now = HyperStigmergicMorphogenesis::current_timestamp();
        for entry in &req.rewards {
            let row = RewardLogRow {
                tick,
                agent_id: entry.agent_id,
                reward: entry.reward,
                source: "grpo_update".to_string(),
                created_at: now,
            };
            let db = db.clone();
            tokio::spawn(async move {
                let _ = db.insert_reward_log(&row).await;
            });
        }
    }

    persist_world_with_vault(&state).await;
    push_event(&state, line).await;
    StatusCode::OK
}

async fn api_state(State(state): State<AppState>) -> impl IntoResponse {
    let world = state.world.read().await;
    let event_log = state
        .event_log
        .read()
        .await
        .clone()
        .into_iter()
        .collect::<Vec<_>>();
    let snapshot = UiWorldSnapshot::from_world(&world, event_log);
    AxumJson(snapshot)
}

async fn api_context() -> impl IntoResponse {
    AxumJson(json!({ "context": "" }))
}

async fn chat_context_stub(State(state): State<AppState>) -> impl IntoResponse {
    match proxy_get_json(&state, "/api/chat/context").await {
        Ok(resp) => AxumJson(resp),
        Err(_) => AxumJson(json!({
            "percent": 0.0,
            "message_count": 0,
            "estimated_tokens": 0,
            "limit_tokens": 0,
            "regular": 0,
            "cache_read": 0,
            "cache_write": 0,
            "total": 0,
            "total_formatted": "0K",
            "limit_formatted": "0K",
            "has_summary": false,
            "dag_info": {
                "total_nodes": 0,
                "summary_nodes": 0,
                "large_file_nodes": 0,
                "max_depth": 0
            }
        })),
    }
}

async fn get_role_prompts(State(state): State<AppState>) -> impl IntoResponse {
    let prompts = state.role_prompts.read().await.clone();
    AxumJson(prompts)
}

#[derive(serde::Deserialize)]
struct RolePromptUpdate {
    role: String,
    prompt: String,
}

async fn update_role_prompt(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<serde_json::Value>,
) -> impl IntoResponse {
    let mut prompts = state.role_prompts.write().await;
    if let Ok(update) = serde_json::from_value::<RolePromptUpdate>(payload.clone()) {
        prompts.insert(update.role, update.prompt);
    } else if let Ok(map) = serde_json::from_value::<HashMap<String, String>>(payload) {
        for (role, prompt) in map {
            prompts.insert(role, prompt);
        }
    } else {
        return StatusCode::BAD_REQUEST;
    }
    persist_role_prompts(&state.persistence, &prompts);
    StatusCode::OK
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct MessageRecord {
    id: String,
    sender: u64,
    target: String,
    kind: String,
    content: String,
    ts: u64,
}

#[derive(serde::Deserialize)]
struct MessagePayload {
    sender: u64,
    target: String,
    kind: Option<String>,
    content: String,
}

async fn post_message(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<MessagePayload>,
) -> impl IntoResponse {
    let kind = payload.kind.unwrap_or_else(|| "note".to_string());
    let target = payload.target;
    let content = payload.content;
    let sender = payload.sender;
    let record = MessageRecord {
        id: Uuid::new_v4().to_string(),
        sender,
        target: target.clone(),
        kind: kind.clone(),
        content: content.clone(),
        ts: now_ts(),
    };
    append_message(&state.persistence, &record);
    let mut log = state.message_log.write().await;
    log.push_back(record);
    while log.len() > 200 {
        log.pop_front();
    }
    if let Some(base) = state.monolith_url.clone() {
        let url = format!("{}/api/message", base);
        let payload = json!({
            "sender": sender,
            "target": target,
            "kind": kind,
            "content": content,
        });
        let _ = state.http_client.post(url).json(&payload).send().await;
    }
    StatusCode::OK
}

async fn get_messages(State(state): State<AppState>) -> impl IntoResponse {
    let log = state.message_log.read().await;
    AxumJson(log.iter().cloned().collect::<Vec<_>>())
}

async fn components_skills(State(state): State<AppState>) -> impl IntoResponse {
    if state.monolith_url.is_some() {
        if let Ok(resp) = proxy_get_json(&state, "/api/components/skills").await {
            return AxumJson(resp);
        }
    }

    let world = state.world.read().await;
    let mut all = world.skill_bank.all_skills();
    all.sort_by(|a, b| {
        b.credit_ema
            .partial_cmp(&a.credit_ema)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.confidence
                    .partial_cmp(&a.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let top_skills: Vec<UiSkillInfo> = all
        .into_iter()
        .take(64)
        .map(|s| UiSkillInfo {
            id: s.id.clone(),
            title: s.title.clone(),
            principle: s.principle.clone(),
            level: format!("{:?}", s.level),
            confidence: s.confidence,
            status: format!("{:?}", s.status),
            usage_count: s.usage_count,
            credit_ema: s.credit_ema,
        })
        .collect();

    let payload = UiSkillSnapshot {
        total_skills: world.skill_bank.total_skills(),
        general_count: world.skill_bank.general_skills.len(),
        role_count: world.skill_bank.role_skills.values().map(|v| v.len()).sum(),
        task_count: world.skill_bank.task_skills.values().map(|v| v.len()).sum(),
        evolution_epoch: world.skill_bank.evolution_epoch,
        top_skills,
        recent_distillations: Vec::new(),
    };
    AxumJson(json!({
        "skills": payload,
        "timestamp": now_ts(),
    }))
}

async fn skill_evidence(State(state): State<AppState>) -> impl IntoResponse {
    if state.monolith_url.is_some() {
        if let Ok(resp) = proxy_get_json(&state, "/api/skills/evidence").await {
            if resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                return (StatusCode::OK, AxumJson(resp)).into_response();
            }
        }
    }

    if let Some(db) = state.roodb.clone() {
        let rows: Vec<SkillEvidenceRow> = db.fetch_skill_evidence(100).await.unwrap_or_default();
        return (
            StatusCode::OK,
            AxumJson(json!({ "ok": true, "rows": rows })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        AxumJson(json!({
            "ok": true,
            "rows": [],
            "warning": "roodb_unavailable"
        })),
    )
        .into_response()
}

async fn rewards(State(state): State<AppState>) -> impl IntoResponse {
    if state.monolith_url.is_some() {
        if let Ok(resp) = proxy_get_json(&state, "/api/rewards").await {
            if resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                return (StatusCode::OK, AxumJson(resp)).into_response();
            }
        }
    }

    if let Some(db) = state.roodb.clone() {
        let rows: Vec<RewardLogRow> = db.fetch_reward_logs(200).await.unwrap_or_default();
        return (
            StatusCode::OK,
            AxumJson(json!({ "ok": true, "rows": rows })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        AxumJson(json!({
            "ok": true,
            "rows": [],
            "warning": "roodb_unavailable"
        })),
    )
        .into_response()
}

async fn command_stub(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<serde_json::Value>,
) -> impl IntoResponse {
    match proxy_post_json(&state, "/api/command", payload).await {
        Ok(resp) => (StatusCode::OK, AxumJson(resp)),
        Err(err) => (
            StatusCode::OK,
            AxumJson(json!({ "ok": false, "error": err })),
        ),
    }
}

async fn optimize_stub(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<serde_json::Value>,
) -> impl IntoResponse {
    match proxy_post_json(&state, "/api/optimize", payload).await {
        Ok(resp) => (StatusCode::OK, AxumJson(resp)),
        Err(err) => (
            StatusCode::OK,
            AxumJson(json!({ "ok": false, "error": err })),
        ),
    }
}

async fn vault_index_stub(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<serde_json::Value>,
) -> impl IntoResponse {
    match proxy_post_json(&state, "/api/vault/index", payload).await {
        Ok(resp) => (StatusCode::OK, AxumJson(resp)),
        Err(err) => (
            StatusCode::OK,
            AxumJson(json!({ "ok": false, "error": err })),
        ),
    }
}

async fn vault_search_stub(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<serde_json::Value>,
) -> impl IntoResponse {
    match proxy_post_json(&state, "/api/vault/search", payload).await {
        Ok(resp) => (StatusCode::OK, AxumJson(resp)),
        Err(err) => (
            StatusCode::OK,
            AxumJson(json!({ "ok": false, "error": err })),
        ),
    }
}

async fn visual_stub(State(state): State<AppState>) -> impl IntoResponse {
    if state.monolith_url.is_some() {
        if let Ok(resp) = proxy_get_json(&state, "/api/visual").await {
            return AxumJson(resp);
        }
    }
    AxumJson(
        local_visual_list().unwrap_or_else(|err| json!({ "files": [], "error": err.to_string() })),
    )
}

fn local_visual_list() -> anyhow::Result<serde_json::Value> {
    let output_dir = PathBuf::from("visual-explainer/output");
    let _ = fs::create_dir_all(&output_dir);
    let files: Vec<serde_json::Value> = fs::read_dir(&output_dir)
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
                    let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();

                    let mut title = name.clone();
                    let mut viz_type = "unknown".to_string();
                    let mut format = "unknown".to_string();
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&content) {
                            if let Some(t) = json_val.get("title").and_then(|v| v.as_str()) {
                                title = t.to_string();
                            }
                            if let Some(t) = json_val.get("type").and_then(|v| v.as_str()) {
                                viz_type = t.to_string();
                            }
                            if let Some(f) = json_val.get("format").and_then(|v| v.as_str()) {
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

    Ok(json!({ "files": files, "count": files.len() }))
}

fn local_visual_file(name: &str) -> anyhow::Result<(Vec<u8>, Option<String>)> {
    let output_dir = PathBuf::from("visual-explainer/output");
    let file_path = output_dir.join(name);
    let canonical = fs::canonicalize(&file_path)?;
    let canonical_root = fs::canonicalize(&output_dir)?;
    if !canonical.starts_with(&canonical_root) {
        anyhow::bail!("invalid visual file path");
    }
    let bytes = fs::read(canonical)?;
    Ok((bytes, Some("application/json".to_string())))
}

async fn visual_file_stub(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> impl IntoResponse {
    let path = format!("/api/visual/file/{}", name);
    if state.monolith_url.is_some() {
        if let Ok((bytes, content_type)) = proxy_get_bytes(&state, &path).await {
            let mut resp = axum::response::Response::new(axum::body::Body::from(bytes));
            if let Some(ct) = content_type {
                resp.headers_mut()
                    .insert("content-type", ct.parse().unwrap());
            }
            return resp;
        }
    }
    match local_visual_file(&name) {
        Ok((bytes, content_type)) => {
            let mut resp = axum::response::Response::new(axum::body::Body::from(bytes));
            if let Some(ct) = content_type {
                resp.headers_mut()
                    .insert("content-type", ct.parse().unwrap());
            }
            resp
        }
        Err(_) => axum::response::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::from(
                "{\"ok\":false,\"error\":\"visual file not available\"}",
            ))
            .unwrap(),
    }
}

async fn visual_file_delete_stub(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> impl IntoResponse {
    let path = format!("/api/visual/file/{}", name);
    if state.monolith_url.is_some() {
        if let Ok(resp) = proxy_delete_json(&state, &path).await {
            return (StatusCode::OK, AxumJson(resp));
        }
    }
    (
        StatusCode::NOT_FOUND,
        AxumJson(json!({"ok": false, "error": "visual delete unavailable"})),
    )
}

async fn ws_chat_stub(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    if state.monolith_url.is_some() {
        let upstream = state.monolith_url.clone();
        return ws.on_upgrade(move |socket| async move {
            if let Err(err) = ws_proxy(socket, upstream, "/api/chat").await {
                eprintln!("ws chat proxy error: {err}");
            }
        });
    }
    ws.on_upgrade(move |socket| local_ws_stub(socket, "chat"))
}

async fn ws_council_stub(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    if state.monolith_url.is_some() {
        let upstream = state.monolith_url.clone();
        return ws.on_upgrade(move |socket| async move {
            if let Err(err) = ws_proxy(socket, upstream, "/api/council").await {
                eprintln!("ws council proxy error: {err}");
            }
        });
    }
    ws.on_upgrade(move |socket| local_ws_stub(socket, "council"))
}

async fn chat_post_stub(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<serde_json::Value>,
) -> impl IntoResponse {
    let text = extract_interaction_text(&payload);
    if !text.is_empty() {
        learn_from_interaction(&state, "chat", &text).await;
    }
    match proxy_post_json(&state, "/api/chat", payload).await {
        Ok(resp) => (StatusCode::OK, AxumJson(resp)).into_response(),
        Err(err) => (
            StatusCode::OK,
            AxumJson(json!({
                "ok": true,
                "local": true,
                "local_response": format!(
                    "Local chat mode: captured your input, updated memory/graph, and advanced one learning tick. Upstream chat unavailable: {}",
                    err
                )
            })),
        )
            .into_response(),
    }
}

async fn council_post_stub(
    State(state): State<AppState>,
    AxumJson(payload): AxumJson<serde_json::Value>,
) -> impl IntoResponse {
    let text = extract_interaction_text(&payload);
    if !text.is_empty() {
        learn_from_interaction(&state, "council", &text).await;
    }
    match proxy_post_json(&state, "/api/council", payload).await {
        Ok(resp) => (StatusCode::OK, AxumJson(resp)).into_response(),
        Err(err) => (
            StatusCode::OK,
            AxumJson(json!({
                "ok": true,
                "local": true,
                "local_response": format!(
                    "Local council mode: saved your question as belief/experience, linked agents, and advanced one tick. Upstream council unavailable: {}",
                    err
                )
            })),
        )
            .into_response(),
    }
}

async fn ws_code_stub(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    if state.monolith_url.is_some() {
        let upstream = state.monolith_url.clone();
        return ws.on_upgrade(move |socket| async move {
            if let Err(err) = ws_proxy(socket, upstream, "/api/code").await {
                eprintln!("ws code proxy error: {err}");
            }
        });
    }
    ws.on_upgrade(move |socket| local_code_ws(socket, state))
}

async fn local_code_ws(mut socket: WebSocket, state: AppState) {
    let _ = socket
        .send(Message::Text(
            json!({"type":"connected","message":"local code mode"}).to_string(),
        ))
        .await;
    while let Some(msg) = socket.recv().await {
        match msg {
            Ok(Message::Text(text)) => {
                let parsed = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default();
                let query = extract_interaction_text(&parsed);
                let model = parsed
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("local-code-fallback");
                let _ = socket
                    .send(Message::Text(
                        json!({"type":"start","query":query,"model":model}).to_string(),
                    ))
                    .await;
                if !query.is_empty() {
                    learn_from_interaction(&state, "code", &query).await;
                }
                let _ = socket
                    .send(Message::Text(
                        json!({
                            "type":"token",
                            "content":"Local code mode: interaction learned and world updated. Start upstream monolith on :9000 for full coder agent tool execution."
                        })
                        .to_string(),
                    ))
                    .await;
                let _ = socket
                    .send(Message::Text(json!({"type":"complete"}).to_string()))
                    .await;
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
}

fn extract_interaction_text(payload: &serde_json::Value) -> String {
    for key in ["text", "question", "query", "content", "prompt", "message"] {
        if let Some(v) = payload.get(key).and_then(|v| v.as_str()) {
            let t = v.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
    }
    String::new()
}

async fn learn_from_interaction(state: &AppState, channel: &str, text: &str) {
    let mut world = state.world.write().await;
    if world.agents.len() < 2 {
        return;
    }

    let cleaned = text.trim();
    let compact = if cleaned.len() > 240 {
        format!("{}...", &cleaned[..240])
    } else {
        cleaned.to_string()
    };
    let confidence = match channel {
        "council" => 0.76,
        "code" => 0.72,
        _ => 0.68,
    };
    world.add_belief(
        &format!("[{}] {}", channel, compact),
        confidence,
        BeliefSource::UserProvided,
    );
    world.record_experience(
        &format!("{} interaction", channel),
        &compact,
        ExperienceOutcome::Neutral,
    );

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    channel.hash(&mut hasher);
    compact.hash(&mut hasher);
    let h = hasher.finish();
    let n = world.agents.len();
    let i = (h as usize) % n;
    let mut j = ((h >> 16) as usize) % (n - 1);
    if j >= i {
        j += 1;
    }
    let a = world.agents[i].id as usize;
    let b = world.agents[j].id as usize;
    world.apply_action_with_agent(
        &Action::LinkAgents {
            vertices: vec![a, b],
            weight: 0.42,
        },
        Some(a as u64),
    );
    world.tick();
    let tick = world.tick_count;
    drop(world);

    push_event(
        state,
        format!(
            "[t={}] {} interaction: belief+experience captured, edge linked, tick advanced",
            tick, channel
        ),
    )
    .await;
    persist_world_with_vault(state).await;
    let _ = state.reload_tx.send("reload".to_string());
}

async fn ws_visual_stub(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    if state.monolith_url.is_some() {
        let upstream = state.monolith_url.clone();
        return ws.on_upgrade(move |socket| async move {
            if let Err(err) = ws_proxy(socket, upstream, "/api/visual/ws").await {
                eprintln!("ws visual proxy error: {err}");
            }
        });
    }
    ws.on_upgrade(move |socket| local_ws_stub(socket, "visual"))
}

async fn ws_graph_activity_stub(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if state.monolith_url.is_some() {
        let upstream = state.monolith_url.clone();
        return ws.on_upgrade(move |socket| async move {
            if let Err(err) = ws_proxy(socket, upstream, "/api/graph-activity").await {
                eprintln!("ws graph-activity proxy error: {err}");
            }
        });
    }
    ws.on_upgrade(move |socket| local_ws_stub(socket, "graph-activity"))
}

async fn local_ws_stub(mut socket: WebSocket, channel: &'static str) {
    let _ = socket
        .send(Message::Text(
            json!({
                "type": "connected",
                "content": format!("local {} stream ready", channel),
            })
            .to_string(),
        ))
        .await;
    while let Some(msg) = socket.recv().await {
        match msg {
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
}

async fn health() -> StatusCode {
    StatusCode::OK
}

async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state.reload_tx.subscribe()))
}

async fn handle_ws(mut socket: WebSocket, mut rx: broadcast::Receiver<String>) {
    let _ = socket.send(Message::Text("connected".to_string())).await;
    while let Ok(msg) = rx.recv().await {
        if socket.send(Message::Text(msg)).await.is_err() {
            break;
        }
    }
}

async fn proxy_get_json(state: &AppState, path: &str) -> Result<serde_json::Value, String> {
    let base = state
        .monolith_url
        .clone()
        .ok_or_else(|| "monolith_url not configured".to_string())?;
    let url = format!("{}{}", base, path);
    let resp = state
        .http_client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| e.to_string())
}

async fn proxy_post_json(
    state: &AppState,
    path: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let base = state
        .monolith_url
        .clone()
        .ok_or_else(|| "monolith_url not configured".to_string())?;
    let url = format!("{}{}", base, path);
    let resp = state
        .http_client
        .post(url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| e.to_string())
}

async fn proxy_get_bytes(
    state: &AppState,
    path: &str,
) -> Result<(Vec<u8>, Option<String>), String> {
    let base = state
        .monolith_url
        .clone()
        .ok_or_else(|| "monolith_url not configured".to_string())?;
    let url = format!("{}{}", base, path);
    let resp = state
        .http_client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok((bytes.to_vec(), content_type))
}

async fn proxy_delete_json(state: &AppState, path: &str) -> Result<serde_json::Value, String> {
    let base = state
        .monolith_url
        .clone()
        .ok_or_else(|| "monolith_url not configured".to_string())?;
    let url = format!("{}{}", base, path);
    let resp = state
        .http_client
        .delete(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Ok(json!({"ok": true}));
    }
    serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| e.to_string())
}

async fn ws_proxy(
    mut socket: WebSocket,
    monolith_url: Option<String>,
    path: &str,
) -> Result<(), String> {
    let base = monolith_url.ok_or_else(|| "monolith_url not configured".to_string())?;
    let ws_url = to_ws_url(&base, path)?;
    let (upstream, _) = connect_async(ws_url.as_str())
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
                    Some(Err(e)) => return Err(e.to_string()),
                }
            }
            msg = up_rx.next() => {
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
                    Some(Err(e)) => return Err(e.to_string()),
                }
            }
        }
    }
    Ok(())
}

fn to_ws_url(base: &str, path: &str) -> Result<String, String> {
    let mut url = base.to_string();
    if url.starts_with("https://") {
        url = url.replacen("https://", "wss://", 1);
    } else if url.starts_with("http://") {
        url = url.replacen("http://", "ws://", 1);
    }
    let full = format!("{}{}", url.trim_end_matches('/'), path);
    Url::parse(&full).map_err(|e| e.to_string())?;
    Ok(full)
}

async fn push_event(state: &AppState, line: String) {
    append_event(&state.persistence, &line);
    let mut log = state.event_log.write().await;
    log.push_back(line);
    while log.len() > 200 {
        log.pop_front();
    }
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiWorldSnapshot {
    tick: u64,
    coherence: f64,
    coherence_trend: String,
    global_jw: f64,
    agents: Vec<UiAgentSnapshot>,
    edges: Vec<UiEdgeSnapshot>,
    beliefs: Vec<UiBeliefSnapshot>,
    improvements: Vec<UiImprovementSnapshot>,
    ontology: Vec<(String, String)>,
    event_log: Vec<String>,
    council: UiCouncilSnapshot,
    dks: UiDKSSnapshot,
    cass: UiCASSSnapshot,
    navigation: UiNavigationSnapshot,
    communication: UiCommunicationSnapshot,
    llm: UiLlmSnapshot,
    email: UiEmailSnapshot,
    federation: UiFederationSnapshot,
    skills: UiSkillSnapshot,
    chat_context: UiChatContextSnapshot,
}

impl UiWorldSnapshot {
    fn from_world(world: &HyperStigmergicMorphogenesis, event_log: Vec<String>) -> Self {
        let coherence = world.global_coherence();
        let trend = if coherence > world.prev_coherence + 0.0001 {
            "up"
        } else if coherence + 0.0001 < world.prev_coherence {
            "down"
        } else {
            "flat"
        };
        let global_jw = if world.agents.is_empty() {
            0.0
        } else {
            world.agents.iter().map(|a| a.jw).sum::<f64>() / world.agents.len() as f64
        };
        let ontology = world
            .ontology
            .iter()
            .map(|(k, v)| (k.clone(), v.instances.join(", ")))
            .collect::<Vec<_>>();

        Self {
            tick: world.tick_count,
            coherence,
            coherence_trend: trend.to_string(),
            global_jw,
            agents: world.agents.iter().map(UiAgentSnapshot::from).collect(),
            edges: world.edges.iter().map(UiEdgeSnapshot::from).collect(),
            beliefs: world.beliefs.iter().map(UiBeliefSnapshot::from).collect(),
            improvements: world
                .improvement_history
                .iter()
                .map(UiImprovementSnapshot::from)
                .collect(),
            ontology,
            event_log,
            council: UiCouncilSnapshot::default(),
            dks: UiDKSSnapshot::default(),
            cass: UiCASSSnapshot::default(),
            navigation: UiNavigationSnapshot::default(),
            communication: UiCommunicationSnapshot::default(),
            llm: UiLlmSnapshot::default(),
            email: UiEmailSnapshot::default(),
            federation: UiFederationSnapshot::default(),
            skills: UiSkillSnapshot::default(),
            chat_context: UiChatContextSnapshot::default(),
        }
    }
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiAgentSnapshot {
    id: u64,
    role: String,
    curiosity: f64,
    harmony: f64,
    growth: f64,
    learning_rate: f64,
    jw: f64,
}

impl From<&hyper_stigmergy::Agent> for UiAgentSnapshot {
    fn from(agent: &hyper_stigmergy::Agent) -> Self {
        Self {
            id: agent.id,
            role: format!("{:?}", agent.role),
            curiosity: agent.drives.curiosity,
            harmony: agent.drives.harmony,
            growth: agent.drives.growth,
            learning_rate: agent.learning_rate,
            jw: agent.jw,
        }
    }
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiEdgeSnapshot {
    participants: Vec<u64>,
    weight: f64,
    emergent: bool,
    age: u64,
}

impl From<&HyperEdge> for UiEdgeSnapshot {
    fn from(edge: &HyperEdge) -> Self {
        Self {
            participants: edge.participants.clone(),
            weight: edge.weight,
            emergent: edge.emergent,
            age: edge.age,
        }
    }
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiBeliefSnapshot {
    content: String,
    confidence: f64,
    source: String,
}

impl From<&Belief> for UiBeliefSnapshot {
    fn from(belief: &Belief) -> Self {
        Self {
            content: belief.content.clone(),
            confidence: belief.confidence,
            source: format!("{:?}", belief.source),
        }
    }
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiImprovementSnapshot {
    intent: String,
    mutation_type: String,
    coherence_before: f64,
    coherence_after: f64,
    applied: bool,
}

impl From<&ImprovementEvent> for UiImprovementSnapshot {
    fn from(event: &ImprovementEvent) -> Self {
        Self {
            intent: event.intent.clone(),
            mutation_type: format!("{:?}", event.mutation_type),
            coherence_before: event.coherence_before,
            coherence_after: event.coherence_after,
            applied: event.applied,
        }
    }
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiCouncilSnapshot {
    active: bool,
    mode: String,
    member_count: usize,
    recent_decisions: Vec<String>,
    current_proposal: Option<String>,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiDKSSnapshot {
    generation: u64,
    population_size: usize,
    avg_persistence: f64,
    replicator_count: usize,
    flux_intensity: f64,
    multifractal_spectrum: Vec<(f64, f64)>,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiCASSSnapshot {
    skill_count: usize,
    context_depth: usize,
    recent_matches: Vec<String>,
    embedding_dimension: usize,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiNavigationSnapshot {
    indexed_files: usize,
    topics: Vec<String>,
    recent_searches: Vec<String>,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiCommunicationSnapshot {
    active_gossip_rounds: usize,
    swarm_agents: usize,
    stigmergic_fields: usize,
    message_throughput: u64,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiLlmSnapshot {
    model_loaded: bool,
    model_name: Option<String>,
    tokens_generated: u64,
    avg_latency_ms: f64,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiEmailSnapshot {
    inbox_unread: Option<usize>,
    classified_today: Option<usize>,
    auto_responses_sent: Option<usize>,
    memory_entries: usize,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiFederationSnapshot {
    status: String,
    addr: Option<String>,
    system_id: String,
    peers: Vec<String>,
    imported: usize,
    exported: usize,
    conflicts: usize,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiSkillSnapshot {
    total_skills: usize,
    general_count: usize,
    role_count: usize,
    task_count: usize,
    evolution_epoch: u64,
    top_skills: Vec<UiSkillInfo>,
    recent_distillations: Vec<String>,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiSkillInfo {
    id: String,
    title: String,
    principle: String,
    level: String,
    confidence: f64,
    status: String,
    usage_count: u64,
    credit_ema: f64,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiChatContextSnapshot {
    message_count: usize,
    estimated_tokens: usize,
    percent_used: f32,
    limit_tokens: usize,
    regular_tokens: usize,
    cache_read_tokens: usize,
    cache_write_tokens: usize,
    has_summary: bool,
    dag_info: UiDagInfoSnapshot,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct UiDagInfoSnapshot {
    total_nodes: usize,
    summary_nodes: usize,
    large_file_nodes: usize,
    max_depth: u32,
}
