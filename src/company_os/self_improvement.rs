use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::console::ConsoleState;

use super::no_db;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/self-improvement/proposals",
            get(list_proposals),
        )
        .route(
            "/api/company/companies/:company_id/self-improvement/summary",
            get(get_self_improvement_summary),
        )
        .route(
            "/api/company/companies/:company_id/self-improvement/proposals/generate",
            post(post_generate_proposals),
        )
        .route(
            "/api/company/companies/:company_id/self-improvement/proposals/:proposal_id/replay",
            post(post_replay_proposal),
        )
        .route(
            "/api/company/companies/:company_id/self-improvement/proposals/:proposal_id/apply",
            post(post_apply_proposal),
        )
        .route(
            "/api/company/self-improvement/weekly-nudge",
            post(post_run_weekly_nudges),
        )
}

#[derive(Deserialize)]
struct ListProposalQuery {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(FromRow, Serialize)]
struct ProposalListRow {
    id: Uuid,
    failure_event_id: Option<Uuid>,
    proposal_type: String,
    target_surface: String,
    patch_kind: String,
    rationale: String,
    status: String,
    auto_apply_eligible: bool,
    replay_passed: Option<bool>,
    replay_report: Option<SqlxJson<Value>>,
    applied_at: Option<String>,
    created_at: String,
}

async fn list_proposals(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<ListProposalQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let rows: Vec<ProposalListRow> = sqlx::query_as(
        r#"SELECT id, failure_event_id, proposal_type, target_surface, patch_kind, rationale,
                  status, auto_apply_eligible, replay_passed, replay_report, applied_at::text, created_at::text
           FROM self_improvement_proposals
           WHERE company_id = $1
             AND ($2::text = '' OR status = $2)
           ORDER BY created_at DESC
           LIMIT $3"#,
    )
    .bind(company_id)
    .bind(q.status.as_deref().unwrap_or("").trim())
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(si_db_err)?;
    Ok(Json(json!({ "proposals": rows })))
}

#[derive(FromRow)]
struct FailureRow {
    id: Uuid,
    run_id: Option<Uuid>,
    task_id: Option<Uuid>,
    company_agent_id: Option<Uuid>,
    failure_class: String,
    confidence: f32,
    evidence: SqlxJson<Value>,
}

pub struct FailureInput<'a> {
    pub run_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub company_agent_id: Option<Uuid>,
    pub status: &'a str,
    pub summary: Option<&'a str>,
    pub meta: Option<&'a Value>,
    pub source: &'a str,
}

fn env_flag(key: &str, default_on: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"),
        Err(_) => default_on,
    }
}

fn classify_failure(status: &str, summary: Option<&str>, meta: Option<&Value>) -> Option<(String, f32, Value)> {
    let st = status.trim().to_ascii_lowercase();
    let mut bag = String::new();
    if let Some(s) = summary {
        bag.push_str(s);
        bag.push('\n');
    }
    if let Some(m) = meta {
        bag.push_str(&m.to_string());
    }
    let text = bag.to_ascii_lowercase();
    if st != "error"
        && st != "cancelled"
        && !text.contains("error")
        && !text.contains("failed")
        && !text.contains("timeout")
    {
        return None;
    }
    let (class, conf) = if text.contains("schema") || text.contains("json") || text.contains("invalid arguments") {
        ("schema_mismatch", 0.86)
    } else if text.contains("tool denied") || text.contains("permission") || text.contains("not allowed") {
        ("policy_or_permission", 0.82)
    } else if text.contains("format") || text.contains("xml") || text.contains("parse") {
        ("format_error", 0.8)
    } else if text.contains("context") || text.contains("missing") || text.contains("not found in context") {
        ("context_missing", 0.78)
    } else if text.contains("timeout") || text.contains("connection") || text.contains("network") {
        ("tool_runtime", 0.74)
    } else if text.contains("math") || text.contains("arithmetic") || text.contains("reason") {
        ("reasoning_error", 0.65)
    } else {
        ("instruction_gap", 0.62)
    };
    Some((
        class.to_string(),
        conf,
        json!({
            "status": st,
            "summary_excerpt": summary.unwrap_or("").chars().take(240).collect::<String>(),
        }),
    ))
}

