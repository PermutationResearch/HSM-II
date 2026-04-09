//! Lightweight HTTP API for the company console dashboard (trail, memory listing, graph, search, autoDream, email paste).

use axum::{
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderValue, Method, StatusCode},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::error;

use crate::architecture_blueprint::embedded_blueprint;
use crate::harness::{
    run_anti_sycophancy_loop, run_council_socratic_with_anti_sycophancy, AntiSycophancyConfig,
};
use crate::llm::client::LlmClient;
use crate::personal::agent_memory_pipeline::list_memory_markdown_files;
use crate::personal::autodream;
use crate::personal::EnhancedPersonalAgent;
use sqlx::PgPool;

const EMAIL_DRAFT_MAX_BYTES: usize = 256 * 1024;
const ANTI_SYC_MAX_BYTES: usize = 512 * 1024;
const COUNCIL_SOCRATIC_MAX_BYTES: usize = 256 * 1024;

#[derive(Clone)]
pub struct ConsoleState {
    pub home: PathBuf,
    /// When set (`HSM_COMPANY_OS_DATABASE_URL`), Company OS CRUD is available under `/api/company/*`.
    pub company_db: Option<PgPool>,
    /// When set, `POST …/sync/paperclip-goals` and `…/sync/paperclip-dris` can read from this layer without a JSON body.
    pub paperclip: Option<Arc<Mutex<crate::paperclip::IntelligenceLayer>>>,
    /// Lazy-loaded agent for `/api/console/email-draft` (first request may take several seconds).
    draft_agent: Arc<Mutex<Option<EnhancedPersonalAgent>>>,
}

impl ConsoleState {
    pub fn new(home: PathBuf, company_db: Option<PgPool>) -> Self {
        Self {
            home,
            company_db,
            paperclip: None,
            draft_agent: Arc::new(Mutex::new(None)),
        }
    }

    /// Same as [`Self::new`] but attaches the in-process Paperclip [`IntelligenceLayer`](crate::paperclip::IntelligenceLayer) for goal/DRI sync into Postgres.
    pub fn with_paperclip_layer(
        home: PathBuf,
        company_db: Option<PgPool>,
        layer: Arc<Mutex<crate::paperclip::IntelligenceLayer>>,
    ) -> Self {
        Self {
            home,
            company_db,
            paperclip: Some(layer),
            draft_agent: Arc::new(Mutex::new(None)),
        }
    }
}

#[derive(Deserialize)]
pub struct TrailQuery {
    #[serde(default = "default_trail_limit")]
    pub limit: usize,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_trail_limit() -> usize {
    100
}

fn default_search_limit() -> usize {
    40
}

#[derive(Deserialize)]
struct EmailDraftBody {
    /// Raw email or thread pasted from the client (headers + body).
    text: String,
    /// One LLM call (default). Set `false` to allow read_eml / read_file / Maildir tools (slower).
    #[serde(default = "default_email_simple")]
    simple: bool,
}

fn default_email_simple() -> bool {
    true
}

#[derive(Deserialize)]
struct CouncilSocraticBody {
    proposition: String,
    #[serde(default)]
    context: Option<String>,
    /// Role ids (e.g. socratic_questioner, epistemic_critic, integrator). Empty = defaults.
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default = "default_council_rounds")]
    council_rounds: u32,
    #[serde(default)]
    seed_directives: Vec<String>,
    #[serde(default)]
    anti_sycophancy_max_rounds: Option<u32>,
}

fn default_council_rounds() -> u32 {
    1
}

#[derive(Deserialize)]
struct AntiSycophancyBody {
    user_message: String,
    draft_response: String,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    seed_directives: Vec<String>,
    #[serde(default)]
    max_rounds: Option<u32>,
}

async fn get_architecture() -> Json<Value> {
    // Same JSON shape as `GET /api/architecture` on the world API; `runtime` is null here (no mounted world).
    Json(json!({
        "blueprint": embedded_blueprint(),
        "runtime": null
    }))
}

