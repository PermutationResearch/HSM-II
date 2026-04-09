//! Company-scoped shared memory pool (`company_memory_entries`) and HTTP API.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::Response,
    routing::{delete, get, patch, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::console::ConsoleState;

use super::company_memory_hybrid as hybrid;
use super::memory_summaries::derive_summary_l0_l1;
use super::no_db;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/memory",
            get(list_memory).post(create_memory),
        )
        .route(
            "/api/company/companies/:company_id/memory/export.md",
            get(export_shared_memory_markdown),
        )
        .route(
            "/api/company/companies/:company_id/memory/:memory_id/delete",
            post(post_delete_memory_entry),
        )
        .route(
            "/api/company/companies/:company_id/memory/:memory_id",
            patch(patch_memory).delete(delete_memory),
        )
        // ── Memory graph endpoints ───────────────────────────────────────────
        .route(
            "/api/company/companies/:company_id/memory/:memory_id/edges",
            get(list_memory_edges).post(create_memory_edge),
        )
        .route(
            "/api/company/companies/:company_id/memory/:memory_id/lineage",
            get(get_memory_lineage),
        )
        .route(
            "/api/company/companies/:company_id/memory/:memory_id/neighborhood",
            get(get_memory_neighborhood),
        )
        .route(
            "/api/company/companies/:company_id/memory-edges/:edge_id",
            delete(delete_memory_edge),
        )
}

