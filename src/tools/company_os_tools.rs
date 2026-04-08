//! Company OS HTTP helpers for agent runtimes (human inbox / escalation, memory pool).

use reqwest::Client;
use serde_json::{json, Value};

use super::{Tool, ToolOutput};

const TOOL_REQUIRES_HUMAN: &str = "company_task_requires_human";
const TOOL_MEMORY_SEARCH: &str = "company_memory_search";
const TOOL_MEMORY_APPEND: &str = "company_memory_append";
const TOOL_RUN_FEEDBACK: &str = "company_run_feedback_append";
const TOOL_PROMOTE_FEEDBACK: &str = "company_promote_feedback_to_task";

fn company_api_base() -> String {
    std::env::var("HSM_COMPANY_API_BASE")
        .or_else(|_| std::env::var("HSM_API_URL"))
        .or_else(|_| std::env::var("HSM_HYPERGRAPH_URL"))
        .unwrap_or_else(|_| "http://127.0.0.1:3847".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn resolve_actor(params: &Value) -> String {
    params
        .get("actor")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("OUROBOROS_ACTOR_ID").ok())
        .or_else(|| std::env::var("HSM_COMPANY_TASK_ACTOR").ok())
        .unwrap_or_else(|| "agent".to_string())
}

fn resolve_task_id(params: &Value) -> Option<String> {
    let from_param = params
        .get("task_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    from_param.or_else(|| std::env::var("HSM_COMPANY_TASK_ID").ok())
}

fn resolve_company_id_param(params: &Value) -> Option<String> {
    params
        .get("company_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("HSM_COMPANY_ID").ok())
}

fn resolve_company_agent_id_param(params: &Value) -> Option<String> {
    params
        .get("company_agent_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("HSM_COMPANY_AGENT_ID").ok())
}

fn apply_company_bearer(mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    if let Ok(token) = std::env::var("HSM_COMPANY_API_BEARER") {
        let t = token.trim();
        if !t.is_empty() {
            req = req.bearer_auth(t);
        }
    }
    req
}

/// Resolve `company_id` and optional workforce `agent_id` via `GET …/llm-context`.
async fn resolve_from_llm_context(
    client: &Client,
    base: &str,
    task_id: &str,
) -> Result<(String, Option<String>), String> {
    let url = format!("{base}/api/company/tasks/{task_id}/llm-context");
    let mut req = client.get(&url);
    req = apply_company_bearer(req);
    let resp = req
        .send()
        .await
        .map_err(|e| format!("llm-context request failed: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "llm-context HTTP {}: {}",
            status.as_u16(),
            text.chars().take(400).collect::<String>()
        ));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("llm-context JSON: {e}"))?;
    let company_id = v
        .get("company_id")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .ok_or_else(|| "llm-context missing company_id".to_string())?;
    let agent_id = v
        .get("agent_run_profile")
        .and_then(|p| p.get("agent_id"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    Ok((company_id, agent_id))
}

async fn memory_list_get(
    client: &Client,
    base: &str,
    company_id: &str,
    scope: &str,
    company_agent_id: Option<&str>,
    q: Option<&str>,
) -> Result<Value, String> {
    let url = format!("{base}/api/company/companies/{company_id}/memory");
    let mut rb = apply_company_bearer(client.get(&url)).query(&[("scope", scope)]);
    if let Some(a) = company_agent_id {
        rb = rb.query(&[("company_agent_id", a)]);
    }
    if let Some(n) = q.map(str::trim).filter(|s| !s.is_empty()) {
        rb = rb.query(&[("q", n)]);
    }
    let resp = rb
        .send()
        .await
        .map_err(|e| format!("company memory list failed: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "memory list HTTP {}: {}",
            status.as_u16(),
            text.chars().take(500).collect::<String>()
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("memory list JSON: {e}"))
}

pub struct CompanyTaskRequiresHumanTool {
    client: Client,
}

impl CompanyTaskRequiresHumanTool {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }
}

impl Default for CompanyTaskRequiresHumanTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for CompanyTaskRequiresHumanTool {
    fn name(&self) -> &str {
        TOOL_REQUIRES_HUMAN
    }

    fn description(&self) -> &str {
        "Set or clear Company OS `requires_human` on a task (Paperclip-style human inbox). \
         POSTs to the company API. Use when the agent is blocked and needs a person, or to clear the flag after resolution. \
         Base URL: HSM_COMPANY_API_BASE, else HSM_API_URL / HSM_HYPERGRAPH_URL, default http://127.0.0.1:3847. \
         If `task_id` is omitted, uses env HSM_COMPANY_TASK_ID."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Task UUID. If empty, uses HSM_COMPANY_TASK_ID."
                },
                "requires_human": {
                    "type": "boolean",
                    "description": "true = add to human inbox; false = clear flag (default true)."
                },
                "reason": {
                    "type": "string",
                    "description": "Short reason (logged on task when non-empty)."
                },
                "actor": {
                    "type": "string",
                    "description": "Agent or operator id for audit (else OUROBOROS_ACTOR_ID / HSM_COMPANY_TASK_ACTOR / 'agent')."
                }
            }
        })
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let Some(task_id) = resolve_task_id(&params) else {
            return ToolOutput::error(
                "task_id required (parameter or HSM_COMPANY_TASK_ID) for company_task_requires_human",
            );
        };

        let requires_human = params
            .get("requires_human")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let reason = params
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        let actor = resolve_actor(&params);
        let base = company_api_base();
        let url = format!("{base}/api/company/tasks/{task_id}/requires-human");

        let body = json!({
            "requires_human": requires_human,
            "actor": actor,
            "reason": reason,
        });

        let req = apply_company_bearer(
            self.client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body),
        );

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if status.is_success() {
                    ToolOutput::success(format!(
                        "requires_human={requires_human} OK ({})",
                        status.as_u16()
                    ))
                    .with_metadata(json!({ "url": url, "body": text }))
                } else {
                    ToolOutput::error(format!(
                        "company_task_requires_human failed HTTP {}: {}",
                        status.as_u16(),
                        text.chars().take(500).collect::<String>()
                    ))
                }
            }
            Err(e) => ToolOutput::error(format!("company_task_requires_human request failed: {e}")),
        }
    }
}

