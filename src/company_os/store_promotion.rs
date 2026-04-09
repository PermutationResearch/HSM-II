//! Store promotion pipeline: RooDB / Ladybug artifacts → Postgres Company OS.
//!
//! Two ingress paths:
//!   1. **RooDB → Postgres**: reads skills from the connected RooDB instance and upserts into
//!      `company_memory_entries` with `source = "roodb_promotion"` and provenance audit.
//!   2. **Ladybug bundle → Postgres**: accepts a JSON bundle of beliefs/skills and imports them
//!      with `source = "ladybug_import"`.
//!
//! Both paths write to `store_promotions` for audit / rollback inside a single transaction.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use uuid::Uuid;

use crate::console::ConsoleState;

use super::no_db;

const MAX_BATCH_SIZE: usize = 500;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/promote/roodb-skills",
            post(promote_roodb_skills),
        )
        .route(
            "/api/company/companies/:company_id/promote/ladybug-bundle",
            post(import_ladybug_bundle),
        )
        .route(
            "/api/company/companies/:company_id/promote/rollback/:promotion_id",
            post(rollback_promotion),
        )
        .route(
            "/api/company/companies/:company_id/promotions",
            axum::routing::get(list_promotions),
        )
}

fn db_err(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    tracing::error!(target: "hsm.store_promotion", error = %e, "database error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "Internal database error" })),
    )
}

// ── Types ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RooDbPromoteBody {
    #[serde(default)]
    skills: Option<Vec<RooDbSkillInput>>,
    #[serde(default = "default_promoted_by")]
    promoted_by: String,
}

fn default_promoted_by() -> String {
    "operator".into()
}

#[derive(Deserialize, Serialize, Clone)]
struct RooDbSkillInput {
    skill_id: String,
    title: String,
    principle: String,
    #[serde(default = "default_level")]
    level: String,
    role: Option<String>,
    task: Option<String>,
    #[serde(default = "default_confidence")]
    confidence: f64,
    #[serde(default)]
    usage_count: u64,
    #[serde(default)]
    success_count: u64,
    #[serde(default)]
    failure_count: u64,
    #[serde(default = "default_status")]
    status: String,
}

fn default_level() -> String {
    "General".into()
}
fn default_confidence() -> f64 {
    0.5
}
fn default_status() -> String {
    "active".into()
}

#[derive(Deserialize)]
struct LadybugBundleBody {
    #[serde(default)]
    beliefs: Vec<LadybugBeliefInput>,
    #[serde(default = "default_promoted_by")]
    promoted_by: String,
}

#[derive(Deserialize, Serialize, Clone)]
struct LadybugBeliefInput {
    id: Option<String>,
    content: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default = "default_confidence")]
    confidence: f64,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    abstract_l0: Option<String>,
    #[serde(default)]
    overview_l1: Option<String>,
    #[serde(default)]
    owner_namespace: Option<String>,
}

// ── Shared guard: verify company exists ──────────────────────────────────

async fn verify_company(
    pool: &sqlx::PgPool,
    company_id: Uuid,
) -> Result<(), (StatusCode, Json<Value>)> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1)")
        .bind(company_id)
        .fetch_one(pool)
        .await
        .unwrap_or(false);
    if !exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
    }
    Ok(())
}

// ── RooDB → Postgres promotion (C1: transactional, I2: company check, I5: no leak) ──

