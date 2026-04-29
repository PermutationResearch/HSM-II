//! Workspace-level credentials and skill-bank APIs for the company console.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::Engine as _;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json as SqlxJson;
use std::collections::BTreeMap;
use std::path::{Path as StdPath, PathBuf};
use uuid::Uuid;

use crate::console::ConsoleState;
use crate::skill_markdown::{enumerate_skill_md_under_root, external_skill_dir_roots_from_env};

use super::no_db;

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/credentials",
            get(list_credentials)
                .put(put_credential)
                .delete(delete_credential),
        )
        .route(
            "/api/company/companies/:company_id/skills/bank",
            get(get_skill_bank),
        )
        .route(
            "/api/company/companies/:company_id/skills/bank/entry",
            get(get_skill_bank_entry),
        )
        .route(
            "/api/company/companies/:company_id/skills/import-hermes",
            axum::routing::post(import_hermes_skills),
        )
        .route(
            "/api/company/companies/:company_id/skills/agentskills/export",
            get(export_agentskills_bundle),
        )
        .route(
            "/api/company/companies/:company_id/skills/agentskills/import",
            axum::routing::post(import_agentskills_bundle),
        )
        .route(
            "/api/company/companies/:company_id/skills/agentskills/import-from-fs",
            axum::routing::post(import_agentskills_from_fs),
        )
        .route(
            "/api/company/companies/:company_id/skills/proposals/promote",
            axum::routing::post(promote_skill_proposal_governed),
        )
        .route(
            "/api/company/companies/:company_id/migrations/legacy-agent-data",
            axum::routing::post(import_legacy_agent_data),
        )
        .route(
            "/api/company/companies/:company_id/skills/bootstrap/prune",
            axum::routing::post(prune_bootstrap_skills),
        )
        .route(
            "/api/company/companies/:company_id/browser/providers",
            get(get_browser_providers),
        )
        .route(
            "/api/company/companies/:company_id/profile",
            get(get_company_profile).put(put_company_profile),
        )
        .route(
            "/api/company/companies/:company_id/workflow-packs",
            get(get_workflow_packs),
        )
        .route(
            "/api/company/companies/:company_id/operator-inbox",
            get(get_operator_inbox),
        )
        .route(
            "/api/company/companies/:company_id/connectors",
            get(list_connectors).post(upsert_connector),
        )
        .route(
            "/api/company/companies/:company_id/connectors/:connector_id",
            axum::routing::patch(patch_connector),
        )
        .route("/api/company/connectors/templates", get(list_connector_templates))
        .route(
            "/api/company/connectors/openapi/import",
            axum::routing::post(import_openapi_template),
        )
        .route(
            "/api/company/companies/:company_id/email/operator-queue",
            get(list_email_operator_queue).post(ingest_email_operator_item),
        )
        .route(
            "/api/company/email/operator-queue/:item_id/propose-reply",
            axum::routing::post(propose_email_reply),
        )
        .route(
            "/api/company/email/operator-queue/:item_id/decision",
            axum::routing::post(decide_email_reply),
        )
        .route(
            "/api/company/companies/:company_id/thread-sessions",
            get(list_thread_sessions).put(put_thread_session),
        )
        .route(
            "/api/company/companies/:company_id/thread-sessions/:session_key/join",
            axum::routing::post(post_join_thread_session),
        )
}

fn db_err(context: &str, error: &sqlx::Error) -> (StatusCode, Json<Value>) {
    tracing::error!(target: "hsm.workspace_catalog", %context, ?error, "workspace catalog db error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "Internal database error" })),
    )
}

async fn verify_company(
    pool: &sqlx::PgPool,
    company_id: Uuid,
) -> Result<(), (StatusCode, Json<Value>)> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1)")
        .bind(company_id)
        .fetch_one(pool)
        .await
        .map_err(|e| db_err("verify_company", &e))?;
    if exists {
        Ok(())
    } else {
        Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))))
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct CredentialRow {
    id: Uuid,
    company_id: Uuid,
    provider_key: String,
    label: String,
    env_var: Option<String>,
    masked_preview: String,
    notes: Option<String>,
    status: String,
    metadata: SqlxJson<Value>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct PutCredentialBody {
    provider_key: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    env_var: Option<String>,
    secret_value: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct DeleteCredentialBody {
    provider_key: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct CompanyConnectorRow {
    id: Uuid,
    company_id: Uuid,
    connector_key: String,
    label: String,
    provider_key: String,
    base_url: Option<String>,
    auth_mode: String,
    credential_provider_key: Option<String>,
    policy: SqlxJson<Value>,
    status: String,
    last_success_at: Option<chrono::DateTime<chrono::Utc>>,
    last_failure_at: Option<chrono::DateTime<chrono::Utc>>,
    last_error: Option<String>,
    metadata: SqlxJson<Value>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct UpsertConnectorBody {
    connector_key: String,
    #[serde(default)]
    label: Option<String>,
    provider_key: String,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    auth_mode: Option<String>,
    #[serde(default)]
    credential_provider_key: Option<String>,
    #[serde(default)]
    policy: Option<Value>,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct PatchConnectorBody {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    last_error: Option<String>,
    #[serde(default)]
    policy: Option<Value>,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct OpenApiImportBody {
    provider_key: String,
    connector_key: String,
    spec_url: String,
    #[serde(default)]
    max_operations: Option<usize>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct EmailOperatorQueueRow {
    id: Uuid,
    company_id: Uuid,
    connector_key: Option<String>,
    mailbox: String,
    thread_id: Option<String>,
    message_id: Option<String>,
    from_address: String,
    subject: String,
    body_text: String,
    suggested_reply: Option<String>,
    suggested_by_agent: Option<String>,
    status: String,
    owner_decision: Option<String>,
    decided_by: Option<String>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
    sent_at: Option<chrono::DateTime<chrono::Utc>>,
    metadata: SqlxJson<Value>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct IngestEmailOperatorBody {
    mailbox: String,
    from_address: String,
    subject: String,
    body_text: String,
    #[serde(default)]
    connector_key: Option<String>,
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ProposeEmailReplyBody {
    suggested_reply: String,
    #[serde(default)]
    agent_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DecideEmailReplyBody {
    decision: String,
    actor: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct EmailOperatorQueueQuery {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ImportHermesSkillsBody {
    #[serde(default)]
    include_optional: Option<bool>,
    #[serde(default)]
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct PruneBootstrapSkillsBody {
    #[serde(default)]
    pack: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    mode: Option<String>, // prune|disable
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct AgentSkillsProvenance {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    pack: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct AgentSkillsRecord {
    slug: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    body: String,
    #[serde(default)]
    provenance: Option<AgentSkillsProvenance>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct ImportAgentSkillsBody {
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    overwrite: Option<bool>,
    #[serde(default)]
    skills: Vec<AgentSkillsRecord>,
}

#[derive(Debug, Deserialize, Default)]
struct ImportAgentSkillsFromFsBody {
    #[serde(default)]
    roots: Option<Vec<String>>,
    #[serde(default)]
    include_env_roots: Option<bool>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    overwrite: Option<bool>,
    #[serde(default)]
    source_tag: Option<String>,
    #[serde(default)]
    pack: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct AgentSkillFrontmatter {
    name: Option<String>,
    #[serde(alias = "title")]
    title: Option<String>,
    description: Option<String>,
    #[serde(alias = "summary")]
    summary: Option<String>,
    license: Option<String>,
    compatibility: Option<String>,
    metadata: Option<BTreeMap<String, String>>,
    allowed_tools: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct LegacyMemoryImportRecord {
    title: String,
    body: String,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct LegacyAgentDataImportBody {
    source: String,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    overwrite_skills: Option<bool>,
    #[serde(default)]
    skills: Vec<AgentSkillsRecord>,
    #[serde(default)]
    memories: Vec<LegacyMemoryImportRecord>,
    #[serde(default)]
    command_allowlist: Vec<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct CompanyProfileRow {
    company_id: Uuid,
    industry: String,
    business_model: String,
    channel_mix: SqlxJson<Value>,
    compliance_level: String,
    size_tier: String,
    inferred: bool,
    profile_source: String,
    metadata: SqlxJson<Value>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize, Default)]
struct PutCompanyProfileBody {
    #[serde(default)]
    industry: Option<String>,
    #[serde(default)]
    business_model: Option<String>,
    #[serde(default)]
    channel_mix: Option<Value>,
    #[serde(default)]
    compliance_level: Option<String>,
    #[serde(default)]
    size_tier: Option<String>,
    #[serde(default)]
    inferred: Option<bool>,
    #[serde(default)]
    profile_source: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
    #[serde(default)]
    infer_defaults: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct ConnectorTemplateQuery {
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    company_id: Option<Uuid>,
}

#[derive(Debug, sqlx::FromRow, Serialize)]
struct OperatorInboxTaskRow {
    id: Uuid,
    title: String,
    state: String,
    priority: i32,
    requires_human: bool,
    created_at: String,
}

#[derive(Debug, sqlx::FromRow, Serialize)]
struct OperatorInboxFailureRow {
    id: Uuid,
    failure_class: String,
    confidence: f32,
    created_at: String,
}

fn mask_secret(secret: &str) -> String {
    let trimmed = secret.trim();
    let chars = trimmed.chars().count();
    if chars <= 4 {
        return "••••".to_string();
    }
    let tail: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••{tail}")
}

fn require_encryption_key() -> bool {
    std::env::var("HSM_COMPANY_REQUIRE_CREDENTIAL_ENCRYPTION")
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(true)
}

fn credential_key_bytes() -> Option<[u8; 32]> {
    let raw = std::env::var("HSM_COMPANY_CREDENTIALS_KEY").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if let Some(rest) = trimmed.strip_prefix("base64:") {
        base64::engine::general_purpose::STANDARD.decode(rest.trim()).ok()?
    } else if let Ok(v) = base64::engine::general_purpose::STANDARD.decode(trimmed) {
        v
    } else {
        trimmed.as_bytes().to_vec()
    };
    if candidate.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&candidate);
    Some(out)
}

fn encrypt_credential_secret(secret: &str) -> Result<String, String> {
    let Some(key) = credential_key_bytes() else {
        if require_encryption_key() {
            return Err("credential encryption is required but HSM_COMPANY_CREDENTIALS_KEY is missing/invalid".to_string());
        }
        return Ok(secret.to_string());
    };
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init failed: {e}"))?;
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), secret.as_bytes())
        .map_err(|_| "credential encryption failed".to_string())?;
    let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(nonce);
    let ct_b64 = base64::engine::general_purpose::STANDARD.encode(ct);
    Ok(format!("enc:v1:{nonce_b64}:{ct_b64}"))
}

async fn audit_high_risk_action(
    pool: &sqlx::PgPool,
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

async fn list_credentials(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let rows = sqlx::query_as::<_, CredentialRow>(
        r#"SELECT id, company_id, provider_key, label, env_var, masked_preview, notes, status,
                  metadata, created_at::text, updated_at::text
           FROM company_credentials
           WHERE company_id = $1
           ORDER BY lower(provider_key)"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("list_credentials", &e))?;
    Ok(Json(json!({ "credentials": rows })))
}

async fn put_credential(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<PutCredentialBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let provider_key = body.provider_key.trim().to_ascii_lowercase();
    if provider_key.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "provider_key required" })),
        ));
    }
    let secret_value = body.secret_value.trim();
    if secret_value.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "secret_value required" })),
        ));
    }
    let encrypted_secret = encrypt_credential_secret(secret_value).map_err(|e| {
        (
            StatusCode::FAILED_DEPENDENCY,
            Json(json!({ "error": e })),
        )
    })?;
    let row = sqlx::query_as::<_, CredentialRow>(
        r#"INSERT INTO company_credentials (
               company_id, provider_key, label, env_var, secret_value, masked_preview, notes, status, metadata
           ) VALUES (
               $1, $2, $3, $4, $5, $6, $7, 'connected', $8::jsonb
           )
           ON CONFLICT (company_id, provider_key) DO UPDATE
              SET label = EXCLUDED.label,
                  env_var = EXCLUDED.env_var,
                  secret_value = EXCLUDED.secret_value,
                  masked_preview = EXCLUDED.masked_preview,
                  notes = EXCLUDED.notes,
                  status = 'connected',
                  metadata = EXCLUDED.metadata,
                  updated_at = now()
           RETURNING id, company_id, provider_key, label, env_var, masked_preview, notes, status,
                     metadata, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&provider_key)
    .bind(body.label.as_deref().unwrap_or(&provider_key))
    .bind(body.env_var.as_deref())
    .bind(&encrypted_secret)
    .bind(mask_secret(secret_value))
    .bind(body.notes.as_deref())
    .bind(SqlxJson(body.metadata.unwrap_or_else(|| json!({}))))
    .fetch_one(pool)
    .await
    .map_err(|e| db_err("put_credential", &e))?;
    let _ = sqlx::query(
        r#"INSERT INTO company_connectors
              (company_id, connector_key, label, provider_key, auth_mode, credential_provider_key, policy, metadata, status)
           VALUES
              ($1, $2, $3, $4, 'api_key', $5, '{}'::jsonb, '{}'::jsonb, 'configured')
           ON CONFLICT (company_id, connector_key) DO UPDATE
              SET label = EXCLUDED.label,
                  provider_key = EXCLUDED.provider_key,
                  credential_provider_key = EXCLUDED.credential_provider_key,
                  updated_at = NOW()"#,
    )
    .bind(company_id)
    .bind(&provider_key)
    .bind(body.label.as_deref().unwrap_or(&provider_key))
    .bind(&provider_key)
    .bind(&provider_key)
    .execute(pool)
    .await;
    let actor = headers
        .get("x-hsm-actor")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("workspace_credentials");
    audit_high_risk_action(
        pool,
        company_id,
        actor,
        "credential_upsert",
        "credential",
        &provider_key,
        json!({
            "provider_key": provider_key,
            "status": "connected",
            "encrypted": encrypted_secret.starts_with("enc:v1:"),
        }),
    )
    .await;
    Ok(Json(json!({ "credential": row })))
}

async fn delete_credential(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<DeleteCredentialBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let provider_key = body.provider_key.trim().to_ascii_lowercase();
    if provider_key.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "provider_key required" })),
        ));
    }
    let result = sqlx::query("DELETE FROM company_credentials WHERE company_id = $1 AND provider_key = $2")
        .bind(company_id)
        .bind(&provider_key)
        .execute(pool)
        .await
        .map_err(|e| db_err("delete_credential", &e))?;
    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "credential not found" })),
        ));
    }
    let actor = headers
        .get("x-hsm-actor")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("workspace_credentials");
    audit_high_risk_action(
        pool,
        company_id,
        actor,
        "credential_delete",
        "credential",
        &provider_key,
        json!({ "provider_key": provider_key }),
    )
    .await;
    Ok(Json(json!({ "ok": true, "provider_key": provider_key })))
}

