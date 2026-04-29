use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use uuid::Uuid;

use crate::console::ConsoleState;

use super::no_db;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/tool-sources",
            get(list_tool_sources).post(create_tool_source),
        )
        .route(
            "/api/company/companies/:company_id/tool-sources/:source_id/ingest",
            post(ingest_tool_source),
        )
        .route(
            "/api/company/companies/:company_id/tool-catalog",
            get(list_tool_catalog),
        )
        .route(
            "/api/company/companies/:company_id/tools/discover",
            post(discover_tools),
        )
        .route(
            "/api/company/companies/:company_id/tools/seed-builtin",
            post(seed_builtin_tools),
        )
        .route(
            "/api/company/companies/:company_id/tools/:tool_key/describe",
            get(describe_tool),
        )
        .route(
            "/api/company/companies/:company_id/tools/:tool_key/call",
            post(call_tool),
        )
        .route(
            "/api/company/companies/:company_id/tools/executions/:execution_id/resume",
            post(resume_execution),
        )
}

#[derive(sqlx::FromRow, Serialize)]
struct ToolSourceRow {
    id: Uuid,
    company_id: Uuid,
    kind: String,
    name: String,
    source_url: Option<String>,
    auth: SqlxJson<Value>,
    config: SqlxJson<Value>,
    status: String,
    last_ingested_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow, Serialize)]
struct ToolCatalogRow {
    id: Uuid,
    company_id: Uuid,
    source_id: Option<Uuid>,
    tool_key: String,
    display_name: String,
    description: Option<String>,
    schema: SqlxJson<Value>,
    meta: SqlxJson<Value>,
    active: bool,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow, Serialize)]
struct ToolExecRow {
    id: Uuid,
    company_id: Uuid,
    tool_key: String,
    status: String,
    args: SqlxJson<Value>,
    flow: SqlxJson<Value>,
    result: Option<SqlxJson<Value>>,
    error: Option<String>,
    resume_token: Option<String>,
    resumed_from: Option<Uuid>,
    created_at: String,
    updated_at: String,
}

async fn verify_company(pool: &sqlx::PgPool, company_id: Uuid) -> Result<(), (StatusCode, Json<Value>)> {
    let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1)")
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
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))));
    }
    Ok(())
}

fn sanitize_tool_key(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

#[derive(Deserialize)]
struct CreateToolSourceBody {
    kind: String,
    name: String,
    #[serde(default)]
    source_url: Option<String>,
    #[serde(default)]
    auth: Option<Value>,
    #[serde(default)]
    config: Option<Value>,
}

async fn list_tool_sources(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let rows = sqlx::query_as::<_, ToolSourceRow>(
        r#"SELECT id, company_id, kind, name, source_url, auth, config, status, last_ingested_at,
                  created_at::text, updated_at::text
           FROM company_tool_sources
           WHERE company_id = $1
           ORDER BY updated_at DESC"#,
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
    Ok(Json(json!({ "sources": rows })))
}

async fn create_tool_source(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateToolSourceBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;

    let kind = body.kind.trim().to_ascii_lowercase();
    if !matches!(kind.as_str(), "openapi" | "graphql" | "mcp" | "custom") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "kind must be openapi|graphql|mcp|custom" })),
        ));
    }
    let name = body.name.trim();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "name required" }))));
    }

    let row = sqlx::query_as::<_, ToolSourceRow>(
        r#"INSERT INTO company_tool_sources
           (company_id, kind, name, source_url, auth, config, status)
           VALUES ($1, $2, $3, $4, $5, $6, 'active')
           RETURNING id, company_id, kind, name, source_url, auth, config, status, last_ingested_at,
                     created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&kind)
    .bind(name)
    .bind(body.source_url.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(SqlxJson(body.auth.unwrap_or_else(|| json!({}))))
    .bind(SqlxJson(body.config.unwrap_or_else(|| json!({}))))
    .fetch_one(pool)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("uq_company_tool_sources_name") {
            return (StatusCode::CONFLICT, Json(json!({ "error": "tool source name already exists" })));
        }
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": msg })),
        )
    })?;
    Ok((StatusCode::CREATED, Json(json!({ "source": row }))))
}