fn company_memory_http_client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .expect("reqwest client")
}

/// Query company shared and/or per-agent memory via the Company OS REST API (server enforces scopes).
pub struct CompanyMemorySearchTool {
    client: Client,
}

impl CompanyMemorySearchTool {
    pub fn new() -> Self {
        Self {
            client: company_memory_http_client(),
        }
    }
}

impl Default for CompanyMemorySearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for CompanyMemorySearchTool {
    fn name(&self) -> &str {
        TOOL_MEMORY_SEARCH
    }

    fn description(&self) -> &str {
        "Search company memory pool (Postgres-backed). \
         `mode` = shared (company-wide), mine (this workforce agent only), or both. \
         Default mode is shared when omitted. Use `both` when you need company policy plus this agent's private scoped rows in one pass. \
         Uses GET /api/company/companies/{company_id}/memory. \
         Provide `company_id` or `HSM_COMPANY_ID`, or `task_id` / `HSM_COMPANY_TASK_ID` to resolve company + agent from GET …/llm-context. \
         For mine/both, `company_agent_id` or `HSM_COMPANY_AGENT_ID` or a task-bound llm-context with resolved agent is required."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "company_id": { "type": "string", "description": "Company UUID, or use task_id / HSM_COMPANY_ID." },
                "task_id": { "type": "string", "description": "Task UUID to resolve company_id (+ agent) via llm-context." },
                "company_agent_id": { "type": "string", "description": "Workforce agent UUID for mode mine/both." },
                "mode": { "type": "string", "description": "shared | mine | both (default shared)." },
                "q": { "type": "string", "description": "Optional substring filter on title/body/summary." }
            }
        })
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let base = company_api_base();
        let task_id = resolve_task_id(&params);
        let mut company_id = resolve_company_id_param(&params);
        let mut company_agent_id = resolve_company_agent_id_param(&params);
        if company_id.is_none() {
            let Some(ref tid) = task_id else {
                return ToolOutput::error(
                    "company_memory_search: set company_id or HSM_COMPANY_ID, or task_id / HSM_COMPANY_TASK_ID",
                );
            };
            match resolve_from_llm_context(&self.client, &base, tid).await {
                Ok((cid, aid)) => {
                    company_id = Some(cid);
                    if company_agent_id.is_none() {
                        company_agent_id = aid;
                    }
                }
                Err(e) => return ToolOutput::error(e),
            }
        } else if company_agent_id.is_none() {
            if let Some(ref tid) = task_id {
                if let Ok((_, aid)) = resolve_from_llm_context(&self.client, &base, tid).await {
                    company_agent_id = aid;
                }
            }
        }

        let Some(company_id) = company_id else {
            return ToolOutput::error("company_memory_search: missing company_id");
        };

        let mode = params
            .get("mode")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "shared".to_string());

        let q = params
            .get("q")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let needle = q.map(|s| s.to_string());

        let result = match mode.as_str() {
            "shared" => {
                memory_list_get(
                    &self.client,
                    &base,
                    &company_id,
                    "shared",
                    None,
                    needle.as_deref(),
                )
                .await
            }
            "mine" | "agent" => {
                let Some(ref aid) = company_agent_id else {
                    return ToolOutput::error(
                        "company_memory_search: mode mine requires company_agent_id, HSM_COMPANY_AGENT_ID, or task_id with resolved agent",
                    );
                };
                memory_list_get(
                    &self.client,
                    &base,
                    &company_id,
                    "agent",
                    Some(aid.as_str()),
                    needle.as_deref(),
                )
                .await
            }
            "both" => {
                let Some(ref aid) = company_agent_id else {
                    return ToolOutput::error(
                        "company_memory_search: mode both requires company_agent_id or task_id with resolved agent",
                    );
                };
                let shared = memory_list_get(
                    &self.client,
                    &base,
                    &company_id,
                    "shared",
                    None,
                    needle.as_deref(),
                )
                .await;
                let mine = memory_list_get(
                    &self.client,
                    &base,
                    &company_id,
                    "agent",
                    Some(aid.as_str()),
                    needle.as_deref(),
                )
                .await;
                match (shared, mine) {
                    (Ok(s), Ok(m)) => Ok(json!({ "shared": s, "mine": m })),
                    (Err(e), _) | (_, Err(e)) => Err(e),
                }
            }
            _ => {
                return ToolOutput::error(
                    "company_memory_search: mode must be shared, mine, or both",
                );
            }
        };

        match result {
            Ok(v) => ToolOutput::success(
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".into()),
            )
            .with_metadata(json!({ "company_id": company_id, "mode": mode })),
            Err(e) => ToolOutput::error(e),
        }
    }
}