#[derive(Debug, sqlx::FromRow)]
struct AgentSkillUsageRow {
    name: String,
    capabilities: Option<String>,
    adapter_config: SqlxJson<Value>,
}

#[derive(Debug, sqlx::FromRow, Serialize)]
struct CompanySkillBankRow {
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

#[derive(Debug, Serialize, sqlx::FromRow)]
struct SharedThreadSessionRow {
    id: Uuid,
    company_id: Uuid,
    session_key: String,
    title: String,
    participants: SqlxJson<Value>,
    state: SqlxJson<Value>,
    is_active: bool,
    created_by: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct PutThreadSessionBody {
    session_key: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    participants: Option<Value>,
    #[serde(default)]
    state: Option<Value>,
    #[serde(default)]
    is_active: Option<bool>,
    #[serde(default)]
    created_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JoinThreadSessionBody {
    participant: String,
}

fn normalize_skill_ref(raw: &str) -> String {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return String::new();
    }
    if let Some(rest) = value.strip_prefix("skills/") {
        return rest.trim_end_matches("/skill.md").trim_matches('/').to_string();
    }
    value.trim_end_matches("/skill.md").trim_matches('/').to_string()
}

fn capability_csv_refs(capabilities: &Option<String>) -> Vec<String> {
    capabilities
        .as_ref()
        .map(|s| {
            s.split(',')
                .map(normalize_skill_ref)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn adapter_skill_refs(cfg: &Value) -> Vec<String> {
    cfg.get("paperclip")
        .and_then(|x| x.get("skills"))
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(normalize_skill_ref)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

#[derive(Debug, Default, Deserialize)]
struct SkillBankQuery {
    #[serde(default)]
    include_body: Option<bool>,
    #[serde(default)]
    max_body_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SkillBankEntryQuery {
    slug: String,
}

async fn get_skill_bank(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<SkillBankQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;

    let current_skills = sqlx::query_as::<_, CompanySkillBankRow>(
        r#"SELECT id, company_id, slug, name, description, body, skill_path, source, updated_at::text
           FROM company_skills
           WHERE company_id = $1
             AND source NOT LIKE 'hermes_bootstrap_disabled:%'
           ORDER BY lower(name), lower(slug)"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("get_skill_bank_current", &e))?;

    let agents = sqlx::query_as::<_, AgentSkillUsageRow>(
        r#"SELECT name, capabilities, adapter_config
           FROM company_agents
           WHERE company_id = $1 AND status <> 'terminated'"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("get_skill_bank_agents", &e))?;

    let mut refs_to_agents = std::collections::BTreeMap::<String, Vec<String>>::new();
    for agent in &agents {
        let refs = capability_csv_refs(&agent.capabilities)
            .into_iter()
            .chain(adapter_skill_refs(&agent.adapter_config.0).into_iter());
        for rf in refs {
            refs_to_agents.entry(rf).or_default().push(agent.name.clone());
        }
    }

    let include_body = q.include_body.unwrap_or(false);
    let max_body_bytes = q
        .max_body_bytes
        .filter(|v| *v > 0)
        .unwrap_or(16_000)
        .min(200_000);
    let mut emitted_body_bytes = 0usize;

    let current_skill_values = current_skills
        .iter()
        .map(|skill| {
            let key = normalize_skill_ref(&skill.slug);
            let linked_agents = refs_to_agents.get(&key).cloned().unwrap_or_default();
            let category = skill_category_from_slug(&skill.slug);
            let body_value = if include_body && emitted_body_bytes < max_body_bytes {
                let remain = max_body_bytes.saturating_sub(emitted_body_bytes);
                let body = if skill.body.len() > remain {
                    skill.body.chars().take(remain).collect::<String>()
                } else {
                    skill.body.clone()
                };
                emitted_body_bytes += body.len();
                Value::String(body)
            } else {
                Value::Null
            };
            json!({
                "id": skill.id,
                "company_id": skill.company_id,
                "slug": skill.slug,
                "category": category,
                "name": skill.name,
                "description": skill.description,
                "body": body_value,
                "skill_path": skill.skill_path,
                "source": skill.source,
                "updated_at": skill.updated_at,
                "linked_agents": linked_agents,
                "linked_agent_count": linked_agents.len(),
            })
        })
        .collect::<Vec<_>>();

    let mut current_categories = std::collections::BTreeMap::<String, usize>::new();
    for skill in &current_skills {
        *current_categories
            .entry(skill_category_from_slug(&skill.slug))
            .or_insert(0) += 1;
    }

    let current_slugs = current_skills
        .iter()
        .map(|skill| normalize_skill_ref(&skill.slug))
        .collect::<Vec<_>>();

    let recommended_rows: Vec<(String, String, String, i64, Vec<String>)> = sqlx::query_as(
        r#"SELECT s.slug,
                  MIN(s.name) AS name,
                  MIN(s.description) AS description,
                  COUNT(DISTINCT s.company_id)::bigint AS company_count,
                  ARRAY_REMOVE(ARRAY_AGG(DISTINCT c.display_name), NULL) AS company_names
           FROM company_skills s
           JOIN companies c ON c.id = s.company_id
           WHERE s.company_id <> $1
             AND s.source NOT LIKE 'hermes_bootstrap_disabled:%'
             AND NOT (lower(s.slug) = ANY($2))
           GROUP BY s.slug
           ORDER BY company_count DESC, lower(MIN(s.name)), lower(s.slug)
           LIMIT 120"#,
    )
    .bind(company_id)
    .bind(&current_slugs)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("get_skill_bank_recommended", &e))?;

    let recommended = recommended_rows
        .into_iter()
        .map(|(slug, name, description, company_count, company_names)| {
            json!({
                "slug": slug,
                "category": skill_category_from_slug(&slug),
                "name": name,
                "description": description,
                "company_count": company_count,
                "company_names": company_names,
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "current_skills": current_skill_values,
        "current_categories": current_categories,
        "include_body": include_body,
        "body_budget_bytes": max_body_bytes,
        "body_emitted_bytes": emitted_body_bytes,
        "recommended_skills": recommended,
        "active_agent_count": agents.len(),
        "connected_skill_refs": refs_to_agents,
    })))
}

async fn get_skill_bank_entry(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<SkillBankEntryQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let slug = normalize_skill_ref(&q.slug);
    if slug.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "slug required" }))));
    }
    let row = sqlx::query_as::<_, CompanySkillBankRow>(
        r#"SELECT id, company_id, slug, name, description, body, skill_path, source, updated_at::text
           FROM company_skills
           WHERE company_id = $1 AND lower(slug) = $2
           LIMIT 1"#,
    )
    .bind(company_id)
    .bind(slug.to_ascii_lowercase())
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("get_skill_bank_entry", &e))?;
    let Some(skill) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "skill not found" }))));
    };
    Ok(Json(json!({
        "skill": {
            "id": skill.id,
            "company_id": skill.company_id,
            "slug": skill.slug,
            "category": skill_category_from_slug(&skill.slug),
            "name": skill.name,
            "description": skill.description,
            "body": skill.body,
            "skill_path": skill.skill_path,
            "source": skill.source,
            "updated_at": skill.updated_at,
        }
    })))
}