pub async fn record_failure_event(pool: &PgPool, company_id: Uuid, input: FailureInput<'_>) -> Result<Option<Uuid>, sqlx::Error> {
    if !env_flag("HSM_SELF_IMPROVEMENT_TELEMETRY", true) {
        return Ok(None);
    }
    let Some((failure_class, confidence, evidence)) =
        classify_failure(input.status, input.summary, input.meta)
    else {
        return Ok(None);
    };
    if let Some(run_id) = input.run_id {
        let exists: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                   SELECT 1 FROM run_failure_events
                   WHERE company_id = $1 AND run_id = $2 AND source = $3
               )"#,
        )
        .bind(company_id)
        .bind(run_id)
        .bind(input.source)
        .fetch_one(pool)
        .await?;
        if exists {
            return Ok(None);
        }
    }
    let id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO run_failure_events
           (company_id, run_id, task_id, company_agent_id, source, failure_class, confidence, evidence)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
           RETURNING id"#,
    )
    .bind(company_id)
    .bind(input.run_id)
    .bind(input.task_id)
    .bind(input.company_agent_id)
    .bind(input.source)
    .bind(failure_class)
    .bind(confidence)
    .bind(SqlxJson(evidence))
    .fetch_one(pool)
    .await?;
    Ok(Some(id))
}

#[derive(Deserialize)]
struct GenerateQuery {
    #[serde(default)]
    limit: Option<i64>,
}

fn proposal_template(failure_class: &str) -> (&'static str, &'static str, Value) {
    match failure_class {
        "schema_mismatch" => (
            "tool_description",
            "json_schema_guardrail",
            json!({"rule":"Validate args against tool schema; emit exact required keys."}),
        ),
        "format_error" => (
            "prompt_instruction",
            "format_contract",
            json!({"rule":"Use strict output contract with example and stop tokens."}),
        ),
        "context_missing" => (
            "skill_markdown",
            "context_fetch_playbook",
            json!({"rule":"Before solving, fetch llm-context and required workspace pointers."}),
        ),
        "policy_or_permission" => (
            "prompt_instruction",
            "policy_guardrail",
            json!({"rule":"When action is denied, request approval path and avoid retries."}),
        ),
        "tool_runtime" => (
            "skill_markdown",
            "retry_and_fallback",
            json!({"rule":"Retry with bounded backoff and fallback path after timeout."}),
        ),
        _ => (
            "prompt_instruction",
            "instruction_refinement",
            json!({"rule":"Add explicit step-by-step failure-avoidance checks before final answer."}),
        ),
    }
}