/// Append a row to the company memory pool (`scope` enforced by API).
pub struct CompanyMemoryAppendTool {
    client: Client,
}

impl CompanyMemoryAppendTool {
    pub fn new() -> Self {
        Self {
            client: company_memory_http_client(),
        }
    }
}

impl Default for CompanyMemoryAppendTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for CompanyMemoryAppendTool {
    fn name(&self) -> &str {
        TOOL_MEMORY_APPEND
    }

    fn description(&self) -> &str {
        "Append a company memory entry (POST /api/company/companies/{company_id}/memory). \
         You MUST pass `scope` every call: the string `shared` or `agent` (the API has no default—this is intentional). \
         Prefer `shared` for durable facts any teammate on this company would need: policies, canonical URLs, decisions, incident timelines, definitions, handoff facts. \
         Use `agent` only for private preference, scratch notes, or information that must not appear in the company-wide pool. \
         For high-signal announcements everyone should see early in context, use `scope` shared with `kind` broadcast. \
         Resolve `company_id` / `company_agent_id` like company_memory_search (task_id + llm-context when omitted)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "company_id": { "type": "string" },
                "task_id": { "type": "string" },
                "company_agent_id": { "type": "string", "description": "Required when scope=agent." },
                "title": { "type": "string" },
                "body": { "type": "string" },
                "scope": {
                    "type": "string",
                    "description": "Required each call: exactly `shared` or `agent`. Prefer `shared` for company-durable facts another agent would need; use `agent` only for private or explicitly per-agent-only content."
                },
                "tags": { "type": "array", "items": { "type": "string" } },
                "source": { "type": "string", "description": "Optional provenance (default agent)." },
                "kind": { "type": "string", "description": "shared only: general or broadcast (default general)." }
            },
            "required": ["title", "scope"]
        })
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let base = company_api_base();
        let task_id = resolve_task_id(&params);
        let mut company_id = resolve_company_id_param(&params);
        let mut company_agent_id = resolve_company_agent_id_param(&params);
        if company_id.is_none() {
            let Some(ref tid) = task_id else {
                return ToolOutput::error(
                    "company_memory_append: set company_id or HSM_COMPANY_ID, or task_id / HSM_COMPANY_TASK_ID",
                );
            };
            match resolve_from_llm_context(&self.client, &base, tid).await {
                Ok((cid, aid)) => {
                    company_id = Some(cid);
                    if company_agent_id.is_none() {
                        company_agent_id = aid;
                    }
                }
                Err(e) => return ToolOutput::error(e),
            }
        } else if company_agent_id.is_none() {
            if let Some(ref tid) = task_id {
                if let Ok((_, aid)) = resolve_from_llm_context(&self.client, &base, tid).await {
                    company_agent_id = aid;
                }
            }
        }

        let Some(company_id) = company_id else {
            return ToolOutput::error("company_memory_append: missing company_id");
        };

        let title = match params
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(t) => t.to_string(),
            None => return ToolOutput::error("company_memory_append: title required"),
        };
        let scope = match params
            .get("scope")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
        {
            Some(s) if s == "shared" || s == "agent" => s,
            _ => return ToolOutput::error("company_memory_append: scope must be shared or agent"),
        };

        if scope == "agent" && company_agent_id.is_none() {
            return ToolOutput::error(
                "company_memory_append: scope agent requires company_agent_id or task with resolved agent",
            );
        }

        let body_text = params
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        let tags: Vec<String> = params
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let source = params
            .get("source")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "agent".to_string());

        let kind = params
            .get("kind")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| s == "general" || s == "broadcast")
            .unwrap_or_else(|| "general".to_string());
        if scope == "agent" && kind == "broadcast" {
            return ToolOutput::error(
                "company_memory_append: broadcast kind is only valid for scope=shared",
            );
        }

        let post_body = if scope == "agent" {
            let aid = company_agent_id.as_deref().expect("validated");
            json!({
                "title": title,
                "body": body_text,
                "scope": scope,
                "company_agent_id": aid,
                "tags": tags,
                "source": source,
            })
        } else {
            json!({
                "title": title,
                "body": body_text,
                "scope": scope,
                "tags": tags,
                "source": source,
                "kind": kind,
            })
        };

        let url = format!("{base}/api/company/companies/{company_id}/memory");
        let req = apply_company_bearer(
            self.client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&post_body),
        );

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if status.is_success() {
                    ToolOutput::success(format!("memory entry created ({})", status.as_u16()))
                        .with_metadata(json!({ "url": url, "response": text }))
                } else {
                    ToolOutput::error(format!(
                        "company_memory_append HTTP {}: {}",
                        status.as_u16(),
                        text.chars().take(600).collect::<String>()
                    ))
                }
            }
            Err(e) => ToolOutput::error(format!("company_memory_append request failed: {e}")),
        }
    }
}