fn repo_root_guess() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[derive(Debug, Deserialize)]
struct PromoteSkillProposalBody {
    slug: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    proposal_markdown: Option<String>,
    #[serde(default)]
    proposal_path: Option<String>,
    #[serde(default)]
    from_task_id: Option<Uuid>,
    #[serde(default)]
    actor: Option<String>,
    #[serde(default)]
    min_runs: Option<i64>,
}

fn clamp_slug(raw: &str) -> String {
    normalize_skill_ref(raw)
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '/')
        .take(120)
        .collect::<String>()
}

async fn promote_skill_proposal_governed(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<PromoteSkillProposalBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let slug = clamp_slug(&body.slug);
    if slug.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "slug required" }))));
    }
    let actor = body
        .actor
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("operator");
    let min_runs = body.min_runs.unwrap_or(3).clamp(1, 50);

    let proposal_markdown = if let Some(md) = body
        .proposal_markdown
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        md.to_string()
    } else if let Some(path) = body
        .proposal_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        std::fs::read_to_string(path).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "proposal_path unreadable" })),
            )
        })?
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "proposal_markdown or proposal_path required" })),
        ));
    };

    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| slug.split('/').next_back().unwrap_or("skill").replace('-', " "));
    let description = body
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Promoted from governed proposal".to_string());

    let mut gate_reasons = Vec::<String>::new();
    let mut verification_ok = false;
    let mut policy_ok = true;
    if let Some(task_id) = body.from_task_id {
        let snap = sqlx::query_as::<_, (String, String, i32)>(
            r#"SELECT run_status, COALESCE(log_tail,''), tool_calls
               FROM task_run_snapshots
               WHERE task_id = $1"#,
        )
        .bind(task_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| db_err("promote_skill_proposal_snap", &e))?;
        if let Some((status, log_tail, tool_calls)) = snap {
            verification_ok = status == "success"
                && tool_calls > 0
                && !log_tail.contains("verification bundle missing")
                && !log_tail.contains("changed-file summary missing")
                && !log_tail.contains("retrieval bundle missing");
            if !verification_ok {
                gate_reasons.push("from_task_id snapshot lacks verification evidence".to_string());
            }
        } else {
            gate_reasons.push("from_task_id has no task_run_snapshot".to_string());
        }
        let policy_violations: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*)::bigint
               FROM governance_events
               WHERE company_id = $1
                 AND action = 'worker_run_event'
                 AND subject_id = $2
                 AND COALESCE(payload->>'event','') IN ('failed','preflight_blocked')"#,
        )
        .bind(company_id)
        .bind(task_id.to_string())
        .fetch_one(pool)
        .await
        .map_err(|e| db_err("promote_skill_proposal_policy", &e))?;
        policy_ok = policy_violations == 0;
        if !policy_ok {
            gate_reasons.push("policy violations found on referenced task".to_string());
        }
    } else {
        gate_reasons.push("from_task_id not provided for verification/policy gates".to_string());
    }

    let baseline = sqlx::query_scalar::<_, f64>(
        r#"SELECT COALESCE(
               AVG(CASE WHEN s.run_status = 'success' THEN 1.0 ELSE 0.0 END), 0.0)
           FROM task_run_snapshots s
           JOIN tasks t ON t.id = s.task_id
           WHERE t.company_id = $1
             AND s.updated_at >= NOW() - INTERVAL '30 days'"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| db_err("promote_skill_proposal_baseline", &e))?;
    let (skill_runs, skill_success): (i64, f64) = sqlx::query_as(
        r#"SELECT COUNT(*)::bigint,
                  COALESCE(AVG(CASE WHEN s.run_status = 'success' THEN 1.0 ELSE 0.0 END), 0.0)
           FROM task_run_snapshots s
           JOIN tasks t ON t.id = s.task_id
           WHERE t.company_id = $1
             AND s.updated_at >= NOW() - INTERVAL '30 days'
             AND t.capability_refs::text ILIKE $2"#,
    )
    .bind(company_id)
    .bind(format!("%{}%", slug))
    .fetch_one(pool)
    .await
    .map_err(|e| db_err("promote_skill_proposal_skill_outcomes", &e))?;
    let outcome_ok = skill_runs >= min_runs && skill_success >= baseline;
    if !outcome_ok {
        gate_reasons.push(format!(
            "outcome gate failed (runs={skill_runs}, min_runs={min_runs}, skill_success={skill_success:.3}, baseline={baseline:.3})"
        ));
    }

    if !(verification_ok && policy_ok && outcome_ok) {
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            Json(json!({
                "error": "promotion gate failed",
                "gate": {
                    "verification_ok": verification_ok,
                    "policy_ok": policy_ok,
                    "outcome_ok": outcome_ok,
                    "baseline_success": baseline,
                    "skill_runs": skill_runs,
                    "skill_success": skill_success,
                    "min_runs": min_runs,
                    "reasons": gate_reasons
                }
            })),
        ));
    }

    let existing = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        r#"SELECT body, skill_path
           FROM company_skills
           WHERE company_id = $1 AND slug = $2
           LIMIT 1"#,
    )
    .bind(company_id)
    .bind(&slug)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("promote_skill_proposal_existing", &e))?;
    let company_home = sqlx::query_scalar::<_, Option<String>>(
        "SELECT hsmii_home FROM companies WHERE id = $1",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| db_err("promote_skill_proposal_company_home", &e))?;
    let mut rollback_path: Option<String> = None;
    if let (Some((Some(old_body), _old_path)), Some(home)) = (existing.clone(), company_home.clone()) {
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let backup_dir = PathBuf::from(home)
            .join("skills")
            .join("_versions")
            .join(slug.replace('/', "__"));
        let backup_path = backup_dir.join(format!("{ts}.md"));
        if std::fs::create_dir_all(&backup_dir).is_ok()
            && std::fs::write(&backup_path, old_body.as_bytes()).is_ok()
        {
            rollback_path = Some(backup_path.display().to_string());
        }
    }

    let skill_path = format!("skills/{}/", slug);
    sqlx::query(
        r#"INSERT INTO company_skills (company_id, slug, name, description, body, skill_path, source)
           VALUES ($1,$2,$3,$4,$5,$6,'promotion_governed')
           ON CONFLICT (company_id, slug) DO UPDATE SET
              name = EXCLUDED.name,
              description = EXCLUDED.description,
              body = EXCLUDED.body,
              skill_path = EXCLUDED.skill_path,
              source = EXCLUDED.source,
              updated_at = NOW()"#,
    )
    .bind(company_id)
    .bind(&slug)
    .bind(&name)
    .bind(&description)
    .bind(&proposal_markdown)
    .bind(&skill_path)
    .execute(pool)
    .await
    .map_err(|e| db_err("promote_skill_proposal_upsert", &e))?;

    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, $2, 'skill_promoted_governed', 'skill', $3, $4, 'info')"#,
    )
    .bind(company_id)
    .bind(actor)
    .bind(slug.clone())
    .bind(SqlxJson(json!({
        "slug": slug,
        "name": name,
        "skill_path": skill_path,
        "source": "promotion_governed",
        "rollback_path": rollback_path,
        "baseline_success": baseline,
        "skill_runs": skill_runs,
        "skill_success": skill_success,
        "min_runs": min_runs,
        "from_task_id": body.from_task_id
    })))
    .execute(pool)
    .await;

    Ok(Json(json!({
        "promoted": true,
        "slug": slug,
        "rollback_path": rollback_path,
        "gate": {
            "verification_ok": verification_ok,
            "policy_ok": policy_ok,
            "outcome_ok": outcome_ok,
            "baseline_success": baseline,
            "skill_runs": skill_runs,
            "skill_success": skill_success,
            "min_runs": min_runs
        }
    })))
}

fn collect_skill_markdowns(root: &StdPath, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_skill_markdowns(&p, out);
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("SKILL.md"))
            .unwrap_or(false)
        {
            out.push(p);
        }
    }
}

fn hermes_skill_paths(include_optional: bool) -> Vec<PathBuf> {
    let root = repo_root_guess();
    let mut files = Vec::new();
    let main = root.join(".claude/skills/hermes-main");
    if main.is_dir() {
        collect_skill_markdowns(&main, &mut files);
    }
    if include_optional {
        let optional = root.join(".claude/skills/hermes-optional");
        if optional.is_dir() {
            collect_skill_markdowns(&optional, &mut files);
        }
    }
    files
}

