//! Durable Intelligence Layer signal log and inbound Company OS snapshot builder.
//!
//! Two responsibilities:
//!   1. **Persist** every signal the Intelligence Layer processes into `intelligence_signals`.
//!   2. **Build** the `CompanyOsSnapshot` that `scan_world()` receives each tick, pulling
//!      task failures, budget overruns, and unlinked goals from Postgres.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::console::ConsoleState;
use crate::paperclip::intelligence::{
    BudgetOverrunRef, CompanyOsSnapshot, FailedTaskRef, Signal,
};

use super::no_db;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/intelligence/signals",
            get(list_signals),
        )
        .route(
            "/api/company/companies/:company_id/intelligence/snapshot",
            get(get_snapshot),
        )
}

// ── Persist ──────────────────────────────────────────────────────────────────

/// Persist a batch of processed signals (called from the conductor/heartbeat after each tick).
/// Returns the number of rows inserted.
pub async fn persist_signals(
    pool: &PgPool,
    company_id: Uuid,
    signals: &[ProcessedSignal],
) -> Result<u32, sqlx::Error> {
    let mut count = 0u32;
    for s in signals {
        let kind = signal_kind_str(&s.signal);
        let meta = SqlxJson(
            s.signal
                .metadata
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<serde_json::Map<String, Value>>(),
        );
        sqlx::query(
            r#"INSERT INTO intelligence_signals
               (company_id, kind, source, description, severity, metadata,
                composition_success, composed_goal_id, composed_task_id, escalated_to,
                paperclip_signal_id, processed_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, now())
               ON CONFLICT DO NOTHING"#,
        )
        .bind(company_id)
        .bind(kind)
        .bind(&s.signal.source)
        .bind(&s.signal.description)
        .bind(s.signal.severity as f32)
        .bind(meta)
        .bind(s.composition_success)
        .bind(s.composed_goal_pg_id)
        .bind(s.composed_task_pg_id)
        .bind(&s.escalated_to)
        .bind(&s.signal.id)
        .execute(pool)
        .await?;
        count += 1;
    }
    Ok(count)
}

/// Create a task from an escalation and update the signal row with the new task id.
pub async fn persist_escalation_task(
    pool: &PgPool,
    company_id: Uuid,
    signal: &Signal,
    goal_title: &str,
    dri_agent_ref: Option<&str>,
    source_signal_pg_id: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    let mut tx = pool.begin().await?;

    let display_n: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(MAX(display_number), 0) + 1 FROM tasks WHERE company_id = $1"#,
    )
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await?;

    let task_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO tasks
           (company_id, title, specification, owner_persona, state,
            requires_human, priority, display_number,
            goal_ancestry, workspace_attachment_paths, capability_refs,
            source_signal_id)
           VALUES ($1, $2, $3, $4, 'open', true, 80, $5,
                   '[]'::jsonb, '[]'::jsonb, '[]'::jsonb, $6)
           RETURNING id"#,
    )
    .bind(company_id)
    .bind(goal_title)
    .bind(format!(
        "Intelligence Layer escalation.\nSignal: {}\nSource: {}\n\n{}",
        signal.id, signal.source, signal.description
    ))
    .bind(dri_agent_ref)
    .bind(display_n)
    .bind(source_signal_pg_id)
    .fetch_one(&mut *tx)
    .await?;

    // governance event
    let actor = dri_agent_ref.unwrap_or("intelligence_layer");
    let _ = sqlx::query(
        r#"INSERT INTO governance_events
           (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, $2, 'escalation_carried_out', 'task', $3, $4, 'warning')"#,
    )
    .bind(company_id)
    .bind(actor)
    .bind(task_id.to_string())
    .bind(SqlxJson(json!({
        "goal_title": goal_title,
        "signal_id": signal.id,
        "signal_kind": signal_kind_str(signal),
        "source": signal.source,
        "escalated_to": dri_agent_ref,
        "source_signal_pg_id": source_signal_pg_id,
    })))
    .execute(&mut *tx)
    .await;

    // update signal row if we have the pg id
    if let Some(sig_id) = source_signal_pg_id {
        let _ = sqlx::query(
            "UPDATE intelligence_signals SET composed_task_id = $1, processed_at = now() WHERE id = $2",
        )
        .bind(task_id)
        .bind(sig_id)
        .execute(&mut *tx)
        .await;
    }

    tx.commit().await?;
    Ok(task_id)
}

// ── Snapshot builder ─────────────────────────────────────────────────────────