/// Append human feedback on an agent run (`POST …/agent-runs/{run_id}/feedback`).
pub struct CompanyAgentRunFeedbackTool {
    client: Client,
}

impl CompanyAgentRunFeedbackTool {
    pub fn new() -> Self {
        Self {
            client: company_memory_http_client(),
        }
    }
}

impl Default for CompanyAgentRunFeedbackTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for CompanyAgentRunFeedbackTool {
    fn name(&self) -> &str {
        TOOL_RUN_FEEDBACK
    }

    fn description(&self) -> &str {
        "Append feedback to an agent execution run (Nexus-style timeline). \
         POST /api/company/companies/{company_id}/agent-runs/{run_id}/feedback. \
         Provide `company_id` or `HSM_COMPANY_ID`, or `task_id` / `HSM_COMPANY_TASK_ID` to resolve company from GET …/llm-context. \
         `run_id` is required."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "company_id": { "type": "string" },
                "task_id": { "type": "string", "description": "Resolves company_id via llm-context when company_id omitted." },
                "run_id": { "type": "string", "description": "Agent run UUID." },
                "body": { "type": "string", "description": "Feedback text." },
                "actor": { "type": "string", "description": "Operator or agent id (else OUROBOROS_ACTOR_ID / HSM_COMPANY_TASK_ACTOR / agent)." },
                "kind": { "type": "string", "description": "comment | correction | blocker | praise (default comment)." },
                "step_index": { "type": "integer" },
                "step_external_id": { "type": "string" }
            },
            "required": ["run_id", "body"]
        })
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let base = company_api_base();
        let mut company_id = resolve_company_id_param(&params);
        let task_id = resolve_task_id(&params);
        if company_id.is_none() {
            let Some(ref tid) = task_id else {
                return ToolOutput::error(
                    "company_run_feedback_append: set company_id or HSM_COMPANY_ID, or task_id / HSM_COMPANY_TASK_ID",
                );
            };
            match resolve_from_llm_context(&self.client, &base, tid).await {
                Ok((cid, _)) => company_id = Some(cid),
                Err(e) => return ToolOutput::error(e),
            }
        }

        let Some(company_id) = company_id else {
            return ToolOutput::error("company_run_feedback_append: missing company_id");
        };

        let run_id = match params
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => s.to_string(),
            None => return ToolOutput::error("company_run_feedback_append: run_id required"),
        };

        let body_text = match params
            .get("body")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => s.to_string(),
            None => return ToolOutput::error("company_run_feedback_append: body required"),
        };

        let actor = resolve_actor(&params);
        let kind = params
            .get("kind")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let step_index = params.get("step_index").and_then(|v| v.as_i64()).map(|n| n as i32);
        let step_external_id = params
            .get("step_external_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let mut post_body = json!({
            "actor": actor,
            "body": body_text,
        });
        if let Some(k) = kind {
            post_body["kind"] = json!(k);
        }
        if let Some(si) = step_index {
            post_body["step_index"] = json!(si);
        }
        if let Some(se) = step_external_id {
            post_body["step_external_id"] = json!(se);
        }

        let url = format!(
            "{base}/api/company/companies/{company_id}/agent-runs/{run_id}/feedback"
        );
        let req = apply_company_bearer(
            self.client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&post_body),
        );

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if status.is_success() {
                    ToolOutput::success(format!("feedback recorded ({})", status.as_u16()))
                        .with_metadata(json!({ "url": url, "response": text }))
                } else {
                    ToolOutput::error(format!(
                        "company_run_feedback_append HTTP {}: {}",
                        status.as_u16(),
                        text.chars().take(600).collect::<String>()
                    ))
                }
            }
            Err(e) => ToolOutput::error(format!("company_run_feedback_append request failed: {e}")),
        }
    }
}