pub fn console_router(state: ConsoleState) -> Router {
    Router::new()
        .route("/", get(root_landing))
        .route("/api/health", get(health))
        .route("/api/architecture", get(get_architecture))
        .route("/api/console/trail", get(get_trail))
        .route("/api/console/memory-files", get(get_memory_files))
        .route("/api/console/stats", get(get_stats))
        .route("/api/console/graph/trail", get(get_graph_trail))
        .route("/api/console/graph/hypergraph", get(get_graph_hypergraph))
        .route("/api/console/search", get(get_search))
        .route("/api/console/autodream", get(get_autodream))
        .route(
            "/api/console/email-draft",
            post(post_email_draft).layer(DefaultBodyLimit::max(EMAIL_DRAFT_MAX_BYTES)),
        )
        .route(
            "/api/console/anti-sycophancy",
            post(post_anti_sycophancy).layer(DefaultBodyLimit::max(ANTI_SYC_MAX_BYTES)),
        )
        .route(
            "/api/console/council-socratic",
            post(post_council_socratic).layer(DefaultBodyLimit::max(COUNCIL_SOCRATIC_MAX_BYTES)),
        )
        .merge(crate::company_os::router())
        .layer(console_cors_layer())
        .with_state(state)
}

fn console_cors_layer() -> CorsLayer {
    let origins_raw = std::env::var("HSM_CONSOLE_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://127.0.0.1:3001,http://localhost:3001".to_string());
    let origins = origins_raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let methods = [Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS];
    if origins.iter().any(|s| *s == "*") {
        return CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(methods);
    }
    let parsed = origins
        .into_iter()
        .filter_map(|s| s.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();
    CorsLayer::new().allow_origin(parsed).allow_methods(methods)
}

/// Browser-friendly hint: this process is an API; the Next.js UI runs on another port.
async fn root_landing() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>HSM console API</title>
  <style>
    body { font-family: system-ui, sans-serif; max-width: 42rem; margin: 2rem; line-height: 1.5; color: #e8e8e8; background: #111; }
    a { color: #7dd3fc; }
    code { background: #222; padding: 0.1em 0.35em; border-radius: 4px; }
    .warn { border: 2px solid #f85149; background: #2d1b1b; padding: 1rem 1.25rem; border-radius: 8px; margin-bottom: 1.5rem; }
    .warn strong { color: #ffa198; }
  </style>
</head>
<body>
  <div class="warn" role="alert">
    <strong>Not the dashboard.</strong> This port (<code>hsm_console</code>) is the <strong>API only</strong> — no Tailwind UI.
    Open the Next.js app on <strong>http://127.0.0.1:3050</strong> (or the port Electron chose). The footer in the real UI may show this URL as “API base”; that is normal.
  </div>
  <h1>HSM company console API</h1>
  <p>JSON endpoints for the dashboard. Use the links below or open the Next.js UI.</p>
  <ul>
    <li><a href="/api/health"><code>GET /api/health</code></a> — quick check</li>
    <li><a href="/api/architecture"><code>GET /api/architecture</code></a> — embedded HSM-II blueprint (same as world API; <code>runtime</code> null here)</li>
    <li><a href="/api/company/health"><code>GET /api/company/health</code></a> — Company OS / Postgres (requires <code>HSM_COMPANY_OS_DATABASE_URL</code>)</li>
    <li><a href="/api/console/stats"><code>GET /api/console/stats</code></a> — sample JSON</li>
  </ul>
  <p>Full UI: from the repo run <code>cd web/company-console && npm run dev</code> → open <a href="http://localhost:3050">http://localhost:3050</a> (set <code>NEXT_PUBLIC_API_BASE</code> to this host if needed).</p>
</body>
</html>"#,
    )
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "hsm-console" }))
}

async fn read_trail_rows(home: &Path, limit: usize) -> Result<Vec<Value>, std::io::Error> {
    let path = home.join("memory/task_trail.jsonl");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = tokio::fs::read_to_string(&path).await?;
    let mut rows: Vec<Value> = Vec::new();
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            rows.push(v);
        }
    }
    let lim = limit.max(1).min(2000);
    if rows.len() > lim {
        rows = rows[rows.len() - lim..].to_vec();
    }
    Ok(rows)
}

async fn get_trail(
    State(st): State<ConsoleState>,
    Query(q): Query<TrailQuery>,
) -> Result<Json<Value>, StatusCode> {
    let path = st.home.join("memory/task_trail.jsonl");
    let rows = read_trail_rows(&st.home, q.limit)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "path": path.display().to_string(),
        "lines": rows,
    })))
}

