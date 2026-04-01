//! Company OS API — PostgreSQL-backed companies, goals, tasks (Paperclip-class control plane, MVP).
//!
//! Enable with `HSM_COMPANY_OS_DATABASE_URL=postgres://...`. Migrations in `migrations/` run on startup.

mod bundle;
mod spend;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, patch, post},
    Json, Router,
};
pub use bundle::{export_bundle, import_bundle as run_import_bundle, CompanyBundle, ImportRequest};
pub use spend::spawn_record_llm_spend;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Json as SqlxJson;
use sqlx::PgPool;
use uuid::Uuid;

use crate::console::ConsoleState;

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
        .route("/api/company/health", get(company_health))
        .route("/api/company/import", post(import_company_bundle))
        .route("/api/company/companies", get(list_companies).post(create_company))
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
            "/api/company/companies/:company_id/tasks/queue",
            get(list_task_queue),
        )
        .route("/api/company/tasks/:task_id/sla", patch(patch_task_sla))
        .route("/api/company/tasks/:task_id/decision", post(post_task_decision))
        .route("/api/company/tasks/:task_id/checkout", post(checkout_task))
        .route("/api/company/tasks/:task_id/release", post(release_task))
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
        r#"SELECT id, slug, display_name, hsmii_home, created_at::text
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

#[derive(sqlx::FromRow, Serialize)]
struct CompanyRow {
    id: Uuid,
    slug: String,
    display_name: String,
    hsmii_home: Option<String>,
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
    let row: Result<CompanyRow, sqlx::Error> = sqlx::query_as::<_, CompanyRow>(
        r#"INSERT INTO companies (slug, display_name, hsmii_home)
           VALUES ($1, $2, $3)
           RETURNING id, slug, display_name, hsmii_home, created_at::text"#,
    )
    .bind(&slug)
    .bind(&display_name)
    .bind(&body.hsmii_home)
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
    checked_out_by: Option<String>,
    checked_out_until: Option<chrono::DateTime<chrono::Utc>>,
    priority: i32,
    created_at: String,
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
                  owner_persona, checked_out_by, checked_out_until, priority, created_at::text
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
    Ok(Json(json!({ "tasks": rows })))
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

    let row = sqlx::query_as::<_, TaskRow>(
        r#"INSERT INTO tasks (company_id, primary_goal_id, goal_ancestry, title, specification, owner_persona)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, company_id, primary_goal_id, goal_ancestry, title, specification, state,
                     owner_persona, checked_out_by, checked_out_until, priority, created_at::text"#,
    )
    .bind(company_id)
    .bind(&body.primary_goal_id)
    .bind(SqlxJson(ancestry_json))
    .bind(&title)
    .bind(&body.specification)
    .bind(&body.owner_persona)
    .fetch_one(pool)
    .await
    .map_err(|e| {
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
                     owner_persona, checked_out_by, checked_out_until, priority, created_at::text"#,
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
                     owner_persona, checked_out_by, checked_out_until, priority, created_at::text"#,
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
    Ok(Json(json!({ "task": t })))
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
                     owner_persona, checked_out_by, checked_out_until, priority, created_at::text"#,
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
    Ok(Json(json!({ "task": t })))
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
