//! Workforce agents (Paperclip-style): org chart, adapter, budget, briefing.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, patch},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use sqlx::PgPool;
use uuid::Uuid;

use crate::console::ConsoleState;

use super::no_db;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/agents",
            get(list_agents).post(create_agent),
        )
        .route("/api/company/companies/:company_id/org", get(org_chart))
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
struct TaskPersonaRow {
    company_id: Uuid,
    checked_out_by: Option<String>,
    owner_persona: Option<String>,
}

async fn get_task_llm_context(
    State(st): State<ConsoleState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let t = sqlx::query_as::<_, TaskPersonaRow>(
        r#"SELECT company_id, checked_out_by, owner_persona FROM tasks WHERE id = $1"#,
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
    let company_addon = build_company_wide_context_addon(company_context_markdown.as_deref());
    let company_context_addon_bytes = company_addon.len();
    let combined_system_addon = format!("{company_addon}{}", profile.system_context_addon);
    let combined_system_addon_bytes = combined_system_addon.len();
    tracing::info!(
        target: "hsm_company_agent_inject",
        task_id = %task_id,
        company_id = %t.company_id,
        endpoint = "llm_context_get",
        company_agent_row_found = profile.resolved,
        company_context_addon_bytes,
        combined_system_addon_bytes,
        addon_bytes = profile.system_context_addon_bytes,
        matched_as = ?profile.matched_as,
        agent_id = ?profile.agent_id,
    );
    Ok(Json(json!({
        "task_id": task_id,
        "company_context_markdown": company_context_markdown,
        "company_context_addon_bytes": company_context_addon_bytes,
        "agent_run_profile": profile,
        "combined_system_addon": combined_system_addon,
        "combined_system_addon_bytes": combined_system_addon_bytes,
    })))
}

#[derive(Serialize)]
struct AgentListItem {
    #[serde(flatten)]
    agent: CompanyAgentRow,
    live_in_runs_surfaces: &'static [&'static str],
    profile_only_surfaces: &'static [&'static str],
}