#[derive(sqlx::FromRow, Serialize, Clone)]
pub struct CompanyMemoryRow {
    pub id: Uuid,
    pub company_id: Uuid,
    pub scope: String,
    pub company_agent_id: Option<Uuid>,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub source: String,
    pub summary_l0: Option<String>,
    pub summary_l1: Option<String>,
    pub kind: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
struct MemoryListQuery {
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    company_agent_id: Option<Uuid>,
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    latest_only: Option<bool>,
    #[serde(default)]
    entity_type: Option<String>,
    #[serde(default)]
    entity_id: Option<String>,
    #[serde(default)]
    valid_at: Option<DateTime<Utc>>,
    #[serde(default)]
    document_date_from: Option<DateTime<Utc>>,
    #[serde(default)]
    document_date_to: Option<DateTime<Utc>>,
    #[serde(default)]
    event_date_from: Option<DateTime<Utc>>,
    #[serde(default)]
    event_date_to: Option<DateTime<Utc>>,
}

fn hybrid_search_env_enabled() -> bool {
    match std::env::var("HSM_MEMORY_HYBRID") {
        Ok(s) => {
            let t = s.trim().to_ascii_lowercase();
            !(t == "0" || t == "false" || t == "no" || t == "off")
        }
        Err(_) => true,
    }
}

async fn fetch_memory_rows_ordered(
    pool: &sqlx::PgPool,
    ids: &[Uuid],
) -> Result<Vec<CompanyMemoryRow>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    sqlx::query_as::<_, CompanyMemoryRow>(
        r#"SELECT e.id, e.company_id, e.scope, e.company_agent_id, e.title, e.body, e.tags, e.source,
                  e.summary_l0, e.summary_l1, e.kind, e.created_at::text, e.updated_at::text
           FROM company_memory_entries e
           JOIN unnest($1::uuid[]) WITH ORDINALITY AS u(id, ord) ON e.id = u.id
           ORDER BY u.ord"#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await
}

/// Unified list: optional full-text + substring (`ILIKE`) on title/body/summaries; `kind` tie-break.
async fn list_memory(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<MemoryListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let mode = q
        .scope
        .as_deref()
        .unwrap_or("shared")
        .trim()
        .to_ascii_lowercase();
    if mode == "agent" && q.company_agent_id.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company_agent_id required when scope=agent" })),
        ));
    }
    let mode_key = match mode.as_str() {
        "all" => "all",
        "agent" => "agent",
        _ => "shared",
    };
    let agent_bind = q.company_agent_id.unwrap_or_else(Uuid::nil);
    let mut search_options = hybrid::HybridSearchOptions::for_scope(mode_key, agent_bind);
    search_options.latest_only = q.latest_only.unwrap_or(false);
    search_options.entity_type = q.entity_type.clone();
    search_options.entity_id = q.entity_id.clone();
    search_options.valid_at = q.valid_at;
    search_options.document_date_from = q.document_date_from;
    search_options.document_date_to = q.document_date_to;
    search_options.event_date_from = q.event_date_from;
    search_options.event_date_to = q.event_date_to;
    search_options.limit = 200;

    let q_raw =
        q.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("");
    let q_search = if q_raw.len() > 512 {
        q_raw.chars().take(512).collect::<String>()
    } else {
        q_raw.to_string()
    };
    let like_pat = if q_search.is_empty() {
        "%".to_string()
    } else {
        format!("%{}%", q_search.replace('%', "\\%").replace('_', "\\_"))
    };

    if !q_search.is_empty() && hybrid_search_env_enabled() {
        match hybrid::hybrid_search_memory_ids_with_options(pool, company_id, &q_search, &search_options).await {
            Ok((ids, meta)) if !ids.is_empty() => {
                let rows = fetch_memory_rows_ordered(pool, &ids).await.map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": e.to_string() })),
                    )
                })?;
                return Ok(Json(json!({
                    "entries": rows,
                    "scope_filter": mode,
                    "search": {
                        "mode": meta.mode,
                        "channels": meta.channels,
                        "reranked": meta.reranked,
                        "expansion_terms": meta.expansion_terms,
                        "filters": {
                            "latest_only": search_options.latest_only,
                            "entity_type": search_options.entity_type,
                            "entity_id": search_options.entity_id,
                            "valid_at": search_options.valid_at,
                            "document_date_from": search_options.document_date_from,
                            "document_date_to": search_options.document_date_to,
                            "event_date_from": search_options.event_date_from,
                            "event_date_to": search_options.event_date_to,
                        }
                    }
                })));
            }
            Ok((_empty, _meta)) => {
                tracing::debug!(target: "hsm.company_memory", "hybrid fusion returned no ids; falling back to fts");
            }
            Err(e) => {
                tracing::warn!(target: "hsm.company_memory", ?e, "hybrid memory search failed; using fts fallback");
            }
        }
    }

    let rows: Vec<CompanyMemoryRow> = sqlx::query_as::<_, CompanyMemoryRow>(
        r#"SELECT id, company_id, scope, company_agent_id, title, body, tags, source,
                  summary_l0, summary_l1, kind, created_at::text, updated_at::text
           FROM company_memory_entries
           WHERE company_id = $1
             AND CASE $4::text
               WHEN 'all' THEN true
               WHEN 'shared' THEN scope = 'shared'
               WHEN 'agent' THEN scope = 'agent' AND company_agent_id = $5
               ELSE false
             END
             AND ($6::bool = false OR is_latest = true)
             AND ($7::text IS NULL OR entity_type = $7)
             AND ($8::text IS NULL OR entity_id = $8)
             AND ($9::timestamptz IS NULL OR valid_from IS NULL OR valid_from <= $9)
             AND ($9::timestamptz IS NULL OR valid_to IS NULL OR valid_to >= $9)
             AND ($10::timestamptz IS NULL OR document_date >= $10)
             AND ($11::timestamptz IS NULL OR document_date <= $11)
             AND ($12::timestamptz IS NULL OR event_date >= $12)
             AND ($13::timestamptz IS NULL OR event_date <= $13)
             AND (
               trim(coalesce($2::text, '')) = ''
               OR to_tsvector(
                    'english',
                    coalesce(title, '') || ' ' || coalesce(body, '') || ' ' || coalesce(summary_l1, '') || ' ' || coalesce(summary_l0, '')
                  ) @@ plainto_tsquery('english', trim($2::text))
               OR title ILIKE $3 ESCAPE '\'
               OR body ILIKE $3 ESCAPE '\'
               OR COALESCE(summary_l1, '') ILIKE $3 ESCAPE '\'
               OR COALESCE(summary_l0, '') ILIKE $3 ESCAPE '\'
             )
           ORDER BY
             CASE WHEN trim(coalesce($2::text, '')) = '' THEN 0::real
                  ELSE ts_rank_cd(
                    to_tsvector(
                      'english',
                      coalesce(title, '') || ' ' || coalesce(body, '') || ' ' || coalesce(summary_l1, '') || ' ' || coalesce(summary_l0, '')
                    ),
                    plainto_tsquery('english', trim($2::text))
                  )
             END DESC,
             CASE WHEN kind = 'broadcast' THEN 0 ELSE 1 END,
             updated_at DESC
           LIMIT 200"#,
    )
    .bind(company_id)
    .bind(&q_search)
    .bind(&like_pat)
    .bind(mode_key)
    .bind(agent_bind)
    .bind(search_options.latest_only)
    .bind(search_options.entity_type.as_deref())
    .bind(search_options.entity_id.as_deref())
    .bind(search_options.valid_at)
    .bind(search_options.document_date_from)
    .bind(search_options.document_date_to)
    .bind(search_options.event_date_from)
    .bind(search_options.event_date_to)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({ "entries": rows, "scope_filter": mode })))
}

