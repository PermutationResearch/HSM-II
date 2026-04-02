//! Company OS API — PostgreSQL-backed companies, goals, tasks (Paperclip-class control plane, MVP).
//!
//! Enable with `HSM_COMPANY_OS_DATABASE_URL=postgres://...`. Migrations in `migrations/` run on startup.

mod agents;
mod bundle;
mod paperclip_import;
pub mod onboarding_contracts;
mod spend;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch, post},
    Json, Router,
};
pub use bundle::{export_bundle, import_bundle as run_import_bundle, CompanyBundle, ImportRequest};
pub use spend::spawn_record_llm_spend;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Json as SqlxJson;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::console::ConsoleState;
use self::onboarding_contracts::{
    evaluate_gate_results, find_contract, load_contracts_hot, OnboardingGateResult,
};

/// Max size for `companies.context_markdown` (POST/PATCH body).
const MAX_COMPANY_CONTEXT_MARKDOWN_BYTES: usize = 512 * 1024;

/// Connect pool and run migrations when `HSM_COMPANY_OS_DATABASE_URL` is set and non-empty.
pub async fn connect_optional() -> anyhow::Result<Option<PgPool>> {
    let Ok(url) = std::env::var("HSM_COMPANY_OS_DATABASE_URL") else {
        return Ok(None);
    };
    let url = url.trim();
    if url.is_empty() {
        return Ok(None);
    }
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(Some(pool))
}

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .merge(agents::router())
        .route("/api/company/health", get(company_health))
        .route("/api/company/import", post(import_company_bundle))
        .route("/api/company/onboarding/contracts", get(list_onboarding_pack_contracts))
        .route(
            "/api/company/onboarding/contracts/validate",
            post(validate_onboarding_pack_contract),
        )
        .route("/api/company/onboarding/draft", post(generate_onboarding_draft))
        .route("/api/company/onboarding/apply", post(apply_onboarding_draft))
        .route("/api/company/companies", get(list_companies).post(create_company))
        .route(
            "/api/company/companies/:company_id/api-catalog",
            get(company_api_catalog),
        )
        .route(
            "/api/company/companies/:company_id",
            get(get_company)
                .patch(patch_company)
                .delete(delete_company),
        )
        .route(
            "/api/company/companies/:company_id/import-paperclip-home",
            post(import_paperclip_home),
        )
        .route(
            "/api/company/companies/:company_id/export",
            get(export_company_json),
        )
        .route(
            "/api/company/companies/:company_id/spend/summary",
            get(spend_summary),
        )
        .route(
            "/api/company/companies/:company_id/governance/events",
            get(list_governance).post(post_governance),
        )
        .route(
            "/api/company/companies/:company_id/policies/rules",
            get(list_policy_rules).post(post_policy_rule),
        )
        .route(
            "/api/company/companies/:company_id/policies/evaluate",
            post(evaluate_policy),
        )
        .route(
            "/api/company/companies/:company_id/goals",
            get(list_goals).post(create_goal),
        )
        .route(
            "/api/company/companies/:company_id/goals/:goal_id",
            patch(patch_goal),
        )
        .route(
            "/api/company/companies/:company_id/tasks",
            get(list_tasks).post(create_task),
        )
        .route(
            "/api/company/companies/:company_id/spawn-rules",
            get(list_spawn_rules).post(create_spawn_rule),
        )
        .route(
            "/api/company/companies/:company_id/tasks/:task_id/spawn-subagents",
            post(spawn_subagent_tasks),
        )
        .route(
            "/api/company/companies/:company_id/tasks/:task_id/handoffs",
            get(list_task_handoffs).post(post_task_handoff),
        )
        .route(
            "/api/company/task-handoffs/:handoff_id/review",
            post(review_task_handoff),
        )
        .route(
            "/api/company/companies/:company_id/improvement-runs",
            get(list_improvement_runs).post(create_improvement_run),
        )
        .route(
            "/api/company/improvement-runs/:run_id/decision",
            post(decide_improvement_run),
        )
        .route(
            "/api/company/contracts/versions",
            get(list_contract_versions).post(create_contract_version),
        )
        .route(
            "/api/company/contracts/versions/:version_id/status",
            patch(patch_contract_version_status),
        )
        .route(
            "/api/company/connectors/presets",
            get(list_connector_presets).post(upsert_connector_preset),
        )
        .route(
            "/api/company/companies/:company_id/go-live-checklist",
            get(list_go_live_checklist).post(post_go_live_checklist_item),
        )
        .route(
            "/api/company/companies/:company_id/go-live-checklist/seed",
            post(seed_go_live_checklist),
        )
        .route(
            "/api/company/go-live-checklist/:item_id/complete",
            post(complete_go_live_checklist_item),
        )
        .route(
            "/api/company/companies/:company_id/tasks/queue",
            get(list_task_queue),
        )
        .route("/api/company/tasks/:task_id/sla", patch(patch_task_sla))
        .route("/api/company/tasks/:task_id/decision", post(post_task_decision))
        .route("/api/company/tasks/:task_id/checkout", post(checkout_task))
        .route("/api/company/tasks/:task_id/release", post(release_task))
        .route(
            "/api/company/tasks/:task_id/run-telemetry",
            post(post_task_run_telemetry),
        )
}

fn hash_idem_payload(v: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(v.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

async fn register_idempotency(
    pool: &PgPool,
    company_id: Uuid,
    scope: &str,
    idempotency_key: &str,
    payload: &Value,
) -> Result<bool, sqlx::Error> {
    let request_hash = hash_idem_payload(payload);
    let inserted = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO request_idempotency (company_id, scope, idempotency_key, request_hash)
           VALUES ($1,$2,$3,$4)
           ON CONFLICT (company_id, scope, idempotency_key) DO NOTHING
           RETURNING 1"#,
    )
    .bind(company_id)
    .bind(scope)
    .bind(idempotency_key)
    .bind(request_hash)
    .fetch_optional(pool)
    .await?;
    Ok(inserted.is_some())
}

pub fn start_automation_worker(pool: PgPool) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = automation_tick(&pool).await {
                tracing::warn!(target: "hsm_company_automation", "automation tick failed: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        }
    });
}

async fn automation_tick(pool: &PgPool) -> Result<(), sqlx::Error> {
    enqueue_sla_escalation_jobs(pool).await?;
    process_due_automation_jobs(pool).await?;
    run_auto_revert_checks(pool).await?;
    Ok(())
}

async fn enqueue_sla_escalation_jobs(pool: &PgPool) -> Result<(), sqlx::Error> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid)>(
        r#"SELECT id, company_id
           FROM tasks
           WHERE escalate_after IS NOT NULL
             AND escalate_after <= NOW()
             AND state NOT IN ('done','closed','cancelled')
           ORDER BY escalate_after ASC
           LIMIT 50"#,
    )
    .fetch_all(pool)
    .await?;
    for (task_id, company_id) in rows {
        let idem = format!("sla-escalation:{task_id}");
        let _ = sqlx::query(
            r#"INSERT INTO automation_jobs
               (company_id, kind, status, payload, idempotency_key, max_attempts, next_run_at)
               VALUES ($1,'sla_escalation','pending',$2,$3,5,NOW())
               ON CONFLICT DO NOTHING"#,
        )
        .bind(company_id)
        .bind(SqlxJson(json!({ "task_id": task_id })))
        .bind(idem)
        .execute(pool)
        .await;
    }
    Ok(())
}

async fn process_due_automation_jobs(pool: &PgPool) -> Result<(), sqlx::Error> {
    let jobs = sqlx::query_as::<_, (Uuid, Uuid, String, SqlxJson<Value>, i32, i32)>(
        r#"SELECT id, company_id, kind, payload, attempts, max_attempts
           FROM automation_jobs
           WHERE status = 'pending' AND next_run_at <= NOW()
           ORDER BY next_run_at ASC
           LIMIT 20"#,
    )
    .fetch_all(pool)
    .await?;

    for (job_id, company_id, kind, payload, attempts, max_attempts) in jobs {
        let _ = sqlx::query(
            "UPDATE automation_jobs SET status = 'running', updated_at = NOW() WHERE id = $1",
        )
        .bind(job_id)
        .execute(pool)
        .await;

        let run_res = match kind.as_str() {
            "sla_escalation" => run_sla_escalation_job(pool, company_id, &payload.0).await,
            _ => Ok(()),
        };

        match run_res {
            Ok(_) => {
                let _ = sqlx::query(
                    "UPDATE automation_jobs SET status = 'done', updated_at = NOW() WHERE id = $1",
                )
                .bind(job_id)
                .execute(pool)
                .await;
            }
            Err(e) => {
                let next_attempt = attempts + 1;
                if next_attempt >= max_attempts {
                    let _ = sqlx::query(
                        r#"INSERT INTO automation_dead_letters (company_id, job_id, kind, payload, error, attempts)
                           VALUES ($1,$2,$3,$4,$5,$6)"#,
                    )
                    .bind(company_id)
                    .bind(job_id)
                    .bind(&kind)
                    .bind(payload.clone())
                    .bind(e.clone())
                    .bind(next_attempt)
                    .execute(pool)
                    .await;
                    let _ = sqlx::query(
                        "UPDATE automation_jobs SET status = 'dead_letter', attempts = $2, last_error = $3, updated_at = NOW() WHERE id = $1",
                    )
                    .bind(job_id)
                    .bind(next_attempt)
                    .bind(e)
                    .execute(pool)
                    .await;
                } else {
                    let backoff_sec = (2_i64).pow(next_attempt as u32).min(300);
                    let _ = sqlx::query(
                        r#"UPDATE automation_jobs
                           SET status = 'pending',
                               attempts = $2,
                               last_error = $3,
                               next_run_at = NOW() + ($4::bigint * INTERVAL '1 second'),
                               updated_at = NOW()
                           WHERE id = $1"#,
                    )
                    .bind(job_id)
                    .bind(next_attempt)
                    .bind(e)
                    .bind(backoff_sec)
                    .execute(pool)
                    .await;
                }
            }
        }
    }
    Ok(())
}

async fn run_sla_escalation_job(pool: &PgPool, company_id: Uuid, payload: &Value) -> Result<(), String> {
    let Some(task_id) = payload
        .get("task_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
    else {
        return Err("payload.task_id missing".to_string());
    };
    sqlx::query(
        r#"UPDATE tasks
           SET state = CASE WHEN state = 'open' THEN 'waiting_admin' ELSE state END,
               status_reason = COALESCE(status_reason, 'auto:sla_escalation'),
               updated_at = NOW()
           WHERE id = $1"#,
    )
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1,'automation_worker','sla_escalation','task',$2,$3,'warn')"#,
    )
    .bind(company_id)
    .bind(task_id.to_string())
    .bind(SqlxJson(json!({"source":"scheduler"})))
    .execute(pool)
    .await;
    Ok(())
}

async fn run_auto_revert_checks(pool: &PgPool) -> Result<(), sqlx::Error> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid, Option<f64>, SqlxJson<Value>)>(
        r#"SELECT id, company_id, max_regression_pct::float8 as max_regression_pct, metrics_meta
           FROM improvement_runs
           WHERE status = 'promoted'
           ORDER BY updated_at DESC
           LIMIT 50"#,
    )
    .fetch_all(pool)
    .await?;
    for (run_id, company_id, max_regression, metrics) in rows {
        let threshold = max_regression.unwrap_or(5.0);
        let current = metrics
            .0
            .get("current_regression_pct")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if current > threshold {
            let reason = format!("auto-revert: regression {:.2}% > {:.2}%", current, threshold);
            let _ = sqlx::query(
                r#"UPDATE improvement_runs
                   SET status = 'reverted', decision_reason = $2, decided_by = 'automation_worker', decided_at = NOW(), updated_at = NOW()
                   WHERE id = $1"#,
            )
            .bind(run_id)
            .bind(&reason)
            .execute(pool)
            .await;
            let _ = sqlx::query(
                r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, decision, severity)
                   VALUES ($1,'automation_worker','improvement_auto_revert','improvement_run',$2,$3,'reverted','warn')"#,
            )
            .bind(company_id)
            .bind(run_id.to_string())
            .bind(SqlxJson(json!({ "reason": reason, "current_regression_pct": current, "threshold_pct": threshold })))
            .execute(pool)
            .await;
        }
    }
    Ok(())
}

