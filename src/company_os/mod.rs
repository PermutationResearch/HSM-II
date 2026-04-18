//! Company OS API — PostgreSQL-backed **operational graph** per `company_id` (companies, goals, tasks,
//! memory, spend, workforce, `llm-context`, governance events, etc.).
//!
//! **Product contract:** this store is the canonical world model for a company; optional global
//! `/api/paperclip/*` is demo/experiment — do not treat it as a second source of truth in UIs.
//! See `docs/company-os/world-model-and-intelligence.md`.
//!
//! Enable with `HSM_COMPANY_OS_DATABASE_URL=postgres://...`. Migrations in `migrations/` run on startup.

mod agent_runs;
mod agents;
mod bundle;
mod context_repo;
mod company_memory;
mod company_memory_hybrid;
pub mod intelligence_signals;
pub mod markdown_toc;
mod memory_engine;
mod memory_summaries;
pub mod onboarding_contracts;
mod paperclip_import;
pub mod paperclip_sync;
pub mod self_improvement;
mod spend;
mod store_promotion;
mod tool_catalog;
mod workspace_catalog;
mod workspace_files;

use axum::{
    extract::{Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    response::sse::{Event, KeepAlive, Sse},
    routing::{delete, get, patch, post},
    Json, Router,
};
pub use bundle::{export_bundle, import_bundle as run_import_bundle, CompanyBundle, ImportRequest};
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use futures_util::stream;
pub use spend::spawn_record_llm_spend;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Json as SqlxJson;
use sqlx::{PgPool, Postgres, Transaction};
use std::path::{Path as StdPath, PathBuf};
use uuid::Uuid;

use self::onboarding_contracts::{
    evaluate_gate_results, find_contract, load_contracts_hot, OnboardingGateResult,
};
use crate::console::ConsoleState;
use crate::personal::gateway::{Message, Platform};
use crate::personal::EnhancedPersonalAgent;
use crate::personal::ops_config::{
    heartbeat_state_path, load_heartbeat_state, load_ops_config, resolve_ops_config_path,
    BudgetScope, Ticket,
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
    let max_conns = std::env::var("HSM_COMPANY_OS_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(20);
    let pool = PgPoolOptions::new()
        .max_connections(max_conns)
        .connect(url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(Some(pool))
}

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .merge(agents::router())
        .merge(agent_runs::router())
        .merge(intelligence_signals::router())
        .merge(self_improvement::router())
        .merge(store_promotion::router())
        .merge(tool_catalog::router())
        .merge(workspace_catalog::router())
        .merge(workspace_files::router())
        .merge(context_repo::router())
        .merge(company_memory::router())
        .merge(memory_engine::router())
        .route("/api/company/health", get(company_health))
        .route("/api/company/import", post(import_company_bundle))
        .route(
            "/api/company/onboarding/contracts",
            get(list_onboarding_pack_contracts),
        )
        .route(
            "/api/company/onboarding/contracts/validate",
            post(validate_onboarding_pack_contract),
        )
        .route(
            "/api/company/onboarding/draft",
            post(generate_onboarding_draft),
        )
        .route(
            "/api/company/onboarding/apply",
            post(apply_onboarding_draft),
        )
        .route(
            "/api/company/companies",
            get(list_companies).post(create_company),
        )
        .route(
            "/api/company/companies/:company_id/api-catalog",
            get(company_api_catalog),
        )
        .route(
            "/api/company/companies/:company_id/import-paperclip-home",
            post(import_paperclip_home),
        )
        .route(
            "/api/company/companies/:company_id/sync/paperclip-goals",
            post(sync_paperclip_goals_post),
        )
        .route(
            "/api/company/companies/:company_id/sync/paperclip-dris",
            post(sync_paperclip_dris_post),
        )
        .route(
            "/api/company/companies/:company_id/dri-assignments",
            get(list_dri_assignments).post(create_dri_assignment),
        )
        .route(
            "/api/company/companies/:company_id/dri-assignments/:row_id",
            patch(patch_dri_assignment).delete(delete_dri_assignment),
        )
        .route(
            "/api/company/companies/:company_id/skills",
            get(list_company_skills),
        )
        .route(
            "/api/company/companies/:company_id/yc-bench-profile",
            get(get_company_yc_bench_profile),
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
            "/api/company/companies/:company_id/ops/overview",
            get(company_ops_overview),
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
            "/api/company/companies/:company_id/intelligence/summary",
            get(company_intelligence_summary),
        )
        .route(
            "/api/company/companies/:company_id/projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/api/company/companies/:company_id/projects/:project_id",
            patch(patch_project),
        )
        .route(
            "/api/company/companies/:company_id/issue-labels/seed-defaults",
            post(seed_default_issue_labels),
        )
        .route(
            "/api/company/companies/:company_id/issue-labels",
            get(list_issue_labels).post(create_issue_label),
        )
        .route(
            "/api/company/companies/:company_id/tasks",
            get(list_tasks).post(create_task),
        )
        .route(
            "/api/company/companies/:company_id/tasks/:task_id",
            delete(delete_task),
        )
        .route(
            "/api/company/tasks/:task_id/state",
            patch(patch_task_state),
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
            "/api/company/task-handoffs/:handoff_id/actions/token",
            post(issue_handoff_action_token),
        )
        .route(
            "/api/company/task-handoffs/actions/verify",
            post(verify_handoff_action_token),
        )
        .route("/api/company/runtime/activity", get(get_runtime_activity))
        .route("/api/company/runtime/events/stream", get(stream_runtime_events))
        .route(
            "/api/company/runtime/portability-matrix",
            get(runtime_portability_matrix),
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
        // Register after all longer `companies/:company_id/...` paths so matchit does not
        // bind subpaths (e.g. `/projects`) to this handler (avoids spurious 404 on nested routes).
        .route(
            "/api/company/companies/:company_id",
            get(get_company).patch(patch_company).delete(delete_company),
        )
        .route(
            "/api/company/tasks/:task_id/context",
            patch(patch_task_context),
        )
        .route("/api/company/tasks/:task_id/sla", patch(patch_task_sla))
        .route(
            "/api/company/tasks/:task_id/decision",
            post(post_task_decision),
        )
        .route(
            "/api/company/tasks/:task_id/requires-human",
            post(post_task_requires_human),
        )
        .route("/api/company/tasks/:task_id/checkout", post(checkout_task))
        .route("/api/company/tasks/:task_id/release", post(release_task))
        .route(
            "/api/company/tasks/:task_id/execute-worker",
            post(post_execute_task_worker),
        )
        .route(
            "/api/company/tasks/:task_id/run-telemetry",
            post(post_task_run_telemetry),
        )
        .route(
            "/api/company/tasks/:task_id/stigmergic-note",
            post(post_task_stigmergic_note),
        )
        // ── Agent chat: natural-language Company OS interface ──────────────────
        // POST /api/company/companies/{company_id}/agent-chat
        // Dashboard — running/stuck/overdue tasks, goal coverage, active agents
        .route(
            "/api/company/companies/:company_id/dashboard",
            get(get_company_dashboard),
        )
        // Task event log — full audit trail per task
        .route("/api/company/tasks/:task_id/events", get(get_task_events))
        .route(
            "/api/company/tasks/:task_id/events/stream",
            get(stream_task_events),
        )
        // Takes { message, actor, thread_id? } → runs through EnhancedPersonalAgent with full
        // company context + tool loop. The agent can list tasks, create tasks,
        // dispatch to other agents, search memory, etc. — all from one chat message.
        .route(
            "/api/company/companies/:company_id/agent-chat",
            post(post_agent_chat),
        )
        .layer(axum::middleware::from_fn(require_company_api_auth))
}

fn constant_time_eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut v = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        v |= x ^ y;
    }
    v == 0
}

async fn require_company_api_auth(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let insecure_bypass = std::env::var("HSM_COMPANY_API_ALLOW_NO_AUTH")
        .ok()
        .map(|v| {
            let s = v.trim().to_ascii_lowercase();
            s == "1" || s == "true" || s == "yes" || s == "on"
        })
        .unwrap_or(false);
    let Some(expected) = std::env::var("HSM_COMPANY_API_BEARER_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        if insecure_bypass {
            return Ok(next.run(request).await);
        }
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    if request.uri().path() == "/api/company/health" {
        return Ok(next.run(request).await);
    }
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !constant_time_eq_bytes(token.as_bytes(), expected.as_bytes()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
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
    let inserted = sqlx::query_scalar::<_, bool>(
        r#"INSERT INTO request_idempotency (company_id, scope, idempotency_key, request_hash)
           VALUES ($1,$2,$3,$4)
           ON CONFLICT (company_id, scope, idempotency_key) DO NOTHING
           RETURNING true"#,
    )
    .bind(company_id)
    .bind(scope)
    .bind(idempotency_key)
    .bind(request_hash)
    .fetch_optional(pool)
    .await?;
    Ok(inserted.is_some())
}

pub fn start_automation_worker(pool: PgPool, home: std::path::PathBuf) {
    tokio::spawn(async move {
        loop {
            if let Err(e) = automation_tick(&pool, &home).await {
                tracing::warn!(target: "hsm_company_automation", "automation tick failed: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        }
    });
}

/// Fire-and-forget task event log. Call on every state change, tool call, or note.
async fn emit_task_event(
    pool: &PgPool,
    task_id: Uuid,
    company_id: Uuid,
    event_type: &str,
    actor: &str,
    payload: Value,
) {
    let _ = sqlx::query(
        r#"INSERT INTO task_events (task_id, company_id, event_type, actor, payload)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(task_id)
    .bind(company_id)
    .bind(event_type)
    .bind(actor)
    .bind(SqlxJson(payload))
    .execute(pool)
    .await;
}

/// Compute and store goal-coverage KPI for all companies (run hourly, time-gated in tick).
async fn compute_goal_coverage(pool: &PgPool) {
    let companies: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM companies")
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    for cid in companies {
        let result: Option<(i64, i64)> = sqlx::query_as::<_, (i64, i64)>(
            r#"SELECT COUNT(*)::bigint,
                      COUNT(*) FILTER (WHERE primary_goal_id IS NOT NULL)::bigint
               FROM tasks WHERE company_id=$1
                 AND created_at > now() - interval '7 days'"#,
        )
        .bind(cid)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        let (total, with_goal) = result.unwrap_or((0, 0));
        let pct = if total > 0 { with_goal as f64 / total as f64 * 100.0 } else { 0.0 };

        tracing::info!(
            target: "hsm_company_kpi",
            company_id = %cid,
            coverage_pct = pct,
            total_tasks = total,
            tasks_with_goal = with_goal,
            "goal_coverage"
        );

        let _ = sqlx::query(
            r#"INSERT INTO goal_coverage_stats
               (company_id, total_tasks, tasks_with_goal, coverage_pct)
               VALUES ($1, $2, $3, $4)"#,
        )
        .bind(cid)
        .bind(total as i32)
        .bind(with_goal as i32)
        .bind(pct)
        .execute(pool)
        .await;
    }
}

async fn automation_tick(pool: &PgPool, home: &std::path::Path) -> Result<(), sqlx::Error> {
    enqueue_sla_escalation_jobs(pool).await?;
    process_due_automation_jobs(pool).await?;
    run_auto_revert_checks(pool).await?;
    recover_stale_in_progress_tasks(pool).await;
    auto_dispatch_eligible_tasks(pool, home).await;
    auto_complete_finished_goals(pool).await;
    let _ = self_improvement::maybe_run_weekly_nudges(pool).await;
    // Goal coverage KPI — run hourly, time-gated to avoid redundant writes
    let run_coverage: bool = sqlx::query_scalar::<_, bool>(
        r#"SELECT NOT EXISTS(
               SELECT 1 FROM goal_coverage_stats
               WHERE computed_at > now() - interval '50 minutes'
           )"#,
    )
    .fetch_one(pool)
    .await
    .unwrap_or(true);
    if run_coverage {
        compute_goal_coverage(pool).await;
    }
    Ok(())
}

/// Reset tasks that are stuck in `in_progress` with no active agent run — e.g. after a
/// server restart mid-dispatch. Without this, those tasks are permanently blocked by the
/// `NOT EXISTS(running agent_run)` guard in the eligibility query and never re-dispatched.
/// A 30-minute window avoids racing long-running legitimate agent executions.
async fn recover_stale_in_progress_tasks(pool: &PgPool) {
    let recovered = sqlx::query(
        r#"WITH stale AS (
               SELECT t.id
               FROM tasks t
               WHERE t.state = 'in_progress'
                 AND t.updated_at < now() - interval '30 minutes'
                 AND NOT EXISTS (
                     SELECT 1 FROM agent_runs ar
                     WHERE ar.task_id = t.id AND ar.status = 'running'
                 )
               LIMIT 20
           )
           UPDATE tasks SET state = 'open', updated_at = now()
           WHERE id IN (SELECT id FROM stale)"#,
    )
    .execute(pool)
    .await
    .map(|r| r.rows_affected())
    .unwrap_or(0);

    if recovered > 0 {
        tracing::info!(
            target: "hsm_company_automation",
            count = recovered,
            "recovered stale in_progress tasks → open"
        );
    }
}

/// Mark goals as `done` when every non-cancelled task under them is in a terminal state
/// (`done` or `closed`). Requires at least one task to exist — goals with no tasks at all
/// are not auto-completed. Emits a governance event on each transition.
async fn auto_complete_finished_goals(pool: &PgPool) {
    #[derive(sqlx::FromRow)]
    struct CompletedGoal {
        id: Uuid,
        company_id: Uuid,
    }
    let completed = sqlx::query_as::<_, CompletedGoal>(
        r#"WITH eligible AS (
               SELECT g.id, g.company_id
               FROM goals g
               WHERE g.status = 'active'
                 -- at least one task exists under this goal
                 AND EXISTS (
                     SELECT 1 FROM tasks t WHERE t.primary_goal_id = g.id
                 )
                 -- no task is still in a non-terminal state
                 AND NOT EXISTS (
                     SELECT 1 FROM tasks t
                     WHERE t.primary_goal_id = g.id
                       AND t.state NOT IN ('done', 'closed', 'cancelled')
                 )
                 -- at least one task is done/closed (not all cancelled)
                 AND EXISTS (
                     SELECT 1 FROM tasks t
                     WHERE t.primary_goal_id = g.id
                       AND t.state IN ('done', 'closed')
                 )
           )
           UPDATE goals SET status = 'done', updated_at = now()
           WHERE id IN (SELECT id FROM eligible)
           RETURNING id, company_id"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for row in &completed {
        let goal_id = row.id;
        let company_id = row.company_id;
        tracing::info!(
            target: "hsm_company_automation",
            %goal_id,
            %company_id,
            "goal auto-completed: all tasks terminal"
        );
        let _ = sqlx::query(
            r#"INSERT INTO governance_events
               (company_id, actor, action, subject_type, subject_id, payload, severity)
               VALUES ($1, 'automation_worker', 'goal_auto_completed', 'goal', $2, $3, 'info')"#,
        )
        .bind(company_id)
        .bind(goal_id.to_string())
        .bind(serde_json::json!({ "source": "auto_complete_finished_goals" }))
        .execute(pool)
        .await;
    }
}

/// Auto-dispatch tasks that have an assigned agent (checked_out_by set) but are still
/// in state='open' and have not been picked up by a recent agent run.
/// Limits to 3 tasks per tick across all companies to avoid thundering herd.
async fn auto_dispatch_eligible_tasks(pool: &PgPool, home: &std::path::Path) {
    #[derive(sqlx::FromRow)]
    struct EligibleTask {
        id: Uuid,
        company_id: Uuid,
        title: String,
        checked_out_by: String,
        /// True when the task declares required skills via capability_refs.
        has_capability_refs: bool,
    }
    let eligible = sqlx::query_as::<_, EligibleTask>(
        r#"SELECT t.id, t.company_id, t.title,
                  COALESCE(t.checked_out_by, t.owner_persona) AS checked_out_by,
                  (t.capability_refs IS NOT NULL
                   AND jsonb_typeof(t.capability_refs) = 'array'
                   AND jsonb_array_length(t.capability_refs) > 0) AS has_capability_refs
           FROM tasks t
           WHERE t.state = 'open'
             -- eligible if assigned via checked_out_by or owner_persona
             AND (t.checked_out_by IS NOT NULL OR t.owner_persona IS NOT NULL)
             AND COALESCE(t.checked_out_by, t.owner_persona) <> ''
             -- must be at least 10 seconds old to avoid racing manual dispatch
             AND t.updated_at < now() - interval '10 seconds'
             -- no running agent_run already in-flight for this task
             AND NOT EXISTS (
                 SELECT 1 FROM agent_runs ar
                 WHERE ar.task_id = t.id AND ar.status = 'running'
             )
             -- skip tasks flagged for human approval
             AND t.requires_human = false
           ORDER BY t.priority DESC, t.updated_at ASC
           LIMIT 3"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for task_meta in eligible {
        let task_id = task_meta.id;
        let actor = task_meta.checked_out_by.clone();

        // Atomically claim: only proceed if still open
        // Note: RETURNING true (BOOLEAN) avoids integer type-width mismatch with sqlx.
        let claimed: Option<bool> = sqlx::query_scalar(
            "UPDATE tasks SET state = 'in_progress', updated_at = now()
             WHERE id = $1 AND state = 'open' RETURNING true",
        )
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

        if claimed != Some(true) {
            continue; // already picked up by someone else
        }

        tracing::info!(
            target: "hsm_company_automation",
            task_id = %task_id,
            actor = %actor,
            title = %task_meta.title,
            "auto-dispatching eligible task"
        );

        // Skill-based routing: if the task declares required skills, warn when the
        // assigned agent has no matching entries in company_skills. We don't block
        // dispatch (to avoid starving tasks) but the log makes mismatches visible.
        if task_meta.has_capability_refs {
            let match_count: i64 = sqlx::query_scalar(
                r#"SELECT COUNT(*)::bigint
                   FROM company_skills cs
                   WHERE cs.company_id = $1
                     AND cs.slug IS NOT NULL
                     AND EXISTS (
                         SELECT 1 FROM tasks t
                         WHERE t.id = $2
                           AND t.capability_refs @> jsonb_build_array(
                                 jsonb_build_object('kind','skill','ref',cs.slug)
                               )
                     )"#,
            )
            .bind(task_meta.company_id)
            .bind(task_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0);

            if match_count == 0 {
                tracing::warn!(
                    target: "hsm_company_dispatch",
                    task_id = %task_id,
                    actor = %actor,
                    "task has capability_refs but assigned agent has no matching skills"
                );
            } else {
                tracing::debug!(
                    target: "hsm_company_dispatch",
                    task_id = %task_id,
                    actor = %actor,
                    matching_skills = match_count,
                    "skill match verified for task dispatch"
                );
            }
        }

        // Create agent_runs record — no external_run_id to avoid unique-constraint
        // conflicts on retries; each dispatch gets a fresh UUID.
        let run_id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO agent_runs (company_id, task_id, external_system, status)
               VALUES ($1, $2, 'hsm_auto', 'running')
               RETURNING id"#,
        )
        .bind(task_meta.company_id)
        .bind(task_id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(target: "hsm_company_automation",
                "auto-dispatch task {task_id}: agent_runs insert failed: {e}");
            Uuid::new_v4()
        });

        // Build full task row for context prefix
        let task_row = sqlx::query_as::<_, ExecuteTaskRow>(
            r#"SELECT title, specification, checked_out_by,
                      company_id, goal_ancestry, context_notes, capability_refs,
                      primary_goal_id, parent_task_id
               FROM tasks WHERE id = $1"#,
        )
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

        let Some(task_row) = task_row else { continue };

        let context_prefix = build_worker_context_prefix(pool, &task_row, &actor).await;
        let base_task = format!(
            "Execute this task with real tools and return what was done.\n\nTitle: {}\n\nSpecification:\n{}",
            task_row.title,
            task_row.specification.as_deref().unwrap_or("(no specification)")
        );
        let exec_identity = format!(
            "## Your execution identity\n\n\
             - **Actor (you)**: {actor}\n\
             - **company_id**: {company_id}\n\
             - **task_id**: {task_id}\n\
             - **run_id**: {run_id}\n\n\
             When calling company OS tools always pass `company_id`, `task_id`, and `run_id` \
             explicitly using the values above.\n\n\
             ## Available company OS tools\n\
             - `company_list_tasks` — see open/in-progress work across the company\n\
             - `company_create_task` — spawn a subtask and optionally assign it to another agent via `checked_out_by`\n\
             - `company_update_task` — change task state (done/blocked/open) or append a stigmergic note\n\
             - `company_memory_search` / `company_memory_append` — shared + private memory pool\n\
             - `company_task_requires_human` — escalate to human inbox when blocked\n\
             - `company_tool_discover` / `company_tool_describe` / `company_tool_call` — tool catalog\n\n",
            actor = actor,
            company_id = task_meta.company_id,
            task_id = task_id,
            run_id = run_id,
        );
        let prompt = if context_prefix.is_empty() {
            format!("/hermes {exec_identity}\n---\n\n## Your task\n\n{base_task}")
        } else {
            format!("/hermes {context_prefix}{exec_identity}\n---\n\n## Your task\n\n{base_task}")
        };

        let pool_bg = pool.clone();
        let home_bg = home.to_path_buf();
        let actor_bg = actor.clone();
        let company_id_bg = task_meta.company_id;

        tokio::spawn(async move {
            // ── SFT capture setup ─────────────────────────────────────────────
            let sft_capture = if crate::sft::capture_enabled() {
                let cap = crate::sft::SftCapture::new(&actor_bg);
                cap.0.lock().await.task_id = Some(task_id.to_string());
                cap.0.lock().await.run_id = Some(run_id.to_string());
                cap.0.lock().await.company_id = Some(company_id_bg.to_string());
                Some(cap)
            } else {
                None
            };

            let (run_status, agent_reply) =
                match EnhancedPersonalAgent::initialize(&home_bg).await {
                    Ok(mut agent) => {
                        // Attach capture handle before execution
                        agent.sft_capture = sft_capture.clone();

                        let msg = Message {
                            id: format!("auto-{task_id}"),
                            platform: Platform::Web,
                            channel_id: "company-os-auto".to_string(),
                            channel_name: Some("company-os-auto".to_string()),
                            user_id: actor_bg.clone(),
                            user_name: actor_bg.clone(),
                            content: prompt,
                            timestamp: Utc::now(),
                            attachments: Vec::new(),
                            reply_to: None,
                        };
                        match agent.handle_message(msg).await {
                            Ok(reply) => ("success".to_string(), Some(reply)),
                            Err(e) => {
                                tracing::warn!(target: "hsm_company_automation",
                                    "auto-dispatch task {task_id} agent error: {e}");
                                ("error".to_string(), None)
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(target: "hsm_company_automation",
                            "auto-dispatch task {task_id} agent init failed: {e}");
                        ("error".to_string(), None)
                    }
                };

            // ── SFT capture teardown — write trace before DB updates ──────────
            if let Some(cap) = sft_capture {
                let trace = cap.finish(&run_status).await;
                let trace_path = home_bg.join("memory/sft_traces.jsonl");
                if let Err(e) = crate::sft::write_trace(&trace_path, &trace).await {
                    tracing::warn!(target: "hsm_sft", "failed to write sft trace: {e}");
                } else {
                    tracing::debug!(target: "hsm_sft",
                        trace_id = %trace.id,
                        tool_calls = trace.tool_calls_count,
                        outcome = %trace.outcome,
                        "sft trace written");
                }
            }

            let ar_status = if run_status == "success" { "success" } else { "error" };
            let _ = sqlx::query(
                "UPDATE agent_runs SET status = $1, finished_at = now() WHERE id = $2",
            )
            .bind(ar_status)
            .bind(run_id)
            .execute(&pool_bg)
            .await;

            if run_status == "success" {
                let _ = sqlx::query(
                    "UPDATE tasks SET state = 'done', updated_at = now() WHERE id = $1",
                )
                .bind(task_id)
                .execute(&pool_bg)
                .await;

                if let Some(reply) = agent_reply {
                    let summary = truncate_worker_log(&reply, 600);
                    let note = serde_json::json!({
                        "text": summary,
                        "actor": actor_bg,
                        "at": Utc::now().to_rfc3339(),
                        "auto": true,
                    });
                    let _ = sqlx::query(
                        "UPDATE tasks SET context_notes = context_notes || $1::jsonb, updated_at = now() WHERE id = $2",
                    )
                    .bind(serde_json::json!([note]))
                    .bind(task_id)
                    .execute(&pool_bg)
                    .await;
                }

                let _ = sqlx::query(
                    r#"INSERT INTO governance_events
                       (company_id, actor, action, subject_type, subject_id, payload, severity)
                       VALUES ($1, $2, 'task_completed', 'task', $3, $4, 'info')"#,
                )
                .bind(company_id_bg)
                .bind(&actor_bg)
                .bind(task_id.to_string())
                .bind(serde_json::json!({ "source": "auto_dispatch" }))
                .execute(&pool_bg)
                .await;
            } else {
                // Revert to open so it can be retried; leave a stigmergic note so the
                // next agent knows what was attempted and why it failed.
                let _ = sqlx::query(
                    "UPDATE tasks SET state = 'open', updated_at = now() WHERE id = $1",
                )
                .bind(task_id)
                .execute(&pool_bg)
                .await;

                let failure_note = serde_json::json!({
                    "text": "Agent run failed — no output produced. Will retry on next dispatch cycle.",
                    "actor": actor_bg,
                    "at": Utc::now().to_rfc3339(),
                    "auto": true,
                    "error": true,
                });
                let _ = sqlx::query(
                    "UPDATE tasks SET context_notes = context_notes || $1::jsonb, updated_at = now() WHERE id = $2",
                )
                .bind(serde_json::json!([failure_note]))
                .bind(task_id)
                .execute(&pool_bg)
                .await;
            }
        });
    }
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
        let claimed: Option<bool> = sqlx::query_scalar(
            "UPDATE automation_jobs SET status = 'running', updated_at = NOW() WHERE id = $1 AND status = 'pending' RETURNING true",
        )
        .bind(job_id)
        .fetch_optional(pool)
        .await?;
        if claimed != Some(true) {
            continue;
        }

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

async fn run_sla_escalation_job(
    pool: &PgPool,
    company_id: Uuid,
    payload: &Value,
) -> Result<(), String> {
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
            let reason = format!(
                "auto-revert: regression {:.2}% > {:.2}%",
                current, threshold
            );
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
        let row: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT parent_goal_id FROM goals WHERE id = $1 AND company_id = $2")
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
                  context_markdown, webhook_url, created_at::text
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
                  context_markdown, webhook_url, created_at::text
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
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
                  context_markdown, webhook_url, created_at::text
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
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
                     context_markdown, webhook_url, created_at::text"#,
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
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

#[derive(Deserialize, Default)]
struct SyncPaperclipGoalsBody {
    #[serde(default)]
    goals: Option<Vec<crate::paperclip::goal::Goal>>,
}

/// One-way sync: Paperclip in-memory goals → Postgres `goals` (`paperclip_goal_id` + snapshot). Body optional when `ConsoleState` carries a Paperclip layer (`hsm_console` with intelligence).
async fn sync_paperclip_goals_post(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<SyncPaperclipGoalsBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let goals: Vec<crate::paperclip::goal::Goal> = if let Some(g) = body.goals {
        g
    } else if let Some(ref il) = st.paperclip {
        let layer = il.lock().await;
        layer.list_goals().iter().map(|x| (*x).clone()).collect()
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "No in-process Paperclip layer. POST { \"goals\": … } from GET /api/paperclip/goals (same process as IntelligenceLayer) or run hsm_console with intelligence enabled."
            })),
        ));
    };

    let report = paperclip_sync::sync_paperclip_goals(pool, company_id, goals)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;

    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, 'sync', 'paperclip_goals_synced', 'company', $2, $3, 'info')"#,
    )
    .bind(company_id)
    .bind(company_id.to_string())
    .bind(SqlxJson(report.clone()))
    .execute(pool)
    .await;

    Ok(Json(json!({ "ok": true, "report": report })))
}