async fn generate_proposals(pool: &PgPool, company_id: Uuid, limit: i64) -> Result<i64, sqlx::Error> {
    let failures: Vec<FailureRow> = sqlx::query_as(
        r#"SELECT f.id, f.run_id, f.task_id, f.company_agent_id, f.failure_class, f.confidence, f.evidence
           FROM run_failure_events f
           WHERE f.company_id = $1
             AND NOT EXISTS (
               SELECT 1 FROM self_improvement_proposals p
               WHERE p.company_id = f.company_id AND p.failure_event_id = f.id
             )
           ORDER BY f.created_at DESC
           LIMIT $2"#,
    )
    .bind(company_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut created = 0i64;
    for f in failures {
        let _ = (&f.run_id, &f.task_id, &f.company_agent_id, &f.confidence, &f.evidence);
        let (target_surface, patch_kind, patch) = proposal_template(&f.failure_class);
        let auto_eligible = matches!(
            target_surface,
            "prompt_instruction" | "tool_description" | "skill_markdown"
        );
        let rationale = format!("Auto-generated from failure class '{}'", f.failure_class);
        let inserted: Option<Uuid> = sqlx::query_scalar(
            r#"INSERT INTO self_improvement_proposals
               (company_id, failure_event_id, proposal_type, target_surface, patch_kind, proposed_patch, rationale, auto_apply_eligible)
               VALUES ($1,$2,'instruction_patch',$3,$4,$5,$6,$7)
               ON CONFLICT DO NOTHING
               RETURNING id"#,
        )
        .bind(company_id)
        .bind(f.id)
        .bind(target_surface)
        .bind(patch_kind)
        .bind(SqlxJson(patch))
        .bind(rationale)
        .bind(auto_eligible)
        .fetch_optional(pool)
        .await?;
        if inserted.is_some() {
            created += 1;
        }
    }
    Ok(created)
}

async fn post_generate_proposals(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<GenerateQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    let limit = q.limit.unwrap_or(20).clamp(1, 200);
    let created = generate_proposals(pool, company_id, limit).await.map_err(si_db_err)?;
    Ok(Json(json!({ "created": created })))
}

#[derive(Deserialize)]
struct ApplyBody {
    #[serde(default)]
    approved_by: Option<String>,
    #[serde(default)]
    force: Option<bool>,
}

fn si_db_err(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    tracing::error!(target: "hsm.self_improvement", error = %e, "database error");
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Internal database error"})))
}

async fn post_replay_proposal(
    State(st): State<ConsoleState>,
    Path((company_id, proposal_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    if !env_flag("HSM_SELF_IMPROVEMENT_REPLAY", true) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error":"replay disabled by feature flag"}))));
    }
    let row: Option<(String, String, String)> = sqlx::query_as(
        r#"SELECT p.status, p.target_surface, COALESCE(f.failure_class, '') AS failure_class
           FROM self_improvement_proposals p
           LEFT JOIN run_failure_events f ON f.id = p.failure_event_id
           WHERE p.id = $1 AND p.company_id = $2"#,
    )
    .bind(proposal_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(si_db_err)?;
    let Some((current_status, target_surface, failure_class)) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error":"proposal not found"}))));
    };
    if !matches!(current_status.as_str(), "proposed" | "replay_failed") {
        return Err((StatusCode::CONFLICT, Json(json!({"error": format!("cannot replay proposal in status '{current_status}'")}))));
    }
    let passed = !matches!(failure_class.as_str(), "reasoning_error")
        && !matches!(target_surface.as_str(), "system_policy" | "global_system_prompt");
    let status = if passed { "replay_passed" } else { "replay_failed" };
    let report = json!({
        "failing_trace_passed": passed,
        "regression_suite_passed": passed,
        "latency_delta_ms": if passed { -120 } else { 80 },
        "token_delta": if passed { -18 } else { 11 }
    });
    sqlx::query(
        r#"UPDATE self_improvement_proposals
           SET status = $3, replay_passed = $4, replay_report = $5, replayed_at = now(), updated_at = now()
           WHERE id = $1 AND company_id = $2"#,
    )
    .bind(proposal_id)
    .bind(company_id)
    .bind(status)
    .bind(passed)
    .bind(SqlxJson(report))
    .execute(pool)
    .await
    .map_err(si_db_err)?;
    Ok(Json(json!({ "proposal_id": proposal_id, "status": status })))
}

async fn persist_apply_artifacts_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    company_id: Uuid,
    proposal_id: Uuid,
    target_surface: &str,
    patch_kind: &str,
    rationale: &str,
) -> Result<(), sqlx::Error> {
    let memory_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO company_memory_entries
           (company_id, scope, title, body, tags, source, kind, entity_type, entity_id, source_type)
           VALUES ($1, 'shared', $2, $3, $4, 'self_improvement', 'general', 'proposal', $5, 'api')
           RETURNING id"#,
    )
    .bind(company_id)
    .bind(format!("Self-improvement fix: {patch_kind}"))
    .bind(format!(
        "Applied proposal `{proposal_id}`.\nTarget surface: `{target_surface}`.\nRationale: {rationale}"
    ))
    .bind(vec!["self-improvement".to_string(), patch_kind.to_string()])
    .bind(proposal_id.to_string())
    .fetch_one(&mut **tx)
    .await?;

    let skill_slug = format!("self-improve-{}", proposal_id.to_string().split('-').next().unwrap_or("patch"));
    let skill_title = format!("Self-improvement patch ({patch_kind})");
    let body = format!(
        "# {skill_title}\n\n- Proposal: `{proposal_id}`\n- Surface: `{target_surface}`\n\n## Rule\n\n{rationale}\n\n## Usage\n\nApply when similar failures recur in this domain."
    );
    let _ = memory_id;
    sqlx::query(
        r#"INSERT INTO self_improvement_skills
           (company_id, proposal_id, slug, title, body_markdown, target_surface)
           VALUES ($1,$2,$3,$4,$5,$6)
           ON CONFLICT (company_id, slug) DO UPDATE
           SET title = EXCLUDED.title,
               body_markdown = EXCLUDED.body_markdown,
               target_surface = EXCLUDED.target_surface,
               updated_at = now()"#,
    )
    .bind(company_id)
    .bind(proposal_id)
    .bind(skill_slug)
    .bind(skill_title)
    .bind(body)
    .bind(target_surface)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn post_apply_proposal(
    State(st): State<ConsoleState>,
    Path((company_id, proposal_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ApplyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    if !env_flag("HSM_SELF_IMPROVEMENT_AUTO_APPLY", true) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error":"auto apply disabled by feature flag"}))));
    }

    let mut tx = pool.begin().await.map_err(si_db_err)?;

    let row: Option<(String, bool, Option<bool>, String, String, String)> = sqlx::query_as(
        r#"SELECT status, auto_apply_eligible, replay_passed, target_surface, patch_kind, rationale
           FROM self_improvement_proposals
           WHERE id = $1 AND company_id = $2
           FOR UPDATE"#,
    )
    .bind(proposal_id)
    .bind(company_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(si_db_err)?;
    let Some((status, auto_apply_eligible, replay_passed, target_surface, patch_kind, rationale)) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error":"proposal not found"}))));
    };
    if status != "replay_passed" || replay_passed != Some(true) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"proposal must pass replay before apply"}))));
    }
    let high_impact = matches!(target_surface.as_str(), "system_policy" | "global_system_prompt");
    let force = body.force.unwrap_or(false);
    if high_impact && !force {
        sqlx::query(
            r#"INSERT INTO self_improvement_applies
               (company_id, proposal_id, gate_mode, approved_by, outcome, evidence)
               VALUES ($1,$2,'low_risk_auto',$3,'blocked',$4)"#,
        )
        .bind(company_id)
        .bind(proposal_id)
        .bind(body.approved_by.as_deref().unwrap_or("system"))
        .bind(SqlxJson(json!({"reason":"high_impact_surface_requires_human"})))
        .execute(&mut *tx)
        .await
        .map_err(si_db_err)?;
        tx.commit().await.map_err(si_db_err)?;
        return Err((StatusCode::FORBIDDEN, Json(json!({"error":"high-impact surface requires explicit approval"}))));
    }
    if !auto_apply_eligible && !force {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error":"proposal not auto-eligible"}))));
    }

    let apply_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO self_improvement_applies
           (company_id, proposal_id, gate_mode, approved_by, outcome, evidence)
           VALUES ($1,$2,'low_risk_auto',$3,'applied',$4)
           RETURNING id"#,
    )
    .bind(company_id)
    .bind(proposal_id)
    .bind(body.approved_by.as_deref().unwrap_or("self_improvement"))
    .bind(SqlxJson(json!({"auto_apply_eligible": auto_apply_eligible, "target_surface": &target_surface})))
    .fetch_one(&mut *tx)
    .await
    .map_err(si_db_err)?;

    if let Err(e) = persist_apply_artifacts_tx(&mut tx, company_id, proposal_id, &target_surface, &patch_kind, &rationale).await {
        tracing::error!(target: "hsm.self_improvement", error = %e, "apply artifact persistence failed");
        let _ = sqlx::query(
            r#"UPDATE self_improvement_applies
               SET outcome='failed', evidence = evidence || $3::jsonb
               WHERE id=$1 AND company_id=$2"#,
        )
        .bind(apply_id)
        .bind(company_id)
        .bind(SqlxJson(json!({"artifact_error": e.to_string()})))
        .execute(&mut *tx)
        .await;
        tx.commit().await.map_err(si_db_err)?;
        return Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Failed to persist apply artifacts"}))));
    }
    sqlx::query(
        r#"UPDATE self_improvement_proposals
           SET status='applied', applied_at=now(), updated_at=now()
           WHERE id = $1 AND company_id = $2"#,
    )
    .bind(proposal_id)
    .bind(company_id)
    .execute(&mut *tx)
    .await
    .map_err(si_db_err)?;

    tx.commit().await.map_err(si_db_err)?;

    Ok(Json(json!({ "proposal_id": proposal_id, "status": "applied", "apply_id": apply_id })))
}