/// Same as [`compute_goal_ancestry`] but on an open transaction (import path).
pub(super) async fn compute_goal_ancestry_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    goal_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut chain: Vec<Uuid> = Vec::new();
    let mut current = Some(goal_id);
    let mut guard = 0u8;
    while let Some(gid) = current {
        guard += 1;
        if guard > 32 {
            break;
        }
        let row: Option<(Option<Uuid>,)> = sqlx::query_as(
            "SELECT parent_goal_id FROM goals WHERE id = $1 AND company_id = $2",
        )
        .bind(gid)
        .bind(company_id)
        .fetch_optional(&mut **tx)
        .await?;
        let Some((parent,)) = row else {
            break;
        };
        chain.push(gid);
        current = parent;
    }
    chain.reverse();
    Ok(chain)
}

fn no_db() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "Company OS database not configured",
            "hint": "Set HSM_COMPANY_OS_DATABASE_URL to a PostgreSQL URL and restart hsm_console."
        })),
    )
}

async fn company_health(State(st): State<ConsoleState>) -> Json<Value> {
    let Some(ref pool) = st.company_db else {
        return Json(json!({
            "service": "company-os",
            "postgres_configured": false,
            "postgres_ok": false,
        }));
    };
    let ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .is_ok();
    Json(json!({
        "service": "company-os",
        "postgres_configured": true,
        "postgres_ok": ok,
    }))
}

async fn list_companies(
    State(st): State<ConsoleState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, CompanyRow>(
        r#"SELECT id, slug, display_name, hsmii_home, issue_key_prefix,
                  context_markdown, created_at::text
           FROM companies ORDER BY display_name"#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok(Json(json!({ "companies": rows })))
}

async fn get_company(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let row = sqlx::query_as::<_, CompanyRow>(
        r#"SELECT id, slug, display_name, hsmii_home, issue_key_prefix,
                  context_markdown, created_at::text
           FROM companies WHERE id = $1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(c) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))));
    };
    Ok(Json(json!({ "company": c })))
}

#[derive(Deserialize, Default)]
struct PatchCompanyBody {
    #[serde(default)]
    display_name: Option<String>,
    /// `None` = omit field; `Some(None)` = clear; `Some(Some(s))` = set.
    #[serde(default)]
    hsmii_home: Option<Option<String>>,
    #[serde(default)]
    context_markdown: Option<Option<String>>,
}

async fn patch_company(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<PatchCompanyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let current = sqlx::query_as::<_, CompanyRow>(
        r#"SELECT id, slug, display_name, hsmii_home, issue_key_prefix,
                  context_markdown, created_at::text
           FROM companies WHERE id = $1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(mut c) = current else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))));
    };

    if let Some(d) = &body.display_name {
        let d = d.trim();
        if d.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "display_name cannot be empty" })),
            ));
        }
        c.display_name = d.to_string();
    }
    if let Some(h) = body.hsmii_home {
        c.hsmii_home = h.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    }
    if let Some(cm) = body.context_markdown {
        if let Some(ref text) = cm {
            if text.len() > MAX_COMPANY_CONTEXT_MARKDOWN_BYTES {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!(
                            "context_markdown exceeds max {} bytes",
                            MAX_COMPANY_CONTEXT_MARKDOWN_BYTES
                        )
                    })),
                ));
            }
        }
        c.context_markdown = cm.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    }

    let updated = sqlx::query_as::<_, CompanyRow>(
        r#"UPDATE companies SET
               display_name = $2,
               hsmii_home = $3,
               context_markdown = $4,
               updated_at = now()
           WHERE id = $1
           RETURNING id, slug, display_name, hsmii_home, issue_key_prefix,
                     context_markdown, created_at::text"#,
    )
    .bind(company_id)
    .bind(&c.display_name)
    .bind(&c.hsmii_home)
    .bind(&c.context_markdown)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok(Json(json!({ "company": updated })))
}

#[derive(Deserialize)]
struct DeleteCompanyQuery {
    /// Must equal `companies.slug` for this id (typo guard).
    confirm_slug: String,
}

async fn delete_company(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<DeleteCompanyQuery>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let confirm = q.confirm_slug.trim();
    if confirm.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "query parameter confirm_slug required; must match the workspace slug exactly",
                "example": format!("/api/company/companies/{company_id}?confirm_slug=my-workspace-slug"),
            })),
        ));
    }
    let row: Option<(String,)> = sqlx::query_as("SELECT slug FROM companies WHERE id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    let Some((slug,)) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))));
    };
    if slug != confirm {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "confirm_slug does not match this workspace",
                "slug": slug,
            })),
        ));
    }
    let res = sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(company_id)
        .execute(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    if res.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))));
    }
    Ok((
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "deleted_id": company_id,
            "deleted_slug": slug,
        })),
    ))
}

async fn import_paperclip_home(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    paperclip_import::import_paperclip_pack(pool, company_id)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
        })
}

/// Static catalog of Company OS HTTP routes. `{company_id}` / `{task_id}` are placeholders.
fn company_os_api_catalog_endpoints() -> Value {
    json!([
        { "scope": "company", "methods": ["GET", "PATCH", "DELETE"], "path": "/api/company/companies/{company_id}?confirm_slug=", "summary": "Company record; PATCH updates context_markdown, display_name, hsmii_home; DELETE removes workspace and cascades (confirm_slug must match slug)" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/api-catalog", "summary": "Discovery: this list + company" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/import-paperclip-home", "summary": "Import Paperclip pack agents from hsmii_home/agents and skills index into context" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/export", "summary": "Export bundle JSON" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/spend/summary", "summary": "Spend rollup" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/governance/events", "summary": "List / append governance events" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/policies/rules", "summary": "Policy rules" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/policies/evaluate", "summary": "Evaluate policy for an action" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/goals", "summary": "Goals tree" },
        { "scope": "company", "methods": ["PATCH"], "path": "/api/company/companies/{company_id}/goals/{goal_id}", "summary": "Update goal" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/tasks", "summary": "List / create tasks" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/tasks/queue", "summary": "Filtered task queue (tabs)" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/spawn-rules", "summary": "Spawn rules" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/tasks/{task_id}/spawn-subagents", "summary": "Spawn subtasks from rules" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/tasks/{task_id}/handoffs", "summary": "Task handoffs" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/improvement-runs", "summary": "Improvement runs" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/go-live-checklist", "summary": "Go-live checklist" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/go-live-checklist/seed", "summary": "Seed checklist items" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/agents", "summary": "Workforce agents registry" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/org", "summary": "Org chart" },
        { "scope": "company", "methods": ["PATCH", "DELETE"], "path": "/api/company/companies/{company_id}/agents/{agent_id}", "summary": "Update or delete agent row (delete clears direct reports’ manager link)" },
        { "scope": "task", "methods": ["PATCH"], "path": "/api/company/tasks/{task_id}/sla", "summary": "Task SLA fields" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/decision", "summary": "Policy decision on task" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/checkout", "summary": "Lease task to an agent ref" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/release", "summary": "Release checkout" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/run-telemetry", "summary": "Append run snapshot / log tail" },
        { "scope": "task", "methods": ["GET"], "path": "/api/company/tasks/{task_id}/llm-context", "summary": "LLM: company context_markdown + workforce agent profile" },
        { "scope": "global", "methods": ["GET"], "path": "/api/company/health", "summary": "Postgres connectivity" },
        { "scope": "global", "methods": ["POST"], "path": "/api/company/import", "summary": "Import company bundle" },
        { "scope": "global", "methods": ["POST"], "path": "/api/company/task-handoffs/{handoff_id}/review", "summary": "Review handoff" },
        { "scope": "global", "methods": ["POST"], "path": "/api/company/improvement-runs/{run_id}/decision", "summary": "Decision on improvement run" },
        { "scope": "global", "methods": ["POST"], "path": "/api/company/go-live-checklist/{item_id}/complete", "summary": "Complete checklist item" }
    ])
}

async fn company_api_catalog(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let row = sqlx::query_as::<_, CompanyRow>(
        r#"SELECT id, slug, display_name, hsmii_home, issue_key_prefix,
                  context_markdown, created_at::text
           FROM companies WHERE id = $1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(c) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))));
    };
    Ok(Json(json!({
        "api_version": "1",
        "kind": "company-os.company-api",
        "description": "This workspace is a namespaced HTTP API. All company-scoped routes live under /api/company/companies/{company_id}/…; task-scoped routes use /api/company/tasks/{task_id}/… (tasks carry their own company_id).",
        "company": c,
        "base_path": format!("/api/company/companies/{company_id}"),
        "interface_model": company_os_interface_model(),
        "endpoints": company_os_api_catalog_endpoints(),
    })))
}

/// Product thesis for agents and integrators: company-as-algorithm, API-first, funnel as control surface.
fn company_os_interface_model() -> Value {
    json!({
        "version": 2,
        "thesis": [
            "The primary interface to a company is an API consumed by agents; people connect to the company through those agents.",
            "The funnel is an algorithm: who controls the funnel shapes operational reality; algorithmic mediation means consensus is not automatic—govern who manages which decision points.",
            "The company is a graph of algorithms (policies, tasks, goals, spawn rules, handoffs, spend, SOPs); leadership transparency is documentation plus visualization of workflows.",
            "Target: dozens of explicit decision points, every tool and branch visible—20+ algorithmic decisions across the graph, not a black box.",
            "Humans write the rules; AIs execute within them. Prefer versioned control planes over opaque retraining (Meta-Harness direction: policy/version history as steering)."
        ],
        "operating_model": {
            "ai_managed_sops": "Standard operating procedures are machine-checkable; a human layer sets intent and exceptions, a SOP layer encodes procedure, an AI execution layer runs steps within guardrails.",
            "three_layers": {
                "human_layer": "Sets goals, approves exceptions, owns policy change and accountability.",
                "sop_layer": "Structured procedures, checklists, and eligibility encoded for automation.",
                "ai_execution_layer": "Agents and jobs that call tools under permissions, triggers, and guardrails."
            },
            "seven_department_functions": [
                "procurement",
                "sales",
                "marketing",
                "engineering",
                "product_management",
                "security",
                "security_operations_center"
            ],
            "note": "Departments are first-class algorithm bundles (policies + tasks + tools + metrics), not only org-chart labels."
        },
        "control_plane_elements": {
            "trigger_conditions": "What starts a run, escalates, or spawns work (rules, webhooks, schedules—partially in spawn rules and jobs today).",
            "decision_tree": "Branching outcomes from policy evaluation, task decision_mode, and governance choices.",
            "tool_permissions": "Which identities may invoke which capabilities; maps to checkout, policy rules, and future scoped API keys.",
            "guardrails": "Blocked / admin_required modes, budgets, SLA escalation, improvement-run gates.",
            "success_metrics": "Spend rollups, task throughput, improvement metrics—extend per department bundle.",
            "version_history": "Immutable governance events + export bundles; explicit versioning of rules/contracts preferred over silent model retraining."
        },
        "meta_harness": {
            "principle": "Control replaces retraining: steer behavior with documented rule and contract versions rather than only updating weights.",
            "direction": "Tie Meta-Harness-style loops to versioned artifacts (policies, SOPs, handoff contracts) and auditable decisions."
        },
        "decision_surface_today": {
            "policy_rules": "risk bands and decision_mode (auto / admin_required / blocked) per action type",
            "task_decisions": "POST /tasks/{id}/decision updates decision_mode with actor + reason",
            "task_states_and_queue": "queue views (overdue, waiting_admin, pending_approvals, blocked, …)",
            "checkout_and_sla": "checkout lease, SLA patch, escalation jobs",
            "governance_events": "append-only audit stream for human and system actions",
            "improvement_runs_and_handoffs": "contracted review loops",
            "spend_summary": "cost visibility; budget fields on workforce agents",
            "company_context_markdown": "PATCH /companies/{id} with context_markdown (e.g. declaration excerpts, fee tables); GET /tasks/{task_id}/llm-context prepends it before the matched workforce agent profile"
        },
        "transparency_today": [
            "This api-catalog and endpoint list",
            "GET export bundle and governance events",
            "Task list + goal tree + (UI) console charts",
            "Run telemetry snapshots on tasks"
        ],
        "north_star": {
            "department_algorithm_bundles": "Each of the seven functions has a defined policy+task+tool+metric surface in the API.",
            "decision_point_inventory": "Enumerate 20+ decision types with owners and endpoints.",
            "workflow_graph": "Live graph visualization of the company algorithm",
            "sop_as_code": "SOPs versioned alongside policies; AI execution only inside declared permissions"
        }
    })
}

