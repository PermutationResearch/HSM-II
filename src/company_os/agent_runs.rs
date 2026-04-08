//! Agent execution runs (`agent_runs`) and inline feedback (`run_feedback_events`), with optional promotion to `tasks`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use sqlx::FromRow;
use uuid::Uuid;

use crate::console::ConsoleState;

use super::next_task_display_number_tx;
use super::normalize_capability_refs;
use super::no_db;
use super::TaskRow;
use super::workspace_attachment_paths_json;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/agent-runs",
            get(list_agent_runs).post(post_agent_run),
        )
        .route(
            "/api/company/companies/:company_id/agent-runs/:run_id",
            get(get_agent_run).patch(patch_agent_run),
        )
        .route(
            "/api/company/companies/:company_id/agent-runs/:run_id/feedback",
            post(post_run_feedback),
        )
        .route(
            "/api/company/companies/:company_id/agent-runs/:run_id/feedback/:event_id/promote-task",
            post(post_promote_feedback_to_task),
        )
}

#[derive(FromRow, Serialize, Clone)]
struct AgentRunRow {
    id: Uuid,
    company_id: Uuid,
    task_id: Option<Uuid>,
    company_agent_id: Option<Uuid>,
    external_run_id: Option<String>,
    external_system: String,
    status: String,
    started_at: String,
    finished_at: Option<String>,
    summary: Option<String>,
    meta: SqlxJson<Value>,
}

#[derive(FromRow, Serialize)]
struct FeedbackEventRow {
    id: Uuid,
    run_id: Uuid,
    company_id: Uuid,
    step_index: Option<i32>,
    step_external_id: Option<String>,
    actor: String,
    kind: String,
    body: String,
    created_at: String,
    spawned_task_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct ListRunsQuery {
    #[serde(default)]
    task_id: Option<Uuid>,
    #[serde(default)]
    company_agent_id: Option<Uuid>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct CreateRunBody {
    #[serde(default)]
    task_id: Option<Uuid>,
    #[serde(default)]
    company_agent_id: Option<Uuid>,
    #[serde(default)]
    external_run_id: Option<String>,
    #[serde(default)]
    external_system: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    meta: Option<Value>,
}

#[derive(Deserialize)]
struct PatchRunBody {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    meta: Option<Value>,
    #[serde(default)]
    finished_at: Option<bool>,
}

#[derive(Deserialize)]
struct FeedbackBody {
    actor: String,
    body: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    step_index: Option<i32>,
    #[serde(default)]
    step_external_id: Option<String>,
}

#[derive(Deserialize)]
struct PromoteBody {
    title: String,
    #[serde(default)]
    specification: Option<String>,
    #[serde(default)]
    owner_persona: Option<String>,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    workspace_attachment_paths: Option<Vec<String>>,
    #[serde(default)]
    capability_refs: Option<Vec<Value>>,
}

fn normalize_run_status(s: &str) -> Result<&'static str, &'static str> {
    match s.trim().to_ascii_lowercase().as_str() {
        "running" => Ok("running"),
        "success" => Ok("success"),
        "error" => Ok("error"),
        "cancelled" => Ok("cancelled"),
        _ => Err("status must be running|success|error|cancelled"),
    }
}

fn normalize_feedback_kind(s: &str) -> Result<&'static str, &'static str> {
    match s.trim().to_ascii_lowercase().as_str() {
        "comment" => Ok("comment"),
        "correction" => Ok("correction"),
        "blocker" => Ok("blocker"),
        "praise" => Ok("praise"),
        _ => Err("kind must be comment|correction|blocker|praise"),
    }
}

async fn ensure_task_in_company(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    task_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = $1 AND company_id = $2)",
    )
    .bind(task_id)
    .bind(company_id)
    .fetch_one(pool)
    .await
}

async fn ensure_agent_in_company(
    pool: &sqlx::PgPool,
    company_id: Uuid,
    agent_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM company_agents WHERE id = $1 AND company_id = $2)",
    )
    .bind(agent_id)
    .bind(company_id)
    .fetch_one(pool)
    .await
}