#[derive(Deserialize)]
struct CreateMemoryBody {
    title: String,
    #[serde(default)]
    body: String,
    /// `shared` or `agent`
    scope: String,
    #[serde(default)]
    company_agent_id: Option<Uuid>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    summary_l0: Option<String>,
    #[serde(default)]
    summary_l1: Option<String>,
    /// `general` or `broadcast` (shared pool: merged first in llm-context).
    #[serde(default)]
    kind: Option<String>,
}

fn normalize_memory_kind(raw: Option<&str>) -> Result<String, &'static str> {
    let k = raw.unwrap_or("general").trim().to_ascii_lowercase();
    if k == "general" || k == "broadcast" {
        Ok(k)
    } else {
        Err("kind must be general or broadcast")
    }
}

async fn create_memory(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateMemoryBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let title = body.title.trim().to_string();
    if title.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "title required" })),
        ));
    }
    let scope = body.scope.trim().to_ascii_lowercase();
    if scope != "shared" && scope != "agent" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "scope must be shared or agent" })),
        ));
    }
    if scope == "agent" && body.company_agent_id.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company_agent_id required for agent scope" })),
        ));
    }
    let kind = normalize_memory_kind(body.kind.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
    if scope == "agent" && kind == "broadcast" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "broadcast kind is only valid for shared scope" })),
        ));
    }
    if let Some(aid) = body.company_agent_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM company_agents WHERE id = $1 AND company_id = $2)",
        )
        .bind(aid)
        .bind(company_id)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
        if !ok {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "company_agent_id not in company" })),
            ));
        }
    }
    let source = body
        .source
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("human")
        .to_string();

    let body_trim = body.body.trim().to_string();
    let mut s0 = body
        .summary_l0
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut s1 = body
        .summary_l1
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if s0.is_none() && s1.is_none() {
        let (a, b) = derive_summary_l0_l1(&title, &body_trim);
        s0 = a;
        s1 = b;
    }

    let row = sqlx::query_as::<_, CompanyMemoryRow>(
        r#"INSERT INTO company_memory_entries
           (company_id, scope, company_agent_id, title, body, tags, source, summary_l0, summary_l1, kind)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           RETURNING id, company_id, scope, company_agent_id, title, body, tags, source,
                     summary_l0, summary_l1, kind, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&scope)
    .bind(body.company_agent_id)
    .bind(&title)
    .bind(&body_trim)
    .bind(&body.tags)
    .bind(&source)
    .bind(s0)
    .bind(s1)
    .bind(&kind)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let pool_e = pool.clone();
    let mid = row.id;
    let t = row.title.clone();
    let b = row.body.clone();
    tokio::spawn(async move {
        hybrid::embed_row_after_write(pool_e, mid, t, b).await;
    });

    Ok((StatusCode::CREATED, Json(json!({ "entry": row }))))
}

#[derive(Deserialize, Default)]
struct PatchMemoryBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    summary_l0: Option<String>,
    #[serde(default)]
    summary_l1: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