#[derive(sqlx::FromRow, Serialize, Clone)]
struct CompanyRow {
    id: Uuid,
    slug: String,
    display_name: String,
    hsmii_home: Option<String>,
    issue_key_prefix: String,
    context_markdown: Option<String>,
    created_at: String,
}

#[derive(Deserialize)]
struct CreateCompanyBody {
    slug: String,
    display_name: String,
    #[serde(default)]
    hsmii_home: Option<String>,
}

async fn create_company(
    State(st): State<ConsoleState>,
    Json(body): Json<CreateCompanyBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let slug = body.slug.trim().to_string();
    let display_name = body.display_name.trim().to_string();
    if slug.is_empty() || display_name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "slug and display_name required" })),
        ));
    }
    let prefix = derive_issue_key_prefix(&slug);
    let row: Result<CompanyRow, sqlx::Error> = sqlx::query_as::<_, CompanyRow>(
        r#"INSERT INTO companies (slug, display_name, hsmii_home, issue_key_prefix)
           VALUES ($1, $2, $3, $4)
           RETURNING id, slug, display_name, hsmii_home, issue_key_prefix,
                     context_markdown, created_at::text"#,
    )
    .bind(&slug)
    .bind(&display_name)
    .bind(&body.hsmii_home)
    .bind(&prefix)
    .fetch_one(pool)
    .await;
    match row {
        Ok(c) => Ok((StatusCode::CREATED, Json(json!({ "company": c })))),
        Err(sqlx::Error::Database(d)) if d.code().as_deref() == Some("23505") => Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "slug already exists" })),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}

#[derive(sqlx::FromRow, Serialize)]
struct GoalRow {
    id: Uuid,
    company_id: Uuid,
    parent_goal_id: Option<Uuid>,
    title: String,
    description: Option<String>,
    status: String,
    created_at: String,
}

async fn list_goals(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, GoalRow>(
        r#"SELECT id, company_id, parent_goal_id, title, description, status, created_at::text
           FROM goals WHERE company_id = $1 ORDER BY sort_order, created_at"#,
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
    Ok(Json(json!({ "goals": rows })))
}

#[derive(Deserialize)]
struct CreateGoalBody {
    title: String,
    #[serde(default)]
    parent_goal_id: Option<Uuid>,
    #[serde(default)]
    description: Option<String>,
}

async fn create_goal(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateGoalBody>,
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
    if let Some(pid) = body.parent_goal_id {
        let ok = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM goals WHERE id = $1 AND company_id = $2)",
        )
        .bind(pid)
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
                Json(json!({ "error": "parent_goal_id not in company" })),
            ));
        }
    }
    let row = sqlx::query_as::<_, GoalRow>(
        r#"INSERT INTO goals (company_id, parent_goal_id, title, description)
           VALUES ($1, $2, $3, $4)
           RETURNING id, company_id, parent_goal_id, title, description, status, created_at::text"#,
    )
    .bind(company_id)
    .bind(&body.parent_goal_id)
    .bind(&title)
    .bind(&body.description)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok((StatusCode::CREATED, Json(json!({ "goal": row }))))
}

fn company_advisory_lock_key(company_id: Uuid) -> i64 {
    let b = company_id.as_bytes();
    i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

pub(super) fn derive_issue_key_prefix(slug: &str) -> String {
    let s: String = slug
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(4)
        .collect::<String>()
        .to_ascii_uppercase();
    if s.len() >= 2 {
        s
    } else {
        "TSK".to_string()
    }
}

pub(super) async fn next_task_display_number_tx(
    tx: &mut Transaction<'_, Postgres>,
    company_id: Uuid,
) -> Result<i32, sqlx::Error> {
    let key = company_advisory_lock_key(company_id);
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(key)
        .execute(&mut **tx)
        .await?;
    sqlx::query_scalar::<_, i32>(
        "SELECT COALESCE(MAX(display_number), 0) + 1 FROM tasks WHERE company_id = $1",
    )
    .bind(company_id)
    .fetch_one(&mut **tx)
    .await
}

#[derive(sqlx::FromRow, Serialize)]
struct TaskRow {
    id: Uuid,
    company_id: Uuid,
    primary_goal_id: Option<Uuid>,
    goal_ancestry: Value,
    title: String,
    specification: Option<String>,
    state: String,
    owner_persona: Option<String>,
    parent_task_id: Option<Uuid>,
    spawned_by_rule_id: Option<Uuid>,
    checked_out_by: Option<String>,
    checked_out_until: Option<chrono::DateTime<chrono::Utc>>,
    priority: i32,
    display_number: i32,
    created_at: String,
}

#[derive(sqlx::FromRow)]
struct TaskRunSnapRow {
    task_id: Uuid,
    run_status: String,
    tool_calls: i32,
    log_tail: String,
    finished_at: Option<chrono::DateTime<chrono::Utc>>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

async fn upsert_run_snapshot_running(pool: &PgPool, company_id: Uuid, task_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO task_run_snapshots (task_id, company_id, run_status, tool_calls, log_tail, finished_at)
           VALUES ($1, $2, 'running', 0, '', NULL)
           ON CONFLICT (task_id) DO UPDATE SET
             run_status = 'running',
             tool_calls = 0,
             log_tail = '',
             finished_at = NULL,
             updated_at = NOW()"#,
    )
    .bind(task_id)
    .bind(company_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// On checkout release: mark a still-`running` snapshot as success without clobbering `error` / `idle`.
async fn finalize_run_snapshot_on_release(pool: &PgPool, task_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE task_run_snapshots
           SET run_status = CASE WHEN run_status = 'running' THEN 'success' ELSE run_status END,
               finished_at = CASE
                   WHEN run_status = 'running' AND finished_at IS NULL THEN NOW()
                   ELSE finished_at
               END,
               updated_at = NOW()
           WHERE task_id = $1"#,
    )
    .bind(task_id)
    .execute(pool)
    .await?;
    Ok(())
}

const RUN_LOG_TAIL_MAX_CHARS: usize = 6000;

fn append_truncated_log_tail(existing: &str, chunk: &str) -> String {
    let s = format!("{existing}{chunk}");
    let n = s.chars().count();
    if n <= RUN_LOG_TAIL_MAX_CHARS {
        return s;
    }
    let drop = n - RUN_LOG_TAIL_MAX_CHARS;
    s.chars().skip(drop).collect()
}

async fn ensure_task_run_snapshot_row(pool: &PgPool, company_id: Uuid, task_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO task_run_snapshots (task_id, company_id) VALUES ($1, $2)
           ON CONFLICT (task_id) DO NOTHING"#,
    )
    .bind(task_id)
    .bind(company_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Deserialize)]
struct PostRunTelemetryBody {
    #[serde(default)]
    run_status: Option<String>,
    #[serde(default)]
    tool_calls: Option<i32>,
    #[serde(default)]
    log_append: Option<String>,
    #[serde(default)]
    clear_log: Option<bool>,
}

async fn post_task_run_telemetry(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<PostRunTelemetryBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let cid: Option<Uuid> = sqlx::query_scalar("SELECT company_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    let Some(company_id) = cid else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "task not found" }))));
    };

    if let Some(ref st) = body.run_status {
        if !matches!(st.as_str(), "idle" | "running" | "success" | "error") {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "run_status must be idle|running|success|error" })),
            ));
        }
    }

    ensure_task_run_snapshot_row(pool, company_id, task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;

    let mut snap = sqlx::query_as::<_, TaskRunSnapRow>(
        r#"SELECT task_id, run_status, tool_calls, log_tail, finished_at, updated_at
           FROM task_run_snapshots WHERE task_id = $1"#,
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    if body.clear_log == Some(true) {
        snap.log_tail.clear();
    }
    if let Some(ref app) = body.log_append {
        if !app.is_empty() {
            snap.log_tail = append_truncated_log_tail(&snap.log_tail, app);
        }
    }
    if let Some(tc) = body.tool_calls {
        if tc >= 0 {
            snap.tool_calls = tc;
        }
    }

    let status = body
        .run_status
        .clone()
        .unwrap_or_else(|| snap.run_status.clone());
    let finished_at = if body.run_status.is_some() {
        if status == "success" || status == "error" {
            Some(chrono::Utc::now())
        } else {
            None
        }
    } else {
        snap.finished_at
    };

    let updated = sqlx::query_as::<_, TaskRunSnapRow>(
        r#"UPDATE task_run_snapshots SET
             run_status = $2,
             tool_calls = $3,
             log_tail = $4,
             finished_at = $5,
             updated_at = NOW()
           WHERE task_id = $1
           RETURNING task_id, run_status, tool_calls, log_tail, finished_at, updated_at"#,
    )
    .bind(task_id)
    .bind(&status)
    .bind(snap.tool_calls)
    .bind(&snap.log_tail)
    .bind(finished_at)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({
        "run": {
            "status": updated.run_status,
            "tool_calls": updated.tool_calls,
            "log_tail": updated.log_tail,
            "finished_at": updated.finished_at.map(|d| d.to_rfc3339()),
            "updated_at": updated.updated_at.to_rfc3339(),
        }
    })))
}

async fn list_tasks(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, TaskRow>(
        r#"SELECT id, company_id, primary_goal_id, goal_ancestry, title, specification, state,
                  owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, created_at::text
           FROM tasks WHERE company_id = $1 ORDER BY priority DESC, created_at"#,
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
    let snaps = sqlx::query_as::<_, TaskRunSnapRow>(
        r#"SELECT task_id, run_status, tool_calls, log_tail, finished_at, updated_at
           FROM task_run_snapshots WHERE company_id = $1"#,
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
    let smap: std::collections::HashMap<Uuid, TaskRunSnapRow> = snaps.into_iter().map(|s| (s.task_id, s)).collect();
    let tasks: Vec<Value> = rows
        .into_iter()
        .filter_map(|t| {
            let mut v = serde_json::to_value(&t).ok()?;
            if let Value::Object(ref mut obj) = v {
                let run_val = if let Some(s) = smap.get(&t.id) {
                    json!({
                        "status": s.run_status,
                        "tool_calls": s.tool_calls,
                        "log_tail": s.log_tail,
                        "finished_at": s.finished_at.map(|d| d.to_rfc3339()),
                        "updated_at": s.updated_at.to_rfc3339(),
                    })
                } else {
                    Value::Null
                };
                obj.insert("run".to_string(), run_val);
            }
            Some(v)
        })
        .collect();
    Ok(Json(json!({ "tasks": tasks })))
}

#[derive(Deserialize)]
struct CreateTaskBody {
    title: String,
    #[serde(default)]
    specification: Option<String>,
    #[serde(default)]
    primary_goal_id: Option<Uuid>,
    #[serde(default)]
    owner_persona: Option<String>,
    #[serde(default)]
    parent_task_id: Option<Uuid>,
    #[serde(default)]
    spawned_by_rule_id: Option<Uuid>,
}

async fn compute_goal_ancestry(
    pool: &PgPool,
    company_id: Uuid,
    goal_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut chain: Vec<Uuid> = Vec::new();
    let mut current = Some(goal_id);
    let mut guard = 0u8;
    while let Some(gid) = current {
        guard += 1;
        if guard > 32 {
            break;
        }
        let row: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT parent_goal_id FROM goals WHERE id = $1 AND company_id = $2")
                .bind(gid)
                .bind(company_id)
                .fetch_optional(pool)
                .await?;
        let Some((parent,)) = row else {
            break;
        };
        chain.push(gid);
        current = parent;
    }
    chain.reverse();
    Ok(chain)
}

/// Walk from `start` up via `parent_goal_id`; true if `needle` appears on that path (strictly above `start`).
/// Used so `goal_id.parent = start` is rejected when it would close a loop.
async fn parent_chain_contains_goal(
    pool: &PgPool,
    company_id: Uuid,
    mut start: Uuid,
    needle: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut guard = 0u8;
    loop {
        let row: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT parent_goal_id FROM goals WHERE id = $1 AND company_id = $2")
                .bind(start)
                .bind(company_id)
                .fetch_optional(pool)
                .await?;
        let Some((parent_opt,)) = row else {
            break;
        };
        let Some(parent) = parent_opt else {
            break;
        };
        if parent == needle {
            return Ok(true);
        }
        start = parent;
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    Ok(false)
}

async fn create_task(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateTaskBody>,
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
    let mut ancestry_json = json!([]);
    if let Some(gid) = body.primary_goal_id {
        let ok = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM goals WHERE id = $1 AND company_id = $2)",
        )
        .bind(gid)
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
                Json(json!({ "error": "primary_goal_id not in company" })),
            ));
        }
        let chain = compute_goal_ancestry(pool, company_id, gid)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
        ancestry_json = serde_json::to_value(&chain).unwrap_or(json!([]));
    }

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    let display_n = next_task_display_number_tx(&mut tx, company_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    let row = sqlx::query_as::<_, TaskRow>(
        r#"INSERT INTO tasks (company_id, primary_goal_id, goal_ancestry, title, specification, owner_persona, parent_task_id, spawned_by_rule_id, display_number)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
           RETURNING id, company_id, primary_goal_id, goal_ancestry, title, specification, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, created_at::text"#,
    )
    .bind(company_id)
    .bind(&body.primary_goal_id)
    .bind(SqlxJson(ancestry_json))
    .bind(&title)
    .bind(&body.specification)
    .bind(&body.owner_persona)
    .bind(&body.parent_task_id)
    .bind(&body.spawned_by_rule_id)
    .bind(display_n)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok((StatusCode::CREATED, Json(json!({ "task": row }))))
}