#[derive(Deserialize, Default)]
struct SyncPaperclipDrisBody {
    #[serde(default)]
    dris: Option<Vec<crate::paperclip::dri::DriEntry>>,
}

async fn sync_paperclip_dris_post(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<SyncPaperclipDrisBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let entries: Vec<crate::paperclip::dri::DriEntry> = if let Some(d) = body.dris {
        d
    } else if let Some(ref il) = st.paperclip {
        let layer = il.lock().await;
        layer.dri_registry.all().cloned().collect()
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "No in-process Paperclip layer. POST { \"dris\": … } from GET /api/paperclip/dris or run hsm_console with intelligence enabled."
            })),
        ));
    };

    let report = paperclip_sync::sync_paperclip_dris(pool, company_id, entries)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;

    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, 'sync', 'paperclip_dris_synced', 'company', $2, $3, 'info')"#,
    )
    .bind(company_id)
    .bind(company_id.to_string())
    .bind(SqlxJson(report.clone()))
    .execute(pool)
    .await;

    Ok(Json(json!({ "ok": true, "report": report })))
}

#[derive(sqlx::FromRow, Serialize)]
struct DriAssignmentRow {
    id: Uuid,
    company_id: Uuid,
    dri_key: String,
    display_name: String,
    agent_ref: String,
    domains: Vec<String>,
    authority: SqlxJson<Value>,
    tenure_kind: String,
    valid_from: Option<chrono::DateTime<chrono::Utc>>,
    valid_until: Option<chrono::DateTime<chrono::Utc>>,
    paperclip_dri_id: Option<String>,
    created_at: String,
    updated_at: String,
}

async fn list_dri_assignments(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, DriAssignmentRow>(
        r#"SELECT id, company_id, dri_key, display_name, agent_ref, domains, authority,
                  tenure_kind, valid_from, valid_until, paperclip_dri_id,
                  created_at::text, updated_at::text
           FROM dri_assignments WHERE company_id = $1 ORDER BY dri_key"#,
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
    Ok(Json(json!({ "dri_assignments": rows })))
}

#[derive(Deserialize)]
struct CreateDriAssignmentBody {
    dri_key: String,
    display_name: String,
    agent_ref: String,
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    authority: Value,
    #[serde(default)]
    tenure_kind: String,
    valid_from: Option<chrono::DateTime<chrono::Utc>>,
    valid_until: Option<chrono::DateTime<chrono::Utc>>,
}

async fn create_dri_assignment(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateDriAssignmentBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let key = body.dri_key.trim().to_string();
    if key.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "dri_key required" })),
        ));
    }
    let name = body.display_name.trim().to_string();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "display_name required" })),
        ));
    }
    let agent = body.agent_ref.trim().to_string();
    if agent.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "agent_ref required" })),
        ));
    }
    let tk = body.tenure_kind.trim().to_ascii_lowercase();
    let tenure_kind = if tk.is_empty() {
        "persistent".to_string()
    } else {
        tk
    };
    if !matches!(tenure_kind.as_str(), "persistent" | "time_bound") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "tenure_kind must be persistent|time_bound" })),
        ));
    }

    let row = sqlx::query_as::<_, DriAssignmentRow>(
        r#"INSERT INTO dri_assignments (
            company_id, dri_key, display_name, agent_ref, domains, authority,
            tenure_kind, valid_from, valid_until, updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, $8, $9, NOW())
        RETURNING id, company_id, dri_key, display_name, agent_ref, domains, authority,
                  tenure_kind, valid_from, valid_until, paperclip_dri_id,
                  created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&key)
    .bind(&name)
    .bind(&agent)
    .bind(&body.domains)
    .bind(SqlxJson(body.authority))
    .bind(&tenure_kind)
    .bind(body.valid_from)
    .bind(body.valid_until)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(ref d) = e {
            if d.constraint() == Some("uq_dri_assignments_company_key") {
                return (
                    StatusCode::CONFLICT,
                    Json(json!({ "error": "dri_key already exists for company" })),
                );
            }
        }
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok((StatusCode::CREATED, Json(json!({ "dri_assignment": row }))))
}

#[derive(Deserialize, Default)]
struct PatchDriAssignmentBody {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    agent_ref: Option<String>,
    #[serde(default)]
    domains: Option<Vec<String>>,
    #[serde(default)]
    authority: Option<Value>,
    #[serde(default)]
    tenure_kind: Option<String>,
    #[serde(default)]
    valid_from: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    valid_until: Option<Option<chrono::DateTime<chrono::Utc>>>,
}

async fn patch_dri_assignment(
    State(st): State<ConsoleState>,
    Path((company_id, row_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchDriAssignmentBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let cur = sqlx::query_as::<_, DriAssignmentRow>(
        r#"SELECT id, company_id, dri_key, display_name, agent_ref, domains, authority,
                  tenure_kind, valid_from, valid_until, paperclip_dri_id,
                  created_at::text, updated_at::text
           FROM dri_assignments WHERE id = $1 AND company_id = $2"#,
    )
    .bind(row_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(cur) = cur else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "dri_assignment not found" })),
        ));
    };

    let display_name = body
        .display_name
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(cur.display_name);
    let agent_ref = body
        .agent_ref
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(cur.agent_ref);
    let domains = body.domains.unwrap_or(cur.domains);
    let authority = SqlxJson(body.authority.unwrap_or_else(|| cur.authority.0.clone()));
    let tenure_kind = body
        .tenure_kind
        .as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or(cur.tenure_kind.clone());
    if !matches!(tenure_kind.as_str(), "persistent" | "time_bound") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "tenure_kind must be persistent|time_bound" })),
        ));
    }
    let valid_from = body.valid_from.or(cur.valid_from);
    let valid_until = match body.valid_until {
        None => cur.valid_until,
        Some(None) => None,
        Some(Some(t)) => Some(t),
    };

    let row = sqlx::query_as::<_, DriAssignmentRow>(
        r#"UPDATE dri_assignments SET
            display_name = $3,
            agent_ref = $4,
            domains = $5,
            authority = $6::jsonb,
            tenure_kind = $7,
            valid_from = $8,
            valid_until = $9,
            updated_at = NOW()
           WHERE id = $1 AND company_id = $2
           RETURNING id, company_id, dri_key, display_name, agent_ref, domains, authority,
                     tenure_kind, valid_from, valid_until, paperclip_dri_id,
                     created_at::text, updated_at::text"#,
    )
    .bind(row_id)
    .bind(company_id)
    .bind(&display_name)
    .bind(&agent_ref)
    .bind(&domains)
    .bind(authority)
    .bind(&tenure_kind)
    .bind(valid_from)
    .bind(valid_until)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({ "dri_assignment": row })))
}

async fn delete_dri_assignment(
    State(st): State<ConsoleState>,
    Path((company_id, row_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let r = sqlx::query("DELETE FROM dri_assignments WHERE id = $1 AND company_id = $2")
        .bind(row_id)
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
            Json(json!({ "error": "dri_assignment not found" })),
        ));
    }
    Ok((
        StatusCode::OK,
        Json(json!({ "deleted": true, "id": row_id })),
    ))
}

#[derive(sqlx::FromRow, Serialize)]
struct CompanySkillRow {
    id: Uuid,
    company_id: Uuid,
    slug: String,
    name: String,
    description: String,
    body: String,
    skill_path: String,
    source: String,
    updated_at: String,
}

#[derive(Serialize)]
struct YcBenchDomainScore {
    domain: String,
    score: f64,
    matched_terms: Vec<String>,
    evidence: Vec<String>,
}

#[derive(Serialize)]
struct YcBenchAgentHint {
    id: String,
    display_name: String,
    role: String,
    matched_domains: Vec<String>,
}

#[derive(Serialize)]
struct YcBenchProfileSource {
    agent_count: usize,
    skill_count: usize,
    has_context_markdown: bool,
}

#[derive(Serialize)]
struct YcBenchBenchmarkSpecTemplate {
    labels: Value,
    setup_commands: Vec<Vec<String>>,
    command: Vec<String>,
    cwd_hint: String,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct CompanyYcBenchProfile {
    company_id: Uuid,
    slug: String,
    display_name: String,
    issue_key_prefix: String,
    generated_at: String,
    source: YcBenchProfileSource,
    top_domains: Vec<String>,
    domain_scores: Vec<YcBenchDomainScore>,
    agent_hints: Vec<YcBenchAgentHint>,
    imported_skills: Vec<String>,
    strategy_summary: String,
    controller_prompt: String,
    benchmark_spec: YcBenchBenchmarkSpecTemplate,
}

const YC_BENCH_DOMAINS: &[(&str, &[&str])] = &[
    (
        "research",
        &[
            "research",
            "r&d",
            "discovery",
            "prototype",
            "experimentation",
            "roadmap",
            "strategy",
        ],
    ),
    (
        "inference",
        &[
            "inference",
            "serving",
            "customer",
            "ops",
            "latency",
            "deployment",
            "support",
            "reliability",
            "operations",
        ],
    ),
    (
        "data_environment",
        &[
            "data",
            "dataset",
            "pipeline",
            "etl",
            "environment",
            "annotation",
            "intake",
            "integration",
            "analytics",
        ],
    ),
    (
        "training",
        &[
            "training",
            "fine-tune",
            "finetune",
            "model",
            "evaluation",
            "benchmark",
            "optimizer",
            "tuning",
            "weights",
        ],
    ),
];

async fn list_company_skills(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let company_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1)")
            .bind(company_id)
            .fetch_one(pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
    if !company_exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
    }

    let rows = sqlx::query_as::<_, CompanySkillRow>(
        r#"SELECT id, company_id, slug, name, description, body, skill_path, source, updated_at::text
           FROM company_skills
           WHERE company_id = $1
           ORDER BY lower(name), lower(slug)"#,
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

    Ok(Json(json!({ "skills": rows })))
}

fn score_domain_text(
    domain: &str,
    terms: &[&str],
    raw_text: &str,
    weight: f64,
    matched_terms: &mut std::collections::BTreeSet<String>,
    evidence: &mut Vec<String>,
) -> f64 {
    let text = raw_text.trim();
    if text.is_empty() {
        return 0.0;
    }
    let lower = text.to_lowercase();
    let mut score = 0.0;
    for term in terms {
        if lower.contains(term) {
            matched_terms.insert((*term).to_string());
            score += weight;
            if evidence.len() < 5 {
                evidence.push(format!(
                    "{domain}: {}",
                    text.chars().take(160).collect::<String>()
                ));
            }
        }
    }
    score
}

fn compute_domain_scores(
    company: &CompanyRow,
    agents: &[agents::CompanyAgentRow],
    skills: &[CompanySkillRow],
) -> Vec<YcBenchDomainScore> {
    let mut scores = Vec::new();
    for (domain, terms) in YC_BENCH_DOMAINS {
        let mut score = 0.0;
        let mut matched_terms = std::collections::BTreeSet::new();
        let mut evidence = Vec::new();

        score += score_domain_text(
            domain,
            terms,
            &format!("{} {}", company.display_name, company.slug),
            0.6,
            &mut matched_terms,
            &mut evidence,
        );
        if let Some(context) = company.context_markdown.as_deref() {
            for line in context.lines().take(36) {
                score +=
                    score_domain_text(domain, terms, line, 0.9, &mut matched_terms, &mut evidence);
            }
        }
        for agent in agents {
            score += score_domain_text(
                domain,
                terms,
                &agent.name,
                0.6,
                &mut matched_terms,
                &mut evidence,
            );
            if let Some(title) = agent.title.as_deref() {
                score +=
                    score_domain_text(domain, terms, title, 1.2, &mut matched_terms, &mut evidence);
            }
            if let Some(capabilities) = agent.capabilities.as_deref() {
                score += score_domain_text(
                    domain,
                    terms,
                    capabilities,
                    1.4,
                    &mut matched_terms,
                    &mut evidence,
                );
            }
            if let Some(briefing) = agent.briefing.as_deref() {
                for line in briefing.lines().take(10) {
                    score += score_domain_text(
                        domain,
                        terms,
                        line,
                        1.1,
                        &mut matched_terms,
                        &mut evidence,
                    );
                }
            }
        }
        for skill in skills {
            score += score_domain_text(
                domain,
                terms,
                &skill.slug,
                0.9,
                &mut matched_terms,
                &mut evidence,
            );
            score += score_domain_text(
                domain,
                terms,
                &skill.name,
                1.1,
                &mut matched_terms,
                &mut evidence,
            );
            score += score_domain_text(
                domain,
                terms,
                &skill.description,
                1.2,
                &mut matched_terms,
                &mut evidence,
            );
            for line in skill.body.lines().take(12) {
                score +=
                    score_domain_text(domain, terms, line, 0.8, &mut matched_terms, &mut evidence);
            }
        }

        scores.push(YcBenchDomainScore {
            domain: (*domain).to_string(),
            score: (score * 10.0).round() / 10.0,
            matched_terms: matched_terms.into_iter().collect(),
            evidence,
        });
    }

    scores.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.domain.cmp(&b.domain))
    });
    scores
}

fn collect_agent_hints(
    agents: &[agents::CompanyAgentRow],
    domain_scores: &[YcBenchDomainScore],
) -> Vec<YcBenchAgentHint> {
    let top_domains: Vec<String> = domain_scores
        .iter()
        .take(3)
        .map(|d| d.domain.clone())
        .collect();
    let top_terms: std::collections::BTreeMap<String, Vec<String>> = domain_scores
        .iter()
        .take(3)
        .map(|d| (d.domain.clone(), d.matched_terms.clone()))
        .collect();

    let mut hints = Vec::new();
    for agent in agents.iter().take(8) {
        let text = format!(
            "{} {} {} {}",
            agent.name,
            agent.role,
            agent.title.as_deref().unwrap_or(""),
            agent.capabilities.as_deref().unwrap_or("")
        )
        .to_lowercase();
        let mut matched_domains = Vec::new();
        for domain in &top_domains {
            if let Some(terms) = top_terms.get(domain) {
                if terms.iter().any(|term| text.contains(term)) {
                    matched_domains.push(domain.clone());
                }
            }
        }
        hints.push(YcBenchAgentHint {
            id: agent.name.clone(),
            display_name: agent.title.clone().unwrap_or_else(|| agent.name.clone()),
            role: agent.role.clone(),
            matched_domains,
        });
    }
    hints
}