/// Build a `CompanyOsSnapshot` from live Postgres state for the given company.
/// Called by the conductor before each `scan_world()` tick.
pub async fn build_snapshot(
    pool: &PgPool,
    company_id: Uuid,
) -> Result<CompanyOsSnapshot, sqlx::Error> {
    // 1. Failed tasks (error / blocked state) with capability_refs, last 50
    let failed_rows: Vec<(String, String, SqlxJson<Value>)> = sqlx::query_as(
        r#"SELECT id::text, title,
                  COALESCE(capability_refs, '[]'::jsonb) AS capability_refs
           FROM tasks
           WHERE company_id = $1
             AND state IN ('error', 'blocked', 'cancelled')
             AND updated_at > now() - interval '24 hours'
           ORDER BY updated_at DESC
           LIMIT 50"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let failed_task_refs: Vec<FailedTaskRef> = failed_rows
        .into_iter()
        .map(|(task_id, title, cap_refs)| {
            let capability_refs = cap_refs
                .0
                .as_array()
                .cloned()
                .unwrap_or_default();
            FailedTaskRef {
                task_id,
                title,
                capability_refs,
                failure_reason: None,
            }
        })
        .collect();

    // 2. Budget overruns: agents whose spend > budget this month
    let overrun_rows: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"SELECT
               a.name AS agent_ref,
               COALESCE(a.budget_monthly_cents, 0) AS budget_cents,
               COALESCE(SUM(se.amount_cents), 0)::BIGINT AS spend_cents
           FROM company_agents a
           LEFT JOIN spend_events se
               ON se.company_id = a.company_id
              AND se.agent_ref = a.name
              AND se.created_at >= date_trunc('month', now())
           WHERE a.company_id = $1
             AND a.status NOT IN ('terminated', 'paused')
             AND a.budget_monthly_cents IS NOT NULL
             AND a.budget_monthly_cents > 0
           GROUP BY a.name, a.budget_monthly_cents
           HAVING COALESCE(SUM(se.amount_cents), 0) > a.budget_monthly_cents"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let budget_overruns: Vec<BudgetOverrunRef> = overrun_rows
        .into_iter()
        .map(|(agent_ref, budget_cents, spend_cents)| BudgetOverrunRef {
            agent_ref,
            budget_cents,
            spend_cents,
        })
        .collect();

    // 3. Goals with no linked open tasks (unlinked)
    let unlinked: Vec<String> = sqlx::query_scalar(
        r#"SELECT g.id::text
           FROM goals g
           WHERE g.company_id = $1
             AND g.status NOT IN ('done', 'cancelled', 'closed')
             AND g.created_at < now() - interval '2 hours'
             AND NOT EXISTS (
                 SELECT 1 FROM tasks t
                 WHERE t.company_id = $1
                   AND t.state NOT IN ('done', 'closed', 'cancelled')
                   AND (
                       t.primary_goal_id = g.id
                       OR g.id::text = ANY(
                           SELECT jsonb_array_elements_text(t.goal_ancestry)
                       )
                   )
             )
           LIMIT 20"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // 4. Direction from context_markdown (first 500 chars)
    let direction: Option<String> = sqlx::query_scalar(
        "SELECT LEFT(context_markdown, 500) FROM companies WHERE id = $1 AND context_markdown IS NOT NULL",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
    .flatten();

    Ok(CompanyOsSnapshot {
        failed_task_refs,
        budget_overruns,
        unlinked_goal_ids: unlinked,
        direction_summary: direction,
    })
}

// ── API handlers ─────────────────────────────────────────────────────────────

#[derive(FromRow, Serialize)]
struct SignalRow {
    id: Uuid,
    company_id: Uuid,
    kind: String,
    source: String,
    description: String,
    severity: f32,
    metadata: SqlxJson<Value>,
    composition_success: Option<bool>,
    composed_goal_id: Option<Uuid>,
    composed_task_id: Option<Uuid>,
    escalated_to: Option<String>,
    paperclip_signal_id: Option<String>,
    created_at: String,
    processed_at: Option<String>,
}

#[derive(Deserialize)]
struct ListSignalsQuery {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn list_signals(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<ListSignalsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let limit = q.limit.unwrap_or(100).clamp(1, 500);

    let rows: Vec<SignalRow> = if let Some(kind) = q.kind {
        sqlx::query_as::<_, SignalRow>(
            r#"SELECT id, company_id, kind, source, description, severity, metadata,
                      composition_success, composed_goal_id, composed_task_id, escalated_to,
                      paperclip_signal_id, created_at::text, processed_at::text
               FROM intelligence_signals
               WHERE company_id = $1 AND kind = $2
               ORDER BY created_at DESC LIMIT $3"#,
        )
        .bind(company_id)
        .bind(kind)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, SignalRow>(
            r#"SELECT id, company_id, kind, source, description, severity, metadata,
                      composition_success, composed_goal_id, composed_task_id, escalated_to,
                      paperclip_signal_id, created_at::text, processed_at::text
               FROM intelligence_signals
               WHERE company_id = $1
               ORDER BY created_at DESC LIMIT $2"#,
        )
        .bind(company_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    }
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({ "signals": rows })))
}

async fn get_snapshot(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let snapshot = build_snapshot(pool, company_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    Ok(Json(json!({ "snapshot": snapshot })))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// A signal paired with its composition outcome, ready to persist.
pub struct ProcessedSignal {
    pub signal: Signal,
    pub composition_success: Option<bool>,
    pub composed_goal_pg_id: Option<Uuid>,
    pub composed_task_pg_id: Option<Uuid>,
    pub escalated_to: Option<String>,
}

// ── Policy evaluation ────────────────────────────────────────────────────────

/// Result of evaluating policy rules against a proposed composition.
#[derive(Clone, Debug)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub requires_human: bool,
    pub decision_mode: String,
    pub rule_id: Option<Uuid>,
    pub reason: String,
}

/// Evaluate `policy_rules` for a given action_type (signal kind) and severity.
/// Returns the most restrictive matching rule's decision.
///
/// `decision_mode` values: "auto" (proceed), "admin_required" (create task with
/// requires_human), "blocked" (do not compose).
pub async fn evaluate_policy(
    pool: &PgPool,
    company_id: Uuid,
    action_type: &str,
    severity: f64,
) -> Result<PolicyDecision, sqlx::Error> {
    // Find the most restrictive matching rule.
    // policy_rules has: action_type, risk_level, decision_mode, amount_min/max
    // We map severity → risk_level: >=0.8 critical, >=0.6 high, >=0.4 medium, else low
    let risk_level = if severity >= 0.8 {
        "critical"
    } else if severity >= 0.6 {
        "high"
    } else if severity >= 0.4 {
        "medium"
    } else {
        "low"
    };

    let row: Option<(Uuid, String)> = sqlx::query_as(
        r#"SELECT id, decision_mode
           FROM policy_rules
           WHERE company_id = $1
             AND action_type = $2
             AND risk_level = $3
           ORDER BY
             CASE decision_mode
               WHEN 'blocked' THEN 0
               WHEN 'admin_required' THEN 1
               ELSE 2
             END
           LIMIT 1"#,
    )
    .bind(company_id)
    .bind(action_type)
    .bind(risk_level)
    .fetch_optional(pool)
    .await?;

    Ok(match row {
        Some((rule_id, ref mode)) if mode == "blocked" => PolicyDecision {
            allowed: false,
            requires_human: false,
            decision_mode: "blocked".into(),
            rule_id: Some(rule_id),
            reason: format!("Policy rule {rule_id} blocks {action_type} at {risk_level} risk"),
        },
        Some((rule_id, ref mode)) if mode == "admin_required" => PolicyDecision {
            allowed: true,
            requires_human: true,
            decision_mode: "admin_required".into(),
            rule_id: Some(rule_id),
            reason: format!("Policy rule {rule_id} requires human approval for {action_type} at {risk_level} risk"),
        },
        _ => PolicyDecision {
            allowed: true,
            requires_human: false,
            decision_mode: "auto".into(),
            rule_id: None,
            reason: "No restrictive policy matched".into(),
        },
    })
}

// ── Memory-aware DRI context ─────────────────────────────────────────────────

/// Query company shared memory for DRI context relevant to a set of domains.
/// Returns up to `limit` memory entries that mention DRI names or domains.
pub async fn query_dri_memory_context(
    pool: &PgPool,
    company_id: Uuid,
    domains: &[String],
    limit: i64,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    if domains.is_empty() {
        return Ok(Vec::new());
    }
    // Build a tsquery from domain keywords
    let ts_terms: Vec<String> = domains.iter().map(|d| d.replace(' ', " & ")).collect();
    let ts_query = ts_terms.join(" | ");

    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT title, LEFT(body, 300)
           FROM company_memory_entries
           WHERE company_id = $1
             AND scope = 'shared'
             AND to_tsvector('english', title || ' ' || body) @@ to_tsquery('english', $2)
           ORDER BY updated_at DESC
           LIMIT $3"#,
    )
    .bind(company_id)
    .bind(&ts_query)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Check whether a proposed goal title aligns with company direction (context_markdown).
/// Returns (aligned: bool, direction_excerpt: Option<String>).
pub async fn check_direction_alignment(
    pool: &PgPool,
    company_id: Uuid,
    goal_title: &str,
) -> Result<(bool, Option<String>), sqlx::Error> {
    let direction: Option<String> = sqlx::query_scalar(
        "SELECT LEFT(context_markdown, 1000) FROM companies WHERE id = $1 AND context_markdown IS NOT NULL",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .flatten();

    let Some(dir) = direction else {
        return Ok((true, None)); // no direction set = allow everything
    };

    // Simple keyword overlap: tokenize goal title, check if any word appears in direction
    let goal_words: Vec<&str> = goal_title
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .collect();
    let dir_lower = dir.to_lowercase();
    let matches = goal_words
        .iter()
        .filter(|w| dir_lower.contains(&w.to_lowercase()))
        .count();
    let aligned = goal_words.is_empty() || matches > 0;

    Ok((aligned, Some(dir)))
}

fn signal_kind_str(s: &Signal) -> &'static str {
    use crate::paperclip::intelligence::SignalKind;
    match &s.kind {
        SignalKind::CapabilityDegraded { .. } => "capability_degraded",
        SignalKind::GoalStale { .. } => "goal_stale",
        SignalKind::BudgetOverrun { .. } => "budget_overrun",
        SignalKind::CompositionFailed { .. } => "composition_failed",
        SignalKind::MissingCapability { .. } => "missing_capability",
        SignalKind::ExternalSignal { .. } => "external_signal",
        SignalKind::CoherenceDrop { .. } => "coherence_drop",
        SignalKind::AgentAnomaly { .. } => "agent_anomaly",
        SignalKind::Custom { .. } => "custom",
    }
}