#[derive(Deserialize, Default)]
struct PatchTaskSlaBody {
    #[serde(default)]
    due_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    sla_policy: Option<String>,
    #[serde(default)]
    escalate_after: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    status_reason: Option<String>,
    #[serde(default)]
    priority: Option<i32>,
}

#[derive(sqlx::FromRow, Serialize)]
struct TaskSlaRow {
    id: Uuid,
    company_id: Uuid,
    due_at: Option<chrono::DateTime<chrono::Utc>>,
    sla_policy: Option<String>,
    escalate_after: Option<chrono::DateTime<chrono::Utc>>,
    status_reason: Option<String>,
    priority: i32,
    updated_at: String,
}

async fn patch_task_sla(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<PatchTaskSlaBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let row = sqlx::query_as::<_, TaskSlaRow>(
        r#"UPDATE tasks SET
            due_at = COALESCE($2, due_at),
            sla_policy = COALESCE($3, sla_policy),
            escalate_after = COALESCE($4, escalate_after),
            status_reason = COALESCE($5, status_reason),
            priority = COALESCE($6, priority),
            updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, due_at, sla_policy, escalate_after, status_reason, priority, updated_at::text"#,
    )
    .bind(task_id)
    .bind(body.due_at)
    .bind(body.sla_policy.as_ref())
    .bind(body.escalate_after)
    .bind(body.status_reason.as_ref())
    .bind(body.priority)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let Some(task) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "task not found" }))));
    };
    Ok(Json(json!({ "task": task })))
}

#[derive(Deserialize, Default)]
struct TaskQueueQuery {
    #[serde(default)]
    view: Option<String>,
}

#[derive(sqlx::FromRow, Serialize)]
struct QueueTaskRow {
    id: Uuid,
    company_id: Uuid,
    title: String,
    state: String,
    priority: i32,
    due_at: Option<chrono::DateTime<chrono::Utc>>,
    escalate_after: Option<chrono::DateTime<chrono::Utc>>,
    checked_out_by: Option<String>,
    decision_mode: String,
    created_at: String,
}

async fn list_task_queue(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<TaskQueueQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let view = q
        .view
        .as_deref()
        .unwrap_or("all")
        .trim()
        .to_ascii_lowercase();

    let sql = match view.as_str() {
        "overdue" => {
            r#"SELECT id, company_id, title, state, priority, due_at, escalate_after, checked_out_by,
                      CASE
                        WHEN state = 'blocked' THEN 'blocked'
                        WHEN state = 'waiting_admin' THEN 'admin_required'
                        ELSE 'auto'
                      END AS decision_mode,
                      created_at::text
               FROM tasks
               WHERE company_id = $1
                 AND due_at IS NOT NULL
                 AND due_at < NOW()
                 AND state NOT IN ('done','closed','cancelled')
               ORDER BY priority DESC, due_at ASC, created_at"#
        }
        "atrisk" => {
            r#"SELECT id, company_id, title, state, priority, due_at, escalate_after, checked_out_by,
                      CASE
                        WHEN state = 'blocked' THEN 'blocked'
                        WHEN state = 'waiting_admin' THEN 'admin_required'
                        ELSE 'auto'
                      END AS decision_mode,
                      created_at::text
               FROM tasks
               WHERE company_id = $1
                 AND escalate_after IS NOT NULL
                 AND escalate_after <= NOW() + INTERVAL '2 hours'
                 AND state NOT IN ('done','closed','cancelled')
               ORDER BY escalate_after ASC, priority DESC, created_at"#
        }
        "waiting_admin" | "pending_approvals" => {
            r#"SELECT id, company_id, title, state, priority, due_at, escalate_after, checked_out_by,
                      'admin_required' AS decision_mode,
                      created_at::text
               FROM tasks
               WHERE company_id = $1
                 AND state = 'waiting_admin'
               ORDER BY priority DESC, created_at"#
        }
        "blocked" => {
            r#"SELECT id, company_id, title, state, priority, due_at, escalate_after, checked_out_by,
                      'blocked' AS decision_mode,
                      created_at::text
               FROM tasks
               WHERE company_id = $1
                 AND state = 'blocked'
               ORDER BY priority DESC, created_at"#
        }
        _ => {
            r#"SELECT id, company_id, title, state, priority, due_at, escalate_after, checked_out_by,
                      CASE
                        WHEN state = 'blocked' THEN 'blocked'
                        WHEN state = 'waiting_admin' THEN 'admin_required'
                        ELSE 'auto'
                      END AS decision_mode,
                      created_at::text
               FROM tasks
               WHERE company_id = $1
               ORDER BY priority DESC, created_at"#
        }
    };

    let rows = sqlx::query_as::<_, QueueTaskRow>(sql)
        .bind(company_id)
        .fetch_all(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;

    Ok(Json(json!({
        "view": view,
        "tasks": rows,
    })))
}

#[derive(Deserialize)]
struct PostTaskDecisionBody {
    decision_mode: String,
    #[serde(default)]
    actor: String,
    #[serde(default)]
    reason: String,
}

async fn post_task_decision(
    State(st): State<ConsoleState>,
    headers: HeaderMap,
    Path(task_id): Path<Uuid>,
    Json(body): Json<PostTaskDecisionBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let Some(decision) = normalize_decision(&body.decision_mode) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "decision_mode must be auto|admin_required|blocked" })),
        ));
    };
    let next_state = match decision {
        "blocked" => "blocked",
        "admin_required" => "waiting_admin",
        _ => "in_progress",
    };
    let reason = body.reason.trim();
    let status_reason = if reason.is_empty() {
        format!("policy:{decision}")
    } else {
        format!("policy:{decision}:{reason}")
    };

    let task = sqlx::query_as::<_, TaskRow>(
        r#"UPDATE tasks SET
            state = $2,
            status_reason = $3,
            updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, primary_goal_id, goal_ancestry, title, specification, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, created_at::text"#,
    )
    .bind(task_id)
    .bind(next_state)
    .bind(status_reason.clone())
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(task) = task else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "task not found" }))));
    };
    if let Some(k) = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let payload = json!({
            "task_id": task_id,
            "decision_mode": decision,
            "reason": reason,
        });
        let ok = register_idempotency(pool, task.company_id, "task_decision", k, &payload)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
        if !ok {
            return Err((StatusCode::CONFLICT, Json(json!({"error":"duplicate idempotency key"}))));
        }
    }

    let actor = if body.actor.trim().is_empty() {
        "admin"
    } else {
        body.actor.trim()
    };
    let _ = sqlx::query(
        r#"INSERT INTO governance_events
           (company_id, actor, action, subject_type, subject_id, payload, decision, severity)
           VALUES ($1, $2, 'task_policy_decision', 'task', $3, $4, $5, $6)"#,
    )
    .bind(task.company_id)
    .bind(actor)
    .bind(task_id.to_string())
    .bind(SqlxJson(json!({ "decision_mode": decision, "reason": reason })))
    .bind(decision)
    .bind(if decision == "blocked" { "warn" } else { "info" })
    .execute(pool)
    .await;

    Ok(Json(json!({
        "task": task,
        "decision_mode": decision,
    })))
}

#[derive(Deserialize)]
struct CheckoutBody {
    agent_ref: String,
    #[serde(default = "default_ttl")]
    ttl_sec: i64,
}

fn default_ttl() -> i64 {
    3600
}

async fn checkout_task(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<CheckoutBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let agent = body.agent_ref.trim().to_string();
    if agent.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "agent_ref required" })),
        ));
    }
    let row = sqlx::query_as::<_, TaskRow>(
        r#"UPDATE tasks SET
            checked_out_by = $1,
            checked_out_until = NOW() + ($2::bigint * INTERVAL '1 second'),
            state = CASE WHEN state = 'open' THEN 'in_progress' ELSE state END,
            updated_at = NOW()
           WHERE id = $3
             AND (checked_out_by IS NULL OR checked_out_until < NOW())
           RETURNING id, company_id, primary_goal_id, goal_ancestry, title, specification, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, created_at::text"#,
    )
    .bind(&agent)
    .bind(body.ttl_sec)
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(t) = row else {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "checkout failed — task not found or already checked out"
            })),
        ));
    };

    let agent_run_profile = agents::resolve_run_profile_for_task(
        pool,
        t.company_id,
        &agent,
        t.owner_persona.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    tracing::info!(
        target: "hsm_company_agent_inject",
        task_id = %task_id,
        company_id = %t.company_id,
        endpoint = "task_checkout",
        checkout_ref = %agent,
        company_agent_row_found = agent_run_profile.resolved,
        addon_bytes = agent_run_profile.system_context_addon_bytes,
        matched_as = ?agent_run_profile.matched_as,
        agent_id = ?agent_run_profile.agent_id,
        adapter_type = ?agent_run_profile.adapter_type,
        adapter_profile_non_null = agent_run_profile.resolved,
    );

    let gov_payload = json!({
        "checkout_ref": agent,
        "resolved": agent_run_profile.resolved,
        "agent_id": agent_run_profile.agent_id,
        "matched_as": agent_run_profile.matched_as,
        "matched_agent_name": agent_run_profile.matched_agent_name,
        "adapter_type": agent_run_profile.adapter_type,
        "system_context_addon_bytes": agent_run_profile.system_context_addon_bytes,
    });
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, $2, 'task_checkout_agent_profile', 'task', $3, $4, 'info')"#,
    )
    .bind(t.company_id)
    .bind(&agent)
    .bind(task_id.to_string())
    .bind(SqlxJson(gov_payload))
    .execute(pool)
    .await;

    let _ = upsert_run_snapshot_running(pool, t.company_id, task_id).await;

    Ok(Json(json!({
        "task": t,
        "agent_run_profile": agent_run_profile,
    })))
}

#[derive(Deserialize)]
struct ReleaseBody {
    #[serde(default)]
    actor: String,
}