/// Project `hyperedge` trail events into `{ nodes: [{id,label,kind}], links: [{source,target,rel}] }`.
fn trail_lines_to_graph(lines: &[Value]) -> Value {
    let mut nodes: HashMap<String, Value> = HashMap::new();
    let mut links: Vec<Value> = Vec::new();

    let ensure_node = |nodes: &mut HashMap<String, Value>, id: &str, kind: &str| {
        let id = id.to_string();
        nodes.entry(id.clone()).or_insert_with(|| {
            json!({
                "id": id,
                "label": id,
                "kind": kind,
            })
        });
    };

    for line in lines {
        let kind = line.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind != "hyperedge" {
            continue;
        }
        let rel = line
            .get("rel")
            .and_then(|r| r.as_str())
            .unwrap_or("related")
            .to_string();
        let hub_id = format!("rel:{rel}");
        ensure_node(&mut nodes, &hub_id, "relation");
        if let Some(obj) = nodes.get_mut(&hub_id) {
            if let Some(o) = obj.as_object_mut() {
                o.insert("label".to_string(), json!(rel.clone()));
            }
        }
        if let Some(arr) = line.get("participants").and_then(|p| p.as_array()) {
            for p in arr {
                let Some(pid) = p.as_str() else { continue };
                if pid.is_empty() {
                    continue;
                }
                ensure_node(&mut nodes, pid, "participant");
                links.push(json!({
                    "source": hub_id,
                    "target": pid,
                    "rel": rel,
                }));
            }
        }
    }

    let node_list: Vec<Value> = nodes.into_values().collect();
    json!({
        "nodes": node_list,
        "links": links,
    })
}

async fn get_graph_trail(
    State(st): State<ConsoleState>,
    Query(q): Query<TrailQuery>,
) -> Result<Json<Value>, StatusCode> {
    let rows = read_trail_rows(&st.home, q.limit.max(1).min(2000))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "source": "trail",
        "graph": trail_lines_to_graph(&rows),
    })))
}

fn hypergraph_candidate_paths(home: &Path) -> [PathBuf; 4] {
    [
        home.join("viz/hyper_graph.json"),
        home.join("hyper_graph.json"),
        home.join("memory/hyper_graph.json"),
        home.join("memory/viz/hyper_graph.json"),
    ]
}

async fn get_graph_hypergraph(State(st): State<ConsoleState>) -> Json<Value> {
    for p in hypergraph_candidate_paths(&st.home) {
        if p.is_file() {
            if let Ok(raw) = tokio::fs::read_to_string(&p).await {
                if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                    return Json(json!({
                        "path": p.display().to_string(),
                        "graph": normalize_hypergraph_export(&v),
                    }));
                }
            }
        }
    }
    Json(json!({
        "path": null,
        "graph": Value::Object(Map::new()),
        "hint": "Export hypergraph JSON to viz/hyper_graph.json or memory/hyper_graph.json",
    }))
}

/// Accept either `{ nodes, links }` or legacy shapes; pass through known keys.
fn normalize_hypergraph_export(v: &Value) -> Value {
    if v.get("nodes").is_some() || v.get("links").is_some() {
        return v.clone();
    }
    v.clone()
}

async fn get_memory_files(State(st): State<ConsoleState>) -> Json<Value> {
    let entries = list_memory_markdown_files(&st.home);
    Json(json!({
        "count": entries.len(),
        "files": entries.iter().map(|e| json!({
            "path": e.rel_path,
            "snippet": e.snippet,
        })).collect::<Vec<_>>(),
    }))
}

async fn get_stats(State(st): State<ConsoleState>) -> Json<Value> {
    let trail_path = st.home.join("memory/task_trail.jsonl");
    let trail_lines = if trail_path.is_file() {
        tokio::fs::read_to_string(&trail_path)
            .await
            .map(|s| s.lines().filter(|l| !l.is_empty()).count())
            .unwrap_or(0)
    } else {
        0
    };
    let mem_count = list_memory_markdown_files(&st.home).len();
    let mut tasks_in_progress = 0i64;
    if let Some(ref pool) = st.company_db {
        if let Ok(n) = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::bigint FROM tasks WHERE state = 'in_progress'",
        )
        .fetch_one(pool)
        .await
        {
            tasks_in_progress = n;
        }
    }
    Json(json!({
        "home": st.home.display().to_string(),
        "trail_lines": trail_lines,
        "memory_markdown_files": mem_count,
        "agents_enabled": 1,
        "tasks_in_progress": tasks_in_progress,
        "company_os": st.company_db.is_some(),
    }))
}