/// Create a task from a run feedback event (`POST …/promote-task`).
pub struct CompanyPromoteFeedbackToTaskTool {
    client: Client,
}

impl CompanyPromoteFeedbackToTaskTool {
    pub fn new() -> Self {
        Self {
            client: company_memory_http_client(),
        }
    }
}

impl Default for CompanyPromoteFeedbackToTaskTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for CompanyPromoteFeedbackToTaskTool {
    fn name(&self) -> &str {
        TOOL_PROMOTE_FEEDBACK
    }

    fn description(&self) -> &str {
        "Promote run feedback into a new Company OS task (sets spawned_task_id on the event). \
         POST /api/company/companies/{company_id}/agent-runs/{run_id}/feedback/{event_id}/promote-task. \
         Resolve `company_id` like company_run_feedback_append."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "company_id": { "type": "string" },
                "task_id": { "type": "string" },
                "run_id": { "type": "string" },
                "event_id": { "type": "string", "description": "Feedback event UUID." },
                "title": { "type": "string" },
                "specification": { "type": "string" },
                "owner_persona": { "type": "string" },
                "priority": { "type": "integer" },
                "workspace_attachment_paths": { "type": "array", "items": { "type": "string" } },
                "capability_refs": { "type": "array" }
            },
            "required": ["run_id", "event_id", "title"]
        })
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let base = company_api_base();
        let mut company_id = resolve_company_id_param(&params);
        let task_id = resolve_task_id(&params);
        if company_id.is_none() {
            let Some(ref tid) = task_id else {
                return ToolOutput::error(
                    "company_promote_feedback_to_task: set company_id or HSM_COMPANY_ID, or task_id / HSM_COMPANY_TASK_ID",
                );
            };
            match resolve_from_llm_context(&self.client, &base, tid).await {
                Ok((cid, _)) => company_id = Some(cid),
                Err(e) => return ToolOutput::error(e),
            }
        }

        let Some(company_id) = company_id else {
            return ToolOutput::error("company_promote_feedback_to_task: missing company_id");
        };

        let run_id = match params
            .get("run_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => s.to_string(),
            None => return ToolOutput::error("company_promote_feedback_to_task: run_id required"),
        };

        let event_id = match params
            .get("event_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => s.to_string(),
            None => return ToolOutput::error("company_promote_feedback_to_task: event_id required"),
        };

        let title = match params
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(s) => s.to_string(),
            None => return ToolOutput::error("company_promote_feedback_to_task: title required"),
        };

        let mut post_body = json!({ "title": title });
        if let Some(s) = params.get("specification").and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                post_body["specification"] = json!(s);
            }
        }
        if let Some(s) = params.get("owner_persona").and_then(|v| v.as_str()) {
            if !s.trim().is_empty() {
                post_body["owner_persona"] = json!(s);
            }
        }
        if let Some(p) = params.get("priority").and_then(|v| v.as_i64()) {
            post_body["priority"] = json!(p as i32);
        }
        if let Some(a) = params.get("workspace_attachment_paths").and_then(|v| v.as_array()) {
            post_body["workspace_attachment_paths"] = json!(a);
        }
        if let Some(a) = params.get("capability_refs").and_then(|v| v.as_array()) {
            post_body["capability_refs"] = json!(a);
        }

        let url = format!(
            "{base}/api/company/companies/{company_id}/agent-runs/{run_id}/feedback/{event_id}/promote-task"
        );
        let req = apply_company_bearer(
            self.client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&post_body),
        );

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                if status.is_success() {
                    ToolOutput::success(format!("task created ({})", status.as_u16()))
                        .with_metadata(json!({ "url": url, "response": text }))
                } else {
                    ToolOutput::error(format!(
                        "company_promote_feedback_to_task HTTP {}: {}",
                        status.as_u16(),
                        text.chars().take(600).collect::<String>()
                    ))
                }
            }
            Err(e) => ToolOutput::error(format!("company_promote_feedback_to_task request failed: {e}")),
        }
    }
}