fn build_yc_bench_strategy_summary(
    company: &CompanyRow,
    domain_scores: &[YcBenchDomainScore],
    skills: &[CompanySkillRow],
) -> String {
    let top_domains = domain_scores
        .iter()
        .take(2)
        .map(|d| format!("{} ({:.1})", d.domain, d.score))
        .collect::<Vec<_>>()
        .join(", ");
    let top_skills = skills
        .iter()
        .take(4)
        .map(|skill| {
            if skill.name.trim().is_empty() {
                skill.slug.clone()
            } else {
                skill.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let context_hint = company
        .context_markdown
        .as_deref()
        .and_then(|ctx| ctx.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or("Operate with disciplined capital allocation and explicit role ownership.");
    format!(
        "{} appears strongest in {}. Imported operating skills: {}. Context anchor: {}",
        company.display_name,
        top_domains,
        if top_skills.is_empty() {
            "none yet".to_string()
        } else {
            top_skills
        },
        context_hint.chars().take(220).collect::<String>()
    )
}

fn build_yc_bench_controller_prompt(
    company: &CompanyRow,
    domain_scores: &[YcBenchDomainScore],
    agents: &[agents::CompanyAgentRow],
    skills: &[CompanySkillRow],
) -> String {
    let top_domains = domain_scores
        .iter()
        .take(3)
        .map(|d| {
            format!(
                "- {}: score {:.1}, matched {}",
                d.domain,
                d.score,
                d.matched_terms.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let top_agents = agents
        .iter()
        .take(6)
        .map(|agent| {
            format!(
                "- {} ({}){}{}",
                agent.title.clone().unwrap_or_else(|| agent.name.clone()),
                agent.role,
                agent
                    .capabilities
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| format!(" | capabilities: {}", s.trim()))
                    .unwrap_or_default(),
                agent
                    .briefing
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| format!(" | briefing: {}", s.lines().next().unwrap_or("").trim()))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let top_skills = skills
        .iter()
        .take(8)
        .map(|skill| {
            format!(
                "- {} [{}]{}",
                if skill.name.trim().is_empty() {
                    &skill.slug
                } else {
                    &skill.name
                },
                skill.slug,
                if skill.description.trim().is_empty() {
                    String::new()
                } else {
                    format!(" | {}", skill.description.trim())
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let context_excerpt = company
        .context_markdown
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(1200).collect::<String>())
        .unwrap_or_else(|| "No company-wide context markdown imported yet.".to_string());

    format!(
        "You are the CEO/controller for {} in YC-Bench.\n\
Operate like this company would operate, not like a generic startup agent.\n\n\
Top strategic domains:\n{}\n\n\
Workforce hints:\n{}\n\n\
Imported operating skills:\n{}\n\n\
Company context excerpt:\n{}\n\n\
Execution rules:\n\
- Prefer decisions aligned with the strongest domains above.\n\
- Assign work according to the imported workforce roles instead of spreading tasks blindly.\n\
- Use the skill templates as the operating playbook when deciding what to accept, dispatch, or cancel.\n\
- Protect runway and prestige together; do not chase short-term revenue that violates the company profile.\n\
- Keep a compact scratchpad of observed employee strengths and task economics.",
        company.display_name,
        top_domains,
        if top_agents.is_empty() {
            "- No imported agents yet.".to_string()
        } else {
            top_agents
        },
        if top_skills.is_empty() {
            "- No imported skills yet.".to_string()
        } else {
            top_skills
        },
        context_excerpt
    )
}

/// Loads company + workforce + skills for YC-Bench / vision alignment (same inputs as [`get_company_yc_bench_profile`]).
async fn load_yc_bench_profile_inputs(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<Option<(CompanyRow, Vec<agents::CompanyAgentRow>, Vec<CompanySkillRow>)>, sqlx::Error> {
    let company = sqlx::query_as::<_, CompanyRow>(
        r#"SELECT id, slug, display_name, hsmii_home, issue_key_prefix,
                  context_markdown, webhook_url, created_at::text
           FROM companies
           WHERE id = $1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;

    let Some(company) = company else {
        return Ok(None);
    };

    let agents = sqlx::query_as::<_, agents::CompanyAgentRow>(
        r#"SELECT id, company_id, name, role, title, capabilities, reports_to, adapter_type,
                  adapter_config, budget_monthly_cents, briefing, status, sort_order,
                  created_at::text, updated_at::text
           FROM company_agents
           WHERE company_id = $1
           ORDER BY sort_order, lower(name)"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let skills = sqlx::query_as::<_, CompanySkillRow>(
        r#"SELECT id, company_id, slug, name, description, body, skill_path, source, updated_at::text
           FROM company_skills
           WHERE company_id = $1
           ORDER BY lower(name), lower(slug)"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    Ok(Some((company, agents, skills)))
}

/// Markdown block injected into [`crate::company_os::agents::get_task_llm_context`] so workforce LLMs see an explicit **vision alignment** summary (YC-Bench strategy parity).
///
/// Disable with `HSM_VISION_ALIGNMENT_LLM_ADDON=0` (see agents module).
pub async fn build_llm_vision_alignment_addon(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<(String, usize), sqlx::Error> {
    let Some((company, agents, skills)) = load_yc_bench_profile_inputs(pool, company_id).await? else {
        return Ok((String::new(), 0));
    };
    let domain_scores = compute_domain_scores(&company, &agents, &skills);
    let strategy = build_yc_bench_strategy_summary(&company, &domain_scores, &skills);
    let domains_line: String = domain_scores
        .iter()
        .filter(|d| d.score > 0.0)
        .take(4)
        .map(|d| format!("{} ({:.1})", d.domain, d.score))
        .collect::<Vec<_>>()
        .join(", ");
    let mut s = String::new();
    s.push_str("## Vision alignment (company)\n\n");
    s.push_str(
        "Ground task outputs in **Company-wide context** (above) when present. The following is the same **strategy snapshot** used for YC-Bench / marketplace profiling.\n\n",
    );
    s.push_str("### Strategy snapshot\n\n");
    s.push_str(&strategy);
    s.push_str("\n\n");
    if !domains_line.is_empty() {
        s.push_str(&format!("**Strategic domain signals:** {domains_line}\n\n"));
    }
    s.push_str(
        "**Explicit alignment:** Prefer outcomes, constraints, and vocabulary consistent with this strategy, the company’s workforce agents, and imported skill templates. If instructions conflict, defer to Company-wide context and operator-visible policies.\n\n",
    );
    const MAX: usize = 8 * 1024;
    if s.len() > MAX {
        s.truncate(MAX);
        s.push_str("\n… [truncated]\n");
    }
    let bytes = s.len();
    Ok((s, bytes))
}

async fn get_company_yc_bench_profile(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let (company, agents, skills) = load_yc_bench_profile_inputs(pool, company_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "company not found" })),
            )
        })?;

    let domain_scores = compute_domain_scores(&company, &agents, &skills);
    let top_domains = domain_scores
        .iter()
        .take(3)
        .filter(|d| d.score > 0.0)
        .map(|d| d.domain.clone())
        .collect::<Vec<_>>();
    let agent_hints = collect_agent_hints(&agents, &domain_scores);
    let strategy_summary = build_yc_bench_strategy_summary(&company, &domain_scores, &skills);
    let controller_prompt =
        build_yc_bench_controller_prompt(&company, &domain_scores, &agents, &skills);
    let imported_skills = skills
        .iter()
        .take(12)
        .map(|skill| {
            if skill.name.trim().is_empty() {
                skill.slug.clone()
            } else {
                skill.name.clone()
            }
        })
        .collect::<Vec<_>>();
    let marketplace_slug = company
        .hsmii_home
        .as_deref()
        .and_then(|home| {
            home.replace('\\', "/")
                .split('/')
                .filter(|segment| !segment.trim().is_empty())
                .next_back()
                .map(|segment| segment.trim().to_string())
        })
        .filter(|segment| !segment.is_empty())
        .unwrap_or_else(|| company.slug.clone());

    let profile = CompanyYcBenchProfile {
        company_id: company.id,
        slug: company.slug.clone(),
        display_name: company.display_name.clone(),
        issue_key_prefix: company.issue_key_prefix.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        source: YcBenchProfileSource {
            agent_count: agents.len(),
            skill_count: skills.len(),
            has_context_markdown: company
                .context_markdown
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some(),
        },
        top_domains: top_domains.clone(),
        domain_scores,
        agent_hints,
        imported_skills,
        strategy_summary,
        controller_prompt,
        benchmark_spec: YcBenchBenchmarkSpecTemplate {
            labels: json!({
                "benchmark": "yc_bench",
                "company_pack": company.slug,
                "marketplace_slug": marketplace_slug,
                "workspace_slug": company.slug,
                "workspace_name": company.display_name,
                "top_domains": top_domains,
            }),
            setup_commands: vec![vec!["uv".into(), "sync".into()]],
            command: vec![
                "uv".into(),
                "run".into(),
                "yc-bench".into(),
                "run".into(),
                "--model".into(),
                "YOUR_MODEL".into(),
                "--seed".into(),
                "1".into(),
                "--config".into(),
                "medium".into(),
            ],
            cwd_hint: "/ABS/PATH/TO/yc-bench".to_string(),
            notes: vec![
                "Inject controller_prompt into your YC-Bench wrapper or model system prompt."
                    .to_string(),
                "Run the same seed and config across marketplace companies for head-to-head comparison."
                    .to_string(),
                "Persist labels.company_pack so marketplace overlays can attribute scores to the source company."
                    .to_string(),
            ],
        },
    };

    Ok(Json(json!({ "profile": profile })))
}

/// Static catalog of Company OS HTTP routes. `{company_id}` / `{task_id}` are placeholders.
fn company_os_api_catalog_endpoints() -> Value {
    json!([
        { "scope": "company", "methods": ["GET", "PATCH", "DELETE"], "path": "/api/company/companies/{company_id}?confirm_slug=", "summary": "Company record; PATCH updates context_markdown, display_name, hsmii_home; DELETE removes workspace and cascades (confirm_slug must match slug)" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/api-catalog", "summary": "Discovery: this list + company" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/import-paperclip-home", "summary": "Import pack agents + nested skills/**/SKILL.md; merges HSM_SKILL_EXTERNAL_DIRS (e.g. hermes-agent/skills) then pack overrides by slug" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/workspace/list?path=", "summary": "List files/dirs under hsmii_home (relative path); Paperclip pack browser" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/workspace/mkdir", "summary": "Create directory tree JSON body { path } under hsmii_home (mkdir -p)" },
        { "scope": "company", "methods": ["GET", "PUT", "DELETE"], "path": "/api/company/companies/{company_id}/workspace/file?path=", "summary": "Read (UTF-8 text or base64+binary), write, or delete a file under hsmii_home" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/workspace/file/delete", "summary": "Delete file JSON body { path } — use when DELETE returns 405 via proxy" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/workspace/file/trash", "summary": "Move file to .recycle/ (soft delete) JSON body { path }" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/sync/paperclip-goals", "summary": "Upsert in-memory Paperclip goals into Postgres goals (paperclip_goal_id); optional JSON body { goals }" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/sync/paperclip-dris", "summary": "Upsert Paperclip dri_registry into dri_assignments; optional JSON body { dris }" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/dri-assignments", "summary": "List / create org-level DRI assignments" },
        { "scope": "company", "methods": ["PATCH", "DELETE"], "path": "/api/company/companies/{company_id}/dri-assignments/{row_id}", "summary": "Update or delete a DRI assignment row" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/skills", "summary": "Imported skill templates saved from pack skills/<slug>/SKILL.md files" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/skills/bank", "summary": "Skill bank: current company skills, agent-linked usage, and recommended skills used in other companies" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/skills/agentskills/export", "summary": "Export company skill bank in agentskills.io-compatible bundle format with provenance" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/skills/agentskills/import", "summary": "Import agentskills.io-compatible bundle with overwrite/dry-run controls and provenance preservation" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/migrations/legacy-agent-data", "summary": "Migrate legacy agent data (skills, memories, allowlists) with dry-run first pattern" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/context-repo/contract?session_key=", "summary": "Context repository layout contract (manifest, INDEX, notes/) under hsmii_home/context-repos" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/context-repo/ensure", "summary": "Create context-repo dirs + default manifest.json and INDEX.md" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/context-repo/publish", "summary": "Publish context repo snapshot into shared memory (hybrid retrieval) with governance + supersedes chain" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/context-repo/rollback", "summary": "Rollback a context-repo publish (restore previous memory head)" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/context-repo/publishes?session_key=", "summary": "List context-repo publishes for a session" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/skills/bootstrap/prune", "summary": "Disable or prune auto-bootstrapped Hermes packs by provenance source/pack" },
        { "scope": "company", "methods": ["GET", "PUT", "DELETE"], "path": "/api/company/companies/{company_id}/credentials", "summary": "Store masked company credentials for operator-connected services and MCP-style tools" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/browser/providers", "summary": "Cloud-browser and provider status surface (Firecrawl, Browserbase, Browser Use, xAI)" },
        { "scope": "company", "methods": ["GET", "PUT"], "path": "/api/company/companies/{company_id}/thread-sessions", "summary": "List or upsert shared thread sessions for multi-operator context handoff" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/thread-sessions/{session_key}/join", "summary": "Join shared thread session participant list" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/yc-bench-profile", "summary": "Deterministic YC-Bench controller profile derived from company context, workforce agents, and imported skills" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/export", "summary": "Export bundle JSON" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/spend/summary", "summary": "Spend rollup" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/governance/events", "summary": "List / append governance events" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/policies/rules", "summary": "Policy rules" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/policies/evaluate", "summary": "Evaluate policy for an action" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/goals", "summary": "Goals tree" },
        { "scope": "company", "methods": ["PATCH"], "path": "/api/company/companies/{company_id}/goals/{goal_id}", "summary": "Update goal" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/intelligence/summary", "summary": "Per-company intelligence: goals/tasks/spend/workforce + workflow feed (governance_events)" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/projects", "summary": "Paperclip-style project containers for tasks" },
        { "scope": "company", "methods": ["PATCH"], "path": "/api/company/companies/{company_id}/projects/{project_id}", "summary": "Update project" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/issue-labels", "summary": "Company catalog for task labels (capability_refs kind=label)" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/issue-labels/seed-defaults", "summary": "Idempotent starter label set for the company" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/agent-runs", "summary": "List / create agent execution runs (optional external_run_id idempotency)" },
        { "scope": "company", "methods": ["GET", "PATCH"], "path": "/api/company/companies/{company_id}/agent-runs/{run_id}", "summary": "Get run + feedback timeline; patch status/summary/meta" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/agent-runs/{run_id}/feedback", "summary": "Append human feedback on a run (optional step_index)" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/agent-runs/{run_id}/feedback/{event_id}/promote-task", "summary": "Create a task from feedback; sets spawned_task_id on the event" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/memory", "summary": "Company shared / agent-scoped memory: hybrid FTS + vector + recency (RRF), optional Ollama embed + HTTP rerank when q= set; see HSM_MEMORY_* env" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/memory/export.md", "summary": "Export shared memories as SHARED_MEMORY_INDEX.md markdown" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/memory/{memory_id}/delete", "summary": "Delete memory entry (POST alias when DELETE is blocked)" },
        { "scope": "company", "methods": ["PATCH", "DELETE"], "path": "/api/company/companies/{company_id}/memory/{memory_id}", "summary": "Update or delete memory entry" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/memory/ingest/web", "summary": "Queue web ingest into memory_artifacts + memory_chunks + canonical company_memory_entries" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/memory/ingest/file", "summary": "Queue file ingest (text, markdown, json, csv, html, pdf with extracted_text override)" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/memory/ingest/audio", "summary": "Queue audio transcript ingest into multimodal memory substrate" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/memory/ingest/image", "summary": "Queue image OCR/caption ingest into multimodal memory substrate" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/memory/artifacts", "summary": "List artifact ingest jobs and statuses (queued, extracting, chunked, indexed, retry_waiting, dead_letter)" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/memory/artifacts/{artifact_id}", "summary": "Inspect one artifact plus its chunks" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/memory/artifacts/{artifact_id}/retry", "summary": "Retry failed or dead-letter artifact ingest" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/memory/{memory_id}/inspect", "summary": "Memory inspector: canonical node + artifacts + chunks + lineage" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/memory/retrieval-debug?q=", "summary": "Run chunk-level retrieval with graph/time filters and matched_via debug output" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/memory/metrics", "summary": "Memory ingest and retrieval-readiness metrics" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/tasks", "summary": "List / create tasks (optional capability_refs[])" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/tasks/queue", "summary": "Filtered task queue (tabs)" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/spawn-rules", "summary": "Spawn rules" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/tasks/{task_id}/spawn-subagents", "summary": "Spawn subtasks from rules" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/tasks/{task_id}/handoffs", "summary": "Task handoffs" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/improvement-runs", "summary": "Improvement runs" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/go-live-checklist", "summary": "Go-live checklist" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/go-live-checklist/seed", "summary": "Seed checklist items" },
        { "scope": "company", "methods": ["GET", "POST"], "path": "/api/company/companies/{company_id}/agents", "summary": "Workforce agents registry" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/org", "summary": "Org chart" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/agents/{agent_id}/inventory", "summary": "Agent roster skill refs, resolved company_skills, full company skill catalog, Markdown instruction files under agents/<name>/" },
        { "scope": "company", "methods": ["PATCH", "DELETE"], "path": "/api/company/companies/{company_id}/agents/{agent_id}", "summary": "Update or delete agent row (delete clears direct reports’ manager link)" },
        { "scope": "task", "methods": ["PATCH"], "path": "/api/company/tasks/{task_id}/context", "summary": "Task specification, workspace_attachment_paths, capability_refs (skill/sop/tool/pack/agent links)" },
        { "scope": "task", "methods": ["PATCH"], "path": "/api/company/tasks/{task_id}/sla", "summary": "Task SLA fields" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/decision", "summary": "Policy decision on task" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/requires-human", "summary": "Set or clear requires_human (human inbox)" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/checkout", "summary": "Lease task to an agent ref" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/release", "summary": "Release checkout" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/run-telemetry", "summary": "Append run snapshot / log tail" },
        { "scope": "task", "methods": ["POST"], "path": "/api/company/tasks/{task_id}/stigmergic-note", "summary": "Append task handoff note (context_notes); shown in llm-context" },
        { "scope": "task", "methods": ["GET"], "path": "/api/company/tasks/{task_id}/llm-context", "summary": "LLM: company context + vision alignment (YC-Bench strategy snapshot) + shared memories + task spec/attachments + agent profile" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/promote/roodb-skills", "summary": "Promote RooDB skills into company_memory_entries with provenance audit" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/promote/ladybug-bundle", "summary": "Import Ladybug beliefs/skills bundle into company_memory_entries" },
        { "scope": "company", "methods": ["POST"], "path": "/api/company/companies/{company_id}/promote/rollback/{promotion_id}", "summary": "Rollback a promotion (deletes target row, marks rolled_back)" },
        { "scope": "company", "methods": ["GET"], "path": "/api/company/companies/{company_id}/promotions", "summary": "List store promotion audit trail (RooDB/Ladybug → Postgres)" },
        { "scope": "global", "methods": ["GET"], "path": "/api/company/health", "summary": "Postgres connectivity" },
        { "scope": "global", "methods": ["POST"], "path": "/api/company/import", "summary": "Import company bundle" },
        { "scope": "global", "methods": ["POST"], "path": "/api/company/task-handoffs/{handoff_id}/review", "summary": "Review handoff" },
        { "scope": "global", "methods": ["POST"], "path": "/api/company/task-handoffs/{handoff_id}/actions/token", "summary": "Issue signed approval action tokens for chat buttons" },
        { "scope": "global", "methods": ["POST"], "path": "/api/company/task-handoffs/actions/verify", "summary": "Verify signed approval action and apply handoff decision" },
        { "scope": "global", "methods": ["GET"], "path": "/api/company/runtime/activity", "summary": "Runtime activity heartbeat for smart inactivity timeouts" },
        { "scope": "global", "methods": ["GET"], "path": "/api/company/runtime/events/stream", "summary": "SSE stream for background completion notifications" },
        { "scope": "global", "methods": ["GET"], "path": "/api/company/runtime/portability-matrix", "summary": "Terminal backend portability matrix (local/docker/ssh/daytona/modal/singularity) with hibernation hints" },
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
                  context_markdown, webhook_url, created_at::text
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
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
    webhook_url: Option<String>,
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
                     context_markdown, webhook_url, created_at::text"#,
    )
    .bind(&slug)
    .bind(&display_name)
    .bind(&body.hsmii_home)
    .bind(&prefix)
    .fetch_one(pool)
    .await;
    match row {
        Ok(c) => {
            let bootstrap_imported = bootstrap_company_skills(pool, c.id, &c.slug, &c.display_name, None)
                .await
                .unwrap_or(0);
            Ok((
                StatusCode::CREATED,
                Json(json!({ "company": c, "bootstrap": { "imported": bootstrap_imported } })),
            ))
        }
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
    paperclip_goal_id: Option<String>,
    paperclip_snapshot: SqlxJson<Value>,
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
        r#"SELECT id, company_id, parent_goal_id, title, description, status, paperclip_goal_id, paperclip_snapshot, created_at::text
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
           RETURNING id, company_id, parent_goal_id, title, description, status, paperclip_goal_id, paperclip_snapshot, created_at::text"#,
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

fn repo_root_guess() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn collect_skill_markdowns(root: &StdPath, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_skill_markdowns(&p, out);
            continue;
        }
        let is_skill = p
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("SKILL.md"))
            .unwrap_or(false);
        if is_skill {
            out.push(p);
        }
    }
}

fn skill_slug_from_path(path: &StdPath) -> String {
    let mut parts = Vec::new();
    for c in path.components() {
        let s = c.as_os_str().to_string_lossy();
        if s == "hermes-main" {
            parts.clear();
            continue;
        }
        if s.eq_ignore_ascii_case("SKILL.md") {
            break;
        }
        parts.push(s.to_string());
    }
    parts
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
        .to_ascii_lowercase()
}

fn title_desc_from_body(slug: &str, body: &str) -> (String, String) {
    let title = body
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim).filter(|s| !s.is_empty()))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            slug.split('/')
                .last()
                .unwrap_or("Hermes Skill")
                .replace('-', " ")
        });
    let description = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("Bootstrapped Hermes skill")
        .to_string();
    (title, description)
}

fn business_model_from_hints(slug: &str, display_name: &str, vertical: Option<&str>) -> String {
    let combined = format!(
        "{} {} {}",
        slug.to_ascii_lowercase(),
        display_name.to_ascii_lowercase(),
        vertical.unwrap_or("").to_ascii_lowercase()
    );
    if combined.contains("commerce") || combined.contains("ecom") || combined.contains("shop") {
        return "commerce".to_string();
    }
    if combined.contains("content") || combined.contains("creator") || combined.contains("media") {
        return "content".to_string();
    }
    if combined.contains("saas") || combined.contains("software") {
        return "saas".to_string();
    }
    "services".to_string()
}

fn bootstrap_pack_for_skill(slug: &str, business_model: &str) -> Option<&'static str> {
    let category = slug.split('/').next().unwrap_or("");
    let core = [
        "mcp",
        "email",
        "research",
        "productivity",
        "software-development",
    ];
    if core.contains(&category) {
        return Some("core");
    }
    if (business_model == "commerce" || business_model == "content")
        && category == "social-media"
    {
        return Some("growth");
    }
    if (business_model == "commerce" || business_model == "content")
        && (category == "media"
            || (category == "creative"
                && (slug.contains("video") || slug.contains("youtube"))))
    {
        return Some("video");
    }
    None
}

async fn bootstrap_company_skills(
    pool: &PgPool,
    company_id: Uuid,
    company_slug: &str,
    display_name: &str,
    vertical_hint: Option<&str>,
) -> Result<usize, sqlx::Error> {
    let business_model = business_model_from_hints(company_slug, display_name, vertical_hint);
    let root = repo_root_guess().join(".claude/skills/hermes-main");
    if !root.is_dir() {
        return Ok(0);
    }
    let mut files = Vec::new();
    collect_skill_markdowns(&root, &mut files);
    let mut imported = 0usize;
    for path in files {
        let slug = skill_slug_from_path(&path);
        if slug.is_empty() {
            continue;
        }
        let Some(pack) = bootstrap_pack_for_skill(&slug, &business_model) else {
            continue;
        };
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let (name, description) = title_desc_from_body(&slug, &raw);
        let source = format!("hermes_bootstrap:{pack}");
        sqlx::query(
            r#"INSERT INTO company_skills (company_id, slug, name, description, body, skill_path, source)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               ON CONFLICT (company_id, slug) DO UPDATE
               SET name = EXCLUDED.name,
                   description = EXCLUDED.description,
                   body = EXCLUDED.body,
                   skill_path = EXCLUDED.skill_path,
                   source = EXCLUDED.source,
                   updated_at = NOW()"#,
        )
        .bind(company_id)
        .bind(&slug)
        .bind(name)
        .bind(description)
        .bind(raw)
        .bind(path.to_string_lossy().to_string())
        .bind(source)
        .execute(pool)
        .await?;
        imported += 1;
    }
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, 'skills_bootstrap', 'bootstrap_hermes_skills', 'company', $2, $3, 'info')"#,
    )
    .bind(company_id)
    .bind(company_id.to_string())
    .bind(SqlxJson(json!({
        "business_model": business_model,
        "imported_count": imported
    })))
    .execute(pool)
    .await;
    Ok(imported)
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
pub(super) struct TaskRow {
    id: Uuid,
    company_id: Uuid,
    primary_goal_id: Option<Uuid>,
    project_id: Option<Uuid>,
    goal_ancestry: Value,
    title: String,
    specification: Option<String>,
    workspace_attachment_paths: Value,
    capability_refs: SqlxJson<Value>,
    state: String,
    owner_persona: Option<String>,
    parent_task_id: Option<Uuid>,
    spawned_by_rule_id: Option<Uuid>,
    checked_out_by: Option<String>,
    checked_out_until: Option<chrono::DateTime<chrono::Utc>>,
    priority: i32,
    display_number: i32,
    requires_human: bool,
    due_at: Option<chrono::DateTime<chrono::Utc>>,
    blocked_by_task_id: Option<Uuid>,
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

async fn upsert_run_snapshot_running(
    pool: &PgPool,
    company_id: Uuid,
    task_id: Uuid,
) -> Result<(), sqlx::Error> {
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

async fn ensure_task_run_snapshot_row(
    pool: &PgPool,
    company_id: Uuid,
    task_id: Uuid,
) -> Result<(), sqlx::Error> {
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

#[derive(Deserialize)]
struct ExecuteTaskWorkerBody {
    #[serde(default)]
    actor: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    skill_slug: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ExecuteTaskRow {
    title: String,
    specification: Option<String>,
    checked_out_by: Option<String>,
    company_id: Uuid,
    goal_ancestry: serde_json::Value,
    context_notes: serde_json::Value,
    capability_refs: serde_json::Value,
    primary_goal_id: Option<Uuid>,
    parent_task_id: Option<Uuid>,
}

/// Assemble the rich company context prefix injected into every execute-worker prompt.
/// Fetches goals, team roster, skills catalog, active assignments, agent memories,
/// sibling task stigmergic notes, and this task's own handoff notes.
async fn build_worker_context_prefix(pool: &PgPool, task: &ExecuteTaskRow, actor: &str) -> String {
    let cid = task.company_id;
    let mut out = String::with_capacity(4096);

    // ── Company context markdown ───────────────────────────────────────────────
    if let Ok(Some(md)) = sqlx::query_scalar::<_, Option<String>>(
        "SELECT context_markdown FROM companies WHERE id = $1",
    )
    .bind(cid)
    .fetch_optional(pool)
    .await
    {
        if let Some(md) = md.filter(|s| !s.trim().is_empty()) {
            out.push_str("## Company context\n\n");
            // include first 1500 chars to stay compact
            let truncated: String = md.chars().take(1500).collect();
            out.push_str(&truncated);
            if md.chars().count() > 1500 {
                out.push_str("\n\n*(context continues — see `/company-context` for full text)*");
            }
            out.push_str("\n\n");
        }
    }

    // ── Active goals ──────────────────────────────────────────────────────────
    #[derive(sqlx::FromRow)]
    struct GoalBriefRow {
        title: String,
        status: String,
        parent_goal_id: Option<Uuid>,
    }
    if let Ok(goals) = sqlx::query_as::<_, GoalBriefRow>(
        "SELECT title, status, parent_goal_id FROM goals WHERE company_id = $1
         ORDER BY sort_order, created_at LIMIT 20",
    )
    .bind(cid)
    .fetch_all(pool)
    .await
    {
        if !goals.is_empty() {
            out.push_str("## Company goals\n\n");
            for g in &goals {
                let indent = if g.parent_goal_id.is_some() { "  - " } else { "- " };
                out.push_str(&format!("{indent}[{}] {}\n", g.status, g.title));
            }
            out.push('\n');
        }
    }

    // ── Goal ancestry for this task ────────────────────────────────────────────
    let ancestry = task.goal_ancestry.as_array().cloned().unwrap_or_default();
    if !ancestry.is_empty() {
        out.push_str("## This task's goal chain\n\n");
        for entry in &ancestry {
            if let Some(title) = entry.get("title").and_then(|v| v.as_str()) {
                let status = entry
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                out.push_str(&format!("- [{status}] {title}\n"));
            }
        }
        out.push('\n');
    }

    // ── Team roster + current assignments ─────────────────────────────────────
    #[derive(sqlx::FromRow)]
    struct AgentRow {
        name: String,
        role: String,
        title: Option<String>,
    }
    if let Ok(agents) = sqlx::query_as::<_, AgentRow>(
        "SELECT a.name, a.role, a.title
         FROM company_agents a
         WHERE a.company_id = $1 AND a.status = 'active'
         ORDER BY a.sort_order, a.name LIMIT 20",
    )
    .bind(cid)
    .fetch_all(pool)
    .await
    {
        if !agents.is_empty() {
            out.push_str("## Team roster\n\n");
            // Collect agents with their current task assignments
            for ag in &agents {
                let display_title = ag
                    .title
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&ag.role);
                // check what task this agent currently has checked out
                let current: Option<String> = sqlx::query_scalar(
                    "SELECT title FROM tasks WHERE company_id = $1 AND checked_out_by = $2
                     AND state NOT IN ('done','closed','cancelled') LIMIT 1",
                )
                .bind(cid)
                .bind(&ag.name)
                .fetch_optional(pool)
                .await
                .unwrap_or(None);
                if let Some(task_title) = current {
                    out.push_str(&format!(
                        "- **{}** ({}) — currently working on: *{}*\n",
                        ag.name, display_title, task_title
                    ));
                } else {
                    out.push_str(&format!("- **{}** ({}) — available\n", ag.name, display_title));
                }
            }
            out.push('\n');
        }
    }

    // ── Open / in-progress tasks (work-in-flight overview) ────────────────────
    #[derive(sqlx::FromRow)]
    struct TaskSummaryRow {
        title: String,
        state: String,
        owner_persona: Option<String>,
    }
    if let Ok(active) = sqlx::query_as::<_, TaskSummaryRow>(
        "SELECT title, state, owner_persona FROM tasks
         WHERE company_id = $1 AND state IN ('open','in_progress')
         ORDER BY priority DESC, updated_at DESC LIMIT 10",
    )
    .bind(cid)
    .fetch_all(pool)
    .await
    {
        if !active.is_empty() {
            out.push_str("## Active task queue\n\n");
            for t in &active {
                let owner = t
                    .owner_persona
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("unassigned");
                out.push_str(&format!(
                    "- [{}] {} (owner: {})\n",
                    t.state, t.title, owner
                ));
            }
            out.push('\n');
        }
    }

    // ── Available skills catalog ───────────────────────────────────────────────
    #[derive(sqlx::FromRow)]
    struct SkillRow {
        slug: String,
        description: String,
    }
    if let Ok(skills) = sqlx::query_as::<_, SkillRow>(
        "SELECT slug, LEFT(description, 120) as description FROM company_skills
         WHERE company_id = $1 ORDER BY slug LIMIT 30",
    )
    .bind(cid)
    .fetch_all(pool)
    .await
    {
        if !skills.is_empty() {
            out.push_str("## Available skills\n\n");
            for sk in &skills {
                let desc = if sk.description.trim().is_empty() {
                    String::new()
                } else {
                    format!(" — {}", sk.description.trim())
                };
                out.push_str(&format!("- `{}`{}\n", sk.slug, desc));
            }
            out.push('\n');
        }
    }

    // ── Linked capability refs for this task ──────────────────────────────────
    let caps = task.capability_refs.as_array().cloned().unwrap_or_default();
    if !caps.is_empty() {
        out.push_str("## Linked resources\n\n");
        for cap in &caps {
            if let Some(obj) = cap.as_object() {
                let kind = obj.get("kind").and_then(|v| v.as_str()).unwrap_or("resource");
                let r = obj.get("ref").and_then(|v| v.as_str()).unwrap_or("");
                out.push_str(&format!("- [{kind}] `{r}`\n"));
            } else if let Some(s) = cap.as_str() {
                out.push_str(&format!("- `{s}`\n"));
            }
        }
        out.push('\n');
    }

    // ── Stigmergic handoff notes on this task ─────────────────────────────────
    let notes = task.context_notes.as_array().cloned().unwrap_or_default();
    if !notes.is_empty() {
        out.push_str("## Handoff notes (stigmergic context)\n\n");
        // Show up to 10 most recent notes (array is ordered oldest-first)
        let start = notes.len().saturating_sub(10);
        for note in &notes[start..] {
            let note_actor = note.get("actor").and_then(|v| v.as_str()).unwrap_or("?");
            let text = note.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let at = note.get("at").and_then(|v| v.as_str()).unwrap_or("");
            out.push_str(&format!("**{}** ({}):\n{}\n\n", note_actor, at, text));
        }
    }

    // ── Sibling / goal-scoped stigmergic notes (predecessor tasks) ────────────
    // Surface completion notes from other tasks under the same goal or parent task
    // so the agent understands what collaborators have already accomplished.
    if let Some(goal_id) = task.primary_goal_id.or(task.parent_task_id) {
        #[derive(sqlx::FromRow)]
        struct SiblingNotesRow {
            title: String,
            context_notes: serde_json::Value,
        }
        if let Ok(siblings) = sqlx::query_as::<_, SiblingNotesRow>(
            "SELECT title, context_notes FROM tasks
             WHERE company_id = $1
               AND primary_goal_id = $2
               AND context_notes != '[]'::jsonb
               AND context_notes != 'null'::jsonb
             ORDER BY updated_at DESC LIMIT 5",
        )
        .bind(cid)
        .bind(goal_id)
        .fetch_all(pool)
        .await
        {
            // Only show siblings that actually have notes
            let siblings_with_notes: Vec<_> = siblings
                .iter()
                .filter(|s| s.context_notes.as_array().map_or(false, |a| !a.is_empty()))
                .collect();
            if !siblings_with_notes.is_empty() {
                out.push_str("## Related task completions (same goal)\n\n");
                for sib in siblings_with_notes {
                    let sib_notes = sib.context_notes.as_array().cloned().unwrap_or_default();
                    // Only show the most recent note from each sibling
                    if let Some(last_note) = sib_notes.last() {
                        let sib_actor = last_note.get("actor").and_then(|v| v.as_str()).unwrap_or("?");
                        let text = last_note.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        let at = last_note.get("at").and_then(|v| v.as_str()).unwrap_or("");
                        out.push_str(&format!(
                            "**{}** by {} ({}):\n{}\n\n",
                            sib.title, sib_actor, at, text
                        ));
                    }
                }
            }
        }
    }

    // ── Agent memory (this agent's persisted knowledge + shared company memory) ─
    if !actor.trim().is_empty() {
        // Look up the company_agent_id for this actor by name
        let agent_uuid: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM company_agents WHERE company_id = $1 AND name = $2 LIMIT 1",
        )
        .bind(cid)
        .bind(actor)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

        #[derive(sqlx::FromRow)]
        struct MemRow {
            title: String,
            body: String,
            scope: String,
            tags: Vec<String>,
        }
        let memories: Vec<MemRow> = if let Some(aid) = agent_uuid {
            sqlx::query_as::<_, MemRow>(
                "SELECT title, LEFT(body, 400) as body, scope, tags
                 FROM company_memory_entries
                 WHERE company_id = $1 AND (company_agent_id = $2 OR scope = 'shared')
                 ORDER BY updated_at DESC LIMIT 12",
            )
            .bind(cid)
            .bind(aid)
            .fetch_all(pool)
            .await
            .unwrap_or_default()
        } else {
            // No matching agent — only show shared memory
            sqlx::query_as::<_, MemRow>(
                "SELECT title, LEFT(body, 400) as body, scope, tags
                 FROM company_memory_entries
                 WHERE company_id = $1 AND scope = 'shared'
                 ORDER BY updated_at DESC LIMIT 8",
            )
            .bind(cid)
            .fetch_all(pool)
            .await
            .unwrap_or_default()
        };

        if !memories.is_empty() {
            out.push_str("## Agent memory\n\n");
            for m in &memories {
                let scope_badge = if m.scope == "shared" { " *(shared)*" } else { "" };
                let tags_str = if m.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", m.tags.join(", "))
                };
                out.push_str(&format!("### {}{}{}\n{}\n\n", m.title, scope_badge, tags_str, m.body.trim()));
            }
        }
    }

    out
}

fn truncate_worker_log(s: &str, max_chars: usize) -> String {
    let t = s.trim();
    if t.is_empty() {
        return String::new();
    }
    let out: String = t.chars().take(max_chars).collect();
    if out.chars().count() < t.chars().count() {
        format!("{out}…")
    } else {
        out
    }
}

async fn post_execute_task_worker(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<ExecuteTaskWorkerBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let task = sqlx::query_as::<_, ExecuteTaskRow>(
        r#"SELECT title, specification, checked_out_by,
                  company_id, goal_ancestry, context_notes, capability_refs,
                  primary_goal_id, parent_task_id
           FROM tasks WHERE id = $1"#,
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
    let Some(task) = task else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    };

    let actor = body
        .actor
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            task.checked_out_by
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "worker-agent-loop".to_string());

    // Create an agent_runs record so tools can reference the run_id and
    // the governance layer has a canonical execution record.
    // No external_run_id to avoid unique-constraint conflicts on re-dispatch.
    let run_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO agent_runs (company_id, task_id, external_system, status)
           VALUES ($1, $2, 'hsm', 'running')
           RETURNING id"#,
    )
    .bind(task.company_id)
    .bind(task_id)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|_| Uuid::new_v4());

    // Build rich company context prefix (goals, team, skills, memories, handoff notes)
    let context_prefix = build_worker_context_prefix(pool, &task, &actor).await;

    // Assemble the base task instruction
    let base_task = body
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            format!(
                "Execute this task with real tools and return what was done.\n\nTitle: {}\n\nSpecification:\n{}",
                task.title,
                task.specification.as_deref().unwrap_or("(no specification)")
            )
        });

    // Build the execution identity block so tools can reference company_id / task_id / run_id
    // without relying on environment variables (which are not available inside tokio tasks).
    let exec_identity = format!(
        "## Your execution identity\n\n\
         - **Actor (you)**: {actor}\n\
         - **company_id**: {company_id}\n\
         - **task_id**: {task_id}\n\
         - **run_id**: {run_id}\n\n\
         When calling company OS tools always pass `company_id`, `task_id`, and `run_id` \
         explicitly using the values above.\n\n\
         ## Available company OS tools\n\
         - `company_list_tasks` — see open/in-progress work across the company\n\
         - `company_create_task` — spawn a subtask and optionally assign it to another agent via `checked_out_by`\n\
         - `company_update_task` — change task state (done/blocked/open) or append a stigmergic note\n\
         - `company_memory_search` / `company_memory_append` — shared + private memory pool\n\
         - `company_task_requires_human` — escalate to human inbox when blocked\n\
         - `company_tool_discover` / `company_tool_describe` / `company_tool_call` — tool catalog\n\n",
        actor = actor,
        company_id = task.company_id,
        task_id = task_id,
        run_id = run_id,
    );

    // Prefix with /hermes to always route through the agentic tool loop,
    // then inject company context so the agent has full situational awareness.
    let prompt = if context_prefix.is_empty() {
        format!("/hermes {exec_identity}\n---\n\n## Your task\n\n{base_task}")
    } else {
        format!("/hermes {context_prefix}{exec_identity}\n---\n\n## Your task\n\n{base_task}")
    };

    let skill_slug = body
        .skill_slug
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "operator-chat".to_string());

    let actor_for_response = actor.clone();
    let skill_slug_for_response = skill_slug.clone();
    let run_id_for_response = run_id;
    let st_bg = st.clone();
    let pool_bg = pool.clone();
    let task_company_id = task.company_id;
    tokio::spawn(async move {
        // ── Mark task as in_progress ───────────────────────────────────────────
        let _ = sqlx::query(
            "UPDATE tasks SET state = 'in_progress', updated_at = now() WHERE id = $1 AND state = 'open'",
        )
        .bind(task_id)
        .execute(&pool_bg)
        .await;

        let _ = post_task_run_telemetry(
            State(st_bg.clone()),
            Path(task_id),
            Json(PostRunTelemetryBody {
                run_status: Some("running".to_string()),
                tool_calls: Some(1),
                log_append: Some(format!(
                    "worker start actor={} skill={} at={}\n",
                    actor,
                    skill_slug,
                    Utc::now().to_rfc3339()
                )),
                clear_log: Some(true),
            }),
        )
        .await;

        // ── SFT capture setup (manual execute path) ───────────────────────────
        let sft_cap_manual = if crate::sft::capture_enabled() {
            let cap = crate::sft::SftCapture::new(&actor);
            cap.0.lock().await.task_id = Some(task_id.to_string());
            cap.0.lock().await.run_id = Some(run_id.to_string());
            cap.0.lock().await.company_id = Some(task_company_id.to_string());
            Some(cap)
        } else {
            None
        };

        let start_ms = Utc::now().timestamp_millis();
        let mut rx = crate::runtime_control::subscribe_completions();
        let (run_status, mut tool_calls, log_append, agent_reply) =
            match EnhancedPersonalAgent::initialize(&st_bg.home).await {
                Ok(mut agent) => {
                    agent.sft_capture = sft_cap_manual.clone();

                    let msg = Message {
                        id: format!("task-{task_id}"),
                        platform: Platform::Web,
                        channel_id: "company-os".to_string(),
                        channel_name: Some("company-os".to_string()),
                        user_id: actor.clone(),
                        user_name: actor.clone(),
                        content: prompt,
                        timestamp: Utc::now(),
                        attachments: Vec::new(),
                        reply_to: None,
                    };
                    match agent.handle_message(msg).await {
                        Ok(reply) => {
                            let log = format!(
                                "worker reply: {}\n",
                                truncate_worker_log(&reply, 1400)
                            );
                            ("success".to_string(), 0i32, log, Some(reply))
                        }
                        Err(e) => (
                            "error".to_string(),
                            0i32,
                            format!("worker error: {e}\n"),
                            None,
                        ),
                    }
                }
                Err(e) => (
                    "error".to_string(),
                    0i32,
                    format!("worker init failed: {e}\n"),
                    None,
                ),
            };

        loop {
            match rx.try_recv() {
                Ok(evt) => {
                    if evt.ts_ms >= start_ms
                        && (evt.event_type == "tool_start" || evt.event_type == "tool_start_delta")
                    {
                        tool_calls += 1;
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
                | Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            }
        }
        if tool_calls <= 0 {
            tool_calls = 1;
        }

        let _ = post_task_run_telemetry(
            State(st_bg.clone()),
            Path(task_id),
            Json(PostRunTelemetryBody {
                run_status: Some(run_status.clone()),
                tool_calls: Some(tool_calls),
                log_append: Some(log_append),
                clear_log: Some(false),
            }),
        )
        .await;

        // ── SFT capture teardown (manual execute path) ────────────────────────
        if let Some(cap) = sft_cap_manual {
            let trace = cap.finish(&run_status).await;
            let trace_path = st_bg.home.join("memory/sft_traces.jsonl");
            if let Err(e) = crate::sft::write_trace(&trace_path, &trace).await {
                tracing::warn!(target: "hsm_sft", "manual path: failed to write sft trace: {e}");
            }
        }

        // ── Update agent_runs record with terminal status ──────────────────────
        let ar_status = if run_status == "success" { "success" } else { "error" };
        let _ = sqlx::query(
            "UPDATE agent_runs SET status = $1, finished_at = now() WHERE id = $2",
        )
        .bind(ar_status)
        .bind(run_id)
        .execute(&pool_bg)
        .await;

        // ── On success: mark task done + append stigmergic completion note ─────
        if run_status == "success" {
            let _ = sqlx::query(
                "UPDATE tasks SET state = 'done', updated_at = now() WHERE id = $1",
            )
            .bind(task_id)
            .execute(&pool_bg)
            .await;

            if let Some(reply) = agent_reply {
                let summary = truncate_worker_log(&reply, 600);
                let note = serde_json::json!({
                    "text": summary,
                    "actor": actor,
                    "at": Utc::now().to_rfc3339(),
                });
                let _ = sqlx::query(
                    "UPDATE tasks SET context_notes = context_notes || $1::jsonb, updated_at = now()
                     WHERE id = $2",
                )
                .bind(serde_json::json!([note]))
                .bind(task_id)
                .execute(&pool_bg)
                .await;
            }

            // Emit a governance event so the activity feed reflects the completed work
            let _ = sqlx::query(
                r#"INSERT INTO governance_events
                   (company_id, actor, action, subject_type, subject_id, payload, severity)
                   VALUES ($1, $2, 'task_completed', 'task', $3, $4, 'info')"#,
            )
            .bind(task_company_id)
            .bind(&actor)
            .bind(task_id.to_string())
            .bind(serde_json::json!({ "skill": skill_slug }))
            .execute(&pool_bg)
            .await;
        }

        let _ = release_task(
            State(st_bg),
            Path(task_id),
            Json(ReleaseBody {
                actor: actor.clone(),
            }),
        )
        .await;
    });

    Ok(Json(json!({
        "started": true,
        "task_id": task_id,
        "run_id": run_id_for_response,
        "actor": actor_for_response,
        "skill_slug": skill_slug_for_response,
    })))
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
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

    if matches!(body.run_status.as_deref(), Some("success") | Some("error")) {
        let _ = sqlx::query(
            r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
               VALUES ($1, 'run_telemetry', 'task_run_terminal', 'task', $2, $3, 'info')"#,
        )
        .bind(company_id)
        .bind(task_id.to_string())
        .bind(SqlxJson(json!({
            "run_status": updated.run_status,
            "tool_calls": updated.tool_calls,
        })))
        .execute(pool)
        .await;
    }

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

const MAX_STIGMERGIC_NOTES: usize = 100;

#[derive(Deserialize)]
struct PostStigmergicNoteBody {
    text: String,
    #[serde(default)]
    actor: String,
}

/// Append a short handoff note on the task (`context_notes`); merged into `llm-context` for the next assignee.
async fn post_task_stigmergic_note(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<PostStigmergicNoteBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let text = body.text.trim();
    if text.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "text required" })),
        ));
    }
    let actor = body.actor.trim();
    let actor = if actor.is_empty() {
        "operator".to_string()
    } else {
        actor.to_string()
    };

    let row =
        sqlx::query_as::<_, (SqlxJson<Value>,)>("SELECT context_notes FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
    let Some((SqlxJson(notes_val_raw),)) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    };

    let mut arr = match notes_val_raw {
        Value::Array(a) => a,
        _ => Vec::new(),
    };
    arr.push(json!({
        "at": chrono::Utc::now().to_rfc3339(),
        "actor": actor,
        "text": text,
    }));
    if arr.len() > MAX_STIGMERGIC_NOTES {
        let drop = arr.len() - MAX_STIGMERGIC_NOTES;
        arr = arr.split_off(drop);
    }
    let new_notes = Value::Array(arr);

    sqlx::query("UPDATE tasks SET context_notes = $2, updated_at = NOW() WHERE id = $1")
        .bind(task_id)
        .bind(SqlxJson(new_notes.clone()))
        .execute(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;

    // Emit task event for the note — need company_id
    if let Ok(Some(cid)) = sqlx::query_scalar::<_, Uuid>(
        "SELECT company_id FROM tasks WHERE id = $1",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    {
        emit_task_event(pool, task_id, cid, "stigmergic_note", &actor,
            json!({ "note": text })).await;
    }

    Ok(Json(json!({ "ok": true, "context_notes": new_notes })))
}

async fn list_tasks(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, TaskRow>(
        r#"SELECT id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state,
                  owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, due_at, blocked_by_task_id, created_at::text
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
    let smap: std::collections::HashMap<Uuid, TaskRunSnapRow> =
        snaps.into_iter().map(|s| (s.task_id, s)).collect();
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
/// # What is a Task?
///
/// A task is the **atomic unit of organizational intent** in Company OS.  It is not merely a
/// to-do item — it is a *stigmergic coordination artifact* that carries everything an agent
/// (or human) needs to pick up work without re-discovering context.
///
/// ## Purpose & utility
///
/// | Dimension            | What the task provides                                               |
/// |----------------------|----------------------------------------------------------------------|
/// | **Commitment**       | An explicit promise that outcome X will be reached by persona Y      |
/// | **Context capsule**  | `specification`, `workspace_attachment_paths`, `capability_refs` and |
/// |                      | `context_notes` form a self-contained brief any agent can cold-start |
/// | **Stigmergic trail** | `context_notes` accumulates operator messages, agent replies, and    |
/// |                      | run telemetry so every handoff is lossless — no institutional memory |
/// |                      | escapes into chat logs that disappear                                |
/// | **Governance gate**  | `requires_human` stops the flow for explicit human approval;         |
/// |                      | the decision log (`governance_events`) is immutable                  |
/// | **Work queue slot**  | `priority`, `checked_out_by`, `checked_out_until` implement a        |
/// |                      | distributed, lease-based work queue agents poll for their next turn  |
/// | **Learning trigger** | When a task completes, its `context_notes` digest becomes a          |
/// |                      | candidate for company shared memory (`company_memory_entries`)       |
///
/// ## Lifecycle
/// ```text
/// open → in_progress (checkout) → [waiting_admin | blocked] → done / cancelled
///                                      ↑ requires_human gate
/// ```
///
/// ## Design rule
/// Keep tasks *narrow but rich*: one clear outcome, fully specified context.  A task that needs
/// five sub-tasks spawned is healthy.  A task with a vague title and no specification is not.
struct CreateTaskBody {
    title: String,
    #[serde(default)]
    specification: Option<String>,
    /// Paths relative to company `hsmii_home` workspace (Paperclip-style pointers).
    #[serde(default)]
    workspace_attachment_paths: Option<Vec<String>>,
    /// Links to skills, SOPs, tools, packs, or agent templates: strings or `{ "kind", "ref" }`.
    #[serde(default)]
    capability_refs: Option<Vec<Value>>,
    #[serde(default)]
    primary_goal_id: Option<Uuid>,
    #[serde(default)]
    project_id: Option<Uuid>,
    #[serde(default)]
    owner_persona: Option<String>,
    /// Pre-assign to a specific agent persona slug. Validated against company_agents roster.
    #[serde(default)]
    checked_out_by: Option<String>,
    #[serde(default)]
    parent_task_id: Option<Uuid>,
    #[serde(default)]
    spawned_by_rule_id: Option<Uuid>,
    /// Declared dependency: this task cannot start until the referenced task completes.
    #[serde(default)]
    blocked_by_task_id: Option<Uuid>,
    /// Optional deadline (ISO 8601). Surfaced in dashboard overdue alerts and SLA tracking.
    #[serde(default)]
    due_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Task queue ordering (higher runs first in `ORDER BY priority DESC`). Omitted or reviewer-deferred uses 0.
    #[serde(default)]
    priority: Option<i32>,
}

pub(super) fn workspace_attachment_paths_json(paths: Option<Vec<String>>) -> Value {
    let arr: Vec<Value> = paths
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(Value::String)
        .collect();
    Value::Array(arr)
}

const MAX_CAPABILITY_REFS: usize = 32;
const MAX_CAPABILITY_REF_LEN: usize = 256;

/// Normalize task capability links: strings become `{ kind: skill, ref }`; objects require `ref` and optional `kind`.
pub(super) fn normalize_capability_refs(raw: Option<Vec<Value>>) -> Result<Value, String> {
    let Some(items) = raw else {
        return Ok(json!([]));
    };
    if items.len() > MAX_CAPABILITY_REFS {
        return Err(format!(
            "capability_refs: at most {MAX_CAPABILITY_REFS} entries"
        ));
    }
    let mut out: Vec<Value> = Vec::new();
    for (i, v) in items.into_iter().enumerate() {
        let obj = match v {
            Value::String(s) => {
                let r = s.trim();
                if r.is_empty() {
                    return Err(format!("capability_refs[{i}]: empty string"));
                }
                if r.len() > MAX_CAPABILITY_REF_LEN {
                    return Err(format!("capability_refs[{i}]: ref too long"));
                }
                json!({ "kind": "skill", "ref": r })
            }
            Value::Object(map) => {
                let ref_v = map
                    .get("ref")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let Some(r) = ref_v else {
                    return Err(format!("capability_refs[{i}]: missing ref"));
                };
                if r.len() > MAX_CAPABILITY_REF_LEN {
                    return Err(format!("capability_refs[{i}]: ref too long"));
                }
                let kind = map
                    .get("kind")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("skill")
                    .to_ascii_lowercase();
                if !matches!(
                    kind.as_str(),
                    "skill" | "sop" | "tool" | "pack" | "agent" | "ticket" | "mode" | "label"
                ) {
                    return Err(format!(
                        "capability_refs[{i}]: kind must be skill|sop|tool|pack|agent|ticket|mode|label"
                    ));
                }
                let role = map
                    .get("role")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                if let Some(rv) = role {
                    if rv.len() > 32 {
                        return Err(format!("capability_refs[{i}]: role too long"));
                    }
                    json!({ "kind": kind, "ref": r, "role": rv })
                } else {
                    json!({ "kind": kind, "ref": r })
                }
            }
            _ => return Err(format!("capability_refs[{i}]: expected string or object")),
        };
        out.push(obj);
    }
    Ok(Value::Array(out))
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

    let mut project_uuid: Option<Uuid> = None;
    if let Some(pid) = body.project_id {
        let ok = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1 AND company_id = $2)",
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
                Json(json!({ "error": "project_id not in company" })),
            ));
        }
        project_uuid = Some(pid);
    }

    let mut tx = pool.begin().await.map_err(|e| {
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
    let ws_json = workspace_attachment_paths_json(body.workspace_attachment_paths.clone());
    let caps_json = normalize_capability_refs(body.capability_refs.clone())
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
    let priority_val = body.priority.unwrap_or(0).clamp(-1000, 1000);

    // Validate checked_out_by against the agent roster
    let checked_out_by_val = if let Some(ref slug) = body.checked_out_by {
        let slug = slug.trim();
        if !slug.is_empty() {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM company_agents WHERE company_id=$1 AND name=$2 AND status='active')",
            )
            .bind(company_id)
            .bind(slug)
            .fetch_one(&mut *tx)
            .await
            .unwrap_or(false);
            if !exists {
                // Warn but don't block — the agent may be added later
                tracing::warn!(
                    target: "hsm_company_tasks",
                    company_id = %company_id,
                    checked_out_by = %slug,
                    "creating task assigned to unrecognized agent persona"
                );
            }
            Some(slug.to_string())
        } else {
            None
        }
    } else {
        None
    };

    let row = sqlx::query_as::<_, TaskRow>(
        r#"INSERT INTO tasks (company_id, primary_goal_id, project_id, goal_ancestry, title,
                              specification, workspace_attachment_paths, capability_refs,
                              owner_persona, checked_out_by, parent_task_id, spawned_by_rule_id,
                              blocked_by_task_id, due_at, display_number, priority)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
           RETURNING id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, due_at, blocked_by_task_id, created_at::text"#,
    )
    .bind(company_id)
    .bind(&body.primary_goal_id)
    .bind(project_uuid)
    .bind(SqlxJson(ancestry_json))
    .bind(&title)
    .bind(&body.specification)
    .bind(SqlxJson(ws_json))
    .bind(SqlxJson(caps_json))
    .bind(&body.owner_persona)
    .bind(&checked_out_by_val)
    .bind(&body.parent_task_id)
    .bind(&body.spawned_by_rule_id)
    .bind(&body.blocked_by_task_id)
    .bind(&body.due_at)
    .bind(display_n)
    .bind(priority_val)
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
        "primary_goal_id": row.primary_goal_id,
        "parent_task_id": row.parent_task_id,
        "display_number": row.display_number,
        "capability_refs": row.capability_refs.0,
    })))
    .execute(pool)
    .await;

    emit_task_event(
        pool,
        row.id,
        company_id,
        "created",
        actor,
        json!({ "title": row.title, "owner_persona": row.owner_persona }),
    ).await;

    Ok((StatusCode::CREATED, Json(json!({ "task": row }))))
}