async fn patch_memory(
    State(st): State<ConsoleState>,
    Path((company_id, memory_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchMemoryBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let title_upd = body
        .title
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let kind_upd = if let Some(ref k) = body.kind {
        Some(
            normalize_memory_kind(Some(k.as_str()))
                .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?,
        )
    } else {
        None
    };
    let row = sqlx::query_as::<_, CompanyMemoryRow>(
        r#"UPDATE company_memory_entries SET
            title = COALESCE($3, title),
            body = COALESCE($4, body),
            tags = COALESCE($5, tags),
            summary_l0 = COALESCE($6, summary_l0),
            summary_l1 = COALESCE($7, summary_l1),
            kind = COALESCE($8, kind),
            updated_at = NOW()
           WHERE id = $1 AND company_id = $2
           RETURNING id, company_id, scope, company_agent_id, title, body, tags, source,
                     summary_l0, summary_l1, kind, created_at::text, updated_at::text"#,
    )
    .bind(memory_id)
    .bind(company_id)
    .bind(title_upd)
    .bind(
        body.body
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty()),
    )
    .bind(body.tags.as_ref())
    .bind(
        body.summary_l0
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty()),
    )
    .bind(
        body.summary_l1
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty()),
    )
    .bind(kind_upd.as_deref())
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(row) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "memory entry not found" })),
        ));
    };
    if body.title.is_some() || body.body.is_some() {
        let pool_e = pool.clone();
        let mid = memory_id;
        let t = row.title.clone();
        let b = row.body.clone();
        tokio::spawn(async move {
            hybrid::embed_row_after_write(pool_e, mid, t, b).await;
        });
    }
    Ok(Json(json!({ "entry": row })))
}

/// Same as [`delete_memory`] for clients that cannot send `DELETE` through a proxy.
async fn post_delete_memory_entry(
    State(st): State<ConsoleState>,
    Path((company_id, memory_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    delete_memory(State(st), Path((company_id, memory_id))).await
}

async fn delete_memory(
    State(st): State<ConsoleState>,
    Path((company_id, memory_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let r = sqlx::query("DELETE FROM company_memory_entries WHERE id = $1 AND company_id = $2")
        .bind(memory_id)
        .bind(company_id)
        .execute(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    if r.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "memory entry not found" })),
        ));
    }
    Ok(Json(json!({ "ok": true, "id": memory_id })))
}

