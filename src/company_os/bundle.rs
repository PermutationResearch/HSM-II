//! Import / export full company snapshots for backup and templates.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct CompanyExport {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub hsmii_home: Option<String>,
    #[serde(default)]
    pub issue_key_prefix: Option<String>,
    /// Company-wide LLM context (Markdown); optional for older bundles.
    #[serde(default)]
    pub context_markdown: Option<String>,
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

/// Work container (Paperclip-style project); tasks may reference `project_id`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectExport {
    pub id: Uuid,
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
    #[serde(default)]
    pub project_old_id: Option<Uuid>,
    #[serde(default)]
    pub workspace_attachment_paths: Vec<String>,
    /// JSON array of `{ "kind", "ref" }` (optional on older bundles).
    #[serde(default)]
    pub capability_refs: Vec<Value>,
    pub state: String,
    pub owner_persona: Option<String>,
    pub priority: i32,
}

/// Company shared / agent-scoped memory pool entry.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MemoryExport {
    pub scope: String,
    #[serde(default)]
    pub company_agent_old_id: Option<Uuid>,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub summary_l0: Option<String>,
    #[serde(default)]
    pub summary_l1: Option<String>,
    #[serde(default = "default_memory_kind")]
    pub kind: String,
}

fn default_memory_kind() -> String {
    "general".to_string()
}

/// Workforce agent (Paperclip-style); `reports_to_id` is the UUID from the export bundle.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentExport {
    pub id: Uuid,
    pub name: String,
    pub role: String,
    pub title: Option<String>,
    pub capabilities: Option<String>,
    pub reports_to_id: Option<Uuid>,
    pub adapter_type: Option<String>,
    pub adapter_config: Value,
    pub budget_monthly_cents: Option<i32>,
    pub briefing: Option<String>,
    pub status: String,
    pub sort_order: i32,
}

#[derive(Debug, Serialize)]
pub struct CompanyBundle {
    pub schema_version: u32,
    pub company: CompanyExport,
    pub goals: Vec<GoalExport>,
    #[serde(default)]
    pub projects: Vec<ProjectExport>,
    #[serde(default)]
    pub memories: Vec<MemoryExport>,
    pub agents: Vec<AgentExport>,
    pub tasks: Vec<TaskExport>,
}

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    /// If true, append `-import` to slug when slug exists.
    #[serde(default)]
    pub slug_suffix_if_exists: bool,
    pub company: CompanyExport,
    pub goals: Vec<GoalExport>,
    #[serde(default)]
    pub projects: Vec<ProjectExport>,
    #[serde(default)]
    pub memories: Vec<MemoryExport>,
    #[serde(default)]
    pub agents: Vec<AgentExport>,
    pub tasks: Vec<TaskExport>,
}