#[derive(sqlx::FromRow, Serialize)]
struct ProjectRow {
    id: Uuid,
    company_id: Uuid,
    title: String,
    description: Option<String>,
    status: String,
    sort_order: i32,
    created_at: String,
    updated_at: String,
}

async fn list_projects(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, ProjectRow>(
        r#"SELECT id, company_id, title, description, status, sort_order, created_at::text, updated_at::text
           FROM projects WHERE company_id = $1 ORDER BY sort_order, created_at"#,
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
    Ok(Json(json!({ "projects": rows })))
}

#[derive(Deserialize)]
struct CreateProjectBody {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    sort_order: Option<i32>,
}

async fn create_project(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateProjectBody>,
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
    let status = body
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("active");
    let sort_order = body.sort_order.unwrap_or(0);
    let row = sqlx::query_as::<_, ProjectRow>(
        r#"INSERT INTO projects (company_id, title, description, status, sort_order)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id, company_id, title, description, status, sort_order, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&title)
    .bind(&body.description)
    .bind(status)
    .bind(sort_order)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok((StatusCode::CREATED, Json(json!({ "project": row }))))
}

#[derive(Deserialize, Default)]
struct PatchProjectBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    sort_order: Option<i32>,
}

async fn patch_project(
    State(st): State<ConsoleState>,
    Path((company_id, project_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchProjectBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM projects WHERE id = $1 AND company_id = $2)",
    )
    .bind(project_id)
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "project not found" })),
        ));
    }
    let title_upd = body
        .title
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let row = sqlx::query_as::<_, ProjectRow>(
        r#"UPDATE projects SET
            title = COALESCE($3, title),
            description = COALESCE($4, description),
            status = COALESCE($5, status),
            sort_order = COALESCE($6, sort_order),
            updated_at = NOW()
           WHERE id = $1 AND company_id = $2
           RETURNING id, company_id, title, description, status, sort_order, created_at::text, updated_at::text"#,
    )
    .bind(project_id)
    .bind(company_id)
    .bind(title_upd)
    .bind(body.description.as_ref())
    .bind(body.status.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()))
    .bind(body.sort_order)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok(Json(json!({ "project": row })))
}