async fn list_agent_runs(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<ListRunsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let limit = q.limit.unwrap_or(50).clamp(1, 200);

    let rows: Vec<AgentRunRow> = match (q.task_id, q.company_agent_id) {
        (Some(t), Some(a)) => {
            let sql = r#"SELECT id, company_id, task_id, company_agent_id, external_run_id, external_system, status,
                  started_at::text, finished_at::text, summary, meta
           FROM agent_runs WHERE company_id = $1 AND task_id = $2 AND company_agent_id = $3
           ORDER BY started_at DESC LIMIT $4"#;
            sqlx::query_as::<_, AgentRunRow>(sql)
                .bind(company_id)
                .bind(t)
                .bind(a)
                .bind(limit)
                .fetch_all(pool)
                .await
        }
        (Some(t), None) => {
            let sql = r#"SELECT id, company_id, task_id, company_agent_id, external_run_id, external_system, status,
                  started_at::text, finished_at::text, summary, meta
           FROM agent_runs WHERE company_id = $1 AND task_id = $2
           ORDER BY started_at DESC LIMIT $3"#;
            sqlx::query_as::<_, AgentRunRow>(sql)
                .bind(company_id)
                .bind(t)
                .bind(limit)
                .fetch_all(pool)
                .await
        }
        (None, Some(a)) => {
            let sql = r#"SELECT id, company_id, task_id, company_agent_id, external_run_id, external_system, status,
                  started_at::text, finished_at::text, summary, meta
           FROM agent_runs WHERE company_id = $1 AND company_agent_id = $2
           ORDER BY started_at DESC LIMIT $3"#;
            sqlx::query_as::<_, AgentRunRow>(sql)
                .bind(company_id)
                .bind(a)
                .bind(limit)
                .fetch_all(pool)
                .await
        }
        (None, None) => {
            let sql = r#"SELECT id, company_id, task_id, company_agent_id, external_run_id, external_system, status,
                  started_at::text, finished_at::text, summary, meta
           FROM agent_runs WHERE company_id = $1
           ORDER BY started_at DESC LIMIT $2"#;
            sqlx::query_as::<_, AgentRunRow>(sql)
                .bind(company_id)
                .bind(limit)
                .fetch_all(pool)
                .await
        }
    }
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({ "runs": rows })))
}

async fn post_agent_run(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateRunBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let ext_sys = body
        .external_system
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("hsm")
        .to_string();
    let ext_run = body.external_run_id.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty());

    if let Some(tid) = body.task_id {
        if !ensure_task_in_company(pool, company_id, tid)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })? {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "task_id not in company" })),
            ));
        }
    }
    if let Some(aid) = body.company_agent_id {
        if !ensure_agent_in_company(pool, company_id, aid)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })? {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "company_agent_id not in company" })),
            ));
        }
    }

    if let Some(ext) = ext_run {
        let existing: Option<AgentRunRow> = sqlx::query_as::<_, AgentRunRow>(
            r#"SELECT id, company_id, task_id, company_agent_id, external_run_id, external_system, status,
                      started_at::text, finished_at::text, summary, meta
               FROM agent_runs
               WHERE company_id = $1 AND external_system = $2 AND external_run_id = $3"#,
        )
        .bind(company_id)
        .bind(&ext_sys)
        .bind(ext)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
        if let Some(r) = existing {
            return Ok((
                StatusCode::OK,
                Json(json!({ "run": r, "idempotent": true })),
            ));
        }
    }

    let meta = SqlxJson(body.meta.unwrap_or_else(|| json!({})));
    let summary = body
        .summary
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let row = sqlx::query_as::<_, AgentRunRow>(
        r#"INSERT INTO agent_runs
           (company_id, task_id, company_agent_id, external_run_id, external_system, status, summary, meta)
           VALUES ($1, $2, $3, $4, $5, 'running', $6, $7)
           RETURNING id, company_id, task_id, company_agent_id, external_run_id, external_system, status,
                     started_at::text, finished_at::text, summary, meta"#,
    )
    .bind(company_id)
    .bind(body.task_id)
    .bind(body.company_agent_id)
    .bind(ext_run.map(|s| s.to_string()))
    .bind(&ext_sys)
    .bind(summary)
    .bind(meta)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok((StatusCode::CREATED, Json(json!({ "run": row }))))
}

async fn get_agent_run(
    State(st): State<ConsoleState>,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let run = sqlx::query_as::<_, AgentRunRow>(
        r#"SELECT id, company_id, task_id, company_agent_id, external_run_id, external_system, status,
                  started_at::text, finished_at::text, summary, meta
           FROM agent_runs WHERE id = $1 AND company_id = $2"#,
    )
    .bind(run_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(run) = run else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "run not found" })),
        ));
    };
    let feedback = sqlx::query_as::<_, FeedbackEventRow>(
        r#"SELECT id, run_id, company_id, step_index, step_external_id, actor, kind, body, created_at::text, spawned_task_id
           FROM run_feedback_events WHERE run_id = $1 ORDER BY created_at"#,
    )
    .bind(run_id)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok(Json(json!({ "run": run, "feedback": feedback })))
}