fn skill_slug_from_path(path: &StdPath) -> String {
    let mut parts = Vec::new();
    for c in path.components() {
        let s = c.as_os_str().to_string_lossy();
        if s == "hermes-main" || s == "hermes-optional" {
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
        .unwrap_or("Imported Hermes skill")
        .to_string();
    (title, description)
}

async fn import_hermes_skills(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<ImportHermesSkillsBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let include_optional = body.include_optional.unwrap_or(true);
    let dry_run = body.dry_run.unwrap_or(false);
    let files = hermes_skill_paths(include_optional);
    if files.is_empty() {
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            Json(json!({"error":"No Hermes skills found under .claude/skills/hermes-main or hermes-optional"})),
        ));
    }
    let mut imported = 0usize;
    let mut attempted = 0usize;
    for path in &files {
        let Ok(raw) = std::fs::read_to_string(path) else {
            continue;
        };
        let slug = skill_slug_from_path(path);
        if slug.is_empty() {
            continue;
        }
        attempted += 1;
        if dry_run {
            continue;
        }
        let (name, description) = title_desc_from_body(&slug, &raw);
        let skill_path = path.to_string_lossy().to_string();
        let result = sqlx::query(
            r#"INSERT INTO company_skills (company_id, slug, name, description, body, skill_path, source)
               VALUES ($1, $2, $3, $4, $5, $6, 'hermes_mirror')
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
        .bind(skill_path)
        .execute(pool)
        .await;
        if result.is_ok() {
            imported += 1;
        }
    }
    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, 'skills_importer', 'import_hermes_skills', 'company', $2, $3, 'info')"#,
    )
    .bind(company_id)
    .bind(company_id.to_string())
    .bind(SqlxJson(json!({
        "include_optional": include_optional,
        "dry_run": dry_run,
        "files_discovered": files.len(),
        "attempted": attempted,
        "imported": imported,
    })))
    .execute(pool)
    .await;
    Ok(Json(json!({
        "ok": true,
        "include_optional": include_optional,
        "dry_run": dry_run,
        "files_discovered": files.len(),
        "attempted": attempted,
        "imported": imported,
    })))
}

fn normalize_memory_scope(v: Option<&str>) -> &'static str {
    match v.unwrap_or("shared").trim().to_ascii_lowercase().as_str() {
        "agent" => "agent",
        _ => "shared",
    }
}

fn normalize_memory_kind_for_import(v: Option<&str>) -> &'static str {
    match v.unwrap_or("note").trim().to_ascii_lowercase().as_str() {
        "fact" => "fact",
        "rule" => "rule",
        "procedure" => "procedure",
        "artifact" => "artifact",
        _ => "note",
    }
}

async fn export_agentskills_bundle(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let rows = sqlx::query_as::<_, CompanySkillBankRow>(
        r#"SELECT id, company_id, slug, name, description, body, skill_path, source, updated_at::text
           FROM company_skills
           WHERE company_id = $1
             AND source NOT LIKE 'hermes_bootstrap_disabled:%'
           ORDER BY lower(name), lower(slug)"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("export_agentskills_bundle", &e))?;
    let skills = rows
        .into_iter()
        .map(|r| {
            let (prov_source, prov_pack) = if let Some(pack) = r.source.strip_prefix("hermes_bootstrap:") {
                ("hermes_bootstrap".to_string(), Some(pack.to_string()))
            } else if let Some(pack) = r.source.strip_prefix("hermes_bootstrap_disabled:") {
                ("hermes_bootstrap_disabled".to_string(), Some(pack.to_string()))
            } else {
                (r.source.clone(), None)
            };
            json!({
                "slug": r.slug,
                "name": r.name,
                "description": r.description,
                "body": r.body,
                "provenance": {
                    "source": prov_source,
                    "pack": prov_pack
                }
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "standard": "agentskills.io",
        "format_version": "2026-04-company-os-v1",
        "company_id": company_id,
        "skills": skills,
    })))
}

async fn import_agentskills_bundle(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<ImportAgentSkillsBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let dry_run = body.dry_run.unwrap_or(false);
    let overwrite = body.overwrite.unwrap_or(true);
    let mut attempted = 0usize;
    let mut imported = 0usize;
    let mut rejected = 0usize;
    for skill in body.skills {
        let slug = skill.slug.trim().to_ascii_lowercase();
        let name = skill.name.trim().to_string();
        let body_md = skill.body.trim().to_string();
        if slug.is_empty() || name.is_empty() || body_md.is_empty() {
            rejected += 1;
            continue;
        }
        attempted += 1;
        if dry_run {
            continue;
        }
        let desc = skill
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Imported AgentSkill")
            .to_string();
        let prov_source = skill
            .provenance
            .as_ref()
            .and_then(|p| p.source.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("agentskills_import")
            .to_string();
        let source = skill
            .provenance
            .as_ref()
            .and_then(|p| p.pack.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|pack| format!("{prov_source}:{pack}"))
            .unwrap_or(prov_source);
        let skill_path = format!("agentskills://{slug}");
        let result = if overwrite {
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
            .bind(&name)
            .bind(desc)
            .bind(body_md)
            .bind(skill_path)
            .bind(source)
            .execute(pool)
            .await
        } else {
            sqlx::query(
                r#"INSERT INTO company_skills (company_id, slug, name, description, body, skill_path, source)
                   VALUES ($1, $2, $3, $4, $5, $6, $7)
                   ON CONFLICT (company_id, slug) DO NOTHING"#,
            )
            .bind(company_id)
            .bind(&slug)
            .bind(&name)
            .bind(desc)
            .bind(body_md)
            .bind(skill_path)
            .bind(source)
            .execute(pool)
            .await
        };
        if result.is_ok() {
            imported += 1;
        }
    }
    Ok(Json(json!({
        "ok": true,
        "standard": "agentskills.io",
        "dry_run": dry_run,
        "overwrite": overwrite,
        "attempted": attempted,
        "imported": imported,
        "rejected": rejected
    })))
}

fn split_skill_frontmatter(raw: &str) -> (Option<String>, String) {
    let s = raw.trim_start();
    if !s.starts_with("---") {
        return (None, raw.to_string());
    }
    let rest = &s[3..];
    let Some(end) = rest.find("\n---") else {
        return (None, raw.to_string());
    };
    let yaml_part = rest[..end].trim().to_string();
    let body = rest[end + 4..].trim_start().to_string();
    (Some(yaml_part), body)
}

fn parse_agentskill_file(raw: &str) -> (AgentSkillFrontmatter, String) {
    let (yaml, body) = split_skill_frontmatter(raw);
    let fm = yaml
        .as_deref()
        .and_then(|y| serde_yaml::from_str::<AgentSkillFrontmatter>(y).ok())
        .unwrap_or_default();
    (fm, body)
}

fn derive_skill_name(slug: &str, fm: &AgentSkillFrontmatter) -> String {
    fm.name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| fm.title.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| slug.split('/').last().unwrap_or("skill").replace('-', " "))
}

fn derive_skill_description(body: &str, fm: &AgentSkillFrontmatter) -> String {
    fm.description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| fm.summary.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            body.lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .unwrap_or("Imported AgentSkill from filesystem")
                .to_string()
        })
}

fn expand_tilde(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(trimmed)
}

fn build_agentskill_body_with_meta(fm: &AgentSkillFrontmatter, body: &str) -> String {
    let meta = json!({
        "license": fm.license,
        "compatibility": fm.compatibility,
        "metadata": fm.metadata,
        "allowed_tools": fm.allowed_tools
    });
    let has_meta = meta
        .as_object()
        .map(|m| {
            m.get("license").and_then(Value::as_str).is_some()
                || m.get("compatibility").and_then(Value::as_str).is_some()
                || m.get("allowed_tools").and_then(Value::as_str).is_some()
                || m.get("metadata")
                    .and_then(Value::as_object)
                    .map(|x| !x.is_empty())
                    .unwrap_or(false)
        })
        .unwrap_or(false);
    if has_meta {
        format!(
            "<!-- hsm-agentskills-meta {} -->\n\n{}",
            meta,
            body.trim()
        )
    } else {
        body.trim().to_string()
    }
}

fn skill_category_from_slug(slug: &str) -> String {
    slug.split('/')
        .next()
        .unwrap_or("uncategorized")
        .trim()
        .to_ascii_lowercase()
}