fn extract_tools_from_source(src: &ToolSourceRow) -> Vec<(String, String, Option<String>, Value, Value)> {
    let mut out: Vec<(String, String, Option<String>, Value, Value)> = Vec::new();
    if let Some(arr) = src
        .config
        .0
        .get("tools")
        .and_then(|v| v.as_array())
    {
        for t in arr {
            let key_raw = t
                .get("tool_key")
                .or_else(|| t.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let key = sanitize_tool_key(key_raw);
            if key.is_empty() {
                continue;
            }
            let display = t
                .get("display_name")
                .or_else(|| t.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or(key_raw)
                .trim()
                .to_string();
            let desc = t.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
            let schema = t.get("schema").cloned().unwrap_or_else(|| json!({}));
            let meta = json!({
                "source_kind": src.kind,
                "source_name": src.name,
                "source_url": src.source_url,
                "ingested_via": "config.tools"
            });
            out.push((key, display, desc, schema, meta));
        }
    }
    if out.is_empty() {
        let key = sanitize_tool_key(&format!("{}.invoke", src.name));
        let display = format!("{} invoke", src.name);
        let schema = json!({
            "type":"object",
            "properties": {
                "input": {"type":"object"},
                "action": {"type":"string"}
            }
        });
        let meta = json!({
            "source_kind": src.kind,
            "source_name": src.name,
            "source_url": src.source_url,
            "ingested_via": "fallback"
        });
        out.push((key, display, Some(format!("Proxy call for source {}", src.name)), schema, meta));
    }
    out
}

async fn ingest_tool_source(
    State(st): State<ConsoleState>,
    Path((company_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let src = sqlx::query_as::<_, ToolSourceRow>(
        r#"SELECT id, company_id, kind, name, source_url, auth, config, status, last_ingested_at,
                  created_at::text, updated_at::text
           FROM company_tool_sources
           WHERE id = $1 AND company_id = $2"#,
    )
    .bind(source_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(src) = src else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "tool source not found" }))));
    };

    let tools = extract_tools_from_source(&src);
    let mut ingested = 0usize;
    for (tool_key, display_name, description, schema, meta) in tools {
        sqlx::query(
            r#"INSERT INTO company_tool_catalog
               (company_id, source_id, tool_key, display_name, description, schema, meta, active)
               VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE)
               ON CONFLICT (company_id, tool_key) DO UPDATE SET
                 source_id = EXCLUDED.source_id,
                 display_name = EXCLUDED.display_name,
                 description = EXCLUDED.description,
                 schema = EXCLUDED.schema,
                 meta = EXCLUDED.meta,
                 active = TRUE,
                 updated_at = NOW()"#,
        )
        .bind(company_id)
        .bind(source_id)
        .bind(tool_key)
        .bind(display_name)
        .bind(description)
        .bind(SqlxJson(schema))
        .bind(SqlxJson(meta))
        .execute(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
        ingested += 1;
    }

    sqlx::query("UPDATE company_tool_sources SET last_ingested_at = NOW(), updated_at = NOW() WHERE id = $1")
        .bind(source_id)
        .execute(pool)
        .await
        .ok();

    Ok(Json(json!({ "ok": true, "ingested": ingested, "source_id": source_id })))
}

async fn list_tool_catalog(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let rows = sqlx::query_as::<_, ToolCatalogRow>(
        r#"SELECT id, company_id, source_id, tool_key, display_name, description, schema, meta, active,
                  created_at::text, updated_at::text
           FROM company_tool_catalog
           WHERE company_id = $1
           ORDER BY active DESC, updated_at DESC"#,
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
    Ok(Json(json!({ "tools": rows })))
}

#[derive(Deserialize)]
struct DiscoverBody {
    query: String,
    #[serde(default)]
    limit: Option<i64>,
}

async fn discover_tools(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<DiscoverBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let q = body.query.trim().to_ascii_lowercase();
    let limit = body.limit.unwrap_or(8).clamp(1, 40) as usize;

    // Lazy bootstrap: if the catalog is empty for this company, auto-seed built-in tools
    // so the first discover call just works without requiring a manual seed step.
    let catalog_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM company_tool_catalog WHERE company_id = $1")
            .bind(company_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
    if catalog_count == 0 {
        tracing::info!(company_id = %company_id, "company_tool_catalog empty — auto-seeding built-in tools");
        if let Err(e) = do_seed_builtin(pool, company_id).await {
            tracing::warn!(company_id = %company_id, error = %e, "auto-seed failed, continuing with empty catalog");
        }
    }

    let rows = sqlx::query_as::<_, ToolCatalogRow>(
        r#"SELECT id, company_id, source_id, tool_key, display_name, description, schema, meta, active,
                  created_at::text, updated_at::text
           FROM company_tool_catalog
           WHERE company_id = $1 AND active = TRUE"#,
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

    let mut scored: Vec<(i32, &ToolCatalogRow)> = rows
        .iter()
        .map(|r| {
            let mut score = 0;
            let key = r.tool_key.to_ascii_lowercase();
            let name = r.display_name.to_ascii_lowercase();
            let desc = r.description.as_deref().unwrap_or("").to_ascii_lowercase();
            if q.is_empty() {
                score = 1;
            } else {
                if key.contains(&q) {
                    score += 8;
                }
                if name.contains(&q) {
                    score += 6;
                }
                if desc.contains(&q) {
                    score += 3;
                }
            }
            (score, r)
        })
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.tool_key.cmp(&b.1.tool_key)));
    let matches: Vec<Value> = scored
        .into_iter()
        .take(limit)
        .map(|(score, r)| {
            json!({
                "tool_key": r.tool_key,
                "display_name": r.display_name,
                "description": r.description,
                "score": score
            })
        })
        .collect();
    Ok(Json(json!({ "query": body.query, "matches": matches })))
}

async fn describe_tool(
    State(st): State<ConsoleState>,
    Path((company_id, tool_key)): Path<(Uuid, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let tk = sanitize_tool_key(&tool_key);
    let row = sqlx::query_as::<_, ToolCatalogRow>(
        r#"SELECT id, company_id, source_id, tool_key, display_name, description, schema, meta, active,
                  created_at::text, updated_at::text
           FROM company_tool_catalog
           WHERE company_id = $1 AND tool_key = $2 AND active = TRUE"#,
    )
    .bind(company_id)
    .bind(tk)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(row) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "tool not found" }))));
    };
    Ok(Json(json!({
        "tool": {
            "tool_key": row.tool_key,
            "display_name": row.display_name,
            "description": row.description,
            "schema": row.schema.0,
            "meta": row.meta.0,
        }
    })))
}

