//! Workforce agents (Paperclip-style): org chart, adapter, budget, briefing.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use sqlx::{FromRow, PgPool};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::console::ConsoleState;

use super::company_memory::{fetch_agent_memory_addon, fetch_shared_memory_addon};
use super::company_memory_hybrid::HybridSearchOptions;
use super::markdown_toc::heading_outline;
use super::memory_engine::build_memory_context_addon;
use super::no_db;
use super::workspace_files::list_agent_markdown_instructions;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/agents",
            get(list_agents).post(create_agent),
        )
        .route("/api/company/companies/:company_id/org", get(org_chart))
        .route(
            "/api/company/companies/:company_id/agents/:agent_id/inventory",
            get(get_agent_inventory),
        )
        .route(
            "/api/company/companies/:company_id/agents/:agent_id/operator-thread",
            get(get_agent_operator_thread),
        )
        .route(
            "/api/company/companies/:company_id/agents/:agent_id",
            patch(patch_agent).delete(delete_agent),
        )
        .route(
            "/api/company/tasks/:task_id/llm-context",
            get(get_task_llm_context),
        )
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CompanyAgentRow {
    pub id: Uuid,
    pub company_id: Uuid,
    pub name: String,
    pub role: String,
    pub title: Option<String>,
    pub capabilities: Option<String>,
    pub reports_to: Option<Uuid>,
    pub adapter_type: Option<String>,
    pub adapter_config: SqlxJson<Value>,
    pub budget_monthly_cents: Option<i32>,
    pub briefing: Option<String>,
    pub status: String,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
}