async fn promote_roodb_skills(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<RooDbPromoteBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool = st.company_db.as_ref().ok_or_else(no_db)?;
    verify_company(pool, company_id).await?;

    let skills = match body.skills {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Provide `skills` array (fetched from RooDB) in the request body." })),
            ));
        }
    };

    if skills.len() > MAX_BATCH_SIZE {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Batch size {} exceeds maximum {MAX_BATCH_SIZE}", skills.len()) })),
        ));
    }

    let mut tx = pool.begin().await.map_err(db_err)?;

    let mut promoted = 0u32;
    let mut skipped = 0u32;
    let mut rows: Vec<Value> = Vec::new();

    for skill in &skills {
        let already = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM store_promotions WHERE company_id = $1 AND source_store = 'roodb' AND source_id = $2 AND status = 'promoted')",
        )
        .bind(company_id)
        .bind(&skill.skill_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(false);

        if already {
            skipped += 1;
            continue;
        }

        let snapshot = serde_json::to_value(skill).unwrap_or_default();

        let title = format!("[skill] {}", skill.title);
        let body_text = format!(
            "**Principle:** {}\n\n**Level:** {} | **Confidence:** {:.2} | **Usage:** {} (success {}, failure {})\n\n**Status:** {}{}{}",
            skill.principle,
            skill.level,
            skill.confidence,
            skill.usage_count,
            skill.success_count,
            skill.failure_count,
            skill.status,
            skill.role.as_deref().map(|r| format!("\n**Role:** {}", r)).unwrap_or_default(),
            skill.task.as_deref().map(|t| format!("\n**Task:** {}", t)).unwrap_or_default(),
        );

        let tags: Vec<String> = {
            let mut t = vec!["skill".to_string(), "roodb_promotion".to_string()];
            if let Some(ref r) = skill.role {
                t.push(format!("role:{}", r));
            }
            t
        };

        let mem_id = sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO company_memory_entries
               (company_id, scope, title, body, tags, source, kind, source_type, source_uri)
               VALUES ($1, 'shared', $2, $3, $4, 'roodb_promotion', 'skill', 'api', $5)
               RETURNING id"#,
        )
        .bind(company_id)
        .bind(&title)
        .bind(&body_text)
        .bind(&tags)
        .bind(format!("roodb://skills/{}", skill.skill_id))
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;

        let promo_id = sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO store_promotions
               (company_id, source_store, source_id, source_snapshot, target_table, target_id, promoted_by)
               VALUES ($1, 'roodb', $2, $3, 'company_memory_entries', $4, $5)
               RETURNING id"#,
        )
        .bind(company_id)
        .bind(&skill.skill_id)
        .bind(&snapshot)
        .bind(mem_id)
        .bind(&body.promoted_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;

        rows.push(json!({
            "promotion_id": promo_id,
            "memory_id": mem_id,
            "skill_id": skill.skill_id,
            "title": skill.title,
        }));
        promoted += 1;
    }

    tx.commit().await.map_err(db_err)?;

    Ok(Json(json!({
        "ok": true,
        "promoted": promoted,
        "skipped_already_promoted": skipped,
        "rows": rows,
    })))
}

// ── Ladybug bundle → Postgres import (C1: transactional, I2: company check) ──

async fn import_ladybug_bundle(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<LadybugBundleBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool = st.company_db.as_ref().ok_or_else(no_db)?;
    verify_company(pool, company_id).await?;

    if body.beliefs.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Provide `beliefs` array in the request body." })),
        ));
    }

    if body.beliefs.len() > MAX_BATCH_SIZE {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Batch size {} exceeds maximum {MAX_BATCH_SIZE}", body.beliefs.len()) })),
        ));
    }

    let mut tx = pool.begin().await.map_err(db_err)?;

    let mut imported = 0u32;
    let mut skipped = 0u32;
    let mut rows: Vec<Value> = Vec::new();

    for (idx, belief) in body.beliefs.iter().enumerate() {
        let source_id = belief
            .id
            .clone()
            .unwrap_or_else(|| format!("ladybug_belief_{}", idx));

        let already = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM store_promotions WHERE company_id = $1 AND source_store = 'ladybug' AND source_id = $2 AND status = 'promoted')",
        )
        .bind(company_id)
        .bind(&source_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or(false);

        if already {
            skipped += 1;
            continue;
        }

        let snapshot = serde_json::to_value(belief).unwrap_or_default();

        let title = belief
            .title
            .clone()
            .unwrap_or_else(|| truncate_title(&belief.content, 120));

        let kind = if belief.tags.iter().any(|t| t == "skill") {
            "skill"
        } else {
            "belief"
        };

        let tags: Vec<String> = {
            let mut t = belief.tags.clone();
            t.push("ladybug_import".to_string());
            if let Some(ref ns) = belief.owner_namespace {
                t.push(format!("namespace:{}", ns));
            }
            t.sort();
            t.dedup();
            t
        };

        let source_label = belief
            .source
            .clone()
            .unwrap_or_else(|| "ladybug_import".into());

        let mem_id = sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO company_memory_entries
               (company_id, scope, title, body, tags, source, summary_l0, summary_l1, kind, source_type, source_uri)
               VALUES ($1, 'shared', $2, $3, $4, $5, $6, $7, $8, 'file', $9)
               RETURNING id"#,
        )
        .bind(company_id)
        .bind(&title)
        .bind(&belief.content)
        .bind(&tags)
        .bind(&source_label)
        .bind(&belief.abstract_l0)
        .bind(&belief.overview_l1)
        .bind(kind)
        .bind(format!("ladybug://beliefs/{}", source_id))
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;

        let promo_id = sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO store_promotions
               (company_id, source_store, source_id, source_snapshot, target_table, target_id, promoted_by)
               VALUES ($1, 'ladybug', $2, $3, 'company_memory_entries', $4, $5)
               RETURNING id"#,
        )
        .bind(company_id)
        .bind(&source_id)
        .bind(&snapshot)
        .bind(mem_id)
        .bind(&body.promoted_by)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;

        rows.push(json!({
            "promotion_id": promo_id,
            "memory_id": mem_id,
            "source_id": source_id,
            "title": title,
        }));
        imported += 1;
    }

    tx.commit().await.map_err(db_err)?;

    Ok(Json(json!({
        "ok": true,
        "imported": imported,
        "skipped_already_imported": skipped,
        "rows": rows,
    })))
}