async fn import_agentskills_from_fs(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<ImportAgentSkillsFromFsBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;

    let dry_run = body.dry_run.unwrap_or(false);
    let overwrite = body.overwrite.unwrap_or(true);
    let include_env_roots = body.include_env_roots.unwrap_or(true);
    let source_tag = body
        .source_tag
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("agentskills_fs")
        .to_ascii_lowercase();
    let source = body
        .pack
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|pack| format!("{source_tag}:{pack}"))
        .unwrap_or_else(|| source_tag.clone());

    let mut roots = Vec::<PathBuf>::new();
    if let Some(explicit) = body.roots.as_ref() {
        for raw in explicit {
            let p = expand_tilde(raw);
            if !p.as_os_str().is_empty() {
                roots.push(p);
            }
        }
    }
    if include_env_roots {
        roots.extend(external_skill_dir_roots_from_env());
    }
    if roots.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"roots required (or set include_env_roots=true with HSM_SKILL_EXTERNAL_DIRS)"})),
        ));
    }

    let mut scanned_roots = Vec::<String>::new();
    let mut missing_roots = Vec::<String>::new();
    let mut discovered_files = 0usize;
    let mut by_slug = std::collections::BTreeMap::<String, PathBuf>::new();
    for root in &roots {
        if !root.is_dir() {
            missing_roots.push(root.to_string_lossy().to_string());
            continue;
        }
        scanned_roots.push(root.to_string_lossy().to_string());
        if let Ok(entries) = enumerate_skill_md_under_root(root) {
            for (slug, path) in entries {
                discovered_files += 1;
                if slug.trim().is_empty() {
                    continue;
                }
                by_slug.entry(slug).or_insert(path);
            }
        }
    }

    let mut attempted = 0usize;
    let mut imported = 0usize;
    let mut rejected = 0usize;
    let mut categories = std::collections::BTreeMap::<String, usize>::new();

    for (slug, path) in &by_slug {
        let raw = match std::fs::read_to_string(path) {
            Ok(v) => v,
            Err(_) => {
                rejected += 1;
                continue;
            }
        };
        let (fm, body_md_raw) = parse_agentskill_file(&raw);
        let body_md = build_agentskill_body_with_meta(&fm, &body_md_raw);
        if body_md.trim().is_empty() {
            rejected += 1;
            continue;
        }
        let name = derive_skill_name(slug, &fm);
        let description = derive_skill_description(&body_md, &fm);
        attempted += 1;
        *categories.entry(skill_category_from_slug(slug)).or_insert(0) += 1;
        if dry_run {
            continue;
        }
        let skill_path = path.to_string_lossy().to_string();
        let result = if overwrite {
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
            .bind(slug)
            .bind(name)
            .bind(description)
            .bind(body_md)
            .bind(skill_path)
            .bind(&source)
            .execute(pool)
            .await
        } else {
            sqlx::query(
                r#"INSERT INTO company_skills (company_id, slug, name, description, body, skill_path, source)
                   VALUES ($1, $2, $3, $4, $5, $6, $7)
                   ON CONFLICT (company_id, slug) DO NOTHING"#,
            )
            .bind(company_id)
            .bind(slug)
            .bind(name)
            .bind(description)
            .bind(body_md)
            .bind(skill_path)
            .bind(&source)
            .execute(pool)
            .await
        };
        if result.is_ok() {
            imported += 1;
        }
    }

    let _ = sqlx::query(
        r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
           VALUES ($1, 'agentskills_fs_importer', 'import_agentskills_from_fs', 'company', $2, $3, 'info')"#,
    )
    .bind(company_id)
    .bind(company_id.to_string())
    .bind(SqlxJson(json!({
        "dry_run": dry_run,
        "overwrite": overwrite,
        "source": source,
        "roots_scanned": scanned_roots,
        "roots_missing": missing_roots,
        "discovered_files": discovered_files,
        "unique_slugs": by_slug.len(),
        "attempted": attempted,
        "imported": imported,
        "rejected": rejected,
        "categories": categories
    })))
    .execute(pool)
    .await;

    Ok(Json(json!({
        "ok": true,
        "standard": "agentskills.io",
        "import_mode": "filesystem",
        "dry_run": dry_run,
        "overwrite": overwrite,
        "source": source,
        "roots_scanned": scanned_roots,
        "roots_missing": missing_roots,
        "discovered_files": discovered_files,
        "unique_slugs": by_slug.len(),
        "attempted": attempted,
        "imported": imported,
        "rejected": rejected,
        "categories": categories
    })))
}

async fn import_legacy_agent_data(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<LegacyAgentDataImportBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let source = body.source.trim().to_ascii_lowercase();
    if source.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"source is required"})),
        ));
    }
    let dry_run = body.dry_run.unwrap_or(true);
    let overwrite = body.overwrite_skills.unwrap_or(true);
    let mut imported_skills = 0usize;
    let mut imported_memories = 0usize;
    if !dry_run {
        for skill in body.skills.iter() {
            let slug = skill.slug.trim().to_ascii_lowercase();
            let name = skill.name.trim().to_string();
            let body_md = skill.body.trim().to_string();
            if slug.is_empty() || name.is_empty() || body_md.is_empty() {
                continue;
            }
            let desc = skill
                .description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Migrated skill")
                .to_string();
            let skill_source = format!("legacy_migration:{source}");
            let skill_path = format!("migration://{source}/{slug}");
            let result = if overwrite {
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
                .bind(&name)
                .bind(desc)
                .bind(body_md)
                .bind(skill_path)
                .bind(skill_source)
                .execute(pool)
                .await
            } else {
                sqlx::query(
                    r#"INSERT INTO company_skills (company_id, slug, name, description, body, skill_path, source)
                       VALUES ($1, $2, $3, $4, $5, $6, $7)
                       ON CONFLICT (company_id, slug) DO NOTHING"#,
                )
                .bind(company_id)
                .bind(&slug)
                .bind(&name)
                .bind(desc)
                .bind(body_md)
                .bind(skill_path)
                .bind(skill_source)
                .execute(pool)
                .await
            };
            if result.is_ok() {
                imported_skills += 1;
            }
        }
        for mem in body.memories.iter() {
            let title = mem.title.trim();
            let body_text = mem.body.trim();
            if title.is_empty() || body_text.is_empty() {
                continue;
            }
            let mem_source = mem
                .source
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| format!("legacy_migration:{source}:{s}"))
                .unwrap_or_else(|| format!("legacy_migration:{source}"));
            let scope = normalize_memory_scope(mem.scope.as_deref());
            let kind = normalize_memory_kind_for_import(mem.kind.as_deref());
            let result = sqlx::query(
                r#"INSERT INTO company_memory_entries
                   (company_id, scope, company_agent_id, title, body, tags, source, summary_l0, summary_l1, kind)
                   VALUES ($1, $2, NULL, $3, $4, $5, $6, NULL, NULL, $7)"#,
            )
            .bind(company_id)
            .bind(scope)
            .bind(title)
            .bind(body_text)
            .bind(mem.tags.as_ref())
            .bind(mem_source)
            .bind(kind)
            .execute(pool)
            .await;
            if result.is_ok() {
                imported_memories += 1;
            }
        }
        let _ = sqlx::query(
            r#"INSERT INTO governance_events (company_id, actor, action, subject_type, subject_id, payload, severity)
               VALUES ($1, 'migration_importer', 'import_legacy_agent_data', 'company', $2, $3, 'info')"#,
        )
        .bind(company_id)
        .bind(company_id.to_string())
        .bind(SqlxJson(json!({
            "source": source,
            "skills": imported_skills,
            "memories": imported_memories,
            "command_allowlist_count": body.command_allowlist.len(),
        })))
        .execute(pool)
        .await;
    }
    Ok(Json(json!({
        "ok": true,
        "dry_run": dry_run,
        "source": source,
        "skills_received": body.skills.len(),
        "memories_received": body.memories.len(),
        "command_allowlist_count": body.command_allowlist.len(),
        "skills_imported": imported_skills,
        "memories_imported": imported_memories
    })))
}

async fn prune_bootstrap_skills(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<PruneBootstrapSkillsBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let dry_run = body.dry_run.unwrap_or(false);
    let mode = body
        .mode
        .as_deref()
        .unwrap_or("prune")
        .trim()
        .to_ascii_lowercase();
    if mode != "prune" && mode != "disable" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"mode must be prune or disable"})),
        ));
    }
    let pack_norm = body
        .pack
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase());
    let pack_source = pack_norm
        .as_ref()
        .map(|p| format!("hermes_bootstrap:{p}"))
        .unwrap_or_default();
    let count: i64 = if pack_norm.is_some() {
        sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM company_skills
               WHERE company_id = $1 AND source = $2"#,
        )
        .bind(company_id)
        .bind(&pack_source)
        .fetch_one(pool)
        .await
        .map_err(|e| db_err("prune_bootstrap_count_pack", &e))?
    } else {
        sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM company_skills
               WHERE company_id = $1
                 AND (source LIKE 'hermes_bootstrap:%'
                      OR source LIKE 'hermes_bootstrap_disabled:%')"#,
        )
        .bind(company_id)
        .fetch_one(pool)
        .await
        .map_err(|e| db_err("prune_bootstrap_count_all", &e))?
    };
    if !dry_run && count > 0 {
        if mode == "disable" {
            if pack_norm.is_some() {
                sqlx::query(
                    r#"UPDATE company_skills
                       SET source = REPLACE(source, 'hermes_bootstrap:', 'hermes_bootstrap_disabled:')
                       WHERE company_id = $1 AND source = $2"#,
                )
                .bind(company_id)
                .bind(&pack_source)
                .execute(pool)
                .await
                .map_err(|e| db_err("prune_bootstrap_disable_pack", &e))?;
            } else {
                sqlx::query(
                    r#"UPDATE company_skills
                       SET source = REPLACE(source, 'hermes_bootstrap:', 'hermes_bootstrap_disabled:')
                       WHERE company_id = $1 AND source LIKE 'hermes_bootstrap:%'"#,
                )
                .bind(company_id)
                .execute(pool)
                .await
                .map_err(|e| db_err("prune_bootstrap_disable_all", &e))?;
            }
        } else if pack_norm.is_some() {
            sqlx::query(
                r#"DELETE FROM company_skills
                   WHERE company_id = $1 AND source = $2"#,
            )
            .bind(company_id)
            .bind(&pack_source)
            .execute(pool)
            .await
            .map_err(|e| db_err("prune_bootstrap_delete_pack", &e))?;
        } else {
            sqlx::query(
                r#"DELETE FROM company_skills
                   WHERE company_id = $1
                     AND (source LIKE 'hermes_bootstrap:%'
                          OR source LIKE 'hermes_bootstrap_disabled:%')"#,
            )
            .bind(company_id)
            .execute(pool)
            .await
            .map_err(|e| db_err("prune_bootstrap_delete_all", &e))?;
        }
    }
    Ok(Json(json!({
        "ok": true,
        "dry_run": dry_run,
        "mode": mode,
        "pack": pack_norm,
        "matched": count,
    })))
}

fn normalize_size_tier(v: &str) -> &'static str {
    match v.trim().to_ascii_lowercase().as_str() {
        "solo" | "small" | "single" => "solo",
        "team" | "smb" | "startup" => "team",
        "org" | "enterprise" | "large" => "org",
        _ => "solo",
    }
}

fn infer_business_model(company_slug: &str, display_name: &str) -> &'static str {
    let hay = format!(
        "{} {}",
        company_slug.to_ascii_lowercase(),
        display_name.to_ascii_lowercase()
    );
    if hay.contains("shop") || hay.contains("store") || hay.contains("commerce") {
        return "commerce";
    }
    if hay.contains("saas") || hay.contains("app") || hay.contains("software") {
        return "saas";
    }
    if hay.contains("media") || hay.contains("creator") || hay.contains("studio") {
        return "content";
    }
    if hay.contains("capital") || hay.contains("bank") || hay.contains("finance") {
        return "fintech";
    }
    "services"
}