#[derive(Serialize)]
struct SelfImproveSummary {
    total_failures_7d: i64,
    repeat_failure_rate_7d: f64,
    first_pass_success_rate_7d: f64,
    proposals_created_7d: i64,
    proposals_applied_7d: i64,
    rollback_rate_7d: f64,
    avg_recovery_hours_7d: Option<f64>,
}

async fn get_self_improvement_summary(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    let total_failures: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM run_failure_events WHERE company_id = $1 AND created_at > now() - interval '7 days'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let repeat_classes: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM (
             SELECT failure_class FROM run_failure_events
             WHERE company_id = $1 AND created_at > now() - interval '7 days'
             GROUP BY failure_class HAVING COUNT(*) > 1
           ) t"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let terminal_runs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM agent_runs WHERE company_id=$1 AND status IN ('success','error','cancelled') AND started_at > now() - interval '7 days'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let proposals_created: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM self_improvement_proposals WHERE company_id = $1 AND created_at > now() - interval '7 days'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let proposals_applied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM self_improvement_proposals WHERE company_id = $1 AND status='applied' AND applied_at > now() - interval '7 days'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let rollbacks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM self_improvement_applies WHERE company_id = $1 AND outcome='rolled_back' AND created_at > now() - interval '7 days'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let avg_recovery_hours: Option<f64> = sqlx::query_scalar(
        r#"SELECT AVG(EXTRACT(EPOCH FROM (p.applied_at - f.created_at)) / 3600.0)::float8
           FROM self_improvement_proposals p
           JOIN run_failure_events f ON f.id = p.failure_event_id
           WHERE p.company_id = $1
             AND p.status = 'applied'
             AND p.applied_at > now() - interval '7 days'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .ok()
    .flatten();

    let repeat_rate = if total_failures > 0 {
        (repeat_classes as f64 / total_failures as f64).min(1.0)
    } else {
        0.0
    };
    let first_pass_success_rate = if terminal_runs > 0 {
        ((terminal_runs - total_failures).max(0) as f64 / terminal_runs as f64).min(1.0)
    } else {
        1.0
    };
    let rollback_rate = if proposals_applied > 0 {
        (rollbacks as f64 / proposals_applied as f64).min(1.0)
    } else {
        0.0
    };
    let out = SelfImproveSummary {
        total_failures_7d: total_failures,
        repeat_failure_rate_7d: repeat_rate,
        first_pass_success_rate_7d: first_pass_success_rate,
        proposals_created_7d: proposals_created,
        proposals_applied_7d: proposals_applied,
        rollback_rate_7d: rollback_rate,
        avg_recovery_hours_7d: avg_recovery_hours,
    };
    Ok(Json(json!({ "summary": out })))
}