pub async fn export_bundle(pool: &PgPool, company_id: Uuid) -> Result<CompanyBundle> {
    let row: (Uuid, String, String, Option<String>, String, Option<String>) = sqlx::query_as(
        "SELECT id, slug, display_name, hsmii_home, issue_key_prefix, context_markdown FROM companies WHERE id = $1",
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

    let projects: Vec<(Uuid, String, Option<String>, String, i32)> = sqlx::query_as(
        r#"SELECT id, title, description, status, sort_order
           FROM projects WHERE company_id = $1 ORDER BY sort_order, created_at"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let tasks: Vec<(
        String,
        Option<String>,
        Option<Uuid>,
        Option<Uuid>,
        SqlxJson<Value>,
        SqlxJson<Value>,
        String,
        Option<String>,
        i32,
    )> = sqlx::query_as(
        r#"SELECT title, specification, primary_goal_id, project_id, workspace_attachment_paths, capability_refs, state, owner_persona, priority
               FROM tasks WHERE company_id = $1 ORDER BY created_at"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let memory_rows: Vec<(
        String,
        Option<Uuid>,
        String,
        String,
        Vec<String>,
        String,
        Option<String>,
        Option<String>,
        String,
    )> = sqlx::query_as(
        r#"SELECT scope, company_agent_id, title, body, tags, source, summary_l0, summary_l1, kind
           FROM company_memory_entries WHERE company_id = $1 ORDER BY created_at"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let agents_rows: Vec<(
        Uuid,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<Uuid>,
        Option<String>,
        SqlxJson<Value>,
        Option<i32>,
        Option<String>,
        String,
        i32,
    )> = sqlx::query_as(
        r#"SELECT id, name, role, title, capabilities, reports_to, adapter_type, adapter_config,
                  budget_monthly_cents, briefing, status, sort_order
           FROM company_agents WHERE company_id = $1 ORDER BY sort_order, name"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let agents: Vec<AgentExport> = agents_rows
        .into_iter()
        .map(
            |(
                id,
                name,
                role,
                title,
                capabilities,
                reports_to,
                adapter_type,
                adapter_config,
                budget_monthly_cents,
                briefing,
                status,
                sort_order,
            )| AgentExport {
                id,
                name,
                role,
                title,
                capabilities,
                reports_to_id: reports_to,
                adapter_type,
                adapter_config: adapter_config.0,
                budget_monthly_cents,
                briefing,
                status,
                sort_order,
            },
        )
        .collect();

    let any_caps = tasks.iter().any(|t| !match &(t.5).0 {
        Value::Array(a) => a.is_empty(),
        _ => true,
    });
    let schema_version = if any_caps {
        5u32
    } else if !memory_rows.is_empty() {
        4u32
    } else if !projects.is_empty() {
        3u32
    } else if agents.is_empty() {
        1u32
    } else {
        2u32
    };

    fn paths_from_json(v: &Value) -> Vec<String> {
        match v {
            Value::Array(a) => a
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .filter(|s| !s.is_empty())
                .collect(),
            _ => vec![],
        }
    }

    Ok(CompanyBundle {
        schema_version,
        company: CompanyExport {
            id: row.0,
            slug: row.1,
            display_name: row.2,
            hsmii_home: row.3,
            issue_key_prefix: Some(row.4),
            context_markdown: row.5,
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
        projects: projects
            .into_iter()
            .map(
                |(id, title, description, status, sort_order)| ProjectExport {
                    id,
                    title,
                    description,
                    status,
                    sort_order,
                },
            )
            .collect(),
        memories: memory_rows
            .into_iter()
            .map(
                |(
                    scope,
                    company_agent_id,
                    title,
                    body,
                    tags,
                    source,
                    summary_l0,
                    summary_l1,
                    kind,
                )| MemoryExport {
                    scope,
                    company_agent_old_id: company_agent_id,
                    title,
                    body,
                    tags,
                    source,
                    summary_l0,
                    summary_l1,
                    kind,
                },
            )
            .collect(),
        agents,
        tasks: tasks
            .into_iter()
            .map(
                |(
                    title,
                    specification,
                    primary_goal_id,
                    project_id,
                    workspace_attachment_paths,
                    capability_refs,
                    state,
                    owner_persona,
                    priority,
                )| TaskExport {
                    title,
                    specification,
                    primary_goal_old_id: primary_goal_id,
                    project_old_id: project_id,
                    workspace_attachment_paths: paths_from_json(&workspace_attachment_paths.0),
                    capability_refs: match capability_refs.0 {
                        Value::Array(a) => a,
                        _ => vec![],
                    },
                    state,
                    owner_persona,
                    priority,
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

    let prefix = req
        .company
        .issue_key_prefix
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| super::derive_issue_key_prefix(&slug));
    let company_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO companies (slug, display_name, hsmii_home, issue_key_prefix, context_markdown)
           VALUES ($1, $2, $3, $4, $5) RETURNING id"#,
    )
    .bind(&slug)
    .bind(req.company.display_name.trim())
    .bind(&req.company.hsmii_home)
    .bind(&prefix)
    .bind(&req.company.context_markdown)
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

    let mut project_id_map: std::collections::HashMap<Uuid, Uuid> =
        std::collections::HashMap::with_capacity(req.projects.len());
    for p in &req.projects {
        let new_id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO projects (company_id, title, description, status, sort_order)
               VALUES ($1, $2, $3, $4, $5) RETURNING id"#,
        )
        .bind(company_id)
        .bind(p.title.trim())
        .bind(&p.description)
        .bind(if p.status.trim().is_empty() {
            "active"
        } else {
            p.status.trim()
        })
        .bind(p.sort_order)
        .fetch_one(&mut *tx)
        .await
        .context("insert project")?;
        project_id_map.insert(p.id, new_id);
    }

    let mut agent_id_map: std::collections::HashMap<Uuid, Uuid> =
        std::collections::HashMap::with_capacity(req.agents.len());

    let mut pending_agents: Vec<&AgentExport> = req.agents.iter().collect();
    let guard_agents = pending_agents.len().saturating_mul(3).max(1);
    let mut iter_agents = 0usize;
    while !pending_agents.is_empty() {
        iter_agents += 1;
        if iter_agents > guard_agents {
            return Err(anyhow!(
                "agent org chart cycle or missing manager in import"
            ));
        }
        let mut next_agents: Vec<&AgentExport> = Vec::new();
        let mut inserted_agent = false;
        for a in pending_agents {
            let ready = a
                .reports_to_id
                .map(|p| agent_id_map.contains_key(&p))
                .unwrap_or(true);
            if !ready {
                next_agents.push(a);
                continue;
            }
            let new_mgr = a.reports_to_id.and_then(|p| agent_id_map.get(&p).copied());
            let new_id: Uuid = sqlx::query_scalar(
                r#"INSERT INTO company_agents (
                    company_id, name, role, title, capabilities, reports_to,
                    adapter_type, adapter_config, budget_monthly_cents, briefing, status, sort_order
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9, $10, $11, $12)
                RETURNING id"#,
            )
            .bind(company_id)
            .bind(&a.name)
            .bind(&a.role)
            .bind(&a.title)
            .bind(&a.capabilities)
            .bind(new_mgr)
            .bind(&a.adapter_type)
            .bind(SqlxJson(a.adapter_config.clone()))
            .bind(&a.budget_monthly_cents)
            .bind(&a.briefing)
            .bind(&a.status)
            .bind(a.sort_order)
            .fetch_one(&mut *tx)
            .await
            .context("insert company_agent")?;
            agent_id_map.insert(a.id, new_id);
            inserted_agent = true;
        }
        if !inserted_agent {
            return Err(anyhow!(
                "agent org chart cycle or missing manager in import"
            ));
        }
        pending_agents = next_agents;
    }

    for m in &req.memories {
        let agent_uuid = m
            .company_agent_old_id
            .and_then(|old| agent_id_map.get(&old).copied());
        if m.scope.trim().eq_ignore_ascii_case("agent") && agent_uuid.is_none() {
            continue;
        }
        let scope = if m.scope.trim().eq_ignore_ascii_case("agent") {
            "agent"
        } else {
            "shared"
        };
        let src = m.source.trim();
        let source = if src.is_empty() { "import" } else { src };
        let kind = m.kind.trim();
        let kind = if kind.eq_ignore_ascii_case("broadcast") {
            "broadcast"
        } else {
            "general"
        };
        if scope == "agent" && kind == "broadcast" {
            continue;
        }
        sqlx::query(
            r#"INSERT INTO company_memory_entries
               (company_id, scope, company_agent_id, title, body, tags, source, summary_l0, summary_l1, kind)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
        )
        .bind(company_id)
        .bind(scope)
        .bind(agent_uuid)
        .bind(m.title.trim())
        .bind(m.body.trim())
        .bind(&m.tags)
        .bind(source)
        .bind(m.summary_l0.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .bind(m.summary_l1.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .bind(kind)
        .execute(&mut *tx)
        .await
        .context("insert company_memory_entries")?;
    }

    for t in &req.tasks {
        let goal_uuid = t
            .primary_goal_old_id
            .and_then(|old| id_map.get(&old).copied());
        let project_uuid = t
            .project_old_id
            .and_then(|old| project_id_map.get(&old).copied());
        let ancestry: Value = if let Some(gid) = goal_uuid {
            super::compute_goal_ancestry_tx(&mut tx, company_id, gid)
                .await
                .map(|c| serde_json::to_value(&c).unwrap_or(json!([])))
                .unwrap_or(json!([]))
        } else {
            json!([])
        };
        let display_n = super::next_task_display_number_tx(&mut tx, company_id)
            .await
            .context("next task display number")?;
        let ws: Value = serde_json::to_value(&t.workspace_attachment_paths).unwrap_or(json!([]));
        let caps: Value = if t.capability_refs.is_empty() {
            json!([])
        } else {
            Value::Array(t.capability_refs.clone())
        };
        sqlx::query(
            r#"INSERT INTO tasks (company_id, primary_goal_id, project_id, goal_ancestry, title, specification, workspace_attachment_paths, capability_refs, state, owner_persona, priority, display_number)
               VALUES ($1, $2, $3, $4::jsonb, $5, $6, $7::jsonb, $8::jsonb, $9, $10, $11, $12)"#,
        )
        .bind(company_id)
        .bind(goal_uuid)
        .bind(project_uuid)
        .bind(ancestry.to_string())
        .bind(&t.title)
        .bind(&t.specification)
        .bind(SqlxJson(ws))
        .bind(SqlxJson(caps))
        .bind(&t.state)
        .bind(&t.owner_persona)
        .bind(t.priority)
        .bind(display_n)
        .execute(&mut *tx)
        .await
        .context("insert task")?;
    }

    tx.commit().await?;
    Ok(company_id)
}