async fn get_search(
    State(st): State<ConsoleState>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, StatusCode> {
    let needle = q.q.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Json(json!({
            "q": q.q,
            "trail_hits": [],
            "memory_hits": [],
        })));
    }
    let lim = q.limit.max(1).min(200);

    let trail_rows = read_trail_rows(&st.home, 1500)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut trail_hits = Vec::new();
    for (i, row) in trail_rows.iter().enumerate() {
        let s = serde_json::to_string(row)
            .unwrap_or_default()
            .to_lowercase();
        if s.contains(&needle) {
            trail_hits.push(json!({
                "index": i,
                "kind": row.get("kind"),
                "preview":serde_json::to_string(row).ok().map(|x| x.chars().take(240).collect::<String>()),
            }));
            if trail_hits.len() >= lim {
                break;
            }
        }
    }

    let mut memory_hits = Vec::new();
    for e in list_memory_markdown_files(&st.home) {
        let hay = format!("{} {}", e.rel_path, e.snippet).to_lowercase();
        if hay.contains(&needle) {
            memory_hits.push(json!({
                "path": e.rel_path,
                "snippet": e.snippet.chars().take(200).collect::<String>(),
            }));
            if memory_hits.len() >= lim {
                break;
            }
        }
    }

    Ok(Json(json!({
        "q": q.q,
        "trail_hits": trail_hits,
        "memory_hits": memory_hits,
    })))
}

async fn get_autodream(State(st): State<ConsoleState>) -> Json<Value> {
    Json(autodream::staleness_snapshot(&st.home))
}

async fn post_email_draft(
    State(st): State<ConsoleState>,
    Json(body): Json<EmailDraftBody>,
) -> Result<Json<Value>, StatusCode> {
    let text = body.text.trim();
    if text.is_empty() {
        return Ok(Json(json!({
            "ok": false,
            "error": "empty text",
        })));
    }
    if text.len() > EMAIL_DRAFT_MAX_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let mut slot = st.draft_agent.lock().await;
    if slot.is_none() {
        match EnhancedPersonalAgent::initialize(&st.home).await {
            Ok(agent) => *slot = Some(agent),
            Err(e) => {
                error!(target: "hsm_console", "EnhancedPersonalAgent::initialize failed: {e}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    let agent = slot.as_mut().expect("just set");
    match agent
        .draft_email_reply_with_options(text, body.simple)
        .await
    {
        Ok(draft) => Ok(Json(json!({
            "ok": true,
            "draft": draft,
        }))),
        Err(e) => Ok(Json(json!({
            "ok": false,
            "error": e.to_string(),
        }))),
    }
}

async fn post_anti_sycophancy(
    Json(body): Json<AntiSycophancyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let um = body.user_message.trim();
    let dr = body.draft_response.trim();
    if um.is_empty() || dr.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "user_message and draft_response are required",
            })),
        ));
    }
    if body.draft_response.len() > ANTI_SYC_MAX_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "ok": false, "error": "draft_response too large" })),
        ));
    }

    let llm = LlmClient::new().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "error": format!("LLM unavailable: {e}"),
            })),
        )
    })?;

    let mut cfg = AntiSycophancyConfig::default();
    if let Some(m) = body.max_rounds {
        cfg.max_rounds = m.clamp(1, 6);
    }

    let run = run_anti_sycophancy_loop(
        Arc::new(llm),
        cfg,
        um,
        body.context.as_deref(),
        dr,
        &body.seed_directives,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({
        "ok": true,
        "result": run,
    })))
}

async fn post_council_socratic(
    Json(body): Json<CouncilSocraticBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let prop = body.proposition.trim();
    if prop.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "error": "proposition is required",
            })),
        ));
    }
    if body.proposition.len() > COUNCIL_SOCRATIC_MAX_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "ok": false, "error": "proposition too large" })),
        ));
    }

    let llm = LlmClient::new().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "error": format!("LLM unavailable: {e}"),
            })),
        )
    })?;

    let mut anti_cfg = AntiSycophancyConfig::default();
    if let Some(m) = body.anti_sycophancy_max_rounds {
        anti_cfg.max_rounds = m.clamp(1, 6);
    }

    let roles: Vec<String> = body
        .roles
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let run = run_council_socratic_with_anti_sycophancy(
        Arc::new(llm),
        anti_cfg,
        prop,
        body.context.as_deref(),
        &roles,
        body.council_rounds,
        &body.seed_directives,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({
        "ok": true,
        "result": run,
    })))
}