async fn release_task(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<ReleaseBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let row = sqlx::query_as::<_, TaskRow>(
        r#"UPDATE tasks SET
            checked_out_by = NULL,
            checked_out_until = NULL,
            updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, primary_goal_id, goal_ancestry, title, specification, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, created_at::text"#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(t) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "task not found" }))));
    };
    let actor = if body.actor.trim().is_empty() {
        "console"
    } else {
        body.actor.trim()
    };
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload)
           VALUES ($1, $2, 'release_checkout', 'task', $3, $4)"#,
    )
    .bind(t.company_id)
      .bind(actor)
      .bind(task_id.to_string())
      .bind(SqlxJson(json!({ "via": "release_task" })))
      .execute(pool)
      .await;

    let _ = finalize_run_snapshot_on_release(pool, task_id).await;

    Ok(Json(json!({ "task": t })))
}

#[derive(sqlx::FromRow, Serialize)]
struct SpawnRuleRow {
    id: Uuid,
    company_id: Uuid,
    trigger_state: String,
    title_pattern: Option<String>,
    owner_persona: Option<String>,
    max_subtasks: i32,
    subagent_persona: String,
    handoff_contract: SqlxJson<Value>,
    review_contract: SqlxJson<Value>,
    active: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct CreateSpawnRuleBody {
    #[serde(default = "default_trigger_state_open")]
    trigger_state: String,
    #[serde(default)]
    title_pattern: Option<String>,
    #[serde(default)]
    owner_persona: Option<String>,
    #[serde(default = "default_max_subtasks")]
    max_subtasks: i32,
    subagent_persona: String,
    #[serde(default)]
    handoff_contract: Value,
    #[serde(default)]
    review_contract: Value,
}

fn default_trigger_state_open() -> String {
    "open".to_string()
}

fn default_max_subtasks() -> i32 {
    3
}

async fn list_spawn_rules(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, SpawnRuleRow>(
        r#"SELECT id, company_id, trigger_state, title_pattern, owner_persona, max_subtasks, subagent_persona,
                  handoff_contract, review_contract, active, created_at::text, updated_at::text
           FROM task_spawn_rules
           WHERE company_id = $1
           ORDER BY active DESC, created_at"#,
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
    Ok(Json(json!({ "rules": rows })))
}

async fn create_spawn_rule(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateSpawnRuleBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let persona = body.subagent_persona.trim().to_string();
    if persona.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "subagent_persona required" })),
        ));
    }
    if body.max_subtasks < 1 || body.max_subtasks > 20 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "max_subtasks must be 1..20" })),
        ));
    }
    let row = sqlx::query_as::<_, SpawnRuleRow>(
        r#"INSERT INTO task_spawn_rules
           (company_id, trigger_state, title_pattern, owner_persona, max_subtasks, subagent_persona, handoff_contract, review_contract, active)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,true)
           RETURNING id, company_id, trigger_state, title_pattern, owner_persona, max_subtasks, subagent_persona,
                     handoff_contract, review_contract, active, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(body.trigger_state.trim().to_ascii_lowercase())
    .bind(body.title_pattern.as_ref())
    .bind(body.owner_persona.as_ref())
    .bind(body.max_subtasks)
    .bind(persona)
    .bind(SqlxJson(body.handoff_contract))
    .bind(SqlxJson(body.review_contract))
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok((StatusCode::CREATED, Json(json!({ "rule": row }))))
}

#[derive(Deserialize)]
struct SpawnSubagentsBody {
    #[serde(default)]
    actor: String,
}

async fn spawn_subagent_tasks(
    State(st): State<ConsoleState>,
    headers: HeaderMap,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SpawnSubagentsBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let parent = sqlx::query_as::<_, TaskRow>(
        r#"SELECT id, company_id, primary_goal_id, goal_ancestry, title, specification, state, owner_persona,
                  parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, created_at::text
           FROM tasks WHERE id = $1 AND company_id = $2"#,
    )
    .bind(task_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    let Some(parent) = parent else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "task not found" }))));
    };
    let mut kbytes = [0u8; 8];
    kbytes.copy_from_slice(&task_id.as_bytes()[..8]);
    let lock_key = i64::from_be_bytes(kbytes);
    let got_lock: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(lock_key)
        .fetch_one(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    if !got_lock {
        return Err((StatusCode::CONFLICT, Json(json!({"error":"task spawn already in progress"}))));
    }
    if let Some(k) = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let ok = register_idempotency(
            pool,
            company_id,
            "spawn_subagents",
            k,
            &json!({ "task_id": task_id }),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
        if !ok {
            return Err((StatusCode::CONFLICT, Json(json!({"error":"duplicate idempotency key"}))));
        }
    }

    let rules = sqlx::query_as::<_, SpawnRuleRow>(
        r#"SELECT id, company_id, trigger_state, title_pattern, owner_persona, max_subtasks, subagent_persona,
                  handoff_contract, review_contract, active, created_at::text, updated_at::text
           FROM task_spawn_rules WHERE company_id = $1 AND active = true ORDER BY created_at"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;

    let mut created: Vec<TaskRow> = Vec::new();
    for r in rules {
        if r.trigger_state != parent.state.to_ascii_lowercase() {
            continue;
        }
        if let Some(tp) = &r.title_pattern {
            if !parent.title.to_ascii_lowercase().contains(&tp.to_ascii_lowercase()) {
                continue;
            }
        }
        if let Some(owner) = &r.owner_persona {
            if parent.owner_persona.as_deref().unwrap_or("").to_ascii_lowercase()
                != owner.to_ascii_lowercase()
            {
                continue;
            }
        }
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": e.to_string()})),
                )
            })?;
        for i in 0..r.max_subtasks {
            let display_n = next_task_display_number_tx(&mut tx, company_id)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": e.to_string()})),
                    )
                })?;
            let title = format!("{} · {} #{:02}", parent.title, r.subagent_persona, i + 1);
            let row = sqlx::query_as::<_, TaskRow>(
                r#"INSERT INTO tasks
                   (company_id, primary_goal_id, goal_ancestry, title, specification, state, owner_persona, parent_task_id, spawned_by_rule_id, priority, display_number)
                   VALUES ($1,$2,$3,$4,$5,'open',$6,$7,$8,$9,$10)
                   RETURNING id, company_id, primary_goal_id, goal_ancestry, title, specification, state, owner_persona,
                             parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, created_at::text"#,
            )
            .bind(company_id)
            .bind(parent.primary_goal_id)
            .bind(SqlxJson(parent.goal_ancestry.clone()))
            .bind(title)
            .bind(parent.specification.clone())
            .bind(r.subagent_persona.clone())
            .bind(task_id)
            .bind(r.id)
            .bind(parent.priority)
            .bind(display_n)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
            created.push(row);
        }
        tx.commit().await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;
    }
    let actor = if body.actor.trim().is_empty() {
        "spawn_engine"
    } else {
        body.actor.trim()
    };
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1,$2,'task_spawn_subagents','task',$3,$4,'info')"#,
    )
    .bind(company_id)
    .bind(actor)
    .bind(task_id.to_string())
    .bind(SqlxJson(json!({ "spawned_count": created.len() })))
    .execute(pool)
    .await;

    Ok(Json(json!({ "spawned": created, "count": created.len() })))
}

#[derive(sqlx::FromRow, Serialize)]
struct TaskHandoffRow {
    id: Uuid,
    company_id: Uuid,
    task_id: Uuid,
    from_agent: String,
    to_agent: String,
    handoff_contract: SqlxJson<Value>,
    review_contract: SqlxJson<Value>,
    status: String,
    notes: Option<String>,
    created_at: String,
    reviewed_at: Option<chrono::DateTime<chrono::Utc>>,
    reviewed_by: Option<String>,
}

#[derive(Deserialize)]
struct PostTaskHandoffBody {
    from_agent: String,
    to_agent: String,
    #[serde(default)]
    handoff_contract: Value,
    #[serde(default)]
    review_contract: Value,
    #[serde(default)]
    notes: String,
}

async fn list_task_handoffs(
    State(st): State<ConsoleState>,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, TaskHandoffRow>(
        r#"SELECT id, company_id, task_id, from_agent, to_agent, handoff_contract, review_contract, status, notes,
                  created_at::text, reviewed_at, reviewed_by
           FROM task_handoffs
           WHERE company_id = $1 AND task_id = $2
           ORDER BY created_at DESC"#,
    )
    .bind(company_id)
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "handoffs": rows })))
}

async fn post_task_handoff(
    State(st): State<ConsoleState>,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PostTaskHandoffBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if body.from_agent.trim().is_empty() || body.to_agent.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "from_agent and to_agent required"}))));
    }
    let row = sqlx::query_as::<_, TaskHandoffRow>(
        r#"INSERT INTO task_handoffs
           (company_id, task_id, from_agent, to_agent, handoff_contract, review_contract, status, notes)
           VALUES ($1,$2,$3,$4,$5,$6,'pending_review',$7)
           RETURNING id, company_id, task_id, from_agent, to_agent, handoff_contract, review_contract, status, notes,
                     created_at::text, reviewed_at, reviewed_by"#,
    )
    .bind(company_id)
    .bind(task_id)
    .bind(body.from_agent.trim())
    .bind(body.to_agent.trim())
    .bind(SqlxJson(body.handoff_contract))
    .bind(SqlxJson(body.review_contract))
    .bind(body.notes.trim())
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok((StatusCode::CREATED, Json(json!({ "handoff": row }))))
}

#[derive(Deserialize)]
struct ReviewTaskHandoffBody {
    decision: String,
    reviewer: String,
    #[serde(default)]
    notes: String,
}

async fn review_task_handoff(
    State(st): State<ConsoleState>,
    Path(handoff_id): Path<Uuid>,
    Json(body): Json<ReviewTaskHandoffBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let decision = body.decision.trim().to_ascii_lowercase();
    let next = match decision.as_str() {
        "accept" | "accepted" => "accepted",
        "reject" | "rejected" => "rejected",
        _ => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"decision must be accept|reject"}))));
        }
    };
    let row = sqlx::query_as::<_, TaskHandoffRow>(
        r#"UPDATE task_handoffs
           SET status = $2, reviewed_at = NOW(), reviewed_by = $3, notes = COALESCE(NULLIF($4,''), notes)
           WHERE id = $1
           RETURNING id, company_id, task_id, from_agent, to_agent, handoff_contract, review_contract, status, notes,
                     created_at::text, reviewed_at, reviewed_by"#,
    )
    .bind(handoff_id)
    .bind(next)
    .bind(body.reviewer.trim())
    .bind(body.notes.trim())
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    let Some(h) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error":"handoff not found"}))));
    };
    Ok(Json(json!({ "handoff": h })))
}

#[derive(sqlx::FromRow, Serialize)]
struct ImprovementRunRow {
    id: Uuid,
    company_id: Uuid,
    title: String,
    scope: String,
    baseline_meta: SqlxJson<Value>,
    candidate_meta: SqlxJson<Value>,
    gate_contract: SqlxJson<Value>,
    metrics_meta: SqlxJson<Value>,
    status: String,
    decision_reason: Option<String>,
    decided_by: Option<String>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct CreateImprovementRunBody {
    title: String,
    scope: String,
    #[serde(default)]
    baseline_meta: Value,
    #[serde(default)]
    candidate_meta: Value,
    #[serde(default)]
    gate_contract: Value,
    #[serde(default)]
    metrics_meta: Value,
}

async fn list_improvement_runs(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, ImprovementRunRow>(
        r#"SELECT id, company_id, title, scope, baseline_meta, candidate_meta, gate_contract, metrics_meta, status,
                  decision_reason, decided_by, decided_at, created_at::text, updated_at::text
           FROM improvement_runs WHERE company_id = $1 ORDER BY created_at DESC LIMIT 200"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "runs": rows })))
}

async fn create_improvement_run(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateImprovementRunBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if body.title.trim().is_empty() || body.scope.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"title and scope required"}))));
    }
    let row = sqlx::query_as::<_, ImprovementRunRow>(
        r#"INSERT INTO improvement_runs
           (company_id, title, scope, baseline_meta, candidate_meta, gate_contract, metrics_meta, status)
           VALUES ($1,$2,$3,$4,$5,$6,$7,'proposed')
           RETURNING id, company_id, title, scope, baseline_meta, candidate_meta, gate_contract, metrics_meta, status,
                     decision_reason, decided_by, decided_at, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(body.title.trim())
    .bind(body.scope.trim())
    .bind(SqlxJson(body.baseline_meta))
    .bind(SqlxJson(body.candidate_meta))
    .bind(SqlxJson(body.gate_contract))
    .bind(SqlxJson(body.metrics_meta))
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok((StatusCode::CREATED, Json(json!({ "run": row }))))
}