fn default_channels_for_model(model: &str) -> Value {
    match model {
        "commerce" => json!(["email", "ads", "shop", "support"]),
        "saas" => json!(["email", "crm", "docs", "community"]),
        "content" => json!(["email", "video", "social", "ads"]),
        "fintech" => json!(["email", "risk", "support"]),
        _ => json!(["email", "support"]),
    }
}

async fn inferred_profile_row(
    pool: &sqlx::PgPool,
    company_id: Uuid,
) -> Result<CompanyProfileRow, (StatusCode, Json<Value>)> {
    let row: (String, String) = sqlx::query_as(
        r#"SELECT slug, display_name FROM companies WHERE id = $1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("inferred_profile_company", &e))?
    .ok_or((StatusCode::NOT_FOUND, Json(json!({"error":"company not found"}))))?;
    let (slug, display_name) = row;
    let model = infer_business_model(&slug, &display_name);
    let agents_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM company_agents WHERE company_id = $1 AND status <> 'terminated'",
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);
    let size_tier = if agents_total <= 4 {
        "solo"
    } else if agents_total <= 30 {
        "team"
    } else {
        "org"
    };
    Ok(CompanyProfileRow {
        company_id,
        industry: "general".to_string(),
        business_model: model.to_string(),
        channel_mix: SqlxJson(default_channels_for_model(model)),
        compliance_level: "standard".to_string(),
        size_tier: size_tier.to_string(),
        inferred: true,
        profile_source: "system_inference".to_string(),
        metadata: SqlxJson(json!({
            "inferred_from": { "slug": slug, "display_name": display_name, "agents_total": agents_total }
        })),
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
}

async fn get_company_profile(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let stored = sqlx::query_as::<_, CompanyProfileRow>(
        r#"SELECT company_id, industry, business_model, channel_mix, compliance_level, size_tier,
                  inferred, profile_source, metadata, created_at::text, updated_at::text
           FROM company_profiles
           WHERE company_id = $1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("get_company_profile", &e))?;
    let profile = match stored {
        Some(p) => p,
        None => inferred_profile_row(pool, company_id).await?,
    };
    Ok(Json(json!({ "profile": profile })))
}

async fn put_company_profile(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<PutCompanyProfileBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let infer_defaults = body.infer_defaults.unwrap_or(false);
    let inferred = inferred_profile_row(pool, company_id).await?;
    let row = sqlx::query_as::<_, CompanyProfileRow>(
        r#"INSERT INTO company_profiles
              (company_id, industry, business_model, channel_mix, compliance_level, size_tier, inferred, profile_source, metadata)
           VALUES
              ($1,$2,$3,$4::jsonb,$5,$6,$7,$8,$9::jsonb)
           ON CONFLICT (company_id) DO UPDATE
              SET industry = EXCLUDED.industry,
                  business_model = EXCLUDED.business_model,
                  channel_mix = EXCLUDED.channel_mix,
                  compliance_level = EXCLUDED.compliance_level,
                  size_tier = EXCLUDED.size_tier,
                  inferred = EXCLUDED.inferred,
                  profile_source = EXCLUDED.profile_source,
                  metadata = EXCLUDED.metadata,
                  updated_at = NOW()
           RETURNING company_id, industry, business_model, channel_mix, compliance_level, size_tier,
                     inferred, profile_source, metadata, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(
        body.industry
            .as_deref()
            .filter(|_| !infer_defaults)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&inferred.industry),
    )
    .bind(
        body.business_model
            .as_deref()
            .filter(|_| !infer_defaults)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&inferred.business_model),
    )
    .bind(SqlxJson(
        body.channel_mix
            .filter(|_| !infer_defaults)
            .unwrap_or_else(|| inferred.channel_mix.0.clone()),
    ))
    .bind(
        body.compliance_level
            .as_deref()
            .filter(|_| !infer_defaults)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&inferred.compliance_level),
    )
    .bind(normalize_size_tier(
        body.size_tier
            .as_deref()
            .filter(|_| !infer_defaults)
            .unwrap_or(&inferred.size_tier),
    ))
    .bind(body.inferred.unwrap_or(infer_defaults || inferred.inferred))
    .bind(
        body.profile_source
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(if infer_defaults {
                "system_inference"
            } else {
                "operator_override"
            }),
    )
    .bind(SqlxJson(
        body.metadata
            .filter(|_| !infer_defaults)
            .unwrap_or_else(|| inferred.metadata.0.clone()),
    ))
    .fetch_one(pool)
    .await
    .map_err(|e| db_err("put_company_profile", &e))?;
    Ok(Json(json!({ "profile": row })))
}

async fn get_workflow_packs(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let profile = sqlx::query_as::<_, CompanyProfileRow>(
        r#"SELECT company_id, industry, business_model, channel_mix, compliance_level, size_tier,
                  inferred, profile_source, metadata, created_at::text, updated_at::text
           FROM company_profiles
           WHERE company_id = $1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("get_workflow_packs_profile", &e))?
    .unwrap_or(inferred_profile_row(pool, company_id).await?);
    let model = profile
        .business_model
        .as_str();
    let size = profile.size_tier.as_str();
    let automation = match size {
        "org" => "semi_auto",
        "team" => "assisted_auto",
        _ => "approval_first",
    };
    let mut packs = vec![
        json!({"key":"email_ops","label":"Email Ops","default_risk":"medium","automation_limit":automation}),
        json!({"key":"support_triage","label":"Support Triage","default_risk":"low","automation_limit":automation}),
        json!({"key":"finance_ops","label":"Finance Ops","default_risk":"high","automation_limit":"approval_first"}),
    ];
    if model == "commerce" || model == "content" {
        packs.push(json!({"key":"growth_campaigns","label":"Growth Campaigns","default_risk":"medium","automation_limit":automation}));
    }
    if model == "commerce" {
        packs.push(json!({"key":"fulfillment_recovery","label":"Fulfillment Recovery","default_risk":"medium","automation_limit":automation}));
    }
    Ok(Json(json!({
        "company_id": company_id,
        "profile": profile,
        "workflow_packs": packs
    })))
}

async fn get_browser_providers(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;

    let creds: Vec<(String, String)> = sqlx::query_as(
        "SELECT provider_key, masked_preview FROM company_credentials WHERE company_id = $1",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("get_browser_providers", &e))?;
    let by_provider = creds
        .into_iter()
        .collect::<std::collections::HashMap<String, String>>();

    let firecrawl_base = std::env::var("FIRECRAWL_API_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("FIRECRAWL_API_BASE").ok())
        .unwrap_or_else(|| "https://api.firecrawl.dev/v1".to_string());
    let browser_use_base = std::env::var("BROWSER_USE_API_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "https://api.browser-use.com/v1".to_string());

    Ok(Json(json!({
        "providers": [
            {
                "key": "firecrawl",
                "label": "Firecrawl Cloud",
                "kind": "cloud-browser-extract",
                "configured": by_provider.contains_key("firecrawl") || std::env::var("FIRECRAWL_API_KEY").ok().is_some(),
                "credential_preview": by_provider.get("firecrawl").cloned(),
                "api_base": firecrawl_base,
            },
            {
                "key": "browserbase",
                "label": "Browserbase",
                "kind": "cloud-browser-automation",
                "configured": by_provider.contains_key("browserbase") || std::env::var("BROWSERBASE_API_KEY").ok().is_some(),
                "credential_preview": by_provider.get("browserbase").cloned(),
                "api_base": "https://www.browserbase.com/v1",
            },
            {
                "key": "browser_use",
                "label": "Browser Use",
                "kind": "browser-provider",
                "configured": by_provider.contains_key("browser_use") || std::env::var("BROWSER_USE_API_KEY").ok().is_some(),
                "credential_preview": by_provider.get("browser_use").cloned(),
                "api_base": browser_use_base,
            },
            {
                "key": "xai",
                "label": "xAI",
                "kind": "llm-provider",
                "configured": by_provider.contains_key("xai") || std::env::var("XAI_API_KEY").ok().is_some(),
                "credential_preview": by_provider.get("xai").cloned(),
                "api_base": std::env::var("XAI_BASE_URL").ok().unwrap_or_else(|| "https://api.x.ai/v1".to_string()),
                "prompt_cache_enabled": std::env::var("HSM_XAI_PROMPT_CACHE").ok().as_deref() == Some("1"),
                "thinking_prefill_enabled": std::env::var("HSM_XAI_THINKING_PREFILL").ok().map(|s| !s.trim().is_empty()).unwrap_or(false),
            }
        ]
    })))
}

async fn list_connectors(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let rows = sqlx::query_as::<_, CompanyConnectorRow>(
        r#"SELECT id, company_id, connector_key, label, provider_key, base_url, auth_mode,
                  credential_provider_key, policy, status, last_success_at, last_failure_at, last_error,
                  metadata, created_at::text, updated_at::text
           FROM company_connectors
           WHERE company_id = $1
           ORDER BY lower(connector_key)"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("list_connectors", &e))?;
    Ok(Json(json!({ "connectors": rows })))
}

async fn upsert_connector(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<UpsertConnectorBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let connector_key = body.connector_key.trim().to_ascii_lowercase();
    let provider_key = body.provider_key.trim().to_ascii_lowercase();
    if connector_key.is_empty() || provider_key.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"connector_key and provider_key required"})),
        ));
    }
    let auth_mode = body
        .auth_mode
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("api_key");
    let row = sqlx::query_as::<_, CompanyConnectorRow>(
        r#"INSERT INTO company_connectors
              (company_id, connector_key, label, provider_key, base_url, auth_mode, credential_provider_key, policy, metadata, status)
           VALUES
              ($1,$2,$3,$4,$5,$6,$7,$8::jsonb,$9::jsonb,'configured')
           ON CONFLICT (company_id, connector_key) DO UPDATE
              SET label = EXCLUDED.label,
                  provider_key = EXCLUDED.provider_key,
                  base_url = EXCLUDED.base_url,
                  auth_mode = EXCLUDED.auth_mode,
                  credential_provider_key = EXCLUDED.credential_provider_key,
                  policy = EXCLUDED.policy,
                  metadata = EXCLUDED.metadata,
                  updated_at = NOW()
           RETURNING id, company_id, connector_key, label, provider_key, base_url, auth_mode,
                     credential_provider_key, policy, status, last_success_at, last_failure_at, last_error,
                     metadata, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&connector_key)
    .bind(body.label.as_deref().unwrap_or(&connector_key))
    .bind(&provider_key)
    .bind(body.base_url.as_deref())
    .bind(auth_mode)
    .bind(body.credential_provider_key.as_deref().map(|s| s.trim().to_ascii_lowercase()))
    .bind(SqlxJson(body.policy.unwrap_or_else(|| json!({}))))
    .bind(SqlxJson(body.metadata.unwrap_or_else(|| json!({}))))
    .fetch_one(pool)
    .await
    .map_err(|e| db_err("upsert_connector", &e))?;
    Ok((StatusCode::CREATED, Json(json!({ "connector": row }))))
}