#[derive(sqlx::FromRow, Serialize)]
struct IssueLabelRow {
    id: Uuid,
    company_id: Uuid,
    slug: String,
    display_name: String,
    description: Option<String>,
    sort_order: i32,
    created_at: String,
    updated_at: String,
}

fn normalize_issue_label_slug(raw: &str) -> Result<String, String> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err("slug required".to_string());
    }
    if s.len() > 48 {
        return Err("slug too long (max 48)".to_string());
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return Err("slug required".to_string());
    };
    if !first.is_ascii_alphanumeric() {
        return Err("slug must start with a letter or number".to_string());
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("slug may only contain a-z, 0-9, underscore, hyphen".to_string());
    }
    Ok(s)
}

async fn list_issue_labels(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, IssueLabelRow>(
        r#"SELECT id, company_id, slug, display_name, description, sort_order, created_at::text, updated_at::text
           FROM company_issue_labels WHERE company_id = $1 ORDER BY sort_order, display_name"#,
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
    Ok(Json(json!({ "labels": rows })))
}

#[derive(Deserialize)]
struct CreateIssueLabelBody {
    slug: String,
    display_name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    sort_order: Option<i32>,
}

async fn create_issue_label(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateIssueLabelBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let slug = normalize_issue_label_slug(&body.slug).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e })),
        )
    })?;
    let name = body.display_name.trim().to_string();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "display_name required" })),
        ));
    }
    let sort = body.sort_order.unwrap_or(0);
    let row = sqlx::query_as::<_, IssueLabelRow>(
        r#"INSERT INTO company_issue_labels (company_id, slug, display_name, description, sort_order)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id, company_id, slug, display_name, description, sort_order, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&slug)
    .bind(&name)
    .bind(body.description.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()))
    .bind(sort)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("duplicate key") || msg.contains("unique constraint") {
            (
                StatusCode::CONFLICT,
                Json(json!({ "error": "label slug already exists for this company" })),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": msg })),
            )
        }
    })?;
    Ok((StatusCode::CREATED, Json(json!({ "label": row }))))
}

/// Idempotent starter set (product, engineering, risk, and workflow cues).
async fn seed_default_issue_labels(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    const DEFAULTS: &[(&str, &str, i32)] = &[
        ("bug", "Bug", 10),
        ("feature", "Feature", 20),
        ("chore", "Chore", 30),
        ("docs", "Docs", 40),
        ("infra", "Infra", 50),
        ("customer", "Customer", 60),
        ("security", "Security", 70),
        ("data", "Data", 80),
        ("design", "Design", 90),
        ("research", "Research", 100),
    ];
    for (slug, display_name, ord) in DEFAULTS {
        let _ = sqlx::query(
            r#"INSERT INTO company_issue_labels (company_id, slug, display_name, sort_order)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (company_id, slug) DO NOTHING"#,
        )
        .bind(company_id)
        .bind(*slug)
        .bind(*display_name)
        .bind(*ord)
        .execute(pool)
        .await;
    }
    let rows = sqlx::query_as::<_, IssueLabelRow>(
        r#"SELECT id, company_id, slug, display_name, description, sort_order, created_at::text, updated_at::text
           FROM company_issue_labels WHERE company_id = $1 ORDER BY sort_order, display_name"#,
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
    Ok(Json(json!({ "labels": rows })))
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    };
    Ok(Json(json!({ "task": task })))
}

#[derive(Deserialize, Default)]
struct PatchTaskContextBody {
    #[serde(default)]
    specification: Option<String>,
    #[serde(default)]
    workspace_attachment_paths: Option<Vec<String>>,
    #[serde(default)]
    capability_refs: Option<Vec<Value>>,
}

async fn patch_task_context(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<PatchTaskContextBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if body.specification.is_none()
        && body.workspace_attachment_paths.is_none()
        && body.capability_refs.is_none()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "specification, workspace_attachment_paths, or capability_refs required" }),
            ),
        ));
    }
    let ws_bind: Option<SqlxJson<Value>> = body
        .workspace_attachment_paths
        .as_ref()
        .map(|p| SqlxJson(workspace_attachment_paths_json(Some(p.clone()))));
    let caps_bind: Option<SqlxJson<Value>> = if let Some(ref c) = body.capability_refs {
        Some(SqlxJson(
            normalize_capability_refs(Some(c.clone()))
                .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?,
        ))
    } else {
        None
    };
    let row = sqlx::query_as::<_, TaskRow>(
        r#"UPDATE tasks SET
            specification = COALESCE($2, specification),
            workspace_attachment_paths = COALESCE($3, workspace_attachment_paths),
            capability_refs = COALESCE($4, capability_refs),
            updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, due_at, blocked_by_task_id, created_at::text"#,
    )
    .bind(task_id)
    .bind(body.specification.as_deref())
    .bind(ws_bind)
    .bind(caps_bind)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(task) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    };
    if body.capability_refs.is_some() {
        let _ = sqlx::query(
            r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
               VALUES ($1, 'task_context_patch', 'task_capability_refs_updated', 'task', $2, $3, 'info')"#,
        )
        .bind(task.company_id)
        .bind(task_id.to_string())
        .bind(SqlxJson(json!({ "capability_refs": task.capability_refs.0 })))
        .execute(pool)
        .await;
    }
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
    requires_human: bool,
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
                        WHEN requires_human THEN 'admin_required'
                        ELSE 'auto'
                      END AS decision_mode,
                      created_at::text,
                      requires_human
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
                        WHEN requires_human THEN 'admin_required'
                        ELSE 'auto'
                      END AS decision_mode,
                      created_at::text,
                      requires_human
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
                      created_at::text,
                      requires_human
               FROM tasks
               WHERE company_id = $1
                 AND state = 'waiting_admin'
               ORDER BY priority DESC, created_at"#
        }
        "blocked" => {
            r#"SELECT id, company_id, title, state, priority, due_at, escalate_after, checked_out_by,
                      'blocked' AS decision_mode,
                      created_at::text,
                      requires_human
               FROM tasks
               WHERE company_id = $1
                 AND state = 'blocked'
               ORDER BY priority DESC, created_at"#
        }
        "human_inbox" => {
            r#"SELECT id, company_id, title, state, priority, due_at, escalate_after, checked_out_by,
                      CASE
                        WHEN state = 'blocked' THEN 'blocked'
                        WHEN state = 'waiting_admin' THEN 'admin_required'
                        WHEN requires_human THEN 'admin_required'
                        ELSE 'auto'
                      END AS decision_mode,
                      created_at::text,
                      requires_human
               FROM tasks
               WHERE company_id = $1
                 AND state NOT IN ('done','closed','cancelled')
                 AND (requires_human OR state IN ('waiting_admin','blocked'))
               ORDER BY priority DESC, created_at"#
        }
        _ => {
            r#"SELECT id, company_id, title, state, priority, due_at, escalate_after, checked_out_by,
                      CASE
                        WHEN state = 'blocked' THEN 'blocked'
                        WHEN state = 'waiting_admin' THEN 'admin_required'
                        WHEN requires_human THEN 'admin_required'
                        ELSE 'auto'
                      END AS decision_mode,
                      created_at::text,
                      requires_human
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
            requires_human = CASE WHEN $2 = 'in_progress' THEN FALSE ELSE requires_human END,
            updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, due_at, blocked_by_task_id, created_at::text"#,
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
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
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": e.to_string()})),
                )
            })?;
        if !ok {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error":"duplicate idempotency key"})),
            ));
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
    .bind(SqlxJson(
        json!({ "decision_mode": decision, "reason": reason }),
    ))
    .bind(decision)
    .bind(if decision == "blocked" {
        "warn"
    } else {
        "info"
    })
    .execute(pool)
    .await;

    Ok(Json(json!({
        "task": task,
        "decision_mode": decision,
    })))
}

#[derive(Deserialize)]
struct PostRequiresHumanBody {
    requires_human: bool,
    #[serde(default)]
    actor: String,
    #[serde(default)]
    reason: String,
}