#[derive(Deserialize)]
struct DecideImprovementRunBody {
    decision: String,
    actor: String,
    #[serde(default)]
    reason: String,
}

async fn decide_improvement_run(
    State(st): State<ConsoleState>,
    headers: HeaderMap,
    Path(run_id): Path<Uuid>,
    Json(body): Json<DecideImprovementRunBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let decision = body.decision.trim().to_ascii_lowercase();
    let next = match decision.as_str() {
        "promote" | "promoted" => "promoted",
        "revert" | "reverted" => "reverted",
        _ => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"decision must be promote|revert"}))));
        }
    };
    let existing = sqlx::query_as::<_, ImprovementRunRow>(
        r#"SELECT id, company_id, title, scope, baseline_meta, candidate_meta, gate_contract, metrics_meta, status,
                  decision_reason, decided_by, decided_at, created_at::text, updated_at::text
           FROM improvement_runs WHERE id = $1"#,
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    let Some(current) = existing else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error":"improvement run not found"}))));
    };
    if let Some(k) = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let payload = json!({ "run_id": run_id, "decision": next, "actor": body.actor.trim() });
        let ok = register_idempotency(pool, current.company_id, "improvement_decision", k, &payload)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
        if !ok {
            return Err((StatusCode::CONFLICT, Json(json!({"error":"duplicate idempotency key"}))));
        }
    }
    if next == "promoted" {
        let gate = &current.gate_contract.0;
        let metrics = &current.metrics_meta.0;
        let min_samples = gate
            .get("min_eval_samples")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let eval_samples = metrics
            .get("eval_samples")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if eval_samples < min_samples {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"promotion gate failed: min_eval_samples"}))));
        }
        let max_reg = gate
            .get("max_regression_pct")
            .and_then(|v| v.as_f64())
            .unwrap_or(100.0);
        let regression = metrics
            .get("current_regression_pct")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if regression > max_reg {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"promotion gate failed: regression threshold"}))));
        }
        let requires_reviewer = gate
            .get("high_risk_requires_reviewer")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let risk = current
            .candidate_meta
            .0
            .get("risk_level")
            .and_then(|v| v.as_str())
            .unwrap_or("low")
            .to_ascii_lowercase();
        if requires_reviewer && (risk == "high" || risk == "critical") && body.actor.trim().is_empty() {
            return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"promotion gate failed: reviewer required for high risk"}))));
        }
    }
    let rationale = if body.reason.trim().is_empty() {
        if next == "promoted" {
            format!("Promoted run '{}' after gates passed.", current.title)
        } else {
            format!("Reverted run '{}' due to risk/performance decision.", current.title)
        }
    } else {
        body.reason.trim().to_string()
    };
    let row = sqlx::query_as::<_, ImprovementRunRow>(
        r#"UPDATE improvement_runs
           SET status = $2, decision_reason = $3, decided_by = $4, decided_at = NOW(), updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, title, scope, baseline_meta, candidate_meta, gate_contract, metrics_meta, status,
                     decision_reason, decided_by, decided_at, created_at::text, updated_at::text"#,
    )
    .bind(run_id)
    .bind(next)
    .bind(rationale)
    .bind(body.actor.trim())
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    let Some(run) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error":"improvement run not found"}))));
    };
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, decision, severity)
           VALUES ($1,$2,'improvement_decision','improvement_run',$3,$4,$5,'info')"#,
    )
    .bind(run.company_id)
    .bind(body.actor.trim())
    .bind(run.id.to_string())
    .bind(SqlxJson(json!({ "status": run.status, "reason": run.decision_reason })))
    .bind(run.status.clone())
    .execute(pool)
    .await;
    Ok(Json(json!({ "run": run })))
}

#[derive(sqlx::FromRow, Serialize)]
struct ContractVersionRow {
    id: Uuid,
    contract_id: String,
    version: String,
    status: String,
    schema: SqlxJson<Value>,
    created_at: String,
}

#[derive(Deserialize)]
struct CreateContractVersionBody {
    contract_id: String,
    version: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    schema: Value,
}

#[derive(Deserialize)]
struct PatchContractVersionStatusBody {
    status: String,
}

async fn list_contract_versions(
    State(st): State<ConsoleState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, ContractVersionRow>(
        r#"SELECT id, contract_id, version, status, schema, created_at::text
           FROM onboarding_contract_versions
           ORDER BY contract_id, created_at DESC"#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "versions": rows })))
}

async fn create_contract_version(
    State(st): State<ConsoleState>,
    Json(body): Json<CreateContractVersionBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let cid = body.contract_id.trim();
    let ver = body.version.trim();
    if cid.is_empty() || ver.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"contract_id and version required"}))));
    }
    let status = body
        .status
        .as_deref()
        .unwrap_or("active")
        .trim()
        .to_ascii_lowercase();
    if !matches!(status.as_str(), "active" | "deprecated" | "sunset") {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"status must be active|deprecated|sunset"}))));
    }
    let row = sqlx::query_as::<_, ContractVersionRow>(
        r#"INSERT INTO onboarding_contract_versions (contract_id, version, status, schema)
           VALUES ($1,$2,$3,$4)
           RETURNING id, contract_id, version, status, schema, created_at::text"#,
    )
    .bind(cid)
    .bind(ver)
    .bind(status)
    .bind(SqlxJson(body.schema))
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))))?;
    Ok((StatusCode::CREATED, Json(json!({ "version": row }))))
}

async fn patch_contract_version_status(
    State(st): State<ConsoleState>,
    Path(version_id): Path<Uuid>,
    Json(body): Json<PatchContractVersionStatusBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let next = body.status.trim().to_ascii_lowercase();
    if !matches!(next.as_str(), "active" | "deprecated" | "sunset") {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"status must be active|deprecated|sunset"}))));
    }
    let row = sqlx::query_as::<_, ContractVersionRow>(
        r#"UPDATE onboarding_contract_versions
           SET status = $2
           WHERE id = $1
           RETURNING id, contract_id, version, status, schema, created_at::text"#,
    )
    .bind(version_id)
    .bind(next)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    let Some(v) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error":"contract version not found"}))));
    };
    Ok(Json(json!({ "version": v })))
}

#[derive(sqlx::FromRow, Serialize)]
struct ConnectorPresetRow {
    id: Uuid,
    vertical: String,
    connector_provider: String,
    allowed_actions: SqlxJson<Value>,
    blocked_actions: SqlxJson<Value>,
    created_at: String,
}

#[derive(Deserialize)]
struct UpsertConnectorPresetBody {
    vertical: String,
    connector_provider: String,
    #[serde(default)]
    allowed_actions: Value,
    #[serde(default)]
    blocked_actions: Value,
}

async fn list_connector_presets(
    State(st): State<ConsoleState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, ConnectorPresetRow>(
        r#"SELECT id, vertical, connector_provider, allowed_actions, blocked_actions, created_at::text
           FROM connector_permission_presets
           ORDER BY vertical, connector_provider"#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "presets": rows })))
}

async fn upsert_connector_preset(
    State(st): State<ConsoleState>,
    Json(body): Json<UpsertConnectorPresetBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let vertical = body.vertical.trim().to_ascii_lowercase();
    let provider = body.connector_provider.trim().to_ascii_lowercase();
    if vertical.is_empty() || provider.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"vertical and connector_provider required"}))));
    }
    let row = sqlx::query_as::<_, ConnectorPresetRow>(
        r#"INSERT INTO connector_permission_presets (vertical, connector_provider, allowed_actions, blocked_actions)
           VALUES ($1,$2,$3,$4)
           ON CONFLICT (vertical, connector_provider) DO UPDATE
           SET allowed_actions = EXCLUDED.allowed_actions,
               blocked_actions = EXCLUDED.blocked_actions
           RETURNING id, vertical, connector_provider, allowed_actions, blocked_actions, created_at::text"#,
    )
    .bind(vertical)
    .bind(provider)
    .bind(SqlxJson(body.allowed_actions))
    .bind(SqlxJson(body.blocked_actions))
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok((StatusCode::CREATED, Json(json!({ "preset": row }))))
}

#[derive(sqlx::FromRow, Serialize)]
struct GoLiveChecklistRow {
    id: Uuid,
    company_id: Uuid,
    item_key: String,
    item_label: String,
    required: bool,
    completed: bool,
    completed_by: Option<String>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    notes: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct PostGoLiveChecklistBody {
    item_key: String,
    item_label: String,
    #[serde(default)]
    required: Option<bool>,
    #[serde(default)]
    notes: Option<String>,
}

async fn list_go_live_checklist(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, GoLiveChecklistRow>(
        r#"SELECT id, company_id, item_key, item_label, required, completed, completed_by, completed_at, notes,
                  created_at::text, updated_at::text
           FROM company_go_live_checklists
           WHERE company_id = $1
           ORDER BY required DESC, created_at"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok(Json(json!({ "checklist": rows })))
}

async fn post_go_live_checklist_item(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<PostGoLiveChecklistBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if body.item_key.trim().is_empty() || body.item_label.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"item_key and item_label required"}))));
    }
    let row = sqlx::query_as::<_, GoLiveChecklistRow>(
        r#"INSERT INTO company_go_live_checklists
           (company_id, item_key, item_label, required, notes)
           VALUES ($1,$2,$3,$4,$5)
           ON CONFLICT (company_id, item_key) DO UPDATE
           SET item_label = EXCLUDED.item_label,
               required = EXCLUDED.required,
               notes = EXCLUDED.notes,
               updated_at = NOW()
           RETURNING id, company_id, item_key, item_label, required, completed, completed_by, completed_at, notes,
                     created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(body.item_key.trim())
    .bind(body.item_label.trim())
    .bind(body.required.unwrap_or(true))
    .bind(body.notes.as_deref())
    .fetch_one(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    Ok((StatusCode::CREATED, Json(json!({ "item": row }))))
}

#[derive(Deserialize)]
struct CompleteGoLiveChecklistBody {
    actor: String,
    #[serde(default)]
    notes: String,
}

#[derive(Deserialize)]
struct SeedGoLiveChecklistBody {
    vertical: String,
    #[serde(default)]
    actor: String,
}

async fn complete_go_live_checklist_item(
    State(st): State<ConsoleState>,
    Path(item_id): Path<Uuid>,
    Json(body): Json<CompleteGoLiveChecklistBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let row = sqlx::query_as::<_, GoLiveChecklistRow>(
        r#"UPDATE company_go_live_checklists
           SET completed = true,
               completed_by = $2,
               completed_at = NOW(),
               notes = CASE WHEN $3 = '' THEN notes ELSE $3 END,
               updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, item_key, item_label, required, completed, completed_by, completed_at, notes,
                     created_at::text, updated_at::text"#,
    )
    .bind(item_id)
    .bind(body.actor.trim())
    .bind(body.notes.trim())
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    let Some(item) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error":"checklist item not found"}))));
    };
    Ok(Json(json!({ "item": item })))
}

fn go_live_template(vertical: &str) -> Vec<(&'static str, &'static str)> {
    match vertical.trim().to_ascii_lowercase().as_str() {
        "ecommerce" => vec![
            ("contracts_signed", "Customer terms and refund policy approved"),
            ("refund_guardrail", "Refund thresholds and approvers configured"),
            ("channel_integrations", "Shopify/helpdesk/email connectors verified"),
            ("handoff_queue_ready", "Handoff review queue staffed"),
        ],
        "property_management" => vec![
            ("legal_escalation", "Legal/fair-housing escalation path approved"),
            ("maintenance_sla", "Emergency/standard maintenance SLA configured"),
            ("tenant_comms_policy", "Tenant communication templates approved"),
            ("incident_runbook", "Incident and escalation runbook reviewed"),
        ],
        _ => vec![
            ("owner_signoff", "Owner sign-off on policy and risk settings"),
            ("approval_matrix", "Approval matrix configured and tested"),
            ("connector_smoke_test", "Core connectors smoke-tested"),
            ("ops_oncall", "Admin on-call and escalation contact assigned"),
        ],
    }
}