#[derive(Deserialize)]
struct CallBody {
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    flow: Option<Value>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    simulate_pause: Option<String>,
}

fn has_discover_describe(flow: &Value, tool_key: &str) -> bool {
    let discovered = flow
        .get("discovered_tool_keys")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .any(|k| sanitize_tool_key(k) == tool_key)
        })
        .unwrap_or(false);
    let described = flow
        .get("described_tool_key")
        .and_then(|v| v.as_str())
        .map(|k| sanitize_tool_key(k) == tool_key)
        .unwrap_or(false);
    discovered && described
}

async fn call_tool(
    State(st): State<ConsoleState>,
    Path((company_id, tool_key)): Path<(Uuid, String)>,
    Json(body): Json<CallBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let tk = sanitize_tool_key(&tool_key);
    let row: Option<(String, SqlxJson<Value>)> = sqlx::query_as(
        "SELECT tool_key, meta FROM company_tool_catalog WHERE company_id = $1 AND tool_key = $2 AND active = TRUE",
    )
    .bind(company_id)
    .bind(&tk)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    if row.is_none() {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "tool not found" }))));
    }
    let flow = body.flow.unwrap_or_else(|| json!({}));
    if !has_discover_describe(&flow, &tk) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "strict flow required: discover->describe before call" })),
        ));
    }
    let dry_run = body.dry_run.unwrap_or(false);
    let pause_kind = body
        .simulate_pause
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let status = if pause_kind == "auth" {
        "paused_auth"
    } else if pause_kind == "approval" {
        "paused_approval"
    } else {
        "success"
    };
    let resume_token = if status.starts_with("paused_") {
        Some(format!("resume_{}", Uuid::new_v4()))
    } else {
        None
    };
    let exec = sqlx::query_as::<_, ToolExecRow>(
        r#"INSERT INTO company_tool_executions
           (company_id, tool_key, status, args, flow, result, error, resume_token, resumed_from)
           VALUES ($1, $2, $3, $4, $5, $6, NULL, $7, NULL)
           RETURNING id, company_id, tool_key, status, args, flow, result, error, resume_token, resumed_from,
                     created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&tk)
    .bind(status)
    .bind(SqlxJson(body.args.unwrap_or_else(|| json!({}))))
    .bind(SqlxJson(flow))
    .bind(if status == "success" {
        Some(SqlxJson(json!({
            "ok": true,
            "dry_run": dry_run,
            "message": if dry_run { "dry-run call accepted" } else { "call accepted" }
        })))
    } else {
        None
    })
    .bind(resume_token.as_deref())
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok((StatusCode::CREATED, Json(json!({ "execution": exec }))))
}

#[derive(Deserialize)]
struct ResumeBody {
    #[serde(default)]
    actor: Option<String>,
}