/// Agents (or operators) set `requires_human` so the item appears in the Paperclip-style human inbox.
async fn post_task_requires_human(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<PostRequiresHumanBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let reason = body.reason.trim();
    let task = sqlx::query_as::<_, TaskRow>(
        r#"UPDATE tasks SET
            requires_human = $2,
            status_reason = CASE
              WHEN $3 = '' THEN status_reason
              ELSE COALESCE(status_reason, '') || ' | human_queue:' || $3
            END,
            updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, due_at, blocked_by_task_id, created_at::text"#,
    )
    .bind(task_id)
    .bind(body.requires_human)
    .bind(reason)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(task) = task else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    };
    let actor = if body.actor.trim().is_empty() {
        "agent"
    } else {
        body.actor.trim()
    };
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, $2, 'task_requires_human', 'task', $3, $4, 'info')"#,
    )
    .bind(task.company_id)
    .bind(actor)
    .bind(task_id.to_string())
    .bind(SqlxJson(json!({
        "requires_human": body.requires_human,
        "reason": reason,
    })))
    .execute(pool)
    .await;

    // Emit task event
    emit_task_event(
        pool,
        task_id,
        task.company_id,
        "requires_human",
        actor,
        json!({ "requires_human": body.requires_human, "reason": reason }),
    ).await;

    // Fire webhook if company has one configured — non-blocking
    if body.requires_human {
        if let Ok(Some(wh_url)) = sqlx::query_scalar::<_, Option<String>>(
            "SELECT webhook_url FROM companies WHERE id = $1",
        )
        .bind(task.company_id)
        .fetch_one(pool)
        .await
        {
            if !wh_url.is_empty() {
                let wh_payload = json!({
                    "event": "task_requires_human",
                    "task_id": task_id,
                    "company_id": task.company_id,
                    "title": task.title,
                    "reason": reason,
                    "actor": actor,
                });
                tokio::spawn(async move {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(10))
                        .build()
                        .unwrap_or_default();
                    let _ = client.post(&wh_url).json(&wh_payload).send().await;
                });
            }
        }
    }

    Ok(Json(json!({ "task": task })))
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
    let company_id: Option<Uuid> = sqlx::query_scalar("SELECT company_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    let Some(company_id) = company_id else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    };
    enforce_runtime_budget_stop(pool, company_id, &agent).await?;
    let row = sqlx::query_as::<_, TaskRow>(
        r#"UPDATE tasks SET
            checked_out_by = $1,
            checked_out_until = NOW() + ($2::bigint * INTERVAL '1 second'),
            state = CASE WHEN state = 'open' THEN 'in_progress' ELSE state END,
            updated_at = NOW()
           WHERE id = $3
             AND (
               checked_out_by IS NULL
               OR checked_out_until < NOW()
               OR lower(trim(checked_out_by)) = lower($1)
             )
           RETURNING id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, due_at, blocked_by_task_id, created_at::text"#,
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

/// `DELETE /api/company/companies/:company_id/tasks/:task_id`
async fn delete_task(
    State(st): State<ConsoleState>,
    Path((company_id, task_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let deleted = sqlx::query_scalar::<_, bool>(
        "DELETE FROM tasks WHERE id = $1 AND company_id = $2 RETURNING TRUE",
    )
    .bind(task_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    if deleted.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(serde::Deserialize)]
struct PatchTaskStateBody {
    state: String,
    #[serde(default)]
    actor: String,
}

/// `PATCH /api/company/tasks/:task_id/state`
async fn patch_task_state(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<PatchTaskStateBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let allowed = ["done", "closed", "cancelled", "open", "in_progress", "blocked", "waiting_admin"];
    let state = body.state.trim().to_lowercase();
    if !allowed.contains(&state.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("state must be one of: {}", allowed.join(", ")) })),
        ));
    }
    let row = sqlx::query_as::<_, TaskRow>(
        r#"UPDATE tasks SET state = $2, updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification,
                     workspace_attachment_paths, capability_refs, state, owner_persona, parent_task_id,
                     spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number,
                     requires_human, due_at, blocked_by_task_id, created_at::text"#,
    )
    .bind(task_id)
    .bind(&state)
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
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    };
    let actor_str = body.actor.trim();
    let actor_str = if actor_str.is_empty() { "operator" } else { actor_str };
    emit_task_event(pool, task_id, t.company_id, "state_change", actor_str,
        json!({ "to": state })).await;
    Ok(Json(json!({ "task": t })))
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
           RETURNING id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state,
                     owner_persona, parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, due_at, blocked_by_task_id, created_at::text"#,
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
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
        r#"SELECT id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state, owner_persona,
                  parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, due_at, blocked_by_task_id, created_at::text
           FROM tasks WHERE id = $1 AND company_id = $2"#,
    )
    .bind(task_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    let Some(parent) = parent else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    };
    let mut kbytes = [0u8; 8];
    kbytes.copy_from_slice(&task_id.as_bytes()[..8]);
    let lock_key = i64::from_be_bytes(kbytes);
    let got_lock: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(lock_key)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;
    if !got_lock {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error":"task spawn already in progress"})),
        ));
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
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;
        if !ok {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error":"duplicate idempotency key"})),
            ));
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
            if !parent
                .title
                .to_ascii_lowercase()
                .contains(&tp.to_ascii_lowercase())
            {
                continue;
            }
        }
        if let Some(owner) = &r.owner_persona {
            if parent
                .owner_persona
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                != owner.to_ascii_lowercase()
            {
                continue;
            }
        }
        let mut tx = pool.begin().await.map_err(|e| {
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
                   (company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state, owner_persona, parent_task_id, spawned_by_rule_id, priority, display_number)
                   VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'open',$9,$10,$11,$12,$13)
                   RETURNING id, company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state, owner_persona,
                             parent_task_id, spawned_by_rule_id, checked_out_by, checked_out_until, priority, display_number, requires_human, due_at, blocked_by_task_id, created_at::text"#,
            )
            .bind(company_id)
            .bind(parent.primary_goal_id)
            .bind(parent.project_id)
            .bind(SqlxJson(parent.goal_ancestry.clone()))
            .bind(title)
            .bind(parent.specification.clone())
            .bind(SqlxJson(parent.workspace_attachment_paths.clone()))
            .bind(SqlxJson(parent.capability_refs.0.clone()))
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
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "from_agent and to_agent required"})),
        ));
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

#[derive(Deserialize, Serialize)]
struct HandoffActionTokenPayload {
    handoff_id: Uuid,
    company_id: Uuid,
    reviewer: String,
    exp: i64,
    nonce: String,
}

#[derive(Deserialize)]
struct IssueHandoffActionTokenBody {
    reviewer: String,
    #[serde(default)]
    expires_minutes: Option<i64>,
}

#[derive(Deserialize)]
struct VerifyHandoffActionTokenBody {
    payload: HandoffActionTokenPayload,
    decision: String,
    signature: String,
    #[serde(default)]
    notes: String,
}

fn approval_action_secret() -> Option<String> {
    std::env::var("HSM_APPROVAL_ACTION_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn sign_handoff_action(
    secret: &str,
    payload: &HandoffActionTokenPayload,
    decision: &str,
) -> Result<String, String> {
    let payload_json = serde_json::to_string(payload).map_err(|e| format!("serialize payload: {e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b"|");
    hasher.update(payload_json.as_bytes());
    hasher.update(b"|");
    hasher.update(decision.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut v = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        v |= x ^ y;
    }
    v == 0
}

async fn audit_security_action(
    pool: &PgPool,
    company_id: Uuid,
    actor: &str,
    action: &str,
    subject_type: &str,
    subject_id: &str,
    payload: Value,
) {
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, $2, $3, $4, $5, $6, 'high')"#,
    )
    .bind(company_id)
    .bind(actor)
    .bind(action)
    .bind(subject_type)
    .bind(subject_id)
    .bind(SqlxJson(payload))
    .execute(pool)
    .await;
}

async fn apply_handoff_review(
    pool: &PgPool,
    handoff_id: Uuid,
    expected_company_id: Option<Uuid>,
    decision: &str,
    reviewer: &str,
    notes: &str,
) -> Result<TaskHandoffRow, (StatusCode, Json<Value>)> {
    let next = match decision {
        "accept" | "accepted" => "accepted",
        "reject" | "rejected" => "rejected",
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"decision must be accept|reject"})),
            ));
        }
    };
    let row = sqlx::query_as::<_, TaskHandoffRow>(
        r#"UPDATE task_handoffs
           SET status = $2, reviewed_at = NOW(), reviewed_by = $3, notes = COALESCE(NULLIF($4,''), notes)
           WHERE id = $1
             AND status = 'pending_review'
             AND ($5::uuid IS NULL OR company_id = $5)
           RETURNING id, company_id, task_id, from_agent, to_agent, handoff_contract, review_contract, status, notes,
                     created_at::text, reviewed_at, reviewed_by"#,
    )
    .bind(handoff_id)
    .bind(next)
    .bind(reviewer.trim())
    .bind(notes.trim())
    .bind(expected_company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    let Some(h) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"handoff not found"})),
        ));
    };
    Ok(h)
}

async fn issue_handoff_action_token(
    State(st): State<ConsoleState>,
    Path(handoff_id): Path<Uuid>,
    Json(body): Json<IssueHandoffActionTokenBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if body.reviewer.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"reviewer is required"})),
        ));
    }
    let secret = approval_action_secret().ok_or_else(|| {
        (
            StatusCode::FAILED_DEPENDENCY,
            Json(json!({"error":"HSM_APPROVAL_ACTION_SECRET is not configured"})),
        )
    })?;
    let exists: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT id, company_id FROM task_handoffs WHERE id = $1")
        .bind(handoff_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))))?;
    let Some((_hid, company_id)) = exists else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"handoff not found"})),
        ));
    };
    let mins = body.expires_minutes.unwrap_or(30).clamp(1, 240);
    let payload = HandoffActionTokenPayload {
        handoff_id,
        company_id,
        reviewer: body.reviewer.trim().to_string(),
        exp: (Utc::now() + chrono::Duration::minutes(mins)).timestamp(),
        nonce: Uuid::new_v4().to_string(),
    };
    let accept_signature = sign_handoff_action(&secret, &payload, "accept").map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e})),
        )
    })?;
    let reject_signature = sign_handoff_action(&secret, &payload, "reject").map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e})),
        )
    })?;

    Ok(Json(json!({
        "payload": payload,
        "actions": [
            { "decision": "accept", "signature": accept_signature },
            { "decision": "reject", "signature": reject_signature }
        ],
        "expires_minutes": mins
    })))
}

async fn verify_handoff_action_token(
    State(st): State<ConsoleState>,
    Json(body): Json<VerifyHandoffActionTokenBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if Utc::now().timestamp() > body.payload.exp {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({"error":"action token expired"}))));
    }
    let decision = body.decision.trim().to_ascii_lowercase();
    if decision != "accept" && decision != "reject" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"decision must be accept|reject"})),
        ));
    }
    let secret = approval_action_secret().ok_or_else(|| {
        (
            StatusCode::FAILED_DEPENDENCY,
            Json(json!({"error":"HSM_APPROVAL_ACTION_SECRET is not configured"})),
        )
    })?;
    let expected = sign_handoff_action(&secret, &body.payload, &decision).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e})),
        )
    })?;
    if !constant_time_eq(body.signature.trim(), &expected) {
        return Err((StatusCode::UNAUTHORIZED, Json(json!({"error":"invalid action signature"}))));
    }
    let claimed: Option<bool> = sqlx::query_scalar(
        r#"INSERT INTO handoff_action_nonces (nonce, handoff_id, company_id, used_at)
           VALUES ($1, $2, $3, NOW())
           ON CONFLICT (nonce) DO NOTHING
           RETURNING true"#,
    )
    .bind(body.payload.nonce.trim())
    .bind(body.payload.handoff_id)
    .bind(body.payload.company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;
    if claimed.is_none() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error":"approval action already used"})),
        ));
    }
    let row = apply_handoff_review(
        pool,
        body.payload.handoff_id,
        Some(body.payload.company_id),
        &decision,
        &body.payload.reviewer,
        &body.notes,
    )
    .await?;
    audit_security_action(
        pool,
        row.company_id,
        &body.payload.reviewer,
        "handoff_review_verified",
        "task_handoff",
        &row.id.to_string(),
        json!({
            "decision": decision,
            "verified": true,
            "task_id": row.task_id,
        }),
    )
    .await;
    Ok(Json(json!({ "handoff": row, "verified": true })))
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
    let h = apply_handoff_review(pool, handoff_id, None, &decision, &body.reviewer, &body.notes).await?;
    audit_security_action(
        pool,
        h.company_id,
        body.reviewer.trim(),
        "handoff_review",
        "task_handoff",
        &h.id.to_string(),
        json!({
            "decision": decision,
            "verified": false,
            "task_id": h.task_id,
        }),
    )
    .await;
    Ok(Json(json!({ "handoff": h })))
}

async fn get_runtime_activity() -> Json<Value> {
    let snap = crate::runtime_control::activity_snapshot();
    Json(json!({
        "activity": snap,
        "idle_for_ms": crate::runtime_control::idle_for_ms(),
    }))
}

// ── Company dashboard ─────────────────────────────────────────────────────────

async fn get_company_dashboard(
    Path(company_id): Path<Uuid>,
    State(st): State<ConsoleState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };

    let running: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM tasks WHERE company_id=$1 AND state='in_progress'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let stuck: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM tasks t
           WHERE t.company_id=$1 AND t.state='in_progress'
             AND t.updated_at < now() - interval '2 hours'
             AND NOT EXISTS (
                 SELECT 1 FROM agent_runs ar
                 WHERE ar.task_id = t.id AND ar.status = 'running'
             )"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let completed_24h: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM tasks
           WHERE company_id=$1 AND state IN ('done','closed')
             AND updated_at > now() - interval '24 hours'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let needs_human: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM tasks
           WHERE company_id=$1 AND requires_human=true
             AND state NOT IN ('done','closed','cancelled')"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    // Latest goal coverage snapshot
    #[derive(sqlx::FromRow)]
    struct CoverageRow {
        total_tasks: i32,
        tasks_with_goal: i32,
        coverage_pct: f64,
    }
    let coverage = sqlx::query_as::<_, CoverageRow>(
        r#"SELECT total_tasks, tasks_with_goal, coverage_pct::float8
           FROM goal_coverage_stats
           WHERE company_id=$1
           ORDER BY computed_at DESC LIMIT 1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let active_agents: Vec<String> = sqlx::query_scalar::<_, String>(
        r#"SELECT DISTINCT ar.actor FROM agent_runs ar
           WHERE ar.company_id=$1 AND ar.status='running'"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    #[derive(sqlx::FromRow)]
    struct RecentTask {
        id: Uuid,
        title: String,
        state: String,
        updated_at: chrono::DateTime<chrono::Utc>,
    }
    let recent = sqlx::query_as::<_, RecentTask>(
        r#"SELECT id, title, state, updated_at
           FROM tasks
           WHERE company_id=$1 AND state IN ('done','closed','cancelled')
           ORDER BY updated_at DESC LIMIT 5"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // Tasks overdue (due_at in the past and not done)
    let overdue: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM tasks
           WHERE company_id=$1
             AND due_at IS NOT NULL AND due_at < now()
             AND state NOT IN ('done','closed','cancelled')"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    Ok(Json(json!({
        "company_id": company_id,
        "running_tasks": running,
        "stuck_tasks": stuck,
        "overdue_tasks": overdue,
        "needs_human": needs_human,
        "completed_last_24h": completed_24h,
        "active_agents": active_agents,
        "goal_coverage": coverage.as_ref().map(|c| json!({
            "total_tasks": c.total_tasks,
            "tasks_with_goal": c.tasks_with_goal,
            "coverage_pct": c.coverage_pct,
        })).unwrap_or(json!(null)),
        "recent_completions": recent.iter().map(|t| json!({
            "id": t.id,
            "title": t.title,
            "state": t.state,
            "updated_at": t.updated_at,
        })).collect::<Vec<_>>(),
        "generated_at": Utc::now(),
    })))
}

// ── Task event log ─────────────────────────────────────────────────────────────

async fn get_task_events(
    Path(task_id): Path<Uuid>,
    State(st): State<ConsoleState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    #[derive(sqlx::FromRow)]
    struct TaskEvent {
        id: Uuid,
        event_type: String,
        actor: String,
        payload: SqlxJson<Value>,
        created_at: chrono::DateTime<chrono::Utc>,
    }
    let events = sqlx::query_as::<_, TaskEvent>(
        r#"SELECT id, event_type, actor, payload, created_at
           FROM task_events WHERE task_id=$1
           ORDER BY created_at ASC LIMIT 500"#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))))?;

    let items: Vec<Value> = events.into_iter().map(|e| json!({
        "id": e.id,
        "event_type": e.event_type,
        "actor": e.actor,
        "payload": e.payload.0,
        "created_at": e.created_at,
    })).collect();
    Ok(Json(json!({ "events": items, "task_id": task_id })))
}

/// SSE stream of task events — polls the DB every 2 s and emits new rows.
async fn stream_task_events(
    Path(task_id): Path<Uuid>,
    State(st): State<ConsoleState>,
) -> impl IntoResponse {
    use futures_util::stream;
    let pool = match st.company_db.clone() {
        Some(p) => p,
        None => {
            return Sse::new(stream::iter(Vec::<Result<Event, std::convert::Infallible>>::new()))
                .into_response()
        }
    };

    #[derive(sqlx::FromRow)]
    struct EvRow {
        id: Uuid,
        event_type: String,
        actor: String,
        payload: SqlxJson<Value>,
        created_at: chrono::DateTime<chrono::Utc>,
    }

    // Each tick emits one SSE event: a JSON array of new rows (or a keep-alive comment).
    let out = stream::unfold(
        (pool, task_id, None::<chrono::DateTime<chrono::Utc>>),
        |(pool, task_id, since)| async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            let rows: Vec<EvRow> = if let Some(s) = since {
                sqlx::query_as::<_, EvRow>(
                    r#"SELECT id, event_type, actor, payload, created_at
                       FROM task_events WHERE task_id=$1 AND created_at > $2
                       ORDER BY created_at ASC LIMIT 100"#,
                )
                .bind(task_id)
                .bind(s)
                .fetch_all(&pool)
                .await
                .unwrap_or_default()
            } else {
                sqlx::query_as::<_, EvRow>(
                    r#"SELECT id, event_type, actor, payload, created_at
                       FROM task_events WHERE task_id=$1
                       ORDER BY created_at ASC LIMIT 100"#,
                )
                .bind(task_id)
                .fetch_all(&pool)
                .await
                .unwrap_or_default()
            };

            let new_since = rows.last().map(|r| r.created_at).or(since);
            let evt: Result<Event, std::convert::Infallible> = if rows.is_empty() {
                Ok(Event::default().comment("ping"))
            } else {
                let batch: Vec<Value> = rows.into_iter().map(|r| json!({
                    "id": r.id, "event_type": r.event_type,
                    "actor": r.actor, "payload": r.payload.0,
                    "created_at": r.created_at,
                })).collect();
                let s = serde_json::to_string(&batch).unwrap_or_else(|_| "[]".to_string());
                Ok(Event::default().data(s))
            };
            Some((evt, (pool, task_id, new_since)))
        },
    );

    Sse::new(out).keep_alive(KeepAlive::default()).into_response()
}

async fn stream_runtime_events() -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = crate::runtime_control::subscribe_completions();
    let out = stream::unfold(rx, |mut rx| async move {
        let evt = match rx.recv().await {
            Ok(v) => v,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                crate::runtime_control::CompletionEvent {
                    event_type: "lagged".to_string(),
                    task_key: None,
                    tool_name: None,
                    call_id: None,
                    success: false,
                    message: "runtime event stream lagged".to_string(),
                    ts_ms: Utc::now().timestamp_millis(),
                    input: None,
                    stream_event: None,
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
        };
        let json = serde_json::to_string(&evt).unwrap_or_else(|_| "{}".to_string());
        Some((Ok(Event::default().data(json)), rx))
    });
    Sse::new(out).keep_alive(KeepAlive::default())
}

async fn runtime_portability_matrix() -> Json<Value> {
    Json(json!({
        "backends": [
            {
                "key": "local",
                "status": "available",
                "isolation": "host process",
                "hibernation": "manual",
                "notes": "Best for local iteration and debugging."
            },
            {
                "key": "docker",
                "status": "available",
                "isolation": "container",
                "hibernation": "manual",
                "notes": "Strong baseline isolation for tenant boundaries."
            },
            {
                "key": "ssh",
                "status": "available",
                "isolation": "remote host",
                "hibernation": "host-managed",
                "notes": "Good for low-cost VPS deployment."
            },
            {
                "key": "daytona",
                "status": "integratable",
                "isolation": "workspace runtime",
                "hibernation": "native",
                "notes": "Supports near-idle cost profile with resume semantics."
            },
            {
                "key": "modal",
                "status": "integratable",
                "isolation": "serverless runtime",
                "hibernation": "native",
                "notes": "Good fit for burst compute and idle-to-zero economics."
            },
            {
                "key": "singularity",
                "status": "integratable",
                "isolation": "containerized runtime",
                "hibernation": "host-managed",
                "notes": "Useful for HPC and controlled enterprise environments."
            }
        ],
        "positioning": {
            "one_person_company": "Prioritize ssh/daytona/modal to keep idle cost low.",
            "enterprise": "Prefer docker/singularity with strict policy and audit controls."
        }
    }))
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
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"title and scope required"})),
        ));
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
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"decision must be promote|revert"})),
            ));
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"improvement run not found"})),
        ));
    };
    if let Some(k) = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let payload = json!({ "run_id": run_id, "decision": next, "actor": body.actor.trim() });
        let ok = register_idempotency(
            pool,
            current.company_id,
            "improvement_decision",
            k,
            &payload,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;
        if !ok {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error":"duplicate idempotency key"})),
            ));
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
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"promotion gate failed: min_eval_samples"})),
            ));
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
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"promotion gate failed: regression threshold"})),
            ));
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
        if requires_reviewer
            && (risk == "high" || risk == "critical")
            && body.actor.trim().is_empty()
        {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"promotion gate failed: reviewer required for high risk"})),
            ));
        }
    }
    let rationale = if body.reason.trim().is_empty() {
        if next == "promoted" {
            format!("Promoted run '{}' after gates passed.", current.title)
        } else {
            format!(
                "Reverted run '{}' due to risk/performance decision.",
                current.title
            )
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"improvement run not found"})),
        ));
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
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;
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
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"contract_id and version required"})),
        ));
    }
    let status = body
        .status
        .as_deref()
        .unwrap_or("active")
        .trim()
        .to_ascii_lowercase();
    if !matches!(status.as_str(), "active" | "deprecated" | "sunset") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"status must be active|deprecated|sunset"})),
        ));
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
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": e.to_string()})),
        )
    })?;
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
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"status must be active|deprecated|sunset"})),
        ));
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
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;
    let Some(v) = row else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"contract version not found"})),
        ));
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
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"vertical and connector_provider required"})),
        ));
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
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"item_key and item_label required"})),
        ));
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error":"checklist item not found"})),
        ));
    };
    Ok(Json(json!({ "item": item })))
}