fn valid_agent_name(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty()
        && t.len() <= 128
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

async fn company_has_agent(
    pool: &PgPool,
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

/// True if walking managers upward from `from_manager` reaches `target` (would create a cycle if target reports to from_manager).
async fn manager_chain_reaches(
    pool: &PgPool,
    company_id: Uuid,
    from_manager: Uuid,
    target: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut cur = Some(from_manager);
    let mut guard = 0usize;
    while let Some(id) = cur {
        if guard > 512 {
            return Ok(true);
        }
        guard += 1;
        if id == target {
            return Ok(true);
        }
        let row = sqlx::query_scalar::<_, Option<Uuid>>(
            "SELECT reports_to FROM company_agents WHERE id = $1 AND company_id = $2",
        )
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?;
        cur = match row {
            None => None,
            Some(r) => r,
        };
    }
    Ok(false)
}

#[derive(Deserialize)]
pub struct CreateAgentBody {
    pub name: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub capabilities: Option<String>,
    #[serde(default)]
    pub reports_to: Option<Uuid>,
    #[serde(default)]
    pub adapter_type: Option<String>,
    #[serde(default)]
    pub adapter_config: Option<Value>,
    #[serde(default)]
    pub budget_monthly_cents: Option<i32>,
    #[serde(default)]
    pub briefing: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

async fn list_agents(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, CompanyAgentRow>(
        r#"SELECT id, company_id, name, role, title, capabilities, reports_to, adapter_type,
                  adapter_config, budget_monthly_cents, briefing, status, sort_order,
                  created_at::text, updated_at::text
           FROM company_agents WHERE company_id = $1
           ORDER BY sort_order, name"#,
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
    let agents: Vec<AgentListItem> = rows
        .into_iter()
        .map(|agent| AgentListItem {
            agent,
            live_in_runs_surfaces: LIVE_IN_RUNS_SURFACES,
            profile_only_surfaces: PROFILE_ONLY_SURFACES,
        })
        .collect();
    Ok(Json(json!({ "agents": agents })))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct CompanySkillSummaryRow {
    id: Uuid,
    slug: String,
    name: String,
    description: String,
    skill_path: String,
    source: String,
    updated_at: String,
}

fn normalize_skill_key(s: &str) -> String {
    s.trim().to_lowercase()
}

fn paperclip_skill_refs(adapter_config: &Value) -> Vec<String> {
    adapter_config
        .get("paperclip")
        .and_then(|p| p.get("skills"))
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(|t| t.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn capability_csv_refs(capabilities: &Option<String>) -> Vec<String> {
    capabilities
        .as_ref()
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn merge_roster_skill_refs(adapter_config: &Value, capabilities: &Option<String>) -> Vec<String> {
    let mut seen = HashSet::<String>::new();
    let mut out = Vec::new();
    for r in paperclip_skill_refs(adapter_config)
        .into_iter()
        .chain(capability_csv_refs(capabilities))
    {
        let k = normalize_skill_key(&r);
        if k.is_empty() {
            continue;
        }
        if seen.insert(k) {
            out.push(r);
        }
    }
    out
}

/// Paperclip roster + `company_skills` + Markdown instruction files discoverable on disk.
async fn get_agent_inventory(
    State(st): State<ConsoleState>,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let agent = sqlx::query_as::<_, CompanyAgentRow>(
        r#"SELECT id, company_id, name, role, title, capabilities, reports_to, adapter_type,
                  adapter_config, budget_monthly_cents, briefing, status, sort_order,
                  created_at::text, updated_at::text
           FROM company_agents WHERE id = $1 AND company_id = $2"#,
    )
    .bind(agent_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(agent) = agent else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent not found" })),
        ));
    };

    let home_cell: Option<Option<String>> =
        sqlx::query_scalar("SELECT hsmii_home FROM companies WHERE id = $1")
            .bind(company_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;

    let instruction_files = match home_cell.as_ref().and_then(|o| o.as_ref()) {
        Some(h) if !h.trim().is_empty() => {
            match list_agent_markdown_instructions(h.trim(), &agent.name, 4, 200) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "list_agent_markdown_instructions failed");
                    Vec::new()
                }
            }
        }
        _ => Vec::new(),
    };

    let skills: Vec<CompanySkillSummaryRow> = sqlx::query_as(
        r#"SELECT id, slug, name, description, skill_path, source, updated_at::text
           FROM company_skills WHERE company_id = $1
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

    let by_slug: HashMap<String, &CompanySkillSummaryRow> = skills
        .iter()
        .map(|s| (normalize_skill_key(&s.slug), s))
        .collect();

    let roster_refs = merge_roster_skill_refs(&agent.adapter_config.0, &agent.capabilities);
    let roster_keys: HashSet<String> = roster_refs.iter().map(|r| normalize_skill_key(r)).collect();

    let skills_linked: Vec<Value> = roster_refs
        .iter()
        .filter_map(|r| {
            let row = by_slug.get(&normalize_skill_key(r))?;
            Some(json!({
                "ref": r,
                "skill": {
                    "id": row.id,
                    "slug": row.slug,
                    "name": row.name,
                    "description": row.description,
                    "skill_path": row.skill_path,
                    "source": row.source,
                    "updated_at": row.updated_at,
                }
            }))
        })
        .collect();

    let unresolved_skill_refs: Vec<String> = roster_refs
        .iter()
        .filter(|r| !by_slug.contains_key(&normalize_skill_key(r)))
        .cloned()
        .collect();

    let company_skills_catalog: Vec<Value> = skills
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "slug": s.slug,
                "name": s.name,
                "description": s.description,
                "skill_path": s.skill_path,
                "source": s.source,
                "updated_at": s.updated_at,
                "on_agent_roster": roster_keys.contains(&normalize_skill_key(&s.slug)),
            })
        })
        .collect();

    let briefing_preview = agent
        .briefing
        .as_ref()
        .map(|b| b.chars().take(480).collect::<String>());

    Ok(Json(json!({
        "agent": {
            "id": agent.id,
            "company_id": agent.company_id,
            "name": agent.name,
            "role": agent.role,
            "title": agent.title,
            "capabilities": agent.capabilities,
            "adapter_type": agent.adapter_type,
            "adapter_config": agent.adapter_config.0.clone(),
            "briefing_preview": briefing_preview,
        },
        "roster_skill_refs": roster_refs,
        "skills_linked": skills_linked,
        "unresolved_skill_refs": unresolved_skill_refs,
        "company_skills_catalog": company_skills_catalog,
        "instruction_markdown_files": instruction_files,
        "hsmii_home_configured": matches!(
            home_cell.as_ref(),
            Some(Some(s)) if !s.trim().is_empty()
        ),
    })))
}

const OPERATOR_THREAD_DIGEST_MAX: usize = 12_000;
const OPERATOR_THREAD_DIGEST_MAX_TEMP_RESTRICTED: usize = 7_500;

fn normalize_whitespace_compact(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn temperature_restricted_context_mode() -> bool {
    if std::env::var("HSM_CONTEXT_COMPACTION_TEMP_RESTRICTED")
        .ok()
        .as_deref()
        == Some("1")
    {
        return true;
    }
    let model = std::env::var("DEFAULT_LLM_MODEL")
        .unwrap_or_default()
        .to_ascii_lowercase();
    model.contains("o1") || model.contains("o3")
}

fn normalize_stig_notes_json(v: &Value) -> Vec<Value> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in arr {
        let Some(o) = item.as_object() else {
            continue;
        };
        let text = o
            .get("text")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .unwrap_or("");
        if text.is_empty() {
            continue;
        }
        out.push(json!({
            "at": o.get("at").and_then(|x| x.as_str()).unwrap_or(""),
            "actor": o.get("actor").and_then(|x| x.as_str()).unwrap_or("operator"),
            "text": text,
        }));
    }
    out
}

fn build_operator_thread_digest(agent_name: &str, tasks: &[(Uuid, String, Value, String)]) -> String {
    let mut digest = format!("## Operator ↔ {agent_name} — compact thread (for LLM / handoff)\n\n");
    let temp_restricted = temperature_restricted_context_mode();
    let note_limit = if temp_restricted { 8 } else { 16 };
    let text_char_limit = if temp_restricted { 220 } else { 520 };
    let digest_limit = if temp_restricted {
        OPERATOR_THREAD_DIGEST_MAX_TEMP_RESTRICTED
    } else {
        OPERATOR_THREAD_DIGEST_MAX
    };
    for (id, title, notes_val, _created) in tasks {
        let notes = normalize_stig_notes_json(notes_val);
        if notes.is_empty() {
            continue;
        }
        digest.push_str(&format!(
            "### Task: {} (`{id}`)\n",
            normalize_whitespace_compact(title)
        ));
        let mut seen_text = std::collections::HashSet::new();
        for n in notes.into_iter().take(note_limit) {
            let at = n["at"].as_str().unwrap_or("");
            let actor = n["actor"].as_str().unwrap_or("operator");
            let raw_text = n["text"].as_str().unwrap_or("");
            let compact = normalize_whitespace_compact(raw_text);
            if compact.is_empty() {
                continue;
            }
            let dedupe_key = format!("{}|{}", actor.to_ascii_lowercase(), compact.to_ascii_lowercase());
            if !seen_text.insert(dedupe_key) {
                continue;
            }
            let text = compact.chars().take(text_char_limit).collect::<String>();
            digest.push_str(&format!("- [{at}] {actor}: {text}\n"));
        }
        digest.push('\n');
    }
    if digest.chars().count() > digest_limit {
        let mut t = digest.chars().take(digest_limit).collect::<String>();
        t.push_str("\n\n…(truncated; narrow tasks or copy per-task notes)");
        t
    } else {
        digest
    }
}

/// Stigmergic `context_notes` on tasks owned by or checked out to this roster agent (operator rail).
async fn get_agent_operator_thread(
    State(st): State<ConsoleState>,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let agent = sqlx::query_as::<_, CompanyAgentRow>(
        r#"SELECT id, company_id, name, role, title, capabilities, reports_to, adapter_type,
                  adapter_config, budget_monthly_cents, briefing, status, sort_order,
                  created_at::text, updated_at::text
           FROM company_agents WHERE id = $1 AND company_id = $2"#,
    )
    .bind(agent_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    let Some(agent) = agent else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent not found" })),
        ));
    };

    #[derive(FromRow)]
    struct OperatorTaskNotesRow {
        id: Uuid,
        title: String,
        context_notes: SqlxJson<Value>,
        created_at: String,
    }

    let persona = agent.name.trim();
    let rows = sqlx::query_as::<_, OperatorTaskNotesRow>(
        r#"SELECT id, title, context_notes, created_at::text
           FROM tasks
           WHERE company_id = $1
             AND (
               lower(trim(coalesce(owner_persona, ''))) = lower(trim($2))
               OR lower(trim(coalesce(checked_out_by, ''))) = lower(trim($2))
             )
           ORDER BY created_at DESC
           LIMIT 200"#,
    )
    .bind(company_id)
    .bind(persona)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let mut tasks_out: Vec<Value> = Vec::new();
    let mut flat: Vec<Value> = Vec::new();
    let mut digest_inputs: Vec<(Uuid, String, Value, String)> = Vec::new();

    for row in rows {
        let notes = normalize_stig_notes_json(&row.context_notes.0);
        digest_inputs.push((
            row.id,
            row.title.clone(),
            row.context_notes.0.clone(),
            row.created_at.clone(),
        ));
        for n in &notes {
            flat.push(json!({
                "task_id": row.id,
                "task_title": &row.title,
                "note": n,
            }));
        }
        tasks_out.push(json!({
            "task_id": row.id,
            "title": row.title,
            "created_at": row.created_at,
            "notes": notes,
        }));
    }

    let compact_digest = build_operator_thread_digest(persona, &digest_inputs);

    Ok(Json(json!({
        "agent_id": agent_id,
        "agent_name": agent.name,
        "tasks": tasks_out,
        "notes_flat": flat,
        "compact_digest": compact_digest,
        "total_tasks": tasks_out.len(),
    })))
}