async fn resume_execution(
    State(st): State<ConsoleState>,
    Path((company_id, execution_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ResumeBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let row = sqlx::query_as::<_, ToolExecRow>(
        r#"SELECT id, company_id, tool_key, status, args, flow, result, error, resume_token, resumed_from,
                  created_at::text, updated_at::text
           FROM company_tool_executions
           WHERE id = $1 AND company_id = $2"#,
    )
    .bind(execution_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(row) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "execution not found" }))));
    };
    if row.status != "paused_auth" && row.status != "paused_approval" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "execution is not paused" })),
        ));
    }
    let actor = body.actor.unwrap_or_else(|| "operator".to_string());
    let resumed = sqlx::query_as::<_, ToolExecRow>(
        r#"INSERT INTO company_tool_executions
           (company_id, tool_key, status, args, flow, result, error, resume_token, resumed_from)
           VALUES ($1, $2, 'resumed', $3, $4, NULL, NULL, NULL, $5)
           RETURNING id, company_id, tool_key, status, args, flow, result, error, resume_token, resumed_from,
                     created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&row.tool_key)
    .bind(row.args)
    .bind(SqlxJson(json!({
        "resumed_by": actor,
        "from_execution_id": execution_id,
        "previous_flow": row.flow.0
    })))
    .bind(execution_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let final_row = sqlx::query_as::<_, ToolExecRow>(
        r#"UPDATE company_tool_executions
           SET status = 'success',
               result = $3,
               updated_at = NOW()
           WHERE id = $1 AND company_id = $2
           RETURNING id, company_id, tool_key, status, args, flow, result, error, resume_token, resumed_from,
                     created_at::text, updated_at::text"#,
    )
    .bind(resumed.id)
    .bind(company_id)
    .bind(SqlxJson(json!({"ok": true, "message": "execution resumed and completed"})))
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok(Json(json!({ "execution": final_row })))
}

// ---------------------------------------------------------------------------
// Built-in tool seeding
// ---------------------------------------------------------------------------

/// Seed `company_tool_catalog` with the 70+ native HSM tools for `company_id`.
///
/// Uses the in-process `ToolRegistry` (via `register_all_tools`) as the single source
/// of truth so tool_key / description / JSON-schema stay in sync with actual tool impls.
///
/// Creates (or upserts) a `company_tool_sources` row named `"hsm_native"` with
/// `kind = 'custom'`, then upserts every tool from the registry into `company_tool_catalog`.
/// Idempotent — safe to call multiple times.
async fn do_seed_builtin(pool: &sqlx::PgPool, company_id: Uuid) -> Result<usize, String> {
    // Build an in-process registry to get accurate names/descriptions/schemas.
    let mut registry = crate::tools::ToolRegistry::new();
    crate::tools::register_all_tools(&mut registry);
    let schemas = registry.get_schemas(); // Vec<Value>

    // Upsert the built-in tool source row.
    let source_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO company_tool_sources
               (company_id, kind, name, source_url, auth, config, status)
               VALUES ($1, 'custom', 'hsm_native', NULL, '{}'::jsonb, '{}'::jsonb, 'active')
               ON CONFLICT (company_id, name) DO UPDATE SET
                 status = 'active',
                 updated_at = NOW()
               RETURNING id"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut seeded = 0usize;
    for entry in &schemas {
        // Each entry is { "type": "function", "function": { "name":…, "description":…, "parameters":… } }
        let func = match entry.get("function") {
            Some(f) => f,
            None => continue,
        };
        let tool_key_raw = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if tool_key_raw.is_empty() {
            continue;
        }
        let tool_key = sanitize_tool_key(tool_key_raw);
        let display_name = tool_key
            .split(['_', '-'])
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let description = func.get("description").and_then(|v| v.as_str()).map(str::to_string);
        let schema = func.get("parameters").cloned().unwrap_or_else(|| json!({}));
        let meta = json!({
            "source_kind": "custom",
            "source_name": "hsm_native",
            "ingested_via": "seed_builtin"
        });

        sqlx::query(
            r#"INSERT INTO company_tool_catalog
               (company_id, source_id, tool_key, display_name, description, schema, meta, active)
               VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE)
               ON CONFLICT (company_id, tool_key) DO UPDATE SET
                 source_id    = EXCLUDED.source_id,
                 display_name = EXCLUDED.display_name,
                 description  = EXCLUDED.description,
                 schema       = EXCLUDED.schema,
                 meta         = EXCLUDED.meta,
                 active       = TRUE,
                 updated_at   = NOW()"#,
        )
        .bind(company_id)
        .bind(source_id)
        .bind(&tool_key)
        .bind(&display_name)
        .bind(description.as_deref())
        .bind(SqlxJson(schema))
        .bind(SqlxJson(meta))
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

        seeded += 1;
    }

    // Stamp last_ingested_at on the source.
    let _ = sqlx::query(
        "UPDATE company_tool_sources SET last_ingested_at = NOW(), updated_at = NOW() WHERE id = $1",
    )
    .bind(source_id)
    .execute(pool)
    .await;

    Ok(seeded)
}

/// `POST /api/company/companies/{company_id}/tools/seed-builtin`
///
/// Idempotent bootstrap: populates (or refreshes) the native HSM tool definitions in
/// `company_tool_catalog` so `company_tool_discover` returns real results.
async fn seed_builtin_tools(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;

    match do_seed_builtin(pool, company_id).await {
        Ok(n) => Ok(Json(json!({ "ok": true, "seeded": n }))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e })))),
    }
}

