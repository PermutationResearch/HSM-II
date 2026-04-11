//! Context repository HTTP API: layout contract, publish into shared memory (hybrid / “supermemory”), rollback.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::console::ConsoleState;
use crate::harness::context_repo::{
    default_manifest_for_session, ContextRepoManifest, CONTEXT_REPO_FORMAT_VERSION, INDEX_FILE,
    MANIFEST_FILE, NOTES_DIR, SNAPSHOTS_DIR,
};
use crate::harness::context_repo::{repo_root_for_company_home, sanitize_session_key};

use super::company_memory_hybrid as hybrid;
use super::memory_summaries::derive_summary_l0_l1;
use super::no_db;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/context-repo/contract",
            get(get_contract),
        )
        .route(
            "/api/company/companies/:company_id/context-repo/ensure",
            post(post_ensure),
        )
        .route(
            "/api/company/companies/:company_id/context-repo/publish",
            post(post_publish),
        )
        .route(
            "/api/company/companies/:company_id/context-repo/rollback",
            post(post_rollback),
        )
        .route(
            "/api/company/companies/:company_id/context-repo/publishes",
            get(list_publishes),
        )
}

async fn fetch_hsmii_home(pool: &PgPool, company_id: Uuid) -> Result<String, (StatusCode, Json<Value>)> {
    let cell: Option<Option<String>> = sqlx::query_scalar("SELECT hsmii_home FROM companies WHERE id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    let Some(inner) = cell else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))));
    };
    let Some(h) = inner else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company has no hsmii_home" })),
        ));
    };
    let t = h.trim();
    if t.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company has no hsmii_home" })),
        ));
    }
    Ok(t.to_string())
}

fn db_err(ctx: &str, e: &sqlx::Error) -> (StatusCode, Json<Value>) {
    tracing::error!(target: "hsm.context_repo", %ctx, ?e, "context repo db error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "Internal database error" })),
    )
}

#[derive(Deserialize)]
struct SessionQuery {
    session_key: String,
}

async fn get_contract(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<SessionQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let sk = q.session_key.trim();
    if sk.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "session_key required" })),
        ));
    }
    let home_str = fetch_hsmii_home(pool, company_id).await?;
    let home = PathBuf::from(&home_str);
    let root = repo_root_for_company_home(&home, sk);
    let rel = format!("context-repos/{}/", sanitize_session_key(sk));
    Ok(Json(json!({
        "contract": {
            "format_version": CONTEXT_REPO_FORMAT_VERSION,
            "expected_paths": crate::harness::context_repo::expected_relative_paths(),
            "company_repo_root_relative": rel,
            "manifest_schema": {
                "format_version": "string (required, use \"1\")",
                "title": "string",
                "session_key": "string",
                "notes_globs": ["glob patterns for markdown"],
                "description": "string"
            }
        },
        "absolute_repo_root": root.to_string_lossy(),
        "exists": root.is_dir(),
    })))
}

#[derive(Deserialize)]
struct EnsureBody {
    session_key: String,
}

async fn post_ensure(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<EnsureBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let sk = body.session_key.trim();
    if sk.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "session_key required" })),
        ));
    }
    let _ = company_exists(pool, company_id).await?;
    let home_str = fetch_hsmii_home(pool, company_id).await?;
    let home = PathBuf::from(&home_str);
    let root = repo_root_for_company_home(&home, sk);
    std::fs::create_dir_all(root.join(NOTES_DIR)).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    std::fs::create_dir_all(root.join(SNAPSHOTS_DIR)).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let manifest_path = root.join(MANIFEST_FILE);
    if !manifest_path.is_file() {
        let m = default_manifest_for_session(sk);
        let txt = serde_json::to_string_pretty(&m).unwrap_or_else(|_| "{}".to_string());
        std::fs::write(&manifest_path, txt).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    }
    let index_path = root.join(INDEX_FILE);
    if !index_path.is_file() {
        let stub = format!("# Context repository index\n\nSession: `{sk}`\n\nLink key notes under `notes/`.\n");
        std::fs::write(&index_path, stub).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    }
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, 'context_repo', 'ensure_layout', 'company', $2, $3, 'info')"#,
    )
    .bind(company_id)
    .bind(company_id.to_string())
    .bind(SqlxJson(json!({
        "session_key": sk,
        "repo_root": root.to_string_lossy(),
    })))
    .execute(pool)
    .await;
    Ok(Json(json!({
        "ok": true,
        "repo_root": root.to_string_lossy(),
    })))
}