async fn create_agent(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<CreateAgentBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let name = body.name.trim().to_string();
    if !valid_agent_name(&name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "name must be 1–128 chars: letters, digits, underscore, hyphen only"
            })),
        ));
    }
    let role = body
        .role
        .unwrap_or_else(|| "worker".to_string())
        .trim()
        .to_string();
    if role.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "role cannot be empty" })),
        ));
    }
    if let Some(rid) = body.reports_to {
        if !company_has_agent(pool, company_id, rid)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?
        {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "reports_to agent not in this company" })),
            ));
        }
    }
    let cfg = body.adapter_config.unwrap_or_else(|| json!({}));
    let sort_order = body.sort_order.unwrap_or(0);
    let row = sqlx::query_as::<_, CompanyAgentRow>(
        r#"INSERT INTO company_agents (
               company_id, name, role, title, capabilities, reports_to,
               adapter_type, adapter_config, budget_monthly_cents, briefing, sort_order
           ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8::jsonb,$9,$10,$11)
           RETURNING id, company_id, name, role, title, capabilities, reports_to, adapter_type,
                     adapter_config, budget_monthly_cents, briefing, status, sort_order,
                     created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&name)
    .bind(&role)
    .bind(&body.title)
    .bind(&body.capabilities)
    .bind(&body.reports_to)
    .bind(&body.adapter_type)
    .bind(SqlxJson(cfg))
    .bind(&body.budget_monthly_cents)
    .bind(&body.briefing)
    .bind(sort_order)
    .fetch_one(pool)
    .await;
    match row {
        Ok(r) => Ok((StatusCode::CREATED, Json(json!({ "agent": r })))),
        Err(sqlx::Error::Database(d)) if d.code().as_deref() == Some("23505") => Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "agent name already exists in this company" })),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )),
    }
}