async fn patch_agent_run(
    State(st): State<ConsoleState>,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchRunBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let status_upd = if let Some(ref s) = body.status {
        Some(
            normalize_run_status(s)
                .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?
                .to_string(),
        )
    } else {
        None
    };
    let summary_upd = body
        .summary
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let meta_upd = body.meta.as_ref().map(|m| SqlxJson(m.clone()));
    let finished = body.finished_at.unwrap_or(false);

    let row = sqlx::query_as::<_, AgentRunRow>(
        r#"UPDATE agent_runs SET
            status = COALESCE($3, status),
            summary = COALESCE($4, summary),
            meta = COALESCE($5, meta),
            finished_at = CASE WHEN $6 THEN COALESCE(finished_at, NOW()) ELSE finished_at END
           WHERE id = $1 AND company_id = $2
           RETURNING id, company_id, task_id, company_agent_id, external_run_id, external_system, status,
                     started_at::text, finished_at::text, summary, meta"#,
    )
    .bind(run_id)
    .bind(company_id)
    .bind(status_upd.as_deref())
    .bind(summary_upd.as_deref())
    .bind(meta_upd.as_ref())
    .bind(finished)
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
            Json(json!({ "error": "run not found" })),
        ));
    };
    Ok(Json(json!({ "run": row })))
}

async fn post_run_feedback(
    State(st): State<ConsoleState>,
    Path((company_id, run_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<FeedbackBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let actor = body.actor.trim();
    if actor.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "actor required" })),
        ));
    }
    let text = body.body.trim();
    if text.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "body required" })),
        ));
    }
    let kind = body
        .kind
        .as_deref()
        .map(normalize_feedback_kind)
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?
        .unwrap_or("comment")
        .to_string();

    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE id = $1 AND company_id = $2)",
    )
    .bind(run_id)
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
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "run not found" })),
        ));
    }

    let row = sqlx::query_as::<_, FeedbackEventRow>(
        r#"INSERT INTO run_feedback_events
           (run_id, company_id, step_index, step_external_id, actor, kind, body)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id, run_id, company_id, step_index, step_external_id, actor, kind, body, created_at::text, spawned_task_id"#,
    )
    .bind(run_id)
    .bind(company_id)
    .bind(body.step_index)
    .bind(
        body.step_external_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    )
    .bind(actor)
    .bind(&kind)
    .bind(text)
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

async fn post_promote_feedback_to_task(
    State(st): State<ConsoleState>,
    Path((company_id, run_id, event_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<PromoteBody>,
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

    let mut tx = pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let event_ok: Option<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM run_feedback_events
           WHERE id = $1 AND run_id = $2 AND company_id = $3 AND spawned_task_id IS NULL"#,
    )
    .bind(event_id)
    .bind(run_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    if event_ok.is_none() {
        let _ = tx.rollback().await;
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "feedback event not found, wrong run, or already promoted" })),
        ));
    }

    let display_n = next_task_display_number_tx(&mut tx, company_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;

    let ws_json = workspace_attachment_paths_json(body.workspace_attachment_paths.clone());
    let caps_json = normalize_capability_refs(body.capability_refs.clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;

    let priority = body.priority.unwrap_or(0);

    let row = sqlx::query_as::<_, TaskRow>(
        r#"INSERT INTO tasks (company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, owner_persona, parent_task_id, spawned_by_rule_id, display_number, priority)
           VALUES ($1, NULL, NULL, $2, $3, $4, $5, $6, $7, NULL, NULL, $8, $9)
           RETURNING id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, created_at::text"#,
    )
    .bind(company_id)
    .bind(SqlxJson(json!([])))
    .bind(&title)
    .bind(&body.specification)
    .bind(SqlxJson(ws_json))
    .bind(SqlxJson(caps_json))
    .bind(&body.owner_persona)
    .bind(display_n)
    .bind(priority)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    sqlx::query(
        "UPDATE run_feedback_events SET spawned_task_id = $1 WHERE id = $2 AND company_id = $3",
    )
    .bind(row.id)
    .bind(event_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let actor = row
        .owner_persona
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("company_os");
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, $2, 'task_created', 'task', $3, $4, 'info')"#,
    )
    .bind(company_id)
    .bind(actor)
    .bind(row.id.to_string())
    .bind(SqlxJson(json!({
        "title": row.title,
        "owner_persona": row.owner_persona,
        "source": "run_feedback_promote",
        "run_id": run_id,
        "feedback_event_id": event_id,
    })))
    .execute(&mut *tx)
    .await;

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "task": row,
            "run_id": run_id,
            "feedback_event_id": event_id,
        })),
    ))
}