async fn company_exists(pool: &PgPool, company_id: Uuid) -> Result<(), (StatusCode, Json<Value>)> {
    let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1)")
        .bind(company_id)
        .fetch_one(pool)
        .await
        .map_err(|e| db_err("company_exists", &e))?;
    if !ok {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))));
    }
    Ok(())
}

#[derive(Deserialize)]
struct PublishBody {
    session_key: String,
    #[serde(default)]
    dry_run: Option<bool>,
}

async fn post_publish(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<PublishBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let sk = body.session_key.trim();
    if sk.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "session_key required" })),
        ));
    }
    let dry_run = body.dry_run.unwrap_or(false);
    company_exists(pool, company_id).await?;
    let home_str = fetch_hsmii_home(pool, company_id).await?;
    let home = PathBuf::from(&home_str);
    let root = repo_root_for_company_home(&home, sk);
    if !root.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "context repo root does not exist; call POST .../context-repo/ensure first",
                "repo_root": root.to_string_lossy(),
            })),
        ));
    }
    let manifest_path = root.join(MANIFEST_FILE);
    let index_path = root.join(INDEX_FILE);
    if !manifest_path.is_file() || !index_path.is_file() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "manifest.json and INDEX.md are required under the context repo root" })),
        ));
    }
    let manifest_raw = std::fs::read_to_string(&manifest_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let manifest: ContextRepoManifest = serde_json::from_str(&manifest_raw).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("invalid manifest.json: {e}") })),
        )
    })?;
    if manifest.format_version != CONTEXT_REPO_FORMAT_VERSION {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("unsupported manifest format_version (expected {CONTEXT_REPO_FORMAT_VERSION})"),
            })),
        ));
    }
    let manifest_sha = crate::harness::context_repo::sha256_hex(manifest_raw.as_bytes());
    let index_txt = std::fs::read_to_string(&index_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let notes_root = root.join(NOTES_DIR);
    let mut note_sections = String::new();
    if notes_root.is_dir() {
        let mut paths: Vec<PathBuf> = WalkDir::new(&notes_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("md"))
            .map(|e| e.path().to_path_buf())
            .collect();
        paths.sort();
        for p in paths {
            let rel = p.strip_prefix(&root).unwrap_or(&p);
            let piece = std::fs::read_to_string(&p).unwrap_or_default();
            note_sections.push_str(&format!("\n### {}\n\n{}\n", rel.display(), piece));
        }
    }
    let canonical = format!(
        "## manifest.json (sha256 {})\n{}\n\n## INDEX.md\n{}\n\n## Notes{}",
        manifest_sha, manifest_raw, index_txt, note_sections
    );
    let content_sha = crate::harness::context_repo::sha256_hex(canonical.as_bytes());
    let base_rel = format!("context-repos/{}", sanitize_session_key(sk));
    let title = format!(
        "Context repo · {}",
        manifest
            .title
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(sk)
    );
    if dry_run {
        return Ok(Json(json!({
            "ok": true,
            "dry_run": true,
            "manifest_sha256": manifest_sha,
            "content_sha256": content_sha,
            "base_rel_path": base_rel,
            "byte_length": canonical.len(),
        })));
    }
    let prev_pub: Option<(Uuid, Uuid)> = sqlx::query_as(
        r#"SELECT id, memory_id FROM company_context_repo_publishes
           WHERE company_id = $1 AND session_key = $2 AND rolled_back_at IS NULL
           ORDER BY created_at DESC LIMIT 1"#,
    )
    .bind(company_id)
    .bind(sk)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("publish_prev", &e))?;
    let (previous_publish_id, old_memory_id): (Option<Uuid>, Option<Uuid>) = match prev_pub {
        Some((pid, mid)) => (Some(pid), Some(mid)),
        None => (None, None),
    };
    let tags = json!(["context-repo", format!("session:{sk}")]);
    let source = "context_repo:snapshot".to_string();
    let (s0, s1) = derive_summary_l0_l1(&title, &canonical);
    let mut tx = pool.begin().await.map_err(|e| db_err("publish_tx", &e))?;
    let memory_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO company_memory_entries
           (company_id, scope, company_agent_id, title, body, tags, source, summary_l0, summary_l1, kind, is_latest)
           VALUES ($1, 'shared', NULL, $2, $3, $4, $5, $6, $7, 'general', true)
           RETURNING id"#,
    )
    .bind(company_id)
    .bind(&title)
    .bind(&canonical)
    .bind(SqlxJson(tags.clone()))
    .bind(&source)
    .bind(s0)
    .bind(s1)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| db_err("publish_insert_memory", &e))?;
    if let Some(old_id) = old_memory_id {
        let eid: Uuid = sqlx::query_scalar(
            r#"INSERT INTO memory_edges (company_id, from_memory_id, to_memory_id, relation_type, confidence, metadata)
               VALUES ($1, $2, $3, 'supersedes', 1.0, $4)
               ON CONFLICT (from_memory_id, to_memory_id, relation_type) DO UPDATE
                 SET confidence = EXCLUDED.confidence, metadata = EXCLUDED.metadata
               RETURNING id"#,
        )
        .bind(company_id)
        .bind(memory_id)
        .bind(old_id)
        .bind(SqlxJson(json!({ "kind": "context_repo_publish" })))
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| db_err("publish_edge", &e))?;
        sqlx::query("UPDATE company_memory_entries SET is_latest = false WHERE id = $1 AND company_id = $2")
            .bind(old_id)
            .bind(company_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| db_err("publish_deprecate_old", &e))?;
        let _ = eid;
    }
    let publish_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO company_context_repo_publishes
           (company_id, session_key, base_rel_path, manifest_sha256, content_sha256, memory_id, previous_publish_id)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id"#,
    )
    .bind(company_id)
    .bind(sk)
    .bind(&base_rel)
    .bind(&manifest_sha)
    .bind(&content_sha)
    .bind(memory_id)
    .bind(previous_publish_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| db_err("publish_row", &e))?;
    sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, 'context_repo', 'publish_snapshot', 'memory', $2, $3, 'info')"#,
    )
    .bind(company_id)
    .bind(memory_id.to_string())
    .bind(SqlxJson(json!({
        "publish_id": publish_id,
        "session_key": sk,
        "manifest_sha256": manifest_sha,
        "content_sha256": content_sha,
        "previous_publish_id": previous_publish_id,
    })))
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err("publish_gov", &e))?;
    tx.commit().await.map_err(|e| db_err("publish_commit", &e))?;
    let pool_e = pool.clone();
    let t = title.clone();
    let b = canonical.clone();
    tokio::spawn(async move {
        hybrid::embed_row_after_write(pool_e, memory_id, t, b).await;
    });
    Ok(Json(json!({
        "ok": true,
        "publish_id": publish_id,
        "memory_id": memory_id,
        "manifest_sha256": manifest_sha,
        "content_sha256": content_sha,
        "supersedes_memory_id": old_memory_id,
    })))
}