async fn patch_connector(
    State(st): State<ConsoleState>,
    Path((company_id, connector_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchConnectorBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let row = sqlx::query_as::<_, CompanyConnectorRow>(
        r#"UPDATE company_connectors
           SET status = COALESCE($3, status),
               last_error = COALESCE($4, last_error),
               policy = COALESCE($5::jsonb, policy),
               metadata = COALESCE($6::jsonb, metadata),
               last_success_at = CASE WHEN $3 = 'healthy' THEN NOW() ELSE last_success_at END,
               last_failure_at = CASE WHEN $3 = 'error' THEN NOW() ELSE last_failure_at END,
               updated_at = NOW()
           WHERE company_id = $1 AND id = $2
           RETURNING id, company_id, connector_key, label, provider_key, base_url, auth_mode,
                     credential_provider_key, policy, status, last_success_at, last_failure_at, last_error,
                     metadata, created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(connector_id)
    .bind(body.status.as_deref())
    .bind(body.last_error.as_deref())
    .bind(body.policy.map(SqlxJson))
    .bind(body.metadata.map(SqlxJson))
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("patch_connector", &e))?;
    let Some(connector) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "connector not found" }))));
    };
    Ok(Json(json!({ "connector": connector })))
}

async fn list_connector_templates(
    State(st): State<ConsoleState>,
    Query(q): Query<ConnectorTemplateQuery>,
) -> Json<Value> {
    let profile = if let (Some(pool), Some(company_id)) = (st.company_db.as_ref(), q.company_id) {
        sqlx::query_as::<_, CompanyProfileRow>(
            r#"SELECT company_id, industry, business_model, channel_mix, compliance_level, size_tier,
                      inferred, profile_source, metadata, created_at::text, updated_at::text
               FROM company_profiles
               WHERE company_id = $1"#,
        )
        .bind(company_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
    } else {
        None
    };
    let size_tier = profile
        .as_ref()
        .map(|p| p.size_tier.as_str())
        .unwrap_or("solo");
    let business_model = profile
        .as_ref()
        .map(|p| p.business_model.as_str())
        .unwrap_or("services");
    let recommend = |category: &str, key: &str| -> &'static str {
        if category == "email" {
            return "must_have";
        }
        if size_tier == "solo" && category == "dev" {
            return "deferred";
        }
        if business_model == "commerce" && (category == "growth" || key.contains("ads")) {
            return "must_have";
        }
        if business_model == "content" && category == "video" {
            return "must_have";
        }
        "optional"
    };
    let mut rows = vec![
        json!({"key":"github","label":"GitHub","category":"dev","provider_key":"github","auth_mode":"bearer", "recommendation": recommend("dev","github")}),
        json!({"key":"slack","label":"Slack","category":"comms","provider_key":"slack","auth_mode":"bearer", "recommendation": recommend("comms","slack")}),
        json!({"key":"notion","label":"Notion","category":"knowledge","provider_key":"notion","auth_mode":"bearer", "recommendation": recommend("knowledge","notion")}),
        json!({"key":"stripe","label":"Stripe","category":"finance","provider_key":"stripe","auth_mode":"bearer", "recommendation": recommend("finance","stripe")}),
        json!({"key":"gmail_business","label":"Gmail Business","category":"email","provider_key":"gmail","auth_mode":"oauth2", "recommendation": recommend("email","gmail_business")}),
        json!({"key":"microsoft_365_mail","label":"Microsoft 365 Mail","category":"email","provider_key":"microsoft_graph","auth_mode":"oauth2", "recommendation": recommend("email","microsoft_365_mail")}),
        json!({"key":"imap_smtp","label":"IMAP/SMTP","category":"email","provider_key":"imap_smtp","auth_mode":"password", "recommendation": recommend("email","imap_smtp")}),
        json!({"key":"meta_ads","label":"Meta Ads","category":"growth","provider_key":"meta_ads","auth_mode":"bearer", "recommendation": recommend("growth","meta_ads")}),
        json!({"key":"google_ads","label":"Google Ads","category":"growth","provider_key":"google_ads","auth_mode":"oauth2", "recommendation": recommend("growth","google_ads")}),
        json!({"key":"youtube","label":"YouTube","category":"video","provider_key":"youtube","auth_mode":"oauth2", "recommendation": recommend("video","youtube")}),
        json!({"key":"tiktok_ads","label":"TikTok Ads","category":"growth","provider_key":"tiktok_ads","auth_mode":"oauth2", "recommendation": recommend("growth","tiktok_ads")}),
        json!({"key":"vimeo","label":"Vimeo","category":"video","provider_key":"vimeo","auth_mode":"bearer", "recommendation": recommend("video","vimeo")}),
        json!({"key":"openapi","label":"Generic OpenAPI","category":"generic","provider_key":"openapi","auth_mode":"api_key", "recommendation": recommend("generic","openapi")}),
    ];
    if let Some(cat) = q.category.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        rows.retain(|r| {
            r.get("category")
                .and_then(|v| v.as_str())
                .map(|v| v.eq_ignore_ascii_case(cat))
                .unwrap_or(false)
        });
    }
    Json(json!({
        "templates": rows,
        "profile_context": profile.map(|p| json!({
            "company_id": p.company_id,
            "business_model": p.business_model,
            "size_tier": p.size_tier
        }))
    }))
}

async fn import_openapi_template(
    Json(body): Json<OpenApiImportBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let provider_key = body.provider_key.trim().to_ascii_lowercase();
    let connector_key = body.connector_key.trim().to_ascii_lowercase();
    let spec_url = body.spec_url.trim();
    if provider_key.is_empty() || connector_key.is_empty() || spec_url.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"provider_key, connector_key and spec_url are required"})),
        ));
    }
    let max_ops = body.max_operations.unwrap_or(24).clamp(1, 200);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"http client init failed"}))))?;
    let resp = client
        .get(spec_url)
        .send()
        .await
        .map_err(|_| (StatusCode::BAD_GATEWAY, Json(json!({"error":"failed to fetch OpenAPI spec"}))))?;
    if !resp.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("OpenAPI fetch failed: {}", resp.status())})),
        ));
    }
    let val: Value = resp
        .json()
        .await
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(json!({"error":"invalid OpenAPI JSON"}))))?;
    let mut generated = Vec::new();
    if let Some(paths) = val.get("paths").and_then(|v| v.as_object()) {
        for (p, item) in paths {
            let Some(obj) = item.as_object() else { continue };
            for method in ["get", "post", "put", "patch", "delete"] {
                if generated.len() >= max_ops {
                    break;
                }
                if let Some(op) = obj.get(method).and_then(|v| v.as_object()) {
                    let op_id = op
                        .get("operationId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("operation");
                    generated.push(json!({
                        "name": format!("{}_{}", method, op_id).to_ascii_lowercase().replace('-', "_"),
                        "method": method.to_uppercase(),
                        "path": p,
                        "description": op.get("summary").and_then(|v| v.as_str()).unwrap_or("Imported from OpenAPI"),
                    }));
                }
            }
        }
    }
    Ok(Json(json!({
        "template": {
            "provider_key": provider_key,
            "connector_key": connector_key,
            "spec_url": spec_url,
            "imported_operations": generated.len(),
            "operations": generated
        }
    })))
}

async fn list_email_operator_queue(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(q): Query<EmailOperatorQueueQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let status = q
        .status
        .as_deref()
        .unwrap_or("pending_approval")
        .trim()
        .to_ascii_lowercase();
    let rows = sqlx::query_as::<_, EmailOperatorQueueRow>(
        r#"SELECT id, company_id, connector_key, mailbox, thread_id, message_id, from_address, subject, body_text,
                  suggested_reply, suggested_by_agent, status, owner_decision, decided_by, decided_at, sent_at, metadata,
                  created_at::text, updated_at::text
           FROM company_email_operator_queue
           WHERE company_id = $1
             AND ($2 = 'all' OR status = $2)
           ORDER BY created_at DESC
           LIMIT 200"#,
    )
    .bind(company_id)
    .bind(status)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("list_email_operator_queue", &e))?;
    Ok(Json(json!({ "items": rows })))
}

async fn ingest_email_operator_item(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<IngestEmailOperatorBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    if body.mailbox.trim().is_empty()
        || body.from_address.trim().is_empty()
        || body.subject.trim().is_empty()
        || body.body_text.trim().is_empty()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"mailbox, from_address, subject, and body_text are required"})),
        ));
    }
    let row = sqlx::query_as::<_, EmailOperatorQueueRow>(
        r#"INSERT INTO company_email_operator_queue
              (company_id, connector_key, mailbox, thread_id, message_id, from_address, subject, body_text, metadata, status)
           VALUES
              ($1,$2,$3,$4,$5,$6,$7,$8,$9::jsonb,'pending_draft')
           RETURNING id, company_id, connector_key, mailbox, thread_id, message_id, from_address, subject, body_text,
                     suggested_reply, suggested_by_agent, status, owner_decision, decided_by, decided_at, sent_at, metadata,
                     created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(body.connector_key.as_deref().map(|s| s.trim().to_ascii_lowercase()))
    .bind(body.mailbox.trim())
    .bind(body.thread_id.as_deref())
    .bind(body.message_id.as_deref())
    .bind(body.from_address.trim())
    .bind(body.subject.trim())
    .bind(body.body_text.trim())
    .bind(SqlxJson(body.metadata.unwrap_or_else(|| json!({}))))
    .fetch_one(pool)
    .await
    .map_err(|e| db_err("ingest_email_operator_item", &e))?;
    Ok((StatusCode::CREATED, Json(json!({ "item": row }))))
}

