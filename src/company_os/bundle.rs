//! Import / export full company snapshots for backup and templates.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct CompanyExport {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub hsmii_home: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GoalExport {
    pub id: Uuid,
    pub parent_goal_id: Option<Uuid>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub sort_order: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskExport {
    pub title: String,
    pub specification: Option<String>,
    pub primary_goal_old_id: Option<Uuid>,
    pub state: String,
    pub owner_persona: Option<String>,
    pub priority: i32,
}

#[derive(Debug, Serialize)]
pub struct CompanyBundle {
    pub schema_version: u32,
    pub company: CompanyExport,
    pub goals: Vec<GoalExport>,
    pub tasks: Vec<TaskExport>,
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    /// If true, append `-import` to slug when slug exists.
    #[serde(default)]
    pub slug_suffix_if_exists: bool,
    pub company: CompanyExport,
    pub goals: Vec<GoalExport>,
    pub tasks: Vec<TaskExport>,
}

pub async fn export_bundle(pool: &PgPool, company_id: Uuid) -> Result<CompanyBundle> {
    let row: (Uuid, String, String, Option<String>) = sqlx::query_as(
        "SELECT id, slug, display_name, hsmii_home FROM companies WHERE id = $1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow!("company not found"))?;

    let goals: Vec<(Uuid, Option<Uuid>, String, Option<String>, String, i32)> = sqlx::query_as(
        r#"SELECT id, parent_goal_id, title, description, status, sort_order
           FROM goals WHERE company_id = $1 ORDER BY sort_order, created_at"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let tasks: Vec<(String, Option<String>, Option<Uuid>, String, Option<String>, i32)> =
        sqlx::query_as(
            r#"SELECT title, specification, primary_goal_id, state, owner_persona, priority
               FROM tasks WHERE company_id = $1 ORDER BY created_at"#,
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;

    Ok(CompanyBundle {
        schema_version: 1,
        company: CompanyExport {
            id: row.0,
            slug: row.1,
            display_name: row.2,
            hsmii_home: row.3,
        },
        goals: goals
            .into_iter()
            .map(
                |(id, parent_goal_id, title, description, status, sort_order)| GoalExport {
                    id,
                    parent_goal_id,
                    title,
                    description,
                    status,
                    sort_order,
                },
            )
            .collect(),
        tasks: tasks
            .into_iter()
            .map(
                |(title, specification, primary_goal_id, state, owner_persona, priority)| {
                    TaskExport {
                        title,
                        specification,
                        primary_goal_old_id: primary_goal_id,
                        state,
                        owner_persona,
                        priority,
                    }
                },
            )
            .collect(),
    })
}

pub async fn import_bundle(pool: &PgPool, req: ImportRequest) -> Result<Uuid> {
    let mut slug = req.company.slug.trim().to_string();
    if slug.is_empty() {
        return Err(anyhow!("company.slug required"));
    }
    if req.slug_suffix_if_exists {
        let exists: bool =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM companies WHERE slug = $1)")
                .bind(&slug)
                .fetch_one(pool)
                .await?;
        if exists {
            slug = format!("{}-import", slug);
        }
    }

    let mut tx = pool.begin().await?;

    let company_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO companies (slug, display_name, hsmii_home)
           VALUES ($1, $2, $3) RETURNING id"#,
    )
    .bind(&slug)
    .bind(req.company.display_name.trim())
    .bind(&req.company.hsmii_home)
    .fetch_one(&mut *tx)
    .await
    .context("insert company")?;

    let mut id_map: std::collections::HashMap<Uuid, Uuid> =
        std::collections::HashMap::with_capacity(req.goals.len());

    let mut pending: Vec<&GoalExport> = req.goals.iter().collect();
    let guard_max = pending.len().saturating_mul(3).max(1);
    let mut iterations = 0usize;
    while !pending.is_empty() {
        iterations += 1;
        if iterations > guard_max {
            return Err(anyhow!("goal parent cycle or missing parent"));
        }
        let mut next_round: Vec<&GoalExport> = Vec::new();
        let mut inserted = false;
        for g in pending {
            let ready = g
                .parent_goal_id
                .map(|p| id_map.contains_key(&p))
                .unwrap_or(true);
            if !ready {
                next_round.push(g);
                continue;
            }
            let new_parent = g.parent_goal_id.and_then(|p| id_map.get(&p).copied());
            let new_id: Uuid = sqlx::query_scalar(
                r#"INSERT INTO goals (company_id, parent_goal_id, title, description, status, sort_order)
                   VALUES ($1, $2, $3, $4, $5, $6) RETURNING id"#,
            )
            .bind(company_id)
            .bind(new_parent)
            .bind(&g.title)
            .bind(&g.description)
            .bind(&g.status)
            .bind(g.sort_order)
            .fetch_one(&mut *tx)
            .await
            .context("insert goal")?;
            id_map.insert(g.id, new_id);
            inserted = true;
        }
        if !inserted {
            return Err(anyhow!("goal parent cycle or missing parent in import"));
        }
        pending = next_round;
    }

    for t in &req.tasks {
        let goal_uuid = t
            .primary_goal_old_id
            .and_then(|old| id_map.get(&old).copied());
        let ancestry: Value = if let Some(gid) = goal_uuid {
            super::compute_goal_ancestry_tx(&mut tx, company_id, gid)
                .await
                .map(|c| serde_json::to_value(&c).unwrap_or(json!([])))
                .unwrap_or(json!([]))
        } else {
            json!([])
        };
        sqlx::query(
            r#"INSERT INTO tasks (company_id, primary_goal_id, goal_ancestry, title, specification, state, owner_persona, priority)
               VALUES ($1, $2, $3::jsonb, $4, $5, $6, $7, $8)"#,
        )
        .bind(company_id)
        .bind(goal_uuid)
        .bind(ancestry.to_string())
        .bind(&t.title)
        .bind(&t.specification)
        .bind(&t.state)
        .bind(&t.owner_persona)
        .bind(t.priority)
        .execute(&mut *tx)
        .await
        .context("insert task")?;
    }

    tx.commit().await?;
    Ok(company_id)
}