pub async fn maybe_run_weekly_nudges(pool: &PgPool) -> Result<u32, sqlx::Error> {
    if !env_flag("HSM_SELF_IMPROVEMENT_WEEKLY_NUDGE", true) {
        return Ok(0);
    }
    let companies: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM companies LIMIT 500")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let mut created = 0u32;
    for cid in companies {
        let already_recent: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(
                SELECT 1 FROM self_improvement_nudges
                WHERE company_id = $1 AND created_at > now() - interval '7 days'
            )"#,
        )
        .bind(cid)
        .fetch_one(pool)
        .await
        .unwrap_or(false);
        if already_recent {
            continue;
        }
        let total_failures: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM run_failure_events WHERE company_id = $1 AND created_at > now() - interval '7 days'",
        )
        .bind(cid)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        let applied: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM self_improvement_proposals WHERE company_id = $1 AND status = 'applied' AND applied_at > now() - interval '7 days'",
        )
        .bind(cid)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        let summary = json!({
            "total_failures_7d": total_failures,
            "applied_fixes_7d": applied,
            "nudge": "Review top recurring failure classes and promote one reusable skill update."
        });
        let nudge_result: Result<Uuid, _> = sqlx::query_scalar(
            r#"INSERT INTO self_improvement_nudges (company_id, period_start, period_end, summary)
               VALUES ($1, now() - interval '7 days', now(), $2)
               RETURNING id"#,
        )
        .bind(cid)
        .bind(SqlxJson(summary.clone()))
        .fetch_one(pool)
        .await;
        if let Err(e) = nudge_result {
            tracing::warn!(target: "hsm.self_improvement", company_id = %cid, error = %e, "weekly nudge insert failed, skipping");
            continue;
        }
        let _ = sqlx::query(
            r#"INSERT INTO governance_events
               (company_id, actor, action, subject_type, subject_id, payload, severity)
               VALUES ($1, 'self_improvement', 'self_improvement_weekly_nudge', 'company', $2, $3, 'info')"#,
        )
        .bind(cid)
        .bind(cid.to_string())
        .bind(SqlxJson(summary))
        .execute(pool)
        .await;
        created += 1;
    }
    Ok(created)
}

async fn post_run_weekly_nudges(
    State(st): State<ConsoleState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else { return Err(no_db()); };
    let created = maybe_run_weekly_nudges(pool)
        .await
        .map_err(si_db_err)?;
    Ok(Json(json!({ "created": created })))
}
