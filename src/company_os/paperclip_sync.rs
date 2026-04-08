//! One-way sync: Paperclip `IntelligenceLayer` goals / DRIs → Postgres Company OS.

use crate::paperclip::dri::{DriEntry, DriTenure};
use crate::paperclip::goal::{Goal, GoalStatus};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

fn map_goal_status(s: &GoalStatus) -> &'static str {
    match s {
        GoalStatus::Done => "done",
        GoalStatus::Cancelled { .. } => "cancelled",
        GoalStatus::Blocked { .. } => "blocked",
        _ => "active",
    }
}

fn goal_snapshot(g: &Goal) -> Value {
    json!({
        "assignee": g.assignee,
        "priority": g.priority,
        "tags": g.tags,
        "required_capabilities": g.required_capabilities,
        "updated_at_unix": g.updated_at,
    })
}

/// Upsert goals keyed by `paperclip_goal_id`; then link `parent_goal_id` from Paperclip `parent_id`.
pub async fn sync_paperclip_goals(
    pool: &PgPool,
    company_id: Uuid,
    goals: Vec<Goal>,
) -> Result<Value, sqlx::Error> {
    let mut id_map: HashMap<String, Uuid> = HashMap::new();
    let mut inserted = 0u32;
    let mut updated = 0u32;

    for g in &goals {
        let status = map_goal_status(&g.status);
        let desc = g.description.trim();
        let description = if desc.is_empty() {
            None::<String>
        } else {
            Some(desc.to_string())
        };
        let snap = goal_snapshot(g);

        let existing: Option<Uuid> = sqlx::query_scalar::<_, Uuid>(
            r#"SELECT id FROM goals WHERE company_id = $1 AND paperclip_goal_id = $2"#,
        )
        .bind(company_id)
        .bind(&g.id)
        .fetch_optional(pool)
        .await?;

        let row_id = if let Some(id) = existing {
            sqlx::query_scalar::<_, Uuid>(
                r#"UPDATE goals SET
                    title = $2,
                    description = $3,
                    status = $4,
                    paperclip_snapshot = $5,
                    updated_at = NOW()
                   WHERE id = $1
                   RETURNING id"#,
            )
            .bind(id)
            .bind(g.title.trim())
            .bind(&description)
            .bind(status)
            .bind(sqlx::types::Json(snap.clone()))
            .fetch_one(pool)
            .await?;
            updated += 1;
            id
        } else {
            let id: Uuid = sqlx::query_scalar::<_, Uuid>(
                r#"INSERT INTO goals (company_id, parent_goal_id, title, description, status, paperclip_goal_id, paperclip_snapshot)
                   VALUES ($1, NULL, $2, $3, $4, $5, $6)
                   RETURNING id"#,
            )
            .bind(company_id)
            .bind(g.title.trim())
            .bind(&description)
            .bind(status)
            .bind(&g.id)
            .bind(sqlx::types::Json(snap))
            .fetch_one(pool)
            .await?;
            inserted += 1;
            id
        };
        id_map.insert(g.id.clone(), row_id);
    }

    let mut parents_linked = 0u32;
    for g in &goals {
        let Some(paperclip_parent) = g.parent_id.as_ref() else {
            continue;
        };
        let Some(&child_pg) = id_map.get(&g.id) else {
            continue;
        };
        let Some(&parent_pg) = id_map.get(paperclip_parent) else {
            continue;
        };
        let res = sqlx::query(
            r#"UPDATE goals SET parent_goal_id = $1, updated_at = NOW()
               WHERE id = $2 AND company_id = $3"#,
        )
        .bind(parent_pg)
        .bind(child_pg)
        .bind(company_id)
        .execute(pool)
        .await?;
        if res.rows_affected() > 0 {
            parents_linked += 1;
        }
    }

    Ok(json!({
        "goals_total_input": goals.len(),
        "inserted": inserted,
        "updated": updated,
        "parents_linked": parents_linked,
    }))
}

fn tenure_to_db(
    t: &DriTenure,
) -> (
    &'static str,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    match t {
        DriTenure::Persistent => ("persistent", None, None),
        DriTenure::TimeBound { start, end } => {
            let from = chrono::DateTime::from_timestamp(*start as i64, 0);
            let until = chrono::DateTime::from_timestamp(*end as i64, 0);
            ("time_bound", from, until)
        }
    }
}

/// Upsert DRIs by `(company_id, dri_key)` where `dri_key` = Paperclip entry id.
pub async fn sync_paperclip_dris(
    pool: &PgPool,
    company_id: Uuid,
    entries: Vec<DriEntry>,
) -> Result<Value, sqlx::Error> {
    let mut upserted = 0u32;
    for e in &entries {
        let (tenure_kind, valid_from, valid_until) = tenure_to_db(&e.tenure);
        let authority = sqlx::types::Json(serde_json::to_value(&e.authority).unwrap_or(json!({})));

        sqlx::query(
            r#"INSERT INTO dri_assignments (
                company_id, dri_key, display_name, agent_ref, domains, authority,
                tenure_kind, valid_from, valid_until, paperclip_dri_id, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, $8, $9, $10, NOW())
            ON CONFLICT (company_id, dri_key) DO UPDATE SET
                display_name = EXCLUDED.display_name,
                agent_ref = EXCLUDED.agent_ref,
                domains = EXCLUDED.domains,
                authority = EXCLUDED.authority,
                tenure_kind = EXCLUDED.tenure_kind,
                valid_from = EXCLUDED.valid_from,
                valid_until = EXCLUDED.valid_until,
                paperclip_dri_id = EXCLUDED.paperclip_dri_id,
                updated_at = NOW()"#,
        )
        .bind(company_id)
        .bind(&e.id)
        .bind(e.name.trim())
        .bind(e.agent_ref.trim())
        .bind(&e.domains)
        .bind(authority)
        .bind(tenure_kind)
        .bind(valid_from)
        .bind(valid_until)
        .bind(Some(e.id.clone()))
        .execute(pool)
        .await?;
        upserted += 1;
    }

    Ok(json!({
        "dris_input": entries.len(),
        "upserted": upserted,
    }))
}