/// Git-friendly export of shared memories (Postgres remains source of truth).
async fn export_shared_memory_markdown(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows: Vec<(String, String, Option<String>, String)> = sqlx::query_as(
        r#"SELECT title, body, summary_l1, kind FROM company_memory_entries
           WHERE company_id = $1 AND scope = 'shared'
           ORDER BY CASE WHEN kind = 'broadcast' THEN 0 ELSE 1 END, updated_at DESC"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let mut md = String::from("# SHARED_MEMORY_INDEX\n\n");
    md.push_str(
        "<!-- Generated from company_memory_entries (shared). Source of truth: Postgres. -->\n\n",
    );
    for (title, body, sum1, kind) in rows {
        let body_use = sum1
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(body.as_str());
        md.push_str(&format!("## {title}\n"));
        if kind == "broadcast" {
            md.push_str("*kind: broadcast*\n\n");
        }
        md.push_str(body_use);
        md.push_str("\n\n---\n\n");
    }

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
        .body(Body::from(md))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    Ok(resp)
}

/// UTF-8 budget for each of shared-memory and agent-memory blocks in task `llm-context` (default ~3k).
///
/// Override: `HSM_COMPANY_MEMORY_LLM_CONTEXT_MAX_BYTES`.
pub fn company_memory_llm_context_max_bytes() -> usize {
    std::env::var("HSM_COMPANY_MEMORY_LLM_CONTEXT_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0 && n <= 100_000)
        .unwrap_or(3072)
}

/// Rows to load per pool, newest first (default **1** = single most recent entry only).
///
/// Override: `HSM_COMPANY_MEMORY_LLM_CONTEXT_ENTRY_LIMIT` (1–50).
pub fn company_memory_llm_context_entry_limit() -> i64 {
    std::env::var("HSM_COMPANY_MEMORY_LLM_CONTEXT_ENTRY_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n >= 1 && n <= 50)
        .unwrap_or(1)
}

/// Markdown block injected into `GET /api/company/tasks/:id/llm-context` after company `context_markdown`.
///
/// Loads up to [`company_memory_llm_context_entry_limit`] newest **shared** rows and trims to
/// [`company_memory_llm_context_max_bytes`] so agents are not flooded with old memory.
pub async fn fetch_shared_memory_addon(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<String, sqlx::Error> {
    let limit = company_memory_llm_context_entry_limit();
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        r#"SELECT title, body, summary_l1 FROM company_memory_entries
           WHERE company_id = $1 AND scope = 'shared'
           ORDER BY CASE WHEN kind = 'broadcast' THEN 0 ELSE 1 END, updated_at DESC
           LIMIT $2"#,
    )
    .bind(company_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(String::new());
    }

    let mut out = String::from("## Company shared memory (injected: newest only; size-capped)\n\n");
    let mut total: usize = 0;
    let max_bytes = company_memory_llm_context_max_bytes();
    for (title, body, sum1) in rows {
        let body_use = sum1
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(body.as_str());
        let mut chunk = format!("### {title}\n{body_use}\n\n");
        if total + chunk.len() > max_bytes {
            let room = max_bytes.saturating_sub(total);
            if room > 200 {
                let mut n = room.saturating_sub(80);
                while n > 0 && !chunk.is_char_boundary(n) {
                    n -= 1;
                }
                chunk.truncate(n);
                chunk.push_str("\n\n_(truncated to context budget.)_\n\n");
                out.push_str(&chunk);
            }
            break;
        }
        total += chunk.len();
        out.push_str(&chunk);
    }
    Ok(out)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Memory graph: edges, lineage, neighborhood
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(sqlx::FromRow, Serialize)]
struct MemoryEdgeRow {
    id: Uuid,
    company_id: Uuid,
    from_memory_id: Uuid,
    to_memory_id: Uuid,
    relation_type: String,
    confidence: f32,
    metadata: sqlx::types::Json<Value>,
    created_at: String,
}

async fn list_memory_edges(
    State(st): State<ConsoleState>,
    Path((company_id, memory_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    let rows = sqlx::query_as::<_, MemoryEdgeRow>(
        r#"SELECT id, company_id, from_memory_id, to_memory_id, relation_type,
                  confidence, metadata, created_at::text
           FROM memory_edges
           WHERE company_id = $1 AND (from_memory_id = $2 OR to_memory_id = $2)
           ORDER BY created_at DESC LIMIT 200"#,
    )
    .bind(company_id).bind(memory_id)
    .fetch_all(pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "edges": rows })))
}

#[derive(Deserialize)]
struct CreateEdgeBody {
    to_memory_id: Uuid,
    relation_type: String,
    #[serde(default = "default_confidence")]
    confidence: f32,
    #[serde(default)]
    metadata: Option<Value>,
}
fn default_confidence() -> f32 { 1.0 }

async fn create_memory_edge(
    State(st): State<ConsoleState>,
    Path((company_id, memory_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreateEdgeBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    let meta = body.metadata.unwrap_or(json!({}));
    let id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO memory_edges (company_id, from_memory_id, to_memory_id, relation_type, confidence, metadata)
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (from_memory_id, to_memory_id, relation_type) DO UPDATE
             SET confidence = EXCLUDED.confidence, metadata = EXCLUDED.metadata
           RETURNING id"#,
    )
    .bind(company_id).bind(memory_id).bind(body.to_memory_id)
    .bind(&body.relation_type).bind(body.confidence).bind(sqlx::types::Json(meta))
    .fetch_one(pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    if body.relation_type == "supersedes" {
        let _ = sqlx::query("UPDATE company_memory_entries SET is_latest = false WHERE id = $1 AND company_id = $2")
            .bind(body.to_memory_id).bind(company_id).execute(pool).await;
    }
    Ok(Json(json!({ "id": id })))
}

async fn delete_memory_edge(
    State(st): State<ConsoleState>,
    Path((company_id, edge_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    sqlx::query("DELETE FROM memory_edges WHERE id = $1 AND company_id = $2")
        .bind(edge_id).bind(company_id).execute(pool).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "ok": true })))
}

async fn get_memory_lineage(
    State(st): State<ConsoleState>,
    Path((company_id, memory_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    let rows = sqlx::query_as::<_, CompanyMemoryRow>(
        r#"WITH RECURSIVE chain AS (
             SELECT *, 0 AS depth FROM company_memory_entries WHERE id = $2 AND company_id = $1
           UNION ALL
             SELECT m.*, c.depth + 1
             FROM chain c JOIN company_memory_entries m ON m.id = c.supersedes_memory_id AND m.company_id = $1
             WHERE c.depth < 20
           )
           SELECT id, company_id, scope, company_agent_id, title, body,
                  tags, source, summary_l0, summary_l1, kind, created_at::text, updated_at::text
           FROM chain ORDER BY depth"#,
    )
    .bind(company_id).bind(memory_id)
    .fetch_all(pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "lineage": rows })))
}