async fn seed_go_live_checklist(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<SeedGoLiveChecklistBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let items = go_live_template(&body.vertical);
    for (key, label) in items {
        let _ = sqlx::query(
            r#"INSERT INTO company_go_live_checklists (company_id, item_key, item_label, required)
               VALUES ($1,$2,$3,true)
               ON CONFLICT (company_id, item_key) DO NOTHING"#,
        )
        .bind(company_id)
        .bind(key)
        .bind(label)
        .execute(pool)
        .await;
    }
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1,$2,'go_live_checklist_seed','company',$3,$4,'info')"#,
    )
    .bind(company_id)
    .bind(if body.actor.trim().is_empty() { "admin_ui" } else { body.actor.trim() })
    .bind(company_id.to_string())
    .bind(SqlxJson(json!({ "vertical": body.vertical })))
    .execute(pool)
    .await;
    list_go_live_checklist(State(st), Path(company_id)).await
}

#[derive(Deserialize, Default)]
struct PatchGoalBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    /// Omit = no change; JSON `null` = clear parent; UUID string = set parent.
    #[serde(default)]
    parent_goal_id: Option<Value>,
    #[serde(default)]
    sort_order: Option<i32>,
}

enum ParentGoalPatch {
    Omit,
    Clear,
    Set(Uuid),
}

fn parse_parent_goal_patch(
    v: &Option<Value>,
) -> Result<ParentGoalPatch, (StatusCode, Json<Value>)> {
    match v {
        None => Ok(ParentGoalPatch::Omit),
        Some(Value::Null) => Ok(ParentGoalPatch::Clear),
        Some(x) => {
            let u: Uuid = serde_json::from_value(x.clone()).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "parent_goal_id must be UUID or null" })),
                )
            })?;
            Ok(ParentGoalPatch::Set(u))
        }
    }
}

async fn patch_goal(
    State(st): State<ConsoleState>,
    Path((company_id, goal_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchGoalBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM goals WHERE id = $1 AND company_id = $2)",
    )
    .bind(goal_id)
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    if !exists {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "goal not found" }))));
    }

    let parent_patch = parse_parent_goal_patch(&body.parent_goal_id)?;
    if let ParentGoalPatch::Set(pid) = &parent_patch {
        if *pid == goal_id {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "goal cannot be its own parent" })),
            ));
        }
        let ok = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM goals WHERE id = $1 AND company_id = $2)",
        )
        .bind(pid)
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
                Json(json!({ "error": "parent_goal_id not in company" })),
            ));
        }
        let chain_hits = parent_chain_contains_goal(pool, company_id, *pid, goal_id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
        if chain_hits {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "parent_goal_id would create a cycle" })),
            ));
        }
    }

    let title_upd = body.title.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty());
    let (parent_mode, parent_bind): (i32, Option<Uuid>) = match parent_patch {
        ParentGoalPatch::Omit => (0, None),
        ParentGoalPatch::Clear => (1, None),
        ParentGoalPatch::Set(u) => (2, Some(u)),
    };
    let row = sqlx::query_as::<_, GoalRow>(
        r#"UPDATE goals SET
            title = COALESCE($3, title),
            description = COALESCE($4, description),
            status = COALESCE($5, status),
            parent_goal_id = CASE $8
                WHEN 0 THEN parent_goal_id
                WHEN 1 THEN NULL
                ELSE $6
            END,
            sort_order = COALESCE($7, sort_order),
            updated_at = NOW()
           WHERE id = $1 AND company_id = $2
           RETURNING id, company_id, parent_goal_id, title, description, status, created_at::text"#,
    )
    .bind(goal_id)
    .bind(company_id)
    .bind(title_upd)
    .bind(body.description.as_ref())
    .bind(body.status.as_ref())
    .bind(parent_bind)
    .bind(body.sort_order)
    .bind(parent_mode)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({ "goal": row })))
}

#[derive(sqlx::FromRow, Serialize)]
struct GovRow {
    id: Uuid,
    actor: String,
    action: String,
    subject_type: String,
    subject_id: String,
    payload: SqlxJson<Value>,
    created_at: String,
}

#[derive(sqlx::FromRow, Serialize)]
struct PolicyRuleRow {
    id: Uuid,
    company_id: Uuid,
    action_type: String,
    risk_level: String,
    amount_min: Option<f64>,
    amount_max: Option<f64>,
    decision_mode: String,
    created_at: String,
    updated_at: String,
}

async fn list_governance(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, GovRow>(
        r#"SELECT id, actor, action, subject_type, subject_id, payload, created_at::text
           FROM governance_events WHERE company_id = $1 ORDER BY created_at DESC LIMIT 200"#,
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
    Ok(Json(json!({ "events": rows })))
}

#[derive(Deserialize)]
struct PostGovBody {
    actor: String,
    action: String,
    subject_type: String,
    subject_id: String,
    #[serde(default)]
    payload: Value,
}

async fn post_governance(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<PostGovBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if body.actor.trim().is_empty() || body.action.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "actor and action required" })),
        ));
    }
    let row = sqlx::query_as::<_, GovRow>(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, actor, action, subject_type, subject_id, payload, created_at::text"#,
    )
    .bind(company_id)
    .bind(body.actor.trim())
    .bind(body.action.trim())
    .bind(body.subject_type.trim())
    .bind(body.subject_id.trim())
    .bind(SqlxJson(body.payload))
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok((StatusCode::CREATED, Json(json!({ "event": row }))))
}

#[derive(Deserialize)]
struct PostPolicyRuleBody {
    action_type: String,
    risk_level: String,
    #[serde(default)]
    amount_min: Option<f64>,
    #[serde(default)]
    amount_max: Option<f64>,
    decision_mode: String,
}

#[derive(Deserialize)]
struct EvaluatePolicyBody {
    action_type: String,
    risk_level: String,
    #[serde(default)]
    amount: Option<f64>,
}

fn normalize_risk(v: &str) -> Option<&'static str> {
    match v.trim().to_ascii_lowercase().as_str() {
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "critical" => Some("critical"),
        _ => None,
    }
}

fn normalize_decision(v: &str) -> Option<&'static str> {
    match v.trim().to_ascii_lowercase().as_str() {
        "auto" => Some("auto"),
        "admin_required" => Some("admin_required"),
        "blocked" => Some("blocked"),
        _ => None,
    }
}

async fn list_policy_rules(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, PolicyRuleRow>(
        r#"SELECT id, company_id, action_type, risk_level,
                  amount_min::float8 as amount_min, amount_max::float8 as amount_max,
                  decision_mode, created_at::text, updated_at::text
           FROM policy_rules
           WHERE company_id = $1
           ORDER BY action_type, risk_level, amount_min NULLS FIRST, created_at"#,
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
    Ok(Json(json!({ "rules": rows })))
}

async fn post_policy_rule(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<PostPolicyRuleBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let action = body.action_type.trim().to_ascii_lowercase();
    if action.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "action_type required" })),
        ));
    }
    let Some(risk) = normalize_risk(&body.risk_level) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "risk_level must be low|medium|high|critical" })),
        ));
    };
    let Some(decision) = normalize_decision(&body.decision_mode) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "decision_mode must be auto|admin_required|blocked" })),
        ));
    };
    if let (Some(min), Some(max)) = (body.amount_min, body.amount_max) {
        if min > max {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "amount_min must be <= amount_max" })),
            ));
        }
    }

    let row = sqlx::query_as::<_, PolicyRuleRow>(
        r#"INSERT INTO policy_rules (company_id, action_type, risk_level, amount_min, amount_max, decision_mode)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, company_id, action_type, risk_level,
                     amount_min::float8 as amount_min, amount_max::float8 as amount_max,
                     decision_mode, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(action)
    .bind(risk)
    .bind(body.amount_min)
    .bind(body.amount_max)
    .bind(decision)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok((StatusCode::CREATED, Json(json!({ "rule": row }))))
}

async fn evaluate_policy(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<EvaluatePolicyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let action = body.action_type.trim().to_ascii_lowercase();
    if action.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "action_type required" })),
        ));
    }
    let Some(risk) = normalize_risk(&body.risk_level) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "risk_level must be low|medium|high|critical" })),
        ));
    };

    let matched = sqlx::query_as::<_, PolicyRuleRow>(
        r#"SELECT id, company_id, action_type, risk_level,
                  amount_min::float8 as amount_min, amount_max::float8 as amount_max,
                  decision_mode, created_at::text, updated_at::text
           FROM policy_rules
           WHERE company_id = $1
             AND action_type = $2
             AND risk_level = $3
             AND ($4::float8 IS NULL OR amount_min IS NULL OR amount_min <= $4::float8)
             AND ($4::float8 IS NULL OR amount_max IS NULL OR amount_max >= $4::float8)
           ORDER BY
             CASE decision_mode
               WHEN 'blocked' THEN 0
               WHEN 'admin_required' THEN 1
               ELSE 2
             END,
             amount_min DESC NULLS LAST,
             created_at
           LIMIT 1"#,
    )
    .bind(company_id)
    .bind(action.clone())
    .bind(risk)
    .bind(body.amount)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    if let Some(rule) = matched {
        return Ok(Json(json!({
            "decision_mode": rule.decision_mode,
            "matched_rule": rule,
            "reason": "matched_company_rule"
        })));
    }

    Ok(Json(json!({
        "decision_mode": "admin_required",
        "matched_rule": Value::Null,
        "reason": "no_matching_rule_default_admin_required",
        "action_type": action,
        "risk_level": risk,
    })))
}