fn go_live_template(vertical: &str) -> Vec<(&'static str, &'static str)> {
    match vertical.trim().to_ascii_lowercase().as_str() {
        "ecommerce" => vec![
            (
                "contracts_signed",
                "Customer terms and refund policy approved",
            ),
            (
                "refund_guardrail",
                "Refund thresholds and approvers configured",
            ),
            (
                "channel_integrations",
                "Shopify/helpdesk/email connectors verified",
            ),
            ("handoff_queue_ready", "Handoff review queue staffed"),
        ],
        "property_management" => vec![
            (
                "legal_escalation",
                "Legal/fair-housing escalation path approved",
            ),
            (
                "maintenance_sla",
                "Emergency/standard maintenance SLA configured",
            ),
            (
                "tenant_comms_policy",
                "Tenant communication templates approved",
            ),
            (
                "incident_runbook",
                "Incident and escalation runbook reviewed",
            ),
        ],
        _ => vec![
            (
                "owner_signoff",
                "Owner sign-off on policy and risk settings",
            ),
            ("approval_matrix", "Approval matrix configured and tested"),
            ("connector_smoke_test", "Core connectors smoke-tested"),
            (
                "ops_oncall",
                "Admin on-call and escalation contact assigned",
            ),
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
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM goals WHERE id = $1 AND company_id = $2)")
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
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "goal not found" })),
        ));
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

    let title_upd = body
        .title
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
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
           RETURNING id, company_id, parent_goal_id, title, description, status, paperclip_goal_id, paperclip_snapshot, created_at::text"#,
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

/// Single read model for workspace Intelligence: Postgres company_os only (goals + tasks + spend + workforce + workflow signals).
async fn company_intelligence_summary(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };

    let (goals_total, goals_active): (i64, i64) = sqlx::query_as(
        r#"SELECT COUNT(*)::bigint,
                  COUNT(*) FILTER (WHERE lower(trim(status)) IN ('active', 'open'))::bigint
           FROM goals WHERE company_id = $1"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let (
        tasks_total,
        tasks_open,
        tasks_in_progress,
        tasks_done,
        tasks_requires_human,
        tasks_checked_out,
    ): (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT
            COUNT(*)::bigint,
            COUNT(*) FILTER (WHERE state = 'open')::bigint,
            COUNT(*) FILTER (WHERE state = 'in_progress')::bigint,
            COUNT(*) FILTER (WHERE state IN ('done', 'closed'))::bigint,
            COUNT(*) FILTER (
              WHERE requires_human = true
                AND state NOT IN ('done', 'closed', 'cancelled')
            )::bigint,
            COUNT(*) FILTER (
              WHERE checked_out_by IS NOT NULL
                AND (checked_out_until IS NULL OR checked_out_until > NOW())
            )::bigint
           FROM tasks WHERE company_id = $1"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let workforce_agents: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM company_agents
           WHERE company_id = $1 AND lower(trim(status)) <> 'terminated'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let spend_total: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::float8 FROM spend_events WHERE company_id = $1",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let workflow_feed = sqlx::query_as::<_, GovRow>(
        r#"SELECT id, actor, action, subject_type, subject_id, payload, created_at::text
           FROM governance_events
           WHERE company_id = $1
             AND action IN (
               'task_created',
               'task_checkout_agent_profile',
               'release_checkout',
               'task_requires_human',
               'task_spawn_subagents',
               'task_policy_decision',
               'task_run_terminal',
               'task_capability_refs_updated',
               'paperclip_goals_synced',
               'paperclip_dris_synced'
             )
           ORDER BY created_at DESC
           LIMIT 100"#,
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

    // Recent signals from the intelligence layer
    let signal_counts: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT kind, COUNT(*)::bigint
           FROM intelligence_signals
           WHERE company_id = $1
             AND created_at > now() - interval '7 days'
           GROUP BY kind
           ORDER BY COUNT(*) DESC"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let recent_signals: Vec<serde_json::Value> = sqlx::query_as::<_, (Uuid, String, String, f32, Option<bool>, Option<String>, String)>(
        r#"SELECT id, kind, description, severity, composition_success, escalated_to, created_at::text
           FROM intelligence_signals
           WHERE company_id = $1
           ORDER BY created_at DESC
           LIMIT 30"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(id, kind, description, severity, success, escalated_to, created_at)| {
        json!({
            "id": id,
            "kind": kind,
            "description": description,
            "severity": severity,
            "composition_success": success,
            "escalated_to": escalated_to,
            "created_at": created_at,
        })
    })
    .collect();

    let signal_summary: serde_json::Map<String, serde_json::Value> = signal_counts
        .into_iter()
        .map(|(k, v)| (k, json!(v)))
        .collect();

    Ok(Json(json!({
        "company_id": company_id,
        "source": "postgres_company_os",
        "goals": { "total": goals_total, "active": goals_active },
        "tasks": {
            "total": tasks_total,
            "open": tasks_open,
            "in_progress": tasks_in_progress,
            "done_or_closed": tasks_done,
            "requires_human_open": tasks_requires_human,
            "checked_out_now": tasks_checked_out,
        },
        "workforce": { "agents_non_terminated": workforce_agents },
        "spend": { "total_usd": spend_total },
        "workflow_feed": workflow_feed,
        "signals": {
            "recent": recent_signals,
            "by_kind_7d": signal_summary,
        },
    })))
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

struct LoadedOpsOverview {
    path: Option<String>,
    error: Option<String>,
    summary: Value,
    org: Value,
    heartbeats: Value,
    tickets: Value,
    config: Option<crate::personal::ops_config::OperationsConfig>,
}

fn load_company_ops_overview(hsmii_home: Option<&str>) -> LoadedOpsOverview {
    let Some(home_str) = hsmii_home.map(str::trim).filter(|s| !s.is_empty()) else {
        return LoadedOpsOverview {
            path: None,
            error: Some("company has no hsmii_home configured".to_string()),
            summary: Value::Null,
            org: Value::Null,
            heartbeats: Value::Null,
            tickets: Value::Null,
            config: None,
        };
    };
    let home = StdPath::new(home_str);
    let path = resolve_ops_config_path(home);
    let path_string = path.display().to_string();
    if !path.is_file() {
        return LoadedOpsOverview {
            path: Some(path_string),
            error: Some("operations config not found".to_string()),
            summary: Value::Null,
            org: Value::Null,
            heartbeats: Value::Null,
            tickets: Value::Null,
            config: None,
        };
    }
    match load_ops_config(&path).and_then(|cfg| {
        cfg.validate()?;
        Ok(cfg)
    }) {
        Ok(cfg) => LoadedOpsOverview {
            path: Some(path_string),
            error: None,
            summary: cfg.summary_without_tickets(),
            org: serde_json::to_value(&cfg.org).unwrap_or(Value::Null),
            heartbeats: serde_json::to_value(&cfg.heartbeats).unwrap_or(Value::Null),
            tickets: serde_json::to_value(&cfg.tickets).unwrap_or(Value::Null),
            config: Some(cfg),
        },
        Err(e) => LoadedOpsOverview {
            path: Some(path_string),
            error: Some(e.to_string()),
            summary: Value::Null,
            org: Value::Null,
            heartbeats: Value::Null,
            tickets: Value::Null,
            config: None,
        },
    }
}

async fn enforce_runtime_budget_stop(
    pool: &PgPool,
    company_id: Uuid,
    agent_ref: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let company_home: Option<String> =
        sqlx::query_scalar("SELECT hsmii_home FROM companies WHERE id = $1")
            .bind(company_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?
            .flatten();
    let ops = load_company_ops_overview(company_home.as_deref());
    let Some(cfg) = ops.config else {
        return Ok(());
    };
    if cfg.budgets.is_empty() {
        return Ok(());
    }
    let spend_total: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount), 0)::float8 FROM spend_events WHERE company_id = $1",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);
    let spend_by_agent: Vec<(String, f64)> = sqlx::query_as(
        r#"SELECT agent_ref, COALESCE(SUM(amount), 0)::float8
           FROM spend_events WHERE company_id = $1 GROUP BY agent_ref"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for budget in cfg.budgets {
        if !budget.hard_stop || budget.cap_monthly <= 0.0 {
            continue;
        }
        let usage = match budget.scope {
            BudgetScope::Company => Some(spend_total),
            BudgetScope::Role => budget.role_id.as_ref().and_then(|rid| {
                if rid == agent_ref {
                    spend_by_agent
                        .iter()
                        .find_map(|(ref_name, amount)| (ref_name == agent_ref).then_some(*amount))
                } else {
                    None
                }
            }),
        };
        if let Some(used) = usage {
            if used >= budget.cap_monthly {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "error": "hard_stop budget exceeded",
                        "budget_id": budget.id,
                        "scope": budget.scope,
                        "role_id": budget.role_id,
                        "usage_usd": used,
                        "cap_monthly_usd": budget.cap_monthly,
                        "agent_ref": agent_ref,
                    })),
                ));
            }
        }
    }
    Ok(())
}

async fn mirror_ops_tickets_to_tasks(
    pool: &PgPool,
    company_id: Uuid,
    tickets: &[Ticket],
) -> Result<Value, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut created = 0usize;
    let mut updated = 0usize;
    for ticket in tickets {
        let ticket_id = ticket.id.trim();
        let title = ticket.title.trim();
        if ticket_id.is_empty() || title.is_empty() {
            continue;
        }
        let capability = json!([{ "kind": "ticket", "ref": ticket_id }]);
        let spec = format!(
            "operations ticket mirror\nrequester_role={}\nstate={}\nbudget_ticket_usd={}\ndelegated_to={}",
            ticket.requester_role,
            ticket.state,
            ticket
                .budget_ticket_usd
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".into()),
            ticket.delegated_to.clone().unwrap_or_default()
        );
        let desired_state = match ticket.state.trim().to_ascii_lowercase().as_str() {
            "done" | "closed" | "cancelled" => "done",
            "blocked" => "blocked",
            "waiting_admin" | "admin_required" => "waiting_admin",
            "in_progress" => "in_progress",
            _ => "open",
        };
        let existing: Option<(Uuid, String)> = sqlx::query_as(
            r#"SELECT id, state
               FROM tasks
               WHERE company_id = $1
                 AND capability_refs @> $2::jsonb
               ORDER BY created_at DESC
               LIMIT 1"#,
        )
        .bind(company_id)
        .bind(SqlxJson(capability.clone()))
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((task_id, current_state)) = existing {
            let next_state = if matches!(current_state.as_str(), "done" | "closed" | "cancelled") {
                current_state
            } else {
                desired_state.to_string()
            };
            sqlx::query(
                r#"UPDATE tasks
                   SET title = $2,
                       specification = $3,
                       owner_persona = $4,
                       capability_refs = $5,
                       state = $6,
                       updated_at = NOW()
                   WHERE id = $1"#,
            )
            .bind(task_id)
            .bind(title)
            .bind(&spec)
            .bind(ticket.owner_role.trim())
            .bind(SqlxJson(capability))
            .bind(next_state)
            .execute(&mut *tx)
            .await?;
            updated += 1;
        } else {
            let display_n = next_task_display_number_tx(&mut tx, company_id).await?;
            sqlx::query(
                r#"INSERT INTO tasks
                   (company_id, title, specification, workspace_attachment_paths, capability_refs, state, owner_persona, priority, display_number)
                   VALUES ($1, $2, $3, $4, $5, $6, $7, 2, $8)"#,
            )
            .bind(company_id)
            .bind(title)
            .bind(&spec)
            .bind(SqlxJson(json!([])))
            .bind(SqlxJson(capability))
            .bind(desired_state)
            .bind(ticket.owner_role.trim())
            .bind(display_n)
            .execute(&mut *tx)
            .await?;
            created += 1;
        }
    }
    tx.commit().await?;
    Ok(json!({
        "configured_tickets": tickets.len(),
        "created": created,
        "updated": updated,
    }))
}

struct AuditSummary {
    available: bool,
    payload: Value,
}

async fn load_company_audit_summary(hsmii_home: Option<&str>) -> AuditSummary {
    let Some(home_str) = hsmii_home.map(str::trim).filter(|s| !s.is_empty()) else {
        return AuditSummary {
            available: false,
            payload: json!({ "error": "company has no hsmii_home configured" }),
        };
    };
    let path = StdPath::new(home_str)
        .join("memory")
        .join("task_trail.jsonl");
    if !path.is_file() {
        return AuditSummary {
            available: false,
            payload: json!({
                "path": path.display().to_string(),
                "error": "task_trail.jsonl not found",
            }),
        };
    }
    let raw = match tokio::fs::read_to_string(&path).await {
        Ok(raw) => raw,
        Err(e) => {
            return AuditSummary {
                available: false,
                payload: json!({
                    "path": path.display().to_string(),
                    "error": e.to_string(),
                }),
            }
        }
    };
    let mut turns = 0usize;
    let mut tool_prompt_tokens = 0f64;
    let mut skill_prompt_tokens = 0f64;
    let mut exposed_tools = 0f64;
    let mut hidden_tools = 0f64;
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        turns += 1;
        tool_prompt_tokens += v
            .get("tool_prompt_tokens")
            .and_then(|v| v.as_f64())
            .or_else(|| {
                v.get("tool_prompt_tokens")
                    .and_then(|v| v.as_i64())
                    .map(|n| n as f64)
            })
            .unwrap_or(0.0);
        skill_prompt_tokens += v
            .get("skill_prompt_tokens")
            .and_then(|v| v.as_f64())
            .or_else(|| {
                v.get("skill_prompt_tokens")
                    .and_then(|v| v.as_i64())
                    .map(|n| n as f64)
            })
            .unwrap_or(0.0);
        exposed_tools += v
            .get("tool_prompt_exposed_count")
            .and_then(|v| v.as_f64())
            .or_else(|| {
                v.get("tool_prompt_exposed_count")
                    .and_then(|v| v.as_i64())
                    .map(|n| n as f64)
            })
            .unwrap_or(0.0);
        hidden_tools += v
            .get("tool_prompt_hidden_count")
            .and_then(|v| v.as_f64())
            .or_else(|| {
                v.get("tool_prompt_hidden_count")
                    .and_then(|v| v.as_i64())
                    .map(|n| n as f64)
            })
            .unwrap_or(0.0);
    }
    let denom = if turns == 0 { 1.0 } else { turns as f64 };
    AuditSummary {
        available: turns > 0,
        payload: json!({
            "path": path.display().to_string(),
            "turns": turns,
            "avg_tool_prompt_tokens": tool_prompt_tokens / denom,
            "avg_skill_prompt_tokens": skill_prompt_tokens / denom,
            "avg_exposed_tools": exposed_tools / denom,
            "avg_hidden_tools": hidden_tools / denom,
        }),
    }
}