async fn propose_email_reply(
    State(st): State<ConsoleState>,
    Path(item_id): Path<Uuid>,
    Json(body): Json<ProposeEmailReplyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    if body.suggested_reply.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error":"suggested_reply required"}))));
    }
    let row = sqlx::query_as::<_, EmailOperatorQueueRow>(
        r#"UPDATE company_email_operator_queue
           SET suggested_reply = $2,
               suggested_by_agent = $3,
               status = 'pending_approval',
               updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, connector_key, mailbox, thread_id, message_id, from_address, subject, body_text,
                     suggested_reply, suggested_by_agent, status, owner_decision, decided_by, decided_at, sent_at, metadata,
                     created_at::text, updated_at::text"#,
    )
    .bind(item_id)
    .bind(body.suggested_reply.trim())
    .bind(
        body.agent_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("company_agent"),
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("propose_email_reply", &e))?;
    let Some(item) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error":"email queue item not found"}))));
    };
    Ok(Json(json!({ "item": item })))
}

async fn decide_email_reply(
    State(st): State<ConsoleState>,
    Path(item_id): Path<Uuid>,
    Json(body): Json<DecideEmailReplyBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let decision = body.decision.trim().to_ascii_lowercase();
    let next_status = match decision.as_str() {
        "approve" | "approved" | "send" => "sent",
        "reject" | "rejected" => "rejected",
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"decision must be approve|reject"})),
            ));
        }
    };
    let row = sqlx::query_as::<_, EmailOperatorQueueRow>(
        r#"UPDATE company_email_operator_queue
           SET owner_decision = $2,
               decided_by = $3,
               decided_at = NOW(),
               sent_at = CASE WHEN $4 = 'sent' THEN NOW() ELSE sent_at END,
               status = $4,
               metadata = metadata || jsonb_build_object('decision_reason', COALESCE($5, '')),
               updated_at = NOW()
           WHERE id = $1
           RETURNING id, company_id, connector_key, mailbox, thread_id, message_id, from_address, subject, body_text,
                     suggested_reply, suggested_by_agent, status, owner_decision, decided_by, decided_at, sent_at, metadata,
                     created_at::text, updated_at::text"#,
    )
    .bind(item_id)
    .bind(decision)
    .bind(body.actor.trim())
    .bind(next_status)
    .bind(body.reason.as_deref())
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("decide_email_reply", &e))?;
    let Some(item) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error":"email queue item not found"}))));
    };
    Ok(Json(json!({ "item": item })))
}

async fn get_operator_inbox(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let profile = sqlx::query_as::<_, CompanyProfileRow>(
        r#"SELECT company_id, industry, business_model, channel_mix, compliance_level, size_tier,
                  inferred, profile_source, metadata, created_at::text, updated_at::text
           FROM company_profiles
           WHERE company_id = $1"#,
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| db_err("get_operator_inbox_profile", &e))?
    .unwrap_or(inferred_profile_row(pool, company_id).await?);

    let tasks = sqlx::query_as::<_, OperatorInboxTaskRow>(
        r#"SELECT id, title, state, priority, requires_human, created_at::text
           FROM tasks
           WHERE company_id = $1
             AND state NOT IN ('done','closed','cancelled')
             AND (requires_human OR state IN ('waiting_admin','blocked'))
           ORDER BY priority DESC, created_at DESC
           LIMIT 120"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("get_operator_inbox_tasks", &e))?;

    let emails = sqlx::query_as::<_, EmailOperatorQueueRow>(
        r#"SELECT id, company_id, connector_key, mailbox, thread_id, message_id, from_address, subject, body_text,
                  suggested_reply, suggested_by_agent, status, owner_decision, decided_by, decided_at, sent_at, metadata,
                  created_at::text, updated_at::text
           FROM company_email_operator_queue
           WHERE company_id = $1
             AND status IN ('pending_draft','pending_approval')
           ORDER BY created_at DESC
           LIMIT 120"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("get_operator_inbox_emails", &e))?;

    let failures = sqlx::query_as::<_, OperatorInboxFailureRow>(
        r#"SELECT id, failure_class, confidence, created_at::text
           FROM run_failure_events
           WHERE company_id = $1
             AND created_at >= NOW() - INTERVAL '14 days'
           ORDER BY created_at DESC
           LIMIT 120"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("get_operator_inbox_failures", &e))?;

    let lanes = match profile.size_tier.as_str() {
        "org" => vec![
            json!({"id":"executive","label":"Executive decisions","item_kinds":["task","email"],"sla":"<4h"}),
            json!({"id":"risk","label":"Risk + reliability","item_kinds":["failure"],"sla":"<24h"}),
        ],
        "team" => vec![
            json!({"id":"ops","label":"Ops approvals","item_kinds":["task"],"sla":"today"}),
            json!({"id":"support","label":"Customer responses","item_kinds":["email"],"sla":"today"}),
            json!({"id":"reliability","label":"Reliability follow-ups","item_kinds":["failure"],"sla":"this week"}),
        ],
        _ => vec![json!({"id":"today","label":"Today","item_kinds":["task","email","failure"],"sla":"today"})],
    };

    let mut merged = Vec::new();
    for t in &tasks {
        merged.push(json!({
            "kind":"task",
            "id": t.id,
            "title": t.title,
            "state": t.state,
            "priority": t.priority,
            "requires_human": t.requires_human,
            "created_at": t.created_at
        }));
    }
    for e in &emails {
        merged.push(json!({
            "kind":"email",
            "id": e.id,
            "title": e.subject,
            "state": e.status,
            "priority": if e.suggested_reply.is_some() { 8 } else { 5 },
            "mailbox": e.mailbox,
            "from_address": e.from_address,
            "created_at": e.created_at
        }));
    }
    for f in &failures {
        merged.push(json!({
            "kind":"failure",
            "id": f.id,
            "title": f.failure_class,
            "state":"needs_triage",
            "priority": (f.confidence * 10.0).round() as i32,
            "confidence": f.confidence,
            "created_at": f.created_at
        }));
    }
    merged.sort_by(|a, b| {
        let pa = a.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
        let pb = b.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
        pb.cmp(&pa)
    });

    Ok(Json(json!({
        "company_id": company_id,
        "profile": profile,
        "lanes": lanes,
        "counts": {
            "tasks": tasks.len(),
            "emails": emails.len(),
            "failures": failures.len(),
            "total": merged.len(),
        },
        "items": merged
    })))
}

async fn list_thread_sessions(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let rows = sqlx::query_as::<_, SharedThreadSessionRow>(
        r#"SELECT id, company_id, session_key, title, participants, state, is_active, created_by,
                  created_at::text, updated_at::text
           FROM shared_thread_sessions
           WHERE company_id = $1
           ORDER BY updated_at DESC"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| db_err("list_thread_sessions", &e))?;
    Ok(Json(json!({ "sessions": rows })))
}

async fn put_thread_session(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<PutThreadSessionBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let session_key = body.session_key.trim().to_ascii_lowercase();
    if session_key.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "session_key required" }))));
    }
    let participants = body.participants.unwrap_or_else(|| json!([]));
    let state = body.state.unwrap_or_else(|| json!({}));
    let row = sqlx::query_as::<_, SharedThreadSessionRow>(
        r#"INSERT INTO shared_thread_sessions
              (company_id, session_key, title, participants, state, is_active, created_by)
           VALUES
              ($1, $2, $3, $4::jsonb, $5::jsonb, $6, $7)
           ON CONFLICT (company_id, session_key) DO UPDATE
              SET title = EXCLUDED.title,
                  participants = EXCLUDED.participants,
                  state = EXCLUDED.state,
                  is_active = EXCLUDED.is_active,
                  updated_at = now()
           RETURNING id, company_id, session_key, title, participants, state, is_active, created_by,
                     created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(&session_key)
    .bind(body.title.as_deref().unwrap_or(&session_key))
    .bind(SqlxJson(participants))
    .bind(SqlxJson(state))
    .bind(body.is_active.unwrap_or(true))
    .bind(body.created_by.as_deref())
    .fetch_one(pool)
    .await
    .map_err(|e| db_err("put_thread_session", &e))?;
    Ok(Json(json!({ "session": row })))
}

async fn post_join_thread_session(
    State(st): State<ConsoleState>,
    Path((company_id, session_key)): Path<(Uuid, String)>,
    Json(body): Json<JoinThreadSessionBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let participant = body.participant.trim();
    if participant.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "participant required" }))));
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| db_err("join_thread_session_begin", &e))?;
    let row = sqlx::query_as::<_, SharedThreadSessionRow>(
        r#"SELECT id, company_id, session_key, title, participants, state, is_active, created_by,
                  created_at::text, updated_at::text
           FROM shared_thread_sessions
           WHERE company_id = $1 AND session_key = $2
           FOR UPDATE"#,
    )
    .bind(company_id)
    .bind(session_key.trim().to_ascii_lowercase())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| db_err("join_thread_session_select", &e))?;
    let Some(mut row) = row else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "session not found" }))));
    };
    let mut participants = row
        .participants
        .0
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    if !participants.iter().any(|p| p.eq_ignore_ascii_case(participant)) {
        participants.push(participant.to_string());
    }
    row = sqlx::query_as::<_, SharedThreadSessionRow>(
        r#"UPDATE shared_thread_sessions
              SET participants = $3::jsonb,
                  updated_at = now()
           WHERE company_id = $1 AND session_key = $2
           RETURNING id, company_id, session_key, title, participants, state, is_active, created_by,
                     created_at::text, updated_at::text"#,
    )
    .bind(company_id)
    .bind(session_key.trim().to_ascii_lowercase())
    .bind(SqlxJson(json!(participants)))
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| db_err("join_thread_session_update", &e))?;
    tx.commit()
        .await
        .map_err(|e| db_err("join_thread_session_commit", &e))?;
    Ok(Json(json!({ "session": row })))
}