// ── Rollback (C2: atomic transaction, check DELETE affected) ─────────────

async fn rollback_promotion(
    State(st): State<ConsoleState>,
    Path((company_id, promotion_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool = st.company_db.as_ref().ok_or_else(no_db)?;

    let mut tx = pool.begin().await.map_err(db_err)?;

    let row = sqlx::query_as::<_, (Uuid, Option<Uuid>, String, String)>(
        "SELECT id, target_id, target_table, status FROM store_promotions WHERE id = $1 AND company_id = $2 FOR UPDATE",
    )
    .bind(promotion_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;

    let Some((_id, target_id, target_table, status)) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "promotion not found" })),
        ));
    };

    if status != "promoted" {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": format!("promotion status is '{}', not 'promoted'", status) })),
        ));
    }

    if target_table == "company_memory_entries" {
        if let Some(tid) = target_id {
            sqlx::query("DELETE FROM company_memory_entries WHERE id = $1 AND company_id = $2")
                .bind(tid)
                .bind(company_id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
    }

    sqlx::query("UPDATE store_promotions SET status = 'rolled_back' WHERE id = $1")
        .bind(promotion_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, $2, 'promotion_rollback', 'promotion', $3, $4, 'high')"#,
    )
    .bind(company_id)
    .bind("store_promotion")
    .bind(promotion_id.to_string())
    .bind(SqlxJson(json!({
        "target_table": target_table,
        "target_id": target_id,
    })))
    .execute(&mut *tx)
    .await;

    tx.commit().await.map_err(db_err)?;

    Ok(Json(json!({ "ok": true, "rolled_back": promotion_id })))
}

// ── List promotions ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListPromotionsQuery {
    #[serde(default)]
    source_store: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    100
}

async fn list_promotions(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<ListPromotionsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let pool = st.company_db.as_ref().ok_or_else(no_db)?;

    let rows = sqlx::query_as::<_, (Uuid, Uuid, String, String, Value, String, Option<Uuid>, String, String, String)>(
        r#"SELECT id, company_id, source_store, source_id, source_snapshot,
                  target_table, target_id, promoted_by, status, created_at::text
           FROM store_promotions
           WHERE company_id = $1
             AND ($2::text IS NULL OR source_store = $2)
             AND ($3::text IS NULL OR status = $3)
           ORDER BY created_at DESC
           LIMIT $4"#,
    )
    .bind(company_id)
    .bind(&q.source_store)
    .bind(&q.status)
    .bind(q.limit.min(500))
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let promotions: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, cid, source_store, source_id, snapshot, target_table, target_id, promoted_by, status, created_at)| {
                json!({
                    "id": id,
                    "company_id": cid,
                    "source_store": source_store,
                    "source_id": source_id,
                    "source_snapshot": snapshot,
                    "target_table": target_table,
                    "target_id": target_id,
                    "promoted_by": promoted_by,
                    "status": status,
                    "created_at": created_at,
                })
            },
        )
        .collect();

    Ok(Json(json!({ "promotions": promotions })))
}

// ── Helpers (C4: UTF-8 safe truncate) ────────────────────────────────────

fn truncate_title(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() <= max {
        first_line.to_string()
    } else {
        let boundary = first_line
            .char_indices()
            .take_while(|(i, _)| *i <= max)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        format!("{}…", &first_line[..boundary])
    }
}