#[derive(Deserialize)]
struct NeighborhoodQuery {
    #[serde(default = "default_hops")]
    hops: u32,
    #[serde(default)]
    relation_types: Option<String>,
}
fn default_hops() -> u32 { 1 }

async fn get_memory_neighborhood(
    State(st): State<ConsoleState>,
    Path((company_id, memory_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<NeighborhoodQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    let max_hops = q.hops.min(3) as i32;
    let type_filter = q.relation_types.as_deref().unwrap_or("");

    let neighbor_ids: Vec<(Uuid, i32)> = sqlx::query_as(
        r#"WITH RECURSIVE bfs AS (
             SELECT $2::uuid AS node_id, 0 AS depth
           UNION
             SELECT CASE WHEN e.from_memory_id = b.node_id THEN e.to_memory_id ELSE e.from_memory_id END,
                    b.depth + 1
             FROM bfs b JOIN memory_edges e ON e.company_id = $1
               AND (e.from_memory_id = b.node_id OR e.to_memory_id = b.node_id)
               AND ($4 = '' OR e.relation_type = ANY(string_to_array($4, ',')))
             WHERE b.depth < $3
           )
           SELECT DISTINCT node_id, MIN(depth) AS depth FROM bfs GROUP BY node_id ORDER BY depth LIMIT 50"#,
    )
    .bind(company_id).bind(memory_id).bind(max_hops).bind(type_filter)
    .fetch_all(pool).await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    let node_ids: Vec<Uuid> = neighbor_ids.iter().map(|(id, _)| *id).collect();
    let memories = if node_ids.is_empty() { Vec::new() } else {
        sqlx::query_as::<_, CompanyMemoryRow>(
            r#"SELECT id, company_id, scope, company_agent_id, title, body,
                      tags, source, summary_l0, summary_l1, kind, created_at::text, updated_at::text
               FROM company_memory_entries WHERE company_id = $1 AND id = ANY($2)"#,
        ).bind(company_id).bind(&node_ids).fetch_all(pool).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
    };
    let edges = if node_ids.is_empty() { Vec::new() } else {
        sqlx::query_as::<_, MemoryEdgeRow>(
            r#"SELECT id, company_id, from_memory_id, to_memory_id, relation_type,
                      confidence, metadata, created_at::text
               FROM memory_edges WHERE company_id = $1 AND from_memory_id = ANY($2) AND to_memory_id = ANY($2)"#,
        ).bind(company_id).bind(&node_ids).fetch_all(pool).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?
    };
    Ok(Json(json!({
        "root": memory_id, "hops": max_hops,
        "memories": memories, "edges": edges,
        "node_count": memories.len(), "edge_count": edges.len(),
    })))
}

/// Graph-aware retrieval expansion: given initial hit IDs, traverse 1–2 hops on
/// memory_edges and return expanded set with relation-aware scoring.
pub async fn expand_via_graph(
    pool: &PgPool,
    company_id: Uuid,
    seed_ids: &[Uuid],
    max_hops: i32,
) -> Result<Vec<(Uuid, f64)>, sqlx::Error> {
    if seed_ids.is_empty() { return Ok(Vec::new()); }

    fn relation_weight(rt: &str) -> f64 {
        match rt {
            "extends" => 0.9, "updates" => 0.85, "derives" => 0.8,
            "supports" => 0.75, "supersedes" => 0.7, "related" => 0.6,
            "contradicts" => 0.4, _ => 0.5,
        }
    }

    let rows: Vec<(Uuid, String, i32)> = sqlx::query_as(
        r#"WITH RECURSIVE expansion AS (
             SELECT unnest($2::uuid[]) AS node_id, ''::text AS via_relation, 0 AS depth
           UNION
             SELECT CASE WHEN e.from_memory_id = ex.node_id THEN e.to_memory_id ELSE e.from_memory_id END,
                    e.relation_type, ex.depth + 1
             FROM expansion ex JOIN memory_edges e ON e.company_id = $1
               AND (e.from_memory_id = ex.node_id OR e.to_memory_id = ex.node_id)
             WHERE ex.depth < $3
           )
           SELECT DISTINCT node_id, via_relation, MIN(depth) AS depth
           FROM expansion WHERE depth > 0 GROUP BY node_id, via_relation ORDER BY depth LIMIT 100"#,
    )
    .bind(company_id).bind(seed_ids).bind(max_hops)
    .fetch_all(pool).await?;

    let seed_set: std::collections::HashSet<Uuid> = seed_ids.iter().copied().collect();
    let mut scored: std::collections::HashMap<Uuid, f64> = std::collections::HashMap::new();
    for (nid, rel, depth) in &rows {
        if seed_set.contains(nid) { continue; }
        let score = (1.0 / (1.0 + *depth as f64)) * relation_weight(rel);
        let entry = scored.entry(*nid).or_insert(0.0);
        if score > *entry { *entry = score; }
    }

    let candidate_ids: Vec<Uuid> = scored.keys().copied().collect();
    if candidate_ids.is_empty() { return Ok(Vec::new()); }
    let latest_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM company_memory_entries WHERE company_id = $1 AND id = ANY($2) AND is_latest = true",
    ).bind(company_id).bind(&candidate_ids).fetch_all(pool).await?;

    let latest_set: std::collections::HashSet<Uuid> = latest_ids.into_iter().collect();
    let mut result: Vec<(Uuid, f64)> = scored.into_iter().filter(|(id, _)| latest_set.contains(id)).collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(result)
}