async fn company_ops_overview(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let company = sqlx::query_as::<_, CompanyRow>(
        r#"SELECT id, slug, display_name, hsmii_home, issue_key_prefix,
                  context_markdown, webhook_url, created_at::text
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
    let Some(company) = company else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "company not found" })),
        ));
    };
    let profile: Option<Value> = sqlx::query_scalar(
        r#"SELECT jsonb_build_object(
                'company_id', company_id,
                'industry', industry,
                'business_model', business_model,
                'channel_mix', channel_mix,
                'compliance_level', compliance_level,
                'size_tier', size_tier,
                'inferred', inferred,
                'profile_source', profile_source,
                'metadata', metadata,
                'created_at', created_at::text,
                'updated_at', updated_at::text
            )
           FROM company_profiles
           WHERE company_id = $1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    let goals_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM goals WHERE company_id = $1")
        .bind(company_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let agents_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM company_agents WHERE company_id = $1 AND status <> 'terminated'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let governance_recent: Vec<GovRow> = sqlx::query_as::<_, GovRow>(
        r#"SELECT id, actor, action, subject_type, subject_id, payload, created_at::text
           FROM governance_events WHERE company_id = $1 ORDER BY created_at DESC LIMIT 20"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let spend_rows: Vec<(String, f64)> = sqlx::query_as(
        r#"SELECT kind, COALESCE(SUM(amount), 0)::float8
           FROM spend_events WHERE company_id = $1 GROUP BY kind ORDER BY kind"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let spend_total_usd: f64 = spend_rows.iter().map(|(_, amount)| *amount).sum();
    let spend_by_agent: Vec<(String, f64)> = sqlx::query_as(
        r#"SELECT agent_ref, COALESCE(SUM(amount), 0)::float8
           FROM spend_events WHERE company_id = $1 GROUP BY agent_ref ORDER BY agent_ref"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let ops = load_company_ops_overview(company.hsmii_home.as_deref());
    let ticket_sync = if let Some(cfg) = ops.config.as_ref() {
        mirror_ops_tickets_to_tasks(pool, company_id, &cfg.tickets)
            .await
            .unwrap_or_else(|e| json!({ "error": e.to_string() }))
    } else {
        json!({ "skipped": true, "reason": "operations config unavailable" })
    };
    let tasks_total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE company_id = $1")
        .bind(company_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let tasks_open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE company_id = $1 AND state NOT IN ('done','closed','cancelled')",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let tasks_requires_human: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE company_id = $1 AND requires_human = true AND state NOT IN ('done','closed','cancelled')",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let avg_cycle_hours_30d: f64 = sqlx::query_scalar(
        r#"SELECT COALESCE(AVG(EXTRACT(EPOCH FROM (updated_at - created_at)) / 3600.0), 0)::float8
           FROM tasks
           WHERE company_id = $1
             AND state IN ('done','closed')
             AND updated_at >= NOW() - INTERVAL '30 days'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0.0);
    let manual_interventions_7d: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)
           FROM governance_events
           WHERE company_id = $1
             AND action IN ('task_requires_human', 'task_policy_decision')
             AND created_at >= NOW() - INTERVAL '7 days'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let active_tasks_7d: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM tasks WHERE company_id = $1 AND created_at >= NOW() - INTERVAL '7 days'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let retries_7d: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM run_failure_events WHERE company_id = $1 AND created_at >= NOW() - INTERVAL '7 days'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let tasks_closed_14d: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM tasks
           WHERE company_id = $1
             AND state IN ('done','closed')
             AND updated_at >= NOW() - INTERVAL '14 days'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let profile_size_tier = profile
        .as_ref()
        .and_then(|v| v.get("size_tier"))
        .and_then(|v| v.as_str())
        .unwrap_or("solo")
        .to_string();
    let profile_business_model = profile
        .as_ref()
        .and_then(|v| v.get("business_model"))
        .and_then(|v| v.as_str())
        .unwrap_or("services")
        .to_string();
    let connected_connectors: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM company_connectors WHERE company_id = $1",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let template_events_30d: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM company_template_adoption_events
           WHERE company_id = $1 AND created_at >= NOW() - INTERVAL '30 days'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let first_completed_hours: Option<f64> = sqlx::query_scalar(
        r#"SELECT EXTRACT(EPOCH FROM (MIN(updated_at) - MIN(created_at))) / 3600.0
           FROM tasks
           WHERE company_id = $1
             AND state IN ('done','closed')"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .ok()
    .flatten();
    let setup_completion_rate = if connected_connectors <= 0 {
        0.0
    } else {
        (connected_connectors as f64 / 6.0).min(1.0)
    };
    let cost_per_resolved_operation = if tasks_closed_14d <= 0 {
        spend_total_usd
    } else {
        spend_total_usd / tasks_closed_14d as f64
    };
    let audit = load_company_audit_summary(company.hsmii_home.as_deref()).await;
    let heartbeat_runtime = if let Some(home_str) = company
        .hsmii_home
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let state_path = heartbeat_state_path(StdPath::new(home_str));
        match load_heartbeat_state(&state_path) {
            Ok(state) => serde_json::to_value(state).unwrap_or(Value::Null),
            Err(e) => json!({
                "path": state_path.display().to_string(),
                "error": e.to_string(),
            }),
        }
    } else {
        Value::Null
    };

    let budgets = match ops.config.as_ref() {
        Some(cfg) => cfg
            .budgets
            .iter()
            .map(|b| {
                let usage_usd = match b.scope {
                    BudgetScope::Company => Some(spend_total_usd),
                    BudgetScope::Role => b.role_id.as_ref().and_then(|rid| {
                        spend_by_agent
                            .iter()
                            .find_map(|(agent_ref, amount)| (agent_ref == rid).then_some(*amount))
                    }),
                };
                let utilization = usage_usd.map(|used| {
                    if b.cap_monthly <= 0.0 {
                        0.0
                    } else {
                        (used / b.cap_monthly).max(0.0)
                    }
                });
                json!({
                    "id": b.id,
                    "scope": b.scope,
                    "role_id": b.role_id,
                    "kind": b.kind,
                    "cap_monthly": b.cap_monthly,
                    "hard_stop": b.hard_stop,
                    "usage_usd": usage_usd,
                    "utilization": utilization,
                    "over_cap": utilization.map(|u| u >= 1.0),
                    "enforcement_ready": matches!(b.scope, BudgetScope::Company) || usage_usd.is_some(),
                })
            })
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };

    Ok(Json(json!({
        "company": company,
        "ops_config": {
            "loaded": ops.config.is_some(),
            "path": ops.path,
            "error": ops.error,
            "summary": ops.summary,
        },
        "profile": profile,
        "overview": {
            "goals_total": goals_total,
            "tasks_total": tasks_total,
            "tasks_open": tasks_open,
            "tasks_requires_human": tasks_requires_human,
            "agents_total": agents_total,
            "spend_total_usd": spend_total_usd,
            "month": format!("{:04}-{:02}", Utc::now().year(), Utc::now().month()),
        },
        "budgets": budgets,
        "heartbeats": {
            "configured": ops.heartbeats,
            "runtime_state": heartbeat_runtime,
        },
        "tickets": ops.tickets,
        "ticket_sync": ticket_sync,
        "org": ops.org,
        "governance_recent": governance_recent,
        "spend": {
            "total_usd": spend_total_usd,
            "by_kind": spend_rows.iter().map(|(kind, amount)| json!({
                "kind": kind,
                "amount_usd": amount,
            })).collect::<Vec<_>>(),
            "by_agent_ref": spend_by_agent.iter().map(|(agent_ref, amount)| json!({
                "agent_ref": agent_ref,
                "amount_usd": amount,
            })).collect::<Vec<_>>(),
        },
        "audit": audit.payload,
        "roi": {
            "avg_cycle_time_hours_30d": avg_cycle_hours_30d,
            "manual_interventions_per_task_7d": if active_tasks_7d <= 0 {
                0.0
            } else {
                manual_interventions_7d as f64 / active_tasks_7d as f64
            },
            "retries_per_task_7d": if active_tasks_7d <= 0 {
                0.0
            } else {
                retries_7d as f64 / active_tasks_7d as f64
            },
            "tasks_closed_per_day_14d": tasks_closed_14d as f64 / 14.0,
            "tasks_created_7d": active_tasks_7d,
            "manual_interventions_7d": manual_interventions_7d,
            "retries_7d": retries_7d,
        },
        "universality": {
            "profile_size_tier": profile_size_tier,
            "profile_business_model": profile_business_model,
            "time_to_first_value_hours": first_completed_hours,
            "setup_completion_rate": setup_completion_rate,
            "template_adoption_events_30d": template_events_30d,
            "cost_per_resolved_operation": cost_per_resolved_operation,
        },
        "integration_status": {
            "agent_budget_enforcement": {
                "configured": ops.config.as_ref().map(|c| !c.budgets.is_empty()).unwrap_or(false),
                "hard_stop_budget_present": budgets.iter().any(|b| b.get("hard_stop").and_then(|v| v.as_bool()).unwrap_or(false)),
                "company_budget_usage_available": !budgets.is_empty(),
            },
            "heartbeat_scheduler": {
                "configured": ops.config.as_ref().map(|c| !c.heartbeats.is_empty()).unwrap_or(false),
                "runtime_present": true,
            },
            "task_ticket_runtime": {
                "implemented": true,
                "task_count": tasks_total,
            },
            "org_chart_role_model": {
                "configured": !ops.org.is_null(),
                "agents_total": agents_total,
            },
            "governance_layer": {
                "implemented": true,
                "recent_events": governance_recent.len(),
            },
            "multi_company_isolation": { "implemented": true },
            "operator_ui": { "implemented": true },
            "portable_company_templates": {
                "workspace_home_configured": company.hsmii_home.is_some(),
                "bundle_export_import": true,
            },
            "persistent_audit_views": {
                "task_trail_available": audit.available,
                "governance_events_available": true,
                "spend_events_available": true,
            },
            "model_routing_policy": {
                "auto_enabled": std::env::var("HSM_MODEL_ROUTING_AUTO")
                    .ok()
                    .map(|v| {
                        let s = v.trim().to_ascii_lowercase();
                        s == "1" || s == "true" || s == "yes" || s == "on"
                    })
                    .unwrap_or(true),
                "policy_source": "llm_risk_routing",
            },
        },
    })))
}

async fn export_company_json(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let bundle = export_bundle(pool, company_id).await.map_err(|e| {
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
    let id = run_import_bundle(pool, req).await.map_err(|e| {
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

fn template_defaults(
    vertical: &str,
) -> (
    &'static str,
    Vec<OnboardWorkflowDraft>,
    Vec<OnboardPolicyDraft>,
) {
    let v = vertical.trim().to_ascii_lowercase();
    match v.as_str() {
        "ecommerce" | "online_commerce" => (
            "ecommerce",
            vec![
                OnboardWorkflowDraft {
                    title: "Customer inquiry triage".into(),
                    owner_role: "support_admin".into(),
                    priority: "high".into(),
                    sla_target: "1h".into(),
                    approval: "auto".into(),
                },
                OnboardWorkflowDraft {
                    title: "Order issue follow-up".into(),
                    owner_role: "ops_admin".into(),
                    priority: "high".into(),
                    sla_target: "same_day".into(),
                    approval: "admin_required".into(),
                },
                OnboardWorkflowDraft {
                    title: "Product content refresh".into(),
                    owner_role: "marketing_admin".into(),
                    priority: "medium".into(),
                    sla_target: "24h".into(),
                    approval: "auto".into(),
                },
            ],
            vec![
                OnboardPolicyDraft {
                    action_type: "send_message".into(),
                    risk_level: "medium".into(),
                    decision_mode: "auto".into(),
                    amount_min: None,
                    amount_max: None,
                    approver_role: "support_admin".into(),
                },
                OnboardPolicyDraft {
                    action_type: "refund".into(),
                    risk_level: "high".into(),
                    decision_mode: "admin_required".into(),
                    amount_min: Some(100.0),
                    amount_max: None,
                    approver_role: "finance_admin".into(),
                },
                OnboardPolicyDraft {
                    action_type: "update_budget".into(),
                    risk_level: "critical".into(),
                    decision_mode: "blocked".into(),
                    amount_min: None,
                    amount_max: None,
                    approver_role: "owner".into(),
                },
            ],
        ),
        "marketing" | "marketing_agency" => (
            "marketing",
            vec![
                OnboardWorkflowDraft {
                    title: "Lead response drafting".into(),
                    owner_role: "account_manager".into(),
                    priority: "high".into(),
                    sla_target: "1h".into(),
                    approval: "auto".into(),
                },
                OnboardWorkflowDraft {
                    title: "Campaign performance summary".into(),
                    owner_role: "ads_manager".into(),
                    priority: "medium".into(),
                    sla_target: "same_day".into(),
                    approval: "auto".into(),
                },
                OnboardWorkflowDraft {
                    title: "Client update email".into(),
                    owner_role: "account_manager".into(),
                    priority: "medium".into(),
                    sla_target: "24h".into(),
                    approval: "admin_required".into(),
                },
            ],
            vec![
                OnboardPolicyDraft {
                    action_type: "send_message".into(),
                    risk_level: "low".into(),
                    decision_mode: "auto".into(),
                    amount_min: None,
                    amount_max: None,
                    approver_role: "account_manager".into(),
                },
                OnboardPolicyDraft {
                    action_type: "publish_campaign".into(),
                    risk_level: "high".into(),
                    decision_mode: "admin_required".into(),
                    amount_min: None,
                    amount_max: None,
                    approver_role: "ads_manager".into(),
                },
                OnboardPolicyDraft {
                    action_type: "update_budget".into(),
                    risk_level: "critical".into(),
                    decision_mode: "blocked".into(),
                    amount_min: None,
                    amount_max: None,
                    approver_role: "owner".into(),
                },
            ],
        ),
        "property_management" | "property" => (
            "property_management",
            vec![
                OnboardWorkflowDraft {
                    title: "Tenant request triage".into(),
                    owner_role: "property_admin".into(),
                    priority: "high".into(),
                    sla_target: "same_day".into(),
                    approval: "auto".into(),
                },
                OnboardWorkflowDraft {
                    title: "Maintenance dispatch & follow-up".into(),
                    owner_role: "maintenance_coord".into(),
                    priority: "high".into(),
                    sla_target: "24h".into(),
                    approval: "admin_required".into(),
                },
                OnboardWorkflowDraft {
                    title: "Owner financial packet review".into(),
                    owner_role: "owner".into(),
                    priority: "medium".into(),
                    sla_target: "same_day".into(),
                    approval: "admin_required".into(),
                },
            ],
            vec![
                OnboardPolicyDraft {
                    action_type: "send_message".into(),
                    risk_level: "low".into(),
                    decision_mode: "auto".into(),
                    amount_min: None,
                    amount_max: None,
                    approver_role: "property_admin".into(),
                },
                OnboardPolicyDraft {
                    action_type: "refund".into(),
                    risk_level: "high".into(),
                    decision_mode: "admin_required".into(),
                    amount_min: Some(0.0),
                    amount_max: None,
                    approver_role: "owner".into(),
                },
                OnboardPolicyDraft {
                    action_type: "update_budget".into(),
                    risk_level: "critical".into(),
                    decision_mode: "blocked".into(),
                    amount_min: None,
                    amount_max: None,
                    approver_role: "owner".into(),
                },
            ],
        ),
        _ => (
            "generic_smb",
            vec![
                OnboardWorkflowDraft {
                    title: "Inbound message triage".into(),
                    owner_role: "ops_admin".into(),
                    priority: "high".into(),
                    sla_target: "1h".into(),
                    approval: "auto".into(),
                },
                OnboardWorkflowDraft {
                    title: "Follow-up scheduling".into(),
                    owner_role: "ops_admin".into(),
                    priority: "medium".into(),
                    sla_target: "same_day".into(),
                    approval: "auto".into(),
                },
                OnboardWorkflowDraft {
                    title: "Exception escalation".into(),
                    owner_role: "manager".into(),
                    priority: "high".into(),
                    sla_target: "1h".into(),
                    approval: "admin_required".into(),
                },
            ],
            vec![
                OnboardPolicyDraft {
                    action_type: "send_message".into(),
                    risk_level: "medium".into(),
                    decision_mode: "auto".into(),
                    amount_min: None,
                    amount_max: None,
                    approver_role: "ops_admin".into(),
                },
                OnboardPolicyDraft {
                    action_type: "refund".into(),
                    risk_level: "high".into(),
                    decision_mode: "admin_required".into(),
                    amount_min: Some(50.0),
                    amount_max: None,
                    approver_role: "manager".into(),
                },
                OnboardPolicyDraft {
                    action_type: "update_budget".into(),
                    risk_level: "critical".into(),
                    decision_mode: "blocked".into(),
                    amount_min: None,
                    amount_max: None,
                    approver_role: "owner".into(),
                },
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

    let company_name = req.company_name.unwrap_or_default().trim().to_string();

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
    conf.insert(
        "company_name".to_string(),
        if company_name.is_empty() { 0.2 } else { 0.95 },
    );
    conf.insert(
        "industry".to_string(),
        if selected_vertical == "generic_smb" {
            0.6
        } else {
            0.85
        },
    );
    conf.insert("workflows".to_string(), 0.82);
    conf.insert("policy_rules".to_string(), 0.79);
    conf.insert(
        "ownership".to_string(),
        if missing.iter().any(|x| x == "approver_role") {
            0.45
        } else {
            0.8
        },
    );

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
    let vertical_hint = d.vertical_template.clone();
    let unsatisfied_required = d
        .kpi_gates
        .iter()
        .chain(d.risk_gates.iter())
        .any(|g| g.required && !g.satisfied);
    if unsatisfied_required {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "cannot apply draft: required KPI/risk gates are not satisfied" }),
            ),
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
            r#"INSERT INTO tasks (company_id, title, specification, workspace_attachment_paths, capability_refs, state, owner_persona, priority, display_number)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
        )
        .bind(company_id)
        .bind(w.title.trim())
        .bind(format!(
            "onboarding workflow · sla_target={} · approval={}",
            w.sla_target, w.approval
        ))
        .bind(SqlxJson(json!([])))
        .bind(SqlxJson(json!([])))
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
    let bootstrap_imported = bootstrap_company_skills(
        pool,
        company_id,
        &slug,
        display_name,
        Some(vertical_hint.as_str()),
    )
    .await
    .unwrap_or(0);

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "company_id": company_id,
            "slug": slug,
            "message": "onboarding draft applied",
            "bootstrap": { "imported": bootstrap_imported }
        })),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Agent Chat — natural-language Company OS interface
// POST /api/company/companies/{company_id}/agent-chat
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AgentChatBody {
    /// The user's natural-language message.
    message: String,
    /// Agent or operator identity (used for tool audit + stigmergic notes).
    #[serde(default)]
    actor: String,
    /// Optional thread ID for conversational continuity. If omitted, a new thread is created
    /// and returned in the response. Pass the returned `thread_id` on subsequent messages
    /// to maintain conversation history as context.
    #[serde(default)]
    thread_id: Option<Uuid>,
}

/// Build a company-level context prefix for agent-chat (no task_id required).
/// Mirrors `build_worker_context_prefix` but scoped to the company, not a task.
async fn build_company_chat_context(pool: &PgPool, company_id: Uuid, actor: &str) -> String {
    let mut out = String::with_capacity(4096);

    // Company context markdown
    if let Ok(Some(md)) = sqlx::query_scalar::<_, Option<String>>(
        "SELECT context_markdown FROM companies WHERE id = $1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    {
        if let Some(md) = md.filter(|s| !s.trim().is_empty()) {
            out.push_str("## Company context\n\n");
            let truncated: String = md.chars().take(1500).collect();
            out.push_str(&truncated);
            if md.chars().count() > 1500 {
                out.push_str("\n\n*(context continues)*");
            }
            out.push_str("\n\n");
        }
    }

    // Active goals
    #[derive(sqlx::FromRow)]
    struct ChatGoalRow { title: String, status: String, parent_goal_id: Option<Uuid> }
    if let Ok(goals) = sqlx::query_as::<_, ChatGoalRow>(
        "SELECT title, status, parent_goal_id FROM goals WHERE company_id = $1
         ORDER BY sort_order, created_at LIMIT 20",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    {
        if !goals.is_empty() {
            out.push_str("## Company goals\n\n");
            for g in &goals {
                let indent = if g.parent_goal_id.is_some() { "  - " } else { "- " };
                out.push_str(&format!("{indent}[{}] {}\n", g.status, g.title));
            }
            out.push('\n');
        }
    }

    // Team roster with current task assignments
    #[derive(sqlx::FromRow)]
    struct ChatAgentRow { name: String, role: String, title: Option<String> }
    if let Ok(agents) = sqlx::query_as::<_, ChatAgentRow>(
        "SELECT name, role, title FROM company_agents
         WHERE company_id = $1 AND status = 'active'
         ORDER BY sort_order, name LIMIT 20",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    {
        if !agents.is_empty() {
            out.push_str("## Team roster\n\n");
            for ag in &agents {
                let display = ag.title.as_deref().filter(|s| !s.is_empty()).unwrap_or(&ag.role);
                let current: Option<String> = sqlx::query_scalar(
                    "SELECT title FROM tasks WHERE company_id = $1 AND checked_out_by = $2
                     AND state NOT IN ('done','closed','cancelled') LIMIT 1",
                )
                .bind(company_id)
                .bind(&ag.name)
                .fetch_optional(pool)
                .await
                .unwrap_or(None);
                if let Some(t) = current {
                    out.push_str(&format!("- **{}** ({}) — working on: *{}*\n", ag.name, display, t));
                } else {
                    out.push_str(&format!("- **{}** ({}) — available\n", ag.name, display));
                }
            }
            out.push('\n');
        }
    }

    // Open / in-progress task snapshot
    #[derive(sqlx::FromRow)]
    struct ChatTaskRow { title: String, state: String, checked_out_by: Option<String> }
    if let Ok(tasks) = sqlx::query_as::<_, ChatTaskRow>(
        "SELECT title, state, checked_out_by FROM tasks
         WHERE company_id = $1 AND state IN ('open','in_progress')
         ORDER BY priority DESC, created_at LIMIT 15",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    {
        if !tasks.is_empty() {
            out.push_str("## Open / in-progress tasks\n\n");
            for t in &tasks {
                let agent = t.checked_out_by.as_deref().filter(|s| !s.is_empty()).unwrap_or("unassigned");
                out.push_str(&format!("- [{}] {} (agent: {})\n", t.state, t.title, agent));
            }
            out.push('\n');
        }
    }

    // Company skills brief
    #[derive(sqlx::FromRow)]
    struct ChatSkillRow { slug: String, description: String }
    if let Ok(skills) = sqlx::query_as::<_, ChatSkillRow>(
        "SELECT slug, LEFT(description, 80) as description FROM company_skills
         WHERE company_id = $1 ORDER BY slug LIMIT 12",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    {
        if !skills.is_empty() {
            out.push_str("## Available skills\n\n");
            for s in &skills {
                out.push_str(&format!("- `{}`: {}\n", s.slug, s.description));
            }
            out.push('\n');
        }
    }

    // Identity + tool directory
    out.push_str(&format!(
        "## Your identity\n\n\
         - **Actor (you)**: {actor}\n\
         - **company_id**: {company_id}\n\n\
         When calling any company OS tool always pass `company_id` = `{company_id}` explicitly.\n\n\
         ## Available company OS tools\n\n\
         - `company_list_tasks` — list open/in-progress tasks (add state/agent filters)\n\
         - `company_create_task` — create a task and optionally assign it via `checked_out_by`\n\
         - `company_update_task` — change task state or append a stigmergic context note\n\
         - `company_memory_search` — search company shared memory pool\n\
         - `company_memory_append` — write to shared or private memory\n\
         - `company_task_requires_human` — escalate a task to the human inbox\n\
         - `company_tool_discover` / `company_tool_describe` / `company_tool_call` — tool catalog\n\
         - Plus 60+ general tools: bash, read_file, write_file, web_search, git_*, http_request, …\n\n",
    ));

    out
}

/// POST /api/company/companies/{company_id}/agent-chat
///
/// Natural-language Company OS interface. Send any message — the agent has
/// full tool access and company awareness. Examples:
///
/// - "What tasks are open for the marketing agent?"
/// - "Create a task for the CTO agent to review the API spec"
/// - "Search company memory for our onboarding policy"
/// - "Mark task <id> as done and leave a note for the next agent"
async fn post_agent_chat(
    Path(company_id): Path<Uuid>,
    State(st): State<ConsoleState>,
    Json(body): Json<AgentChatBody>,
) -> impl IntoResponse {
    let message = body.message.trim().to_string();
    if message.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "message is required" })),
        )
            .into_response();
    }

    let actor = if body.actor.trim().is_empty() {
        std::env::var("HSM_COMPANY_TASK_ACTOR")
            .unwrap_or_else(|_| "operator".to_string())
    } else {
        body.actor.trim().to_string()
    };

    let Some(ref pool) = st.company_db else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "company database not configured (HSM_COMPANY_OS_DATABASE_URL)" })),
        )
            .into_response();
    };

    // ── Thread continuity ────────────────────────────────────────────────────
    let thread_id = match body.thread_id {
        Some(tid) => tid,
        None => {
            // Create a new thread; fall back to a random UUID if the table doesn't exist yet
            sqlx::query_scalar::<_, Uuid>(
                r#"INSERT INTO chat_threads (company_id, actor) VALUES ($1, $2) RETURNING id"#,
            )
            .bind(company_id)
            .bind(&actor)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|_| uuid::Uuid::new_v4())
        }
    };

    // Load prior turns (last 20) as context
    #[derive(sqlx::FromRow)]
    struct ThreadMsg { role: String, content: String }
    let prior = sqlx::query_as::<_, ThreadMsg>(
        r#"SELECT role, content FROM chat_thread_messages
           WHERE thread_id=$1 ORDER BY created_at ASC LIMIT 20"#,
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let thread_history = if prior.is_empty() {
        String::new()
    } else {
        let mut h = String::from("\n## Conversation history\n");
        for m in &prior {
            h.push_str(&format!("**{}**: {}\n\n", m.role, m.content));
        }
        h
    };

    // Store user message immediately (before agent runs, so it's always recorded)
    let _ = sqlx::query(
        r#"INSERT INTO chat_thread_messages (thread_id, role, content) VALUES ($1, 'user', $2)"#,
    )
    .bind(thread_id)
    .bind(&message)
    .execute(pool)
    .await;

    // Build rich company context prefix
    let context = build_company_chat_context(pool, company_id, &actor).await;

    // Prefix with /hermes so handle_message always fires the agentic tool loop.
    let prompt = format!(
        "/hermes {context}{thread_history}\n---\n\n## Message from {actor}\n\n{message}"
    );

    let home = st.home.clone();
    let actor_clone = actor.clone();
    let pool_clone = pool.clone();

    // Run in a blocking-friendly task (EnhancedPersonalAgent::initialize does disk I/O)
    let result = tokio::spawn(async move {
        match EnhancedPersonalAgent::initialize(&home).await {
            Ok(mut agent) => {
                let msg = crate::personal::gateway::Message {
                    id: format!("chat-{}", uuid::Uuid::new_v4()),
                    platform: crate::personal::gateway::Platform::Web,
                    channel_id: format!("company-os-chat-{company_id}"),
                    channel_name: Some("company-os-chat".to_string()),
                    user_id: actor_clone.clone(),
                    user_name: actor_clone,
                    content: prompt,
                    timestamp: Utc::now(),
                    attachments: Vec::new(),
                    reply_to: None,
                };
                match agent.handle_message(msg).await {
                    Ok(reply) => Ok(reply),
                    Err(e) => Err(format!("agent error: {e}")),
                }
            }
            Err(e) => Err(format!("agent init failed: {e}")),
        }
    })
    .await;

    match result {
        Ok(Ok(reply)) => {
            // Store assistant reply and update thread timestamp
            let _ = sqlx::query(
                r#"INSERT INTO chat_thread_messages (thread_id, role, content)
                   VALUES ($1, 'assistant', $2)"#,
            )
            .bind(thread_id)
            .bind(&reply)
            .execute(&pool_clone)
            .await;
            let _ = sqlx::query(
                "UPDATE chat_threads SET updated_at=now() WHERE id=$1",
            )
            .bind(thread_id)
            .execute(&pool_clone)
            .await;

            Json(json!({
                "reply": reply,
                "actor": actor,
                "company_id": company_id,
                "thread_id": thread_id,
            }))
            .into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e, "thread_id": thread_id })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("task panic: {e}"), "thread_id": thread_id })),
        )
            .into_response(),
    }
}