#[derive(Deserialize, Default)]
pub struct PatchAgentBody {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub title: Option<Value>,
    #[serde(default)]
    pub capabilities: Option<Value>,
    #[serde(default)]
    pub reports_to: Option<Value>,
    #[serde(default)]
    pub adapter_type: Option<Value>,
    #[serde(default)]
    pub adapter_config: Option<Value>,
    #[serde(default)]
    pub budget_monthly_cents: Option<Value>,
    #[serde(default)]
    pub briefing: Option<Value>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

fn opt_str_patch(v: Option<Value>) -> Result<Option<Option<String>>, &'static str> {
    match v {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(s)) => Ok(Some(Some(s))),
        _ => Err("expected string or null"),
    }
}

fn opt_i32_patch(v: Option<Value>) -> Result<Option<Option<i32>>, &'static str> {
    match v {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::Number(n)) => n
            .as_i64()
            .and_then(|x| i32::try_from(x).ok())
            .map(|x| Ok(Some(Some(x))))
            .unwrap_or(Err("expected integer")),
        _ => Err("expected integer or null"),
    }
}

async fn patch_agent(
    State(st): State<ConsoleState>,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchAgentBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if !company_has_agent(pool, company_id, agent_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent not found" })),
        ));
    }

    let current: CompanyAgentRow = sqlx::query_as::<_, CompanyAgentRow>(
        r#"SELECT id, company_id, name, role, title, capabilities, reports_to, adapter_type,
                  adapter_config, budget_monthly_cents, briefing, status, sort_order,
                  created_at::text, updated_at::text
           FROM company_agents WHERE id = $1 AND company_id = $2"#,
    )
    .bind(agent_id)
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    let mut role = current.role.clone();
    if let Some(r) = &body.role {
        let t = r.trim();
        if t.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "role cannot be empty" })),
            ));
        }
        role = t.to_string();
    }

    let title = match opt_str_patch(body.title.clone()) {
        Ok(None) => current.title.clone(),
        Ok(Some(inner)) => inner,
        Err(msg) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("title: {msg}") })),
            ));
        }
    };
    let capabilities = match opt_str_patch(body.capabilities.clone()) {
        Ok(None) => current.capabilities.clone(),
        Ok(Some(inner)) => inner,
        Err(msg) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("capabilities: {msg}") })),
            ));
        }
    };
    let briefing = match opt_str_patch(body.briefing.clone()) {
        Ok(None) => current.briefing.clone(),
        Ok(Some(inner)) => inner,
        Err(msg) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("briefing: {msg}") })),
            ));
        }
    };

    let budget_monthly_cents = match opt_i32_patch(body.budget_monthly_cents.clone()) {
        Ok(None) => current.budget_monthly_cents,
        Ok(Some(inner)) => inner,
        Err(msg) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("budget_monthly_cents: {msg}") })),
            ));
        }
    };

    let mut reports_to = current.reports_to;
    if let Some(v) = &body.reports_to {
        reports_to = match v {
            Value::Null => None,
            _ => {
                let u: Uuid = serde_json::from_value(v.clone()).map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "reports_to must be UUID or null" })),
                    )
                })?;
                if u == agent_id {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "agent cannot report to itself" })),
                    ));
                }
                if !company_has_agent(pool, company_id, u).await.map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": e.to_string() })),
                    )
                })? {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "reports_to agent not in this company" })),
                    ));
                }
                if manager_chain_reaches(pool, company_id, u, agent_id)
                    .await
                    .map_err(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({ "error": e.to_string() })),
                        )
                    })?
                {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "reports_to would create a cycle in org chart" })),
                    ));
                }
                Some(u)
            }
        };
    }

    let mut adapter_type = current.adapter_type.clone();
    if let Some(v) = &body.adapter_type {
        match v {
            Value::Null => adapter_type = None,
            Value::String(s) => adapter_type = Some(s.clone()),
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "adapter_type must be string or null" })),
                ));
            }
        }
    }

    let mut adapter_config = current.adapter_config.clone();
    if let Some(v) = body.adapter_config.clone() {
        adapter_config = SqlxJson(v);
    }

    let mut status = current.status.clone();
    if let Some(s) = &body.status {
        let t = s.trim();
        if t.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "status cannot be empty" })),
            ));
        }
        status = t.to_string();
    }

    let sort_order = body.sort_order.unwrap_or(current.sort_order);

    let row = sqlx::query_as::<_, CompanyAgentRow>(
        r#"UPDATE company_agents SET
             role = $3, title = $4, capabilities = $5, reports_to = $6,
             adapter_type = $7, adapter_config = $8::jsonb,
             budget_monthly_cents = $9, briefing = $10, status = $11, sort_order = $12,
             updated_at = now()
           WHERE id = $1 AND company_id = $2
           RETURNING id, company_id, name, role, title, capabilities, reports_to, adapter_type,
                     adapter_config, budget_monthly_cents, briefing, status, sort_order,
                     created_at::text, updated_at::text"#,
    )
    .bind(agent_id)
    .bind(company_id)
    .bind(&role)
    .bind(&title)
    .bind(&capabilities)
    .bind(&reports_to)
    .bind(&adapter_type)
    .bind(adapter_config)
    .bind(&budget_monthly_cents)
    .bind(&briefing)
    .bind(&status)
    .bind(sort_order)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;

    Ok(Json(json!({ "agent": row })))
}