async fn spend_summary(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows: Vec<(String, f64)> = sqlx::query_as(
        r#"SELECT kind, COALESCE(SUM(amount), 0)::float8
           FROM spend_events WHERE company_id = $1 GROUP BY kind ORDER BY kind"#,
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
    let total: f64 = rows.iter().map(|(_, a)| a).sum();
    Ok(Json(json!({
        "company_id": company_id,
        "by_kind": rows.iter().map(|(k, a)| json!({ "kind": k, "amount_usd": a })).collect::<Vec<_>>(),
        "total_usd": total,
    })))
}

async fn export_company_json(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let bundle = export_bundle(pool, company_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    Ok(Json(serde_json::to_value(&bundle).unwrap_or(json!({}))))
}

async fn import_company_bundle(
    State(st): State<ConsoleState>,
    Json(req): Json<ImportRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let id = run_import_bundle(pool, req)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "company_id": id, "message": "imported" })),
    ))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct OnboardWorkflowDraft {
    title: String,
    owner_role: String,
    priority: String,
    sla_target: String,
    approval: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct OnboardPolicyDraft {
    action_type: String,
    risk_level: String,
    decision_mode: String,
    amount_min: Option<f64>,
    amount_max: Option<f64>,
    approver_role: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct OnboardingDraft {
    company_name: String,
    industry: String,
    vertical_template: String,
    pack_contract_id: String,
    workflows: Vec<OnboardWorkflowDraft>,
    policy_rules: Vec<OnboardPolicyDraft>,
    kpi_gates: Vec<OnboardingGateResult>,
    risk_gates: Vec<OnboardingGateResult>,
    missing_critical_items: Vec<String>,
    confidence_by_field: std::collections::BTreeMap<String, f64>,
}

#[derive(Debug, Deserialize)]
struct OnboardingDraftRequest {
    transcript: String,
    #[serde(default)]
    vertical_template: Option<String>,
    #[serde(default)]
    pack_contract_id: Option<String>,
    #[serde(default)]
    company_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OnboardingApplyRequest {
    draft: OnboardingDraft,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ValidateOnboardingPackBody {
    pack_contract_id: String,
    transcript: String,
}

async fn list_onboarding_pack_contracts() -> Json<Value> {
    match load_contracts_hot() {
        Ok(contracts) => Json(json!({ "contracts": contracts })),
        Err(e) => Json(json!({ "contracts": [], "error": e.to_string() })),
    }
}

async fn validate_onboarding_pack_contract(
    Json(body): Json<ValidateOnboardingPackBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let contracts = load_contracts_hot().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let transcript = body.transcript.trim();
    let pack = find_contract(&contracts, &body.pack_contract_id, "");
    let kpi = evaluate_gate_results(transcript, &pack.kpi_gates);
    let risk = evaluate_gate_results(transcript, &pack.risk_gates);
    let unsatisfied_required: Vec<String> = kpi
        .iter()
        .chain(risk.iter())
        .filter(|g| g.required && !g.satisfied)
        .map(|g| g.id.clone())
        .collect();
    let ok_to_apply = unsatisfied_required.is_empty();
    Ok(Json(json!({
        "pack_contract": pack,
        "kpi_gates": kpi,
        "risk_gates": risk,
        "unsatisfied_required_gates": unsatisfied_required,
        "ok_to_apply": ok_to_apply
    })))
}

fn template_defaults(vertical: &str) -> (&'static str, Vec<OnboardWorkflowDraft>, Vec<OnboardPolicyDraft>) {
    let v = vertical.trim().to_ascii_lowercase();
    match v.as_str() {
        "ecommerce" | "online_commerce" => (
            "ecommerce",
            vec![
                OnboardWorkflowDraft { title: "Customer inquiry triage".into(), owner_role: "support_admin".into(), priority: "high".into(), sla_target: "1h".into(), approval: "auto".into() },
                OnboardWorkflowDraft { title: "Order issue follow-up".into(), owner_role: "ops_admin".into(), priority: "high".into(), sla_target: "same_day".into(), approval: "admin_required".into() },
                OnboardWorkflowDraft { title: "Product content refresh".into(), owner_role: "marketing_admin".into(), priority: "medium".into(), sla_target: "24h".into(), approval: "auto".into() },
            ],
            vec![
                OnboardPolicyDraft { action_type: "send_message".into(), risk_level: "medium".into(), decision_mode: "auto".into(), amount_min: None, amount_max: None, approver_role: "support_admin".into() },
                OnboardPolicyDraft { action_type: "refund".into(), risk_level: "high".into(), decision_mode: "admin_required".into(), amount_min: Some(100.0), amount_max: None, approver_role: "finance_admin".into() },
                OnboardPolicyDraft { action_type: "update_budget".into(), risk_level: "critical".into(), decision_mode: "blocked".into(), amount_min: None, amount_max: None, approver_role: "owner".into() },
            ],
        ),
        "marketing" | "marketing_agency" => (
            "marketing",
            vec![
                OnboardWorkflowDraft { title: "Lead response drafting".into(), owner_role: "account_manager".into(), priority: "high".into(), sla_target: "1h".into(), approval: "auto".into() },
                OnboardWorkflowDraft { title: "Campaign performance summary".into(), owner_role: "ads_manager".into(), priority: "medium".into(), sla_target: "same_day".into(), approval: "auto".into() },
                OnboardWorkflowDraft { title: "Client update email".into(), owner_role: "account_manager".into(), priority: "medium".into(), sla_target: "24h".into(), approval: "admin_required".into() },
            ],
            vec![
                OnboardPolicyDraft { action_type: "send_message".into(), risk_level: "low".into(), decision_mode: "auto".into(), amount_min: None, amount_max: None, approver_role: "account_manager".into() },
                OnboardPolicyDraft { action_type: "publish_campaign".into(), risk_level: "high".into(), decision_mode: "admin_required".into(), amount_min: None, amount_max: None, approver_role: "ads_manager".into() },
                OnboardPolicyDraft { action_type: "update_budget".into(), risk_level: "critical".into(), decision_mode: "blocked".into(), amount_min: None, amount_max: None, approver_role: "owner".into() },
            ],
        ),
        "property_management" | "property" => (
            "property_management",
            vec![
                OnboardWorkflowDraft { title: "Tenant request triage".into(), owner_role: "property_admin".into(), priority: "high".into(), sla_target: "same_day".into(), approval: "auto".into() },
                OnboardWorkflowDraft { title: "Maintenance dispatch & follow-up".into(), owner_role: "maintenance_coord".into(), priority: "high".into(), sla_target: "24h".into(), approval: "admin_required".into() },
                OnboardWorkflowDraft { title: "Owner financial packet review".into(), owner_role: "owner".into(), priority: "medium".into(), sla_target: "same_day".into(), approval: "admin_required".into() },
            ],
            vec![
                OnboardPolicyDraft { action_type: "send_message".into(), risk_level: "low".into(), decision_mode: "auto".into(), amount_min: None, amount_max: None, approver_role: "property_admin".into() },
                OnboardPolicyDraft { action_type: "refund".into(), risk_level: "high".into(), decision_mode: "admin_required".into(), amount_min: Some(0.0), amount_max: None, approver_role: "owner".into() },
                OnboardPolicyDraft { action_type: "update_budget".into(), risk_level: "critical".into(), decision_mode: "blocked".into(), amount_min: None, amount_max: None, approver_role: "owner".into() },
            ],
        ),
        _ => (
            "generic_smb",
            vec![
                OnboardWorkflowDraft { title: "Inbound message triage".into(), owner_role: "ops_admin".into(), priority: "high".into(), sla_target: "1h".into(), approval: "auto".into() },
                OnboardWorkflowDraft { title: "Follow-up scheduling".into(), owner_role: "ops_admin".into(), priority: "medium".into(), sla_target: "same_day".into(), approval: "auto".into() },
                OnboardWorkflowDraft { title: "Exception escalation".into(), owner_role: "manager".into(), priority: "high".into(), sla_target: "1h".into(), approval: "admin_required".into() },
            ],
            vec![
                OnboardPolicyDraft { action_type: "send_message".into(), risk_level: "medium".into(), decision_mode: "auto".into(), amount_min: None, amount_max: None, approver_role: "ops_admin".into() },
                OnboardPolicyDraft { action_type: "refund".into(), risk_level: "high".into(), decision_mode: "admin_required".into(), amount_min: Some(50.0), amount_max: None, approver_role: "manager".into() },
                OnboardPolicyDraft { action_type: "update_budget".into(), risk_level: "critical".into(), decision_mode: "blocked".into(), amount_min: None, amount_max: None, approver_role: "owner".into() },
            ],
        ),
    }
}

async fn generate_onboarding_draft(
    Json(req): Json<OnboardingDraftRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let transcript = req.transcript.trim();
    if transcript.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "transcript required" })),
        ));
    }

    let t = transcript.to_ascii_lowercase();
    let selected_vertical = req
        .vertical_template
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if t.contains("shopify") || t.contains("order") || t.contains("ecom") {
                "ecommerce".to_string()
            } else if t.contains("campaign") || t.contains("ads") || t.contains("lead") {
                "marketing".to_string()
            } else if t.contains("tenant") || t.contains("property") || t.contains("building") {
                "property_management".to_string()
            } else {
                "generic_smb".to_string()
            }
        });

    let (industry, mut workflows, mut rules) = template_defaults(&selected_vertical);
    let contracts = load_contracts_hot().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let selected_pack = find_contract(
        &contracts,
        req.pack_contract_id.as_deref().unwrap_or(""),
        &selected_vertical,
    );

    if t.contains("refund") {
        rules.push(OnboardPolicyDraft {
            action_type: "refund".into(),
            risk_level: "high".into(),
            decision_mode: "admin_required".into(),
            amount_min: Some(0.0),
            amount_max: None,
            approver_role: "finance_admin".into(),
        });
    }
    if t.contains("legal") || t.contains("contract") {
        rules.push(OnboardPolicyDraft {
            action_type: "legal_reply".into(),
            risk_level: "critical".into(),
            decision_mode: "blocked".into(),
            amount_min: None,
            amount_max: None,
            approver_role: "legal_owner".into(),
        });
    }
    if t.contains("urgent") || t.contains("1h") {
        for w in &mut workflows {
            if w.priority != "low" {
                w.sla_target = "1h".to_string();
            }
        }
    }

    let company_name = req
        .company_name
        .unwrap_or_default()
        .trim()
        .to_string();

    let mut missing = Vec::new();
    if company_name.is_empty() {
        missing.push("company_name".to_string());
    }
    if !t.contains("approve") && !t.contains("manager") && !t.contains("owner") {
        missing.push("approver_role".to_string());
    }
    if workflows.is_empty() {
        missing.push("workflows".to_string());
    }
    let kpi_gates = evaluate_gate_results(transcript, &selected_pack.kpi_gates);
    let risk_gates = evaluate_gate_results(transcript, &selected_pack.risk_gates);
    for g in kpi_gates.iter().chain(risk_gates.iter()) {
        if g.required && !g.satisfied {
            missing.push(format!("gate:{}", g.id));
        }
    }

    let mut conf = std::collections::BTreeMap::new();
    conf.insert("company_name".to_string(), if company_name.is_empty() { 0.2 } else { 0.95 });
    conf.insert("industry".to_string(), if selected_vertical == "generic_smb" { 0.6 } else { 0.85 });
    conf.insert("workflows".to_string(), 0.82);
    conf.insert("policy_rules".to_string(), 0.79);
    conf.insert("ownership".to_string(), if missing.iter().any(|x| x == "approver_role") { 0.45 } else { 0.8 });

    let draft = OnboardingDraft {
        company_name,
        industry: industry.to_string(),
        vertical_template: selected_vertical,
        pack_contract_id: selected_pack.id,
        workflows,
        policy_rules: rules,
        kpi_gates,
        risk_gates,
        missing_critical_items: missing,
        confidence_by_field: conf,
    };
    Ok(Json(json!({ "draft": draft })))
}

async fn apply_onboarding_draft(
    State(st): State<ConsoleState>,
    Json(req): Json<OnboardingApplyRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let d = req.draft;
    let unsatisfied_required = d
        .kpi_gates
        .iter()
        .chain(d.risk_gates.iter())
        .any(|g| g.required && !g.satisfied);
    if unsatisfied_required {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "cannot apply draft: required KPI/risk gates are not satisfied" })),
        ));
    }
    let display_name = req
        .display_name
        .as_deref()
        .unwrap_or(d.company_name.as_str())
        .trim();
    if display_name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company name required in draft/display_name" })),
        ));
    }
    let mut slug = req.slug.unwrap_or_else(|| {
        display_name
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-")
    });
    if slug.is_empty() {
        slug = "company".to_string();
    }

    let mut tx = pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let issue_prefix = derive_issue_key_prefix(&slug);
    let company_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO companies (slug, display_name, issue_key_prefix)
           VALUES ($1, $2, $3) RETURNING id"#,
    )
    .bind(&slug)
    .bind(display_name)
    .bind(&issue_prefix)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    for r in d.policy_rules {
        let risk = normalize_risk(&r.risk_level).unwrap_or("medium");
        let decision = normalize_decision(&r.decision_mode).unwrap_or("admin_required");
        let action = r.action_type.trim().to_ascii_lowercase();
        if action.is_empty() {
            continue;
        }
        sqlx::query(
            r#"INSERT INTO policy_rules (company_id, action_type, risk_level, amount_min, amount_max, decision_mode)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(company_id)
        .bind(action)
        .bind(risk)
        .bind(r.amount_min)
        .bind(r.amount_max)
        .bind(decision)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    }

    for w in d.workflows {
        let p = match w.priority.trim().to_ascii_lowercase().as_str() {
            "high" => 3,
            "low" => 1,
            _ => 2,
        };
        let state = match w.approval.trim().to_ascii_lowercase().as_str() {
            "blocked" => "blocked",
            "admin_required" => "waiting_admin",
            _ => "open",
        };
        let display_n = next_task_display_number_tx(&mut tx, company_id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
        sqlx::query(
            r#"INSERT INTO tasks (company_id, title, specification, state, owner_persona, priority, display_number)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        )
        .bind(company_id)
        .bind(w.title.trim())
        .bind(format!("onboarding workflow · sla_target={} · approval={}", w.sla_target, w.approval))
        .bind(state)
        .bind(w.owner_role.trim())
        .bind(p)
        .bind(display_n)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    }

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "company_id": company_id,
            "slug": slug,
            "message": "onboarding draft applied"
        })),
    ))
}