#[derive(Deserialize)]
struct RollbackBody {
    publish_id: Uuid,
}

async fn post_rollback(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<RollbackBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    company_exists(pool, company_id).await?;
    let row: Option<(Uuid, Uuid, Option<Uuid>)> = sqlx::query_as(
        r#"SELECT id, memory_id, previous_publish_id FROM company_context_repo_publishes
           WHERE id = $1 AND company_id = $2 AND rolled_back_at IS NULL"#,
    )
    .bind(body.publish_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("rollback_fetch", &e))?;
    let Some((pub_id, new_mem, prev_pub)) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "publish not found or already rolled back" })),
        ));
    };
    let Some(prev_pub_id) = prev_pub else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "this publish has no previous snapshot to restore" })),
        ));
    };
    let prev_mem: Option<Uuid> = sqlx::query_scalar(
        "SELECT memory_id FROM company_context_repo_publishes WHERE id = $1 AND company_id = $2",
    )
    .bind(prev_pub_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("rollback_prev_mem", &e))?;
    let Some(old_mem) = prev_mem else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "previous publish row missing" })),
        ));
    };
    let mut tx = pool.begin().await.map_err(|e| db_err("rollback_tx", &e))?;
    sqlx::query("UPDATE company_memory_entries SET is_latest = false WHERE id = $1 AND company_id = $2")
        .bind(new_mem)
        .bind(company_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err("rollback_new", &e))?;
    sqlx::query("UPDATE company_memory_entries SET is_latest = true WHERE id = $1 AND company_id = $2")
        .bind(old_mem)
        .bind(company_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err("rollback_old", &e))?;
    sqlx::query(
        "DELETE FROM memory_edges WHERE company_id = $1 AND from_memory_id = $2 AND to_memory_id = $3 AND relation_type = 'supersedes'",
    )
    .bind(company_id)
    .bind(new_mem)
    .bind(old_mem)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err("rollback_edge", &e))?;
    sqlx::query(
        "UPDATE company_context_repo_publishes SET rolled_back_at = NOW() WHERE id = $1 AND company_id = $2",
    )
    .bind(pub_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err("rollback_mark", &e))?;
    sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, 'context_repo', 'rollback_snapshot', 'memory', $2, $3, 'info')"#,
    )
    .bind(company_id)
    .bind(old_mem.to_string())
    .bind(SqlxJson(json!({
        "rolled_back_publish_id": pub_id,
        "restored_memory_id": old_mem,
        "deprecated_memory_id": new_mem,
    })))
    .execute(&mut *tx)
    .await
    .map_err(|e| db_err("rollback_gov", &e))?;
    tx.commit().await.map_err(|e| db_err("rollback_commit", &e))?;
    Ok(Json(json!({
        "ok": true,
        "restored_memory_id": old_mem,
        "deprecated_memory_id": new_mem,
    })))
}

async fn list_publishes(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<SessionQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let sk = q.session_key.trim();
    if sk.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "session_key required" })),
        ));
    }
    company_exists(pool, company_id).await?;
    let rows: Vec<(Uuid, String, String, String, String, Uuid, Option<Uuid>, Option<String>)> =
        sqlx::query_as(
            r#"SELECT id, session_key, base_rel_path, manifest_sha256, content_sha256, memory_id,
                      previous_publish_id, rolled_back_at::text
               FROM company_context_repo_publishes
               WHERE company_id = $1 AND session_key = $2
               ORDER BY created_at DESC
               LIMIT 50"#,
        )
        .bind(company_id)
        .bind(sk)
        .fetch_all(pool)
        .await
        .map_err(|e| db_err("list_pub", &e))?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, session_key, base_rel, msha, csha, mem, prev, rb)| {
                json!({
                    "id": id,
                    "session_key": session_key,
                    "base_rel_path": base_rel,
                    "manifest_sha256": msha,
                    "content_sha256": csha,
                    "memory_id": mem,
                    "previous_publish_id": prev,
                    "rolled_back_at": rb,
                })
            },
        )
        .collect();
    Ok(Json(json!({ "publishes": out })))
}