async fn delete_agent(
    State(st): State<ConsoleState>,
    Path((company_id, agent_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if !company_has_agent(pool, company_id, agent_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "agent not found" })),
        ));
    }
    let res = sqlx::query("DELETE FROM company_agents WHERE id = $1 AND company_id = $2")
        .bind(agent_id)
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
            Json(json!({ "error": "agent not found" })),
        ));
    }
    Ok((
        StatusCode::OK,
        Json(json!({ "deleted": true, "agent_id": agent_id })),
    ))
}

#[derive(Serialize)]
struct OrgNode {
    #[serde(flatten)]
    pub agent: CompanyAgentRow,
    pub direct_reports: Vec<OrgNode>,
}

fn build_org_forest(agents: Vec<CompanyAgentRow>) -> Vec<OrgNode> {
    use std::collections::HashMap;
    let by_id: HashMap<Uuid, CompanyAgentRow> = agents.iter().cloned().map(|a| (a.id, a)).collect();
    let mut children: HashMap<Option<Uuid>, Vec<Uuid>> = HashMap::new();
    for a in &agents {
        children.entry(a.reports_to).or_default().push(a.id);
    }
    for v in children.values_mut() {
        v.sort_by_key(|id| {
            by_id
                .get(id)
                .map(|r| (r.sort_order, r.name.clone()))
                .unwrap_or_default()
        });
    }
    fn walk(
        id: Uuid,
        by_id: &HashMap<Uuid, CompanyAgentRow>,
        children: &HashMap<Option<Uuid>, Vec<Uuid>>,
    ) -> OrgNode {
        let agent = by_id.get(&id).unwrap().clone();
        let dr = children
            .get(&Some(id))
            .map(|ids| ids.iter().map(|cid| walk(*cid, by_id, children)).collect())
            .unwrap_or_default();
        OrgNode {
            agent,
            direct_reports: dr,
        }
    }
    let roots = children.get(&None).cloned().unwrap_or_default();
    roots
        .into_iter()
        .map(|rid| walk(rid, &by_id, &children))
        .collect()
}