/// Markdown block for `scope = agent` rows tied to a workforce agent (`company_agent_id`).
/// Injected into task LLM context after shared pool, before task spec.
pub async fn fetch_agent_memory_addon(
    pool: &PgPool,
    company_id: Uuid,
    company_agent_id: Uuid,
) -> Result<String, sqlx::Error> {
    let limit = company_memory_llm_context_entry_limit();
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        r#"SELECT title, body, summary_l1 FROM company_memory_entries
           WHERE company_id = $1 AND scope = 'agent' AND company_agent_id = $2
           ORDER BY updated_at DESC LIMIT $3"#,
    )
    .bind(company_id)
    .bind(company_agent_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(String::new());
    }

    let mut out = String::from("## Company agent memory (injected: newest only; size-capped)\n\n");
    let mut total: usize = 0;
    let max_bytes = company_memory_llm_context_max_bytes();
    for (title, body, sum1) in rows {
        let body_use = sum1
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(body.as_str());
        let mut chunk = format!("### {title}\n{body_use}\n\n");
        if total + chunk.len() > max_bytes {
            let room = max_bytes.saturating_sub(total);
            if room > 200 {
                let mut n = room.saturating_sub(80);
                while n > 0 && !chunk.is_char_boundary(n) {
                    n -= 1;
                }
                chunk.truncate(n);
                chunk.push_str("\n\n_(truncated to context budget.)_\n\n");
                out.push_str(&chunk);
            }
            break;
        }
        total += chunk.len();
        out.push_str(&chunk);
    }
    Ok(out)
}