async fn org_chart(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let rows = sqlx::query_as::<_, CompanyAgentRow>(
        r#"SELECT id, company_id, name, role, title, capabilities, reports_to, adapter_type,
                  adapter_config, budget_monthly_cents, briefing, status, sort_order,
                  created_at::text, updated_at::text
           FROM company_agents WHERE company_id = $1 AND status <> 'terminated'
           ORDER BY sort_order, name"#,
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
    let tree = build_org_forest(rows.clone());
    Ok(Json(json!({
        "agents": rows,
        "tree": tree,
    })))
}

impl Clone for CompanyAgentRow {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            company_id: self.company_id,
            name: self.name.clone(),
            role: self.role.clone(),
            title: self.title.clone(),
            capabilities: self.capabilities.clone(),
            reports_to: self.reports_to,
            adapter_type: self.adapter_type.clone(),
            adapter_config: SqlxJson(self.adapter_config.0.clone()),
            budget_monthly_cents: self.budget_monthly_cents,
            briefing: self.briefing.clone(),
            status: self.status.clone(),
            sort_order: self.sort_order,
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

// --- Company agent → LLM / checkout run profile (persona resolution) ---

/// Surfaces where `company_agents` rows are merged into runs or API context (UI truth layer).
pub const LIVE_IN_RUNS_SURFACES: &[&str] = &["task_checkout", "task_llm_context"];
/// Stored profile only until the integration calls the resolver.
pub const PROFILE_ONLY_SURFACES: &[&str] = &["email_draft"];

#[derive(Debug, Serialize)]
pub struct AgentRunProfile {
    pub resolved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<Uuid>,
    /// `checkout_ref` or `owner_persona` — which key matched `company_agents.name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_as: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_type: Option<String>,
    #[serde(default)]
    pub adapter_config: Value,
    /// Append to system (or developer) message for the worker LLM.
    pub system_context_addon: String,
    pub system_context_addon_bytes: usize,
}

/// Markdown block prepended to task LLM context when `companies.context_markdown` is set.
pub fn build_company_wide_context_addon(context_markdown: Option<&str>) -> String {
    let Some(raw) = context_markdown.map(str::trim).filter(|s| !s.is_empty()) else {
        return String::new();
    };
    format!("## Company-wide context\n\n{raw}\n\n")
}

pub fn build_llm_system_addon(row: &CompanyAgentRow) -> String {
    let mut s = String::new();
    s.push_str(&format!("## Company agent profile: {}\n", row.name));
    s.push_str(&format!("- Role: {}\n", row.role));
    if let Some(ref t) = row.title {
        let t = t.trim();
        if !t.is_empty() {
            s.push_str(&format!("- Title: {t}\n"));
        }
    }
    if let Some(ref c) = row.capabilities {
        let c = c.trim();
        if !c.is_empty() {
            s.push_str("\n### Capabilities\n");
            s.push_str(c);
            s.push('\n');
        }
    }
    if let Some(ref b) = row.briefing {
        let b = b.trim();
        if !b.is_empty() {
            s.push_str("\n### Briefing\n");
            s.push_str(b);
            s.push('\n');
        }
    }
    s
}

fn empty_run_profile() -> AgentRunProfile {
    AgentRunProfile {
        resolved: false,
        agent_id: None,
        matched_as: None,
        matched_agent_name: None,
        adapter_type: None,
        adapter_config: json!({}),
        system_context_addon: String::new(),
        system_context_addon_bytes: 0,
    }
}

fn run_profile_from_row(row: &CompanyAgentRow, matched_as: &str) -> AgentRunProfile {
    let system_context_addon = build_llm_system_addon(row);
    let system_context_addon_bytes = system_context_addon.len();
    AgentRunProfile {
        resolved: true,
        agent_id: Some(row.id),
        matched_as: Some(matched_as.to_string()),
        matched_agent_name: Some(row.name.clone()),
        adapter_type: row.adapter_type.clone(),
        adapter_config: row.adapter_config.0.clone(),
        system_context_addon,
        system_context_addon_bytes,
    }
}

/// Load active `company_agents` row by case-insensitive name match.
pub async fn load_active_agent_by_name_ci(
    pool: &PgPool,
    company_id: Uuid,
    name: &str,
) -> Result<Option<CompanyAgentRow>, sqlx::Error> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    sqlx::query_as::<_, CompanyAgentRow>(
        r#"SELECT id, company_id, name, role, title, capabilities, reports_to, adapter_type,
                  adapter_config, budget_monthly_cents, briefing, status, sort_order,
                  created_at::text, updated_at::text
           FROM company_agents
           WHERE company_id = $1 AND status = 'active' AND lower(trim(name)) = lower(trim($2))
           LIMIT 1"#,
    )
    .bind(company_id)
    .bind(name)
    .fetch_optional(pool)
    .await
}

/// Resolve persona: `checkout_ref` first (checked-out worker id), then task `owner_persona`.
pub async fn resolve_run_profile_for_task(
    pool: &PgPool,
    company_id: Uuid,
    checkout_ref: &str,
    owner_persona: Option<&str>,
) -> Result<AgentRunProfile, sqlx::Error> {
    let cr = checkout_ref.trim();
    if !cr.is_empty() {
        if let Some(row) = load_active_agent_by_name_ci(pool, company_id, cr).await? {
            return Ok(run_profile_from_row(&row, "checkout_ref"));
        }
    }
    if let Some(p) = owner_persona.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(row) = load_active_agent_by_name_ci(pool, company_id, p).await? {
            return Ok(run_profile_from_row(&row, "owner_persona"));
        }
    }
    Ok(empty_run_profile())
}

#[derive(sqlx::FromRow)]
struct TaskLlmContextRow {
    company_id: Uuid,
    checked_out_by: Option<String>,
    owner_persona: Option<String>,
    title: String,
    specification: Option<String>,
    workspace_attachment_paths: Value,
    capability_refs: SqlxJson<Value>,
    context_notes: SqlxJson<Value>,
}

fn task_memory_query_text(t: &TaskLlmContextRow) -> String {
    let mut parts = vec![t.title.trim().to_string()];
    if let Some(spec) = t
        .specification
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parts.push(spec.chars().take(1200).collect());
    }
    if let Value::Array(caps) = &t.capability_refs.0 {
        let refs = caps
            .iter()
            .filter_map(|c| {
                let kind = c.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                let rf = c.get("ref").and_then(|v| v.as_str()).unwrap_or("");
                if rf.is_empty() {
                    None
                } else {
                    Some(format!("{kind}:{rf}"))
                }
            })
            .collect::<Vec<_>>();
        if !refs.is_empty() {
            parts.push(refs.join(" "));
        }
    }
    if let Value::Array(notes) = &t.context_notes.0 {
        let tail = notes
            .iter()
            .rev()
            .take(6)
            .filter_map(|n| n.get("text").and_then(|v| v.as_str()))
            .collect::<Vec<_>>();
        if !tail.is_empty() {
            parts.push(tail.join("\n"));
        }
    }
    parts.join("\n")
}

async fn get_task_llm_context(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    crate::policy_config::ensure_loaded();
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let t = sqlx::query_as::<_, TaskLlmContextRow>(
        r#"SELECT company_id, checked_out_by, owner_persona, title, specification, workspace_attachment_paths, capability_refs, context_notes
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
    let Some(t) = t else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "task not found" })),
        ));
    };
    let checkout_ref = t.checked_out_by.as_deref().unwrap_or("");
    let profile =
        resolve_run_profile_for_task(pool, t.company_id, checkout_ref, t.owner_persona.as_deref())
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
    let company_context_markdown: Option<String> =
        sqlx::query_scalar("SELECT context_markdown FROM companies WHERE id = $1")
            .bind(t.company_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
    let hsmii_home: Option<String> =
        sqlx::query_scalar("SELECT hsmii_home FROM companies WHERE id = $1")
            .bind(t.company_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?;
    let hsmii_home_for_task = hsmii_home.clone();
    let task_memory_query = task_memory_query_text(&t);
    let mut shared_opts = HybridSearchOptions::for_scope("shared", Uuid::nil());
    shared_opts.latest_only = true;
    shared_opts.valid_at = Some(Utc::now());
    shared_opts.limit = 6;
    let shared_mem_addon = match build_memory_context_addon(
        pool,
        t.company_id,
        &task_memory_query,
        &shared_opts,
        "Company shared memory (graph/time aware)",
    )
    .await
    {
        Ok(addon) if addon.match_count > 0 => addon.markdown,
        Ok(_) | Err(_) => fetch_shared_memory_addon(pool, t.company_id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?,
    };
    let mut agent_mem_addon = String::new();
    if let Some(aid) = profile.agent_id {
        let mut agent_opts = HybridSearchOptions::for_scope("agent", aid);
        agent_opts.latest_only = true;
        agent_opts.valid_at = Some(Utc::now());
        agent_opts.limit = 4;
        agent_mem_addon = match build_memory_context_addon(
            pool,
            t.company_id,
            &task_memory_query,
            &agent_opts,
            "Company agent memory (graph/time aware)",
        )
        .await
        {
            Ok(addon) if addon.match_count > 0 => addon.markdown,
            Ok(_) | Err(_) => fetch_agent_memory_addon(pool, t.company_id, aid)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": e.to_string() })),
                    )
                })?,
        };
    }
    let agent_memory_addon_bytes = agent_mem_addon.len();
    let mut task_addon = format!("## Current task\n\n- **Title:** {}\n", t.title);
    if let Some(spec) = t
        .specification
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        task_addon.push_str(&format!("\n### Specification\n\n{spec}\n"));
    }
    let paths: Vec<String> = match &t.workspace_attachment_paths {
        Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec![],
    };
    if !paths.is_empty() {
        task_addon.push_str("\n### Workspace file references\n\nPaths are relative to the company pack home (`hsmii_home`).\n\n");
        for p in &paths {
            task_addon.push_str(&format!("- `{p}`\n"));
        }
        task_addon.push('\n');
    }
    if let Value::Array(caps) = &t.capability_refs.0 {
        if !caps.is_empty() {
            task_addon.push_str("\n### Linked capabilities\n\nExplicit skill / SOP / tool / pack / agent refs on this task.\n\n");
            for c in caps.iter().take(24) {
                let kind = c.get("kind").and_then(|x| x.as_str()).unwrap_or("?");
                let rf = c.get("ref").and_then(|x| x.as_str()).unwrap_or("");
                if !rf.is_empty() {
                    task_addon.push_str(&format!("- **{kind}**: `{rf}`\n"));
                }
            }
            task_addon.push('\n');
        }
    }
    if let Value::Array(arr) = &t.context_notes.0 {
        if !arr.is_empty() {
            task_addon.push_str("\n### Task handoff notes (stigmergic)\n\n");
            let take = arr.len().saturating_sub(20);
            for n in arr.iter().skip(take) {
                let text = n.get("text").and_then(|x| x.as_str()).unwrap_or("");
                let actor = n.get("actor").and_then(|x| x.as_str()).unwrap_or("");
                let at = n.get("at").and_then(|x| x.as_str()).unwrap_or("");
                if !text.is_empty() {
                    task_addon.push_str(&format!("- **{at}** `{actor}`: {text}\n"));
                }
            }
            task_addon.push('\n');
        }
    }
    if let Some(home) = hsmii_home_for_task
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        task_addon.push_str(&format!("### Pack home (`hsmii_home`)\n\n`{home}`\n\n"));
    }
    task_addon.push_str(
        "Workspace discipline:\n\
         - Paths above are **pointers** (files stay on disk under `hsmii_home`); they may live under another agent’s folder (e.g. `agents/<id>/…`)—resolve from the pack root.\n\
         - If the pack has a curated **index / TOC** markdown (operator-maintained), **open that first** before loading deep files—same idea as a small surface over a large corpus.\n\
         - Per-agent instructions after pack import: `agents/<agent_folder>/AGENTS.md` (see company import).\n\n\
         Tools: `company_memory_search` / `company_memory_append`; `company_run_feedback_append` / `company_promote_feedback_to_task` (HTTP tools on full registry); optional `GET …/memory/export.md` for a git-friendly index.\n\
         Memory writes (`company_memory_append`): you must pass `scope` as the literal `shared` or `agent` every time (no default). Prefer **`shared`** for durable facts any other agent on this company should see (policies, URLs, decisions, handoffs). Use **`agent`** only for private preference, scratch, or explicitly sensitive per-agent notes. For urgent company-wide lines, use shared with **`kind`: broadcast**.\n\
         Memory reads (`company_memory_search`): default mode is company-wide **`shared`**; use **`mine`** for this agent’s scoped rows only, or **`both`** when you need shared + private together.\n\n",
    );
    let mut company_addon = build_company_wide_context_addon(company_context_markdown.as_deref());
    let toc = heading_outline(company_context_markdown.as_deref().unwrap_or(""), 40);
    if !toc.is_empty() {
        company_addon.push_str("## Company context — heading outline\n\n");
        company_addon.push_str(&toc);
        company_addon.push_str("\n\n");
    }
    let company_context_addon_bytes = company_addon.len();

    let vision_alignment_enabled = std::env::var("HSM_VISION_ALIGNMENT_LLM_ADDON")
        .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"))
        .unwrap_or(true);
    let (vision_alignment_addon, vision_alignment_addon_bytes) = if vision_alignment_enabled {
        super::build_llm_vision_alignment_addon(pool, t.company_id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
            })?
    } else {
        (String::new(), 0)
    };

    let shared_memory_addon_bytes = shared_mem_addon.len();
    let task_context_addon_bytes = task_addon.len();
    let combined_system_addon = format!(
        "{company_addon}{vision_alignment_addon}{shared_mem_addon}{agent_mem_addon}{task_addon}{}",
        profile.system_context_addon
    );
    let combined_system_addon_bytes = combined_system_addon.len();

    let pol = crate::policy_config::get();
    let context_manifest = crate::context_manifest::company_task_llm_context_manifest(
        vec![
            ("company", company_context_addon_bytes),
            ("vision_alignment", vision_alignment_addon_bytes),
            ("shared_memory", shared_memory_addon_bytes),
            ("agent_memory", agent_memory_addon_bytes),
            ("task", task_context_addon_bytes),
            ("agent_profile", profile.system_context_addon_bytes),
        ],
        |k| pol.tier_for_company_llm_section(k),
    );

    tracing::info!(
        target: "hsm.context_manifest",
        summary = %context_manifest.summary_line(),
        scope = "company_llm_context",
        task_id = %task_id,
        company_id = %t.company_id,
        "assembled company task llm-context"
    );
    if std::env::var("HSM_LOG_CONTEXT_MANIFEST")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
    {
        if let Ok(json) = serde_json::to_string(&context_manifest) {
            tracing::debug!(target: "hsm.context_manifest", %json, "full company llm-context manifest");
        }
    }

    crate::telemetry::client().record_technical(
        "company.task.llm_context",
        json!({
            "task_id": task_id,
            "company_id": t.company_id,
            "combined_system_addon_bytes": combined_system_addon_bytes,
            "sections": context_manifest.sections.iter().map(|s| json!({
                "key": s.key,
                "bytes": s.emitted_bytes,
                "tier": format!("{:?}", s.tier),
            })).collect::<Vec<_>>(),
        }),
    );

    tracing::info!(
        target: "hsm_company_agent_inject",
        task_id = %task_id,
        company_id = %t.company_id,
        endpoint = "llm_context_get",
        company_agent_row_found = profile.resolved,
        company_context_addon_bytes,
        vision_alignment_addon_bytes,
        shared_memory_addon_bytes,
        agent_memory_addon_bytes,
        task_context_addon_bytes,
        combined_system_addon_bytes,
        addon_bytes = profile.system_context_addon_bytes,
        matched_as = ?profile.matched_as,
        agent_id = ?profile.agent_id,
    );
    let context_manifest_json =
        serde_json::to_value(&context_manifest).unwrap_or_else(|_| json!({}));
    Ok(Json(json!({
        "task_id": task_id,
        "company_id": t.company_id,
        "company_context_markdown": company_context_markdown,
        "hsmii_home": hsmii_home,
        "context_notes": t.context_notes.0.clone(),
        "capability_refs": t.capability_refs.0.clone(),
        "workspace_attachment_paths": paths,
        "company_context_addon_bytes": company_context_addon_bytes,
        "vision_alignment_addon": vision_alignment_addon,
        "vision_alignment_addon_bytes": vision_alignment_addon_bytes,
        "shared_memory_addon_bytes": shared_memory_addon_bytes,
        "agent_memory_addon_bytes": agent_memory_addon_bytes,
        "task_context_addon_bytes": task_context_addon_bytes,
        "agent_run_profile": profile,
        "combined_system_addon": combined_system_addon,
        "combined_system_addon_bytes": combined_system_addon_bytes,
        "context_manifest": context_manifest_json,
    })))
}

#[derive(Serialize)]
struct AgentListItem {
    #[serde(flatten)]
    agent: CompanyAgentRow,
    live_in_runs_surfaces: &'static [&'static str],
    profile_only_surfaces: &'static [&'static str],
}
