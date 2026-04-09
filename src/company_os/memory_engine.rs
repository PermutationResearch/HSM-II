//! Multimodal artifact ingest + chunk substrate for Company OS memory.

use std::collections::HashMap;
use std::path::{Path as StdPath, PathBuf};
use std::sync::{Arc, OnceLock};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::types::Json as SqlxJson;
use sqlx::{FromRow, PgPool};
use tokio::fs;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::console::ConsoleState;
use crate::tools::security::validate_outbound_url;

use super::company_memory_hybrid::{
    self as hybrid, HybridMatch, HybridSearchOptions, SupportingChunk,
};
use super::memory_summaries::derive_summary_l0_l1;
use super::no_db;

const DEFAULT_CHUNK_CHARS: usize = 1_600;
const DEFAULT_CHUNK_OVERLAP: usize = 180;
const MAX_INGEST_TEXT_CHARS: usize = 250_000;
const MAX_RETRIES: i32 = 3;

static MEMORY_INGEST_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
static SECRET_RE: OnceLock<Regex> = OnceLock::new();
static PHONEISH_RE: OnceLock<Regex> = OnceLock::new();

pub fn router() -> Router<ConsoleState> {
    Router::new()
        .route(
            "/api/company/companies/:company_id/memory/ingest/web",
            post(post_ingest_web),
        )
        .route(
            "/api/company/companies/:company_id/memory/ingest/file",
            post(post_ingest_file),
        )
        .route(
            "/api/company/companies/:company_id/memory/ingest/audio",
            post(post_ingest_audio),
        )
        .route(
            "/api/company/companies/:company_id/memory/ingest/image",
            post(post_ingest_image),
        )
        .route(
            "/api/company/companies/:company_id/memory/artifacts",
            get(list_memory_artifacts),
        )
        .route(
            "/api/company/companies/:company_id/memory/artifacts/:artifact_id",
            get(get_memory_artifact),
        )
        .route(
            "/api/company/companies/:company_id/memory/artifacts/:artifact_id/retry",
            post(post_retry_artifact),
        )
        .route(
            "/api/company/companies/:company_id/memory/:memory_id/inspect",
            get(get_memory_inspect),
        )
        .route(
            "/api/company/companies/:company_id/memory/retrieval-debug",
            get(get_retrieval_debug),
        )
        .route(
            "/api/company/companies/:company_id/memory/metrics",
            get(get_memory_metrics),
        )
}

fn ingest_semaphore() -> Arc<Semaphore> {
    MEMORY_INGEST_SEMAPHORE
        .get_or_init(|| {
            let permits = std::env::var("HSM_MEMORY_INGEST_CONCURRENCY")
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|n: &usize| *n >= 1 && *n <= 32)
                .unwrap_or(4);
            Arc::new(Semaphore::new(permits))
        })
        .clone()
}

fn max_chunk_chars() -> usize {
    std::env::var("HSM_MEMORY_CHUNK_CHARS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n: &usize| *n >= 256 && *n <= 10_000)
        .unwrap_or(DEFAULT_CHUNK_CHARS)
}

fn chunk_overlap_chars() -> usize {
    std::env::var("HSM_MEMORY_CHUNK_OVERLAP")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n: &usize| *n <= 2_000)
        .unwrap_or(DEFAULT_CHUNK_OVERLAP)
}

fn max_ingest_chars() -> usize {
    std::env::var("HSM_MEMORY_INGEST_MAX_CHARS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n: &usize| *n >= 1_000 && *n <= 2_000_000)
        .unwrap_or(MAX_INGEST_TEXT_CHARS)
}

fn ingest_db_err(context: &str, error: &sqlx::Error) -> (StatusCode, Json<Value>) {
    tracing::error!(target: "hsm.memory_engine", %context, ?error, "memory engine db error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "Internal database error" })),
    )
}

#[derive(Debug, Clone, Serialize, FromRow)]
struct MemoryArtifactRow {
    id: Uuid,
    company_id: Uuid,
    memory_id: Option<Uuid>,
    media_type: String,
    source_type: String,
    source_uri: Option<String>,
    storage_uri: Option<String>,
    title: Option<String>,
    checksum: Option<String>,
    size_bytes: Option<i64>,
    extraction_status: String,
    extraction_provider: Option<String>,
    retry_count: i32,
    last_error: Option<String>,
    document_date: Option<DateTime<Utc>>,
    event_date: Option<DateTime<Utc>>,
    valid_from: Option<DateTime<Utc>>,
    valid_to: Option<DateTime<Utc>>,
    entity_type: Option<String>,
    entity_id: Option<String>,
    contains_pii: bool,
    redacted_text: Option<String>,
    extracted_text: Option<String>,
    metadata: SqlxJson<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
struct MemoryChunkRow {
    id: Uuid,
    artifact_id: Uuid,
    memory_id: Option<Uuid>,
    chunk_index: i32,
    text: String,
    summary_l0: Option<String>,
    summary_l1: Option<String>,
    token_count: i32,
    modality: String,
    page_number: Option<i32>,
    time_start_ms: Option<i32>,
    time_end_ms: Option<i32>,
    entity_type: Option<String>,
    entity_id: Option<String>,
    document_date: Option<DateTime<Utc>>,
    event_date: Option<DateTime<Utc>>,
    valid_from: Option<DateTime<Utc>>,
    valid_to: Option<DateTime<Utc>>,
    source_range: SqlxJson<Value>,
    contains_pii: bool,
    redacted_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
struct MemoryDetailRow {
    id: Uuid,
    company_id: Uuid,
    scope: String,
    company_agent_id: Option<Uuid>,
    title: String,
    body: String,
    tags: Vec<String>,
    source: String,
    summary_l0: Option<String>,
    summary_l1: Option<String>,
    kind: String,
    supersedes_memory_id: Option<Uuid>,
    is_latest: bool,
    version: i32,
    document_date: Option<DateTime<Utc>>,
    event_date: Option<DateTime<Utc>>,
    valid_from: Option<DateTime<Utc>>,
    valid_to: Option<DateTime<Utc>>,
    entity_type: Option<String>,
    entity_id: Option<String>,
    source_type: Option<String>,
    source_uri: Option<String>,
    chunk_id: Option<String>,
    source_range: Option<Value>,
    contains_pii: bool,
    redacted_body: Option<String>,
    primary_artifact_id: Option<Uuid>,
    source_artifact_count: i32,
    chunk_count: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum IngestSourceKind {
    Web,
    File,
    Audio,
    Image,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct IngestArtifactPayload {
    source_kind: IngestSourceKind,
    scope: Option<String>,
    company_agent_id: Option<Uuid>,
    title: Option<String>,
    tags: Vec<String>,
    kind: Option<String>,
    source_uri: Option<String>,
    file_path: Option<String>,
    media_type: String,
    provider: Option<String>,
    extracted_text: Option<String>,
    metadata: Value,
    document_date: Option<DateTime<Utc>>,
    event_date: Option<DateTime<Utc>>,
    valid_from: Option<DateTime<Utc>>,
    valid_to: Option<DateTime<Utc>>,
    entity_type: Option<String>,
    entity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IngestWebBody {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    company_agent_id: Option<Uuid>,
    #[serde(default)]
    document_date: Option<DateTime<Utc>>,
    #[serde(default)]
    event_date: Option<DateTime<Utc>>,
    #[serde(default)]
    valid_from: Option<DateTime<Utc>>,
    #[serde(default)]
    valid_to: Option<DateTime<Utc>>,
    #[serde(default)]
    entity_type: Option<String>,
    #[serde(default)]
    entity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IngestFileBody {
    path: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    company_agent_id: Option<Uuid>,
    #[serde(default)]
    extracted_text: Option<String>,
    #[serde(default)]
    document_date: Option<DateTime<Utc>>,
    #[serde(default)]
    event_date: Option<DateTime<Utc>>,
    #[serde(default)]
    valid_from: Option<DateTime<Utc>>,
    #[serde(default)]
    valid_to: Option<DateTime<Utc>>,
    #[serde(default)]
    entity_type: Option<String>,
    #[serde(default)]
    entity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IngestModalBody {
    #[serde(default)]
    source_uri: Option<String>,
    #[serde(default)]
    title: Option<String>,
    extracted_text: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    company_agent_id: Option<Uuid>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    document_date: Option<DateTime<Utc>>,
    #[serde(default)]
    event_date: Option<DateTime<Utc>>,
    #[serde(default)]
    valid_from: Option<DateTime<Utc>>,
    #[serde(default)]
    valid_to: Option<DateTime<Utc>>,
    #[serde(default)]
    entity_type: Option<String>,
    #[serde(default)]
    entity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtifactListQuery {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RetrievalDebugQuery {
    q: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    company_agent_id: Option<Uuid>,
    #[serde(default)]
    latest_only: Option<bool>,
    #[serde(default)]
    entity_type: Option<String>,
    #[serde(default)]
    entity_id: Option<String>,
    #[serde(default)]
    valid_at: Option<DateTime<Utc>>,
    #[serde(default)]
    document_date_from: Option<DateTime<Utc>>,
    #[serde(default)]
    document_date_to: Option<DateTime<Utc>>,
    #[serde(default)]
    event_date_from: Option<DateTime<Utc>>,
    #[serde(default)]
    event_date_to: Option<DateTime<Utc>>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryContextAddon {
    pub markdown: String,
    pub match_count: usize,
    pub matches: Vec<HybridMatch>,
}

fn normalize_scope(scope: Option<&str>) -> Result<String, &'static str> {
    let scope = scope.unwrap_or("shared").trim().to_ascii_lowercase();
    if scope == "shared" || scope == "agent" {
        Ok(scope)
    } else {
        Err("scope must be shared or agent")
    }
}

fn normalize_memory_kind(kind: Option<&str>) -> Result<String, &'static str> {
    let kind = kind.unwrap_or("general").trim().to_ascii_lowercase();
    if kind == "general" || kind == "broadcast" {
        Ok(kind)
    } else {
        Err("kind must be general or broadcast")
    }
}

fn normalize_whitespace(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn email_re() -> &'static Regex {
    EMAIL_RE.get_or_init(|| Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b").unwrap())
}

fn secret_re() -> &'static Regex {
    SECRET_RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:sk-[a-z0-9]{16,}|ghp_[a-z0-9]{20,}|api[_-]?key\s*[:=]\s*[a-z0-9_\-]{10,})\b")
            .unwrap()
    })
}

fn phoneish_re() -> &'static Regex {
    PHONEISH_RE.get_or_init(|| Regex::new(r"\b(?:\+?\d[\d\-\(\) ]{8,}\d)\b").unwrap())
}

fn redact_sensitive_text(text: &str) -> (bool, Option<String>) {
    let mut redacted = text.to_string();
    let mut changed = false;
    for re in [email_re(), secret_re(), phoneish_re()] {
        if re.is_match(&redacted) {
            changed = true;
            redacted = re.replace_all(&redacted, "[redacted]").to_string();
        }
    }
    if changed {
        (true, Some(redacted))
    } else {
        (false, None)
    }
}

fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<(usize, usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let target_end = (start + chunk_size).min(chars.len());
        let mut end = target_end;
        if target_end < chars.len() {
            for idx in (start + chunk_size / 2..target_end).rev() {
                let ch = chars[idx];
                if ch == '\n' || ch == '.' || ch == '!' || ch == '?' {
                    end = idx + 1;
                    break;
                }
            }
        }
        let chunk = chars[start..end].iter().collect::<String>();
        if !chunk.trim().is_empty() {
            chunks.push((start, end, chunk));
        }
        if end >= chars.len() {
            break;
        }
        start = end.saturating_sub(overlap);
        if start >= end {
            start = end;
        }
    }
    chunks
}

fn estimate_tokens(text: &str) -> i32 {
    let words = text.split_whitespace().count();
    i32::try_from(words.max(1)).unwrap_or(i32::MAX)
}

fn bytes_checksum(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn json_to_pretty_text(raw: &str) -> String {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| raw.to_string())
}

fn csv_to_text(raw: &str, separator: char) -> String {
    let mut lines = raw.lines();
    let Some(header) = lines.next() else {
        return String::new();
    };
    let columns: Vec<&str> = header.split(separator).collect();
    let mut out = format!("Columns: {}\n\n", columns.join(" | "));
    for line in lines.take(80) {
        for (idx, value) in line.split(separator).enumerate() {
            if let Some(col) = columns.get(idx) {
                out.push_str(&format!("{col}: {} | ", value.trim()));
            }
        }
        out.push('\n');
    }
    out
}

fn fallback_binary_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let filtered = text
        .chars()
        .map(|ch| {
            if ch.is_control() && ch != '\n' && ch != '\t' {
                ' '
            } else {
                ch
            }
        })
        .collect::<String>();
    normalize_whitespace(&filtered)
}

async fn verify_company(pool: &PgPool, company_id: Uuid) -> Result<(), (StatusCode, Json<Value>)> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1)")
        .bind(company_id)
        .fetch_one(pool)
        .await
        .map_err(|e| ingest_db_err("verify_company", &e))?;
    if exists {
        Ok(())
    } else {
        Err((StatusCode::NOT_FOUND, Json(json!({ "error": "company not found" }))))
    }
}

async fn verify_company_agent(
    pool: &PgPool,
    company_id: Uuid,
    company_agent_id: Option<Uuid>,
) -> Result<(), (StatusCode, Json<Value>)> {
    let Some(agent_id) = company_agent_id else {
        return Ok(());
    };
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM company_agents WHERE id = $1 AND company_id = $2)",
    )
    .bind(agent_id)
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| ingest_db_err("verify_company_agent", &e))?;
    if exists {
        Ok(())
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company_agent_id not in company" })),
        ))
    }
}

async fn company_home(pool: &PgPool, company_id: Uuid) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT hsmii_home FROM companies WHERE id = $1")
        .bind(company_id)
        .fetch_optional(pool)
        .await
}

fn resolve_company_file_path(company_home: Option<&str>, raw_path: &str) -> Result<PathBuf, &'static str> {
    let raw = raw_path.trim();
    if raw.is_empty() {
        return Err("path required");
    }
    let path = StdPath::new(raw);
    if path.is_absolute() {
        return Err("absolute paths are not allowed");
    }
    let Some(home) = company_home else {
        return Err("relative file ingest requires company hsmii_home");
    };
    let base = std::fs::canonicalize(home).map_err(|_| "company hsmii_home is invalid")?;
    let joined = StdPath::new(home).join(path);
    let resolved = std::fs::canonicalize(&joined).map_err(|_| "file path does not exist")?;
    if !resolved.starts_with(&base) {
        return Err("path escapes company workspace");
    }
    Ok(resolved)
}

async fn create_artifact_job(
    pool: &PgPool,
    company_id: Uuid,
    payload: &IngestArtifactPayload,
) -> Result<MemoryArtifactRow, sqlx::Error> {
    sqlx::query_as::<_, MemoryArtifactRow>(
        r#"INSERT INTO memory_artifacts (
               company_id, media_type, source_type, source_uri, title,
               extraction_provider, document_date, event_date, valid_from, valid_to,
               entity_type, entity_id, metadata
           ) VALUES (
               $1, $2, $3, $4, $5,
               $6, $7, $8, $9, $10,
               $11, $12, $13::jsonb
           )
           RETURNING id, company_id, memory_id, media_type, source_type, source_uri, storage_uri,
                     title, checksum, size_bytes, extraction_status, extraction_provider,
                     retry_count, last_error, document_date, event_date, valid_from, valid_to,
                     entity_type, entity_id, contains_pii, redacted_text, extracted_text,
                     metadata, created_at, updated_at"#,
    )
    .bind(company_id)
    .bind(&payload.media_type)
    .bind(match payload.source_kind {
        IngestSourceKind::Web => "web",
        IngestSourceKind::File => "file",
        IngestSourceKind::Audio => "audio",
        IngestSourceKind::Image => "image",
    })
    .bind(payload.source_uri.as_deref())
    .bind(payload.title.as_deref())
    .bind(payload.provider.as_deref())
    .bind(payload.document_date)
    .bind(payload.event_date)
    .bind(payload.valid_from)
    .bind(payload.valid_to)
    .bind(payload.entity_type.as_deref())
    .bind(payload.entity_id.as_deref())
    .bind(SqlxJson(json!({
        "job_payload": payload,
    })))
    .fetch_one(pool)
    .await
}

async fn enqueue_artifact_processing(pool: PgPool, artifact_id: Uuid) {
    tokio::spawn(async move {
        if let Err(err) = process_artifact_job(&pool, artifact_id).await {
            tracing::error!(target: "hsm.memory_engine", %artifact_id, error = %err, "artifact ingest failed");
        }
    });
}

async fn read_artifact_job_payload(
    pool: &PgPool,
    artifact_id: Uuid,
) -> Result<(MemoryArtifactRow, IngestArtifactPayload), anyhow::Error> {
    let artifact = sqlx::query_as::<_, MemoryArtifactRow>(
        r#"SELECT id, company_id, memory_id, media_type, source_type, source_uri, storage_uri,
                  title, checksum, size_bytes, extraction_status, extraction_provider,
                  retry_count, last_error, document_date, event_date, valid_from, valid_to,
                  entity_type, entity_id, contains_pii, redacted_text, extracted_text,
                  metadata, created_at, updated_at
           FROM memory_artifacts
           WHERE id = $1"#,
    )
    .bind(artifact_id)
    .fetch_one(pool)
    .await?;
    let payload_val = artifact
        .metadata
        .0
        .get("job_payload")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("artifact missing job_payload metadata"))?;
    let payload = serde_json::from_value::<IngestArtifactPayload>(payload_val)?;
    Ok((artifact, payload))
}

async fn set_artifact_status(
    pool: &PgPool,
    artifact_id: Uuid,
    status: &str,
    last_error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE memory_artifacts
           SET extraction_status = $2,
               last_error = $3,
               updated_at = NOW()
           WHERE id = $1"#,
    )
    .bind(artifact_id)
    .bind(status)
    .bind(last_error)
    .execute(pool)
    .await?;
    Ok(())
}

async fn extract_text_from_web(url: &str) -> anyhow::Result<(Vec<u8>, String)> {
    let safe_url = validate_outbound_url(url).map_err(anyhow::Error::msg)?;
    let resp = reqwest::Client::new().get(safe_url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("web ingest fetch failed with {}", resp.status());
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = resp.bytes().await?.to_vec();
    let raw_text = String::from_utf8_lossy(&bytes).to_string();
    let text = if content_type.contains("html") {
        strip_html_tags(&raw_text)
    } else if content_type.contains("json") {
        json_to_pretty_text(&raw_text)
    } else {
        normalize_whitespace(&raw_text)
    };
    Ok((bytes, truncate_chars(&text, max_ingest_chars())))
}

async fn extract_text_from_file(
    company_home: Option<&str>,
    raw_path: &str,
    override_text: Option<&str>,
) -> anyhow::Result<(String, Vec<u8>, String)> {
    if let Some(text) = override_text.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok((raw_path.to_string(), Vec::new(), truncate_chars(text, max_ingest_chars())));
    }
    let resolved = resolve_company_file_path(company_home, raw_path)
        .map_err(anyhow::Error::msg)?;
    let bytes = fs::read(&resolved).await?;
    let extension = resolved
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let raw_lossy = String::from_utf8_lossy(&bytes).to_string();
    let text = match extension.as_str() {
        "md" | "txt" | "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "toml" | "yaml" | "yml" => {
            normalize_whitespace(&raw_lossy)
        }
        "json" => json_to_pretty_text(&raw_lossy),
        "csv" => csv_to_text(&raw_lossy, ','),
        "tsv" => csv_to_text(&raw_lossy, '\t'),
        "html" | "htm" => strip_html_tags(&raw_lossy),
        "pdf" => {
            let fallback = fallback_binary_text(&bytes);
            if fallback.trim().is_empty() {
                anyhow::bail!("pdf ingest needs extracted_text until a parser is configured");
            }
            fallback
        }
        _ => fallback_binary_text(&bytes),
    };
    Ok((
        resolved.to_string_lossy().to_string(),
        bytes,
        truncate_chars(&text, max_ingest_chars()),
    ))
}

fn build_artifact_title(payload: &IngestArtifactPayload, fallback_text: &str) -> String {
    if let Some(title) = payload.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return title.to_string();
    }
    if let Some(source_uri) = payload.source_uri.as_deref() {
        let last = source_uri.rsplit('/').next().unwrap_or(source_uri).trim();
        if !last.is_empty() {
            return truncate_chars(last, 140);
        }
    }
    let first_line = fallback_text.lines().next().unwrap_or("Imported memory").trim();
    if first_line.is_empty() {
        "Imported memory".to_string()
    } else {
        truncate_chars(first_line, 140)
    }
}

async fn persist_memory_from_artifact(
    pool: &PgPool,
    artifact: &MemoryArtifactRow,
    payload: &IngestArtifactPayload,
    extracted_text: &str,
    checksum: Option<&str>,
    size_bytes: Option<i64>,
) -> anyhow::Result<(Uuid, Vec<(Uuid, String)>)> {
    let title = build_artifact_title(payload, extracted_text);
    let scope = normalize_scope(payload.scope.as_deref()).map_err(anyhow::Error::msg)?;
    let kind = normalize_memory_kind(payload.kind.as_deref()).map_err(anyhow::Error::msg)?;
    let cleaned = normalize_whitespace(extracted_text);
    let (contains_pii, redacted) = redact_sensitive_text(&cleaned);
    let retrieval_body = redacted.as_deref().unwrap_or(&cleaned);
    let summary_source = truncate_chars(retrieval_body, 8_000);
    let (summary_l0, summary_l1) = derive_summary_l0_l1(&title, &summary_source);
    let chunks = chunk_text(&summary_source, max_chunk_chars(), chunk_overlap_chars());

    let mut tx = pool.begin().await?;
    let memory_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO company_memory_entries (
               company_id, scope, company_agent_id, title, body, tags, source, summary_l0, summary_l1, kind,
               source_type, source_uri, document_date, event_date, valid_from, valid_to,
               entity_type, entity_id, contains_pii, redacted_body, source_artifact_count, chunk_count
           ) VALUES (
               $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
               $11, $12, $13, $14, $15, $16,
               $17, $18, $19, $20, 1, $21
           )
           RETURNING id"#,
    )
    .bind(artifact.company_id)
    .bind(&scope)
    .bind(payload.company_agent_id)
    .bind(&title)
    .bind(&cleaned)
    .bind(&payload.tags)
    .bind(format!("ingest:{}", artifact.source_type))
    .bind(summary_l0)
    .bind(summary_l1)
    .bind(&kind)
    .bind(&artifact.source_type)
    .bind(artifact.source_uri.as_deref())
    .bind(payload.document_date)
    .bind(payload.event_date)
    .bind(payload.valid_from)
    .bind(payload.valid_to)
    .bind(payload.entity_type.as_deref())
    .bind(payload.entity_id.as_deref())
    .bind(contains_pii)
    .bind(redacted.as_deref())
    .bind(i32::try_from(chunks.len()).unwrap_or(i32::MAX))
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"UPDATE memory_artifacts
           SET memory_id = $2,
               checksum = COALESCE($3, checksum),
               size_bytes = COALESCE($4, size_bytes),
               extracted_text = $5,
               redacted_text = $6,
               contains_pii = $7,
               extraction_status = 'summarized',
               updated_at = NOW()
           WHERE id = $1"#,
    )
    .bind(artifact.id)
    .bind(memory_id)
    .bind(checksum)
    .bind(size_bytes)
    .bind(&cleaned)
    .bind(redacted.as_deref())
    .bind(contains_pii)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"UPDATE company_memory_entries
           SET primary_artifact_id = $2
           WHERE id = $1"#,
    )
    .bind(memory_id)
    .bind(artifact.id)
    .execute(&mut *tx)
    .await?;

    let modality = artifact.media_type.clone();
    let mut embed_queue = Vec::new();
    for (idx, (start_offset, end_offset, text)) in chunks.into_iter().enumerate() {
        let chunk_text = text.trim().to_string();
        if chunk_text.is_empty() {
            continue;
        }
        let (chunk_l0, chunk_l1) = derive_summary_l0_l1(&title, &truncate_chars(&chunk_text, 2_500));
        let chunk_pii = redact_sensitive_text(&chunk_text);
        let chunk_row_id: Uuid = sqlx::query_scalar(
            r#"INSERT INTO memory_chunks (
                   company_id, artifact_id, memory_id, chunk_index, text, summary_l0, summary_l1,
                   token_count, modality, start_offset, end_offset, document_date, event_date,
                   valid_from, valid_to, entity_type, entity_id, source_range, contains_pii, redacted_text
               ) VALUES (
                   $1, $2, $3, $4, $5, $6, $7,
                   $8, $9, $10, $11, $12, $13,
                   $14, $15, $16, $17, $18::jsonb, $19, $20
               )
               RETURNING id"#,
        )
        .bind(artifact.company_id)
        .bind(artifact.id)
        .bind(memory_id)
        .bind(i32::try_from(idx).unwrap_or(i32::MAX))
        .bind(&chunk_text)
        .bind(chunk_l0)
        .bind(chunk_l1)
        .bind(estimate_tokens(&chunk_text))
        .bind(&modality)
        .bind(i32::try_from(start_offset).ok())
        .bind(i32::try_from(end_offset).ok())
        .bind(payload.document_date)
        .bind(payload.event_date)
        .bind(payload.valid_from)
        .bind(payload.valid_to)
        .bind(payload.entity_type.as_deref())
        .bind(payload.entity_id.as_deref())
        .bind(SqlxJson(json!({
            "start_char": start_offset,
            "end_char": end_offset,
        })))
        .bind(chunk_pii.0)
        .bind(chunk_pii.1.as_deref())
        .fetch_one(&mut *tx)
        .await?;
        embed_queue.push((
            chunk_row_id,
            chunk_pii
                .1
                .unwrap_or_else(|| chunk_text.clone())
                .chars()
                .take(6_000)
                .collect(),
        ));
    }

    sqlx::query(
        r#"UPDATE memory_artifacts
           SET extraction_status = 'indexed',
               updated_at = NOW()
           WHERE id = $1"#,
    )
    .bind(artifact.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok((memory_id, embed_queue))
}

async fn process_artifact_job(pool: &PgPool, artifact_id: Uuid) -> anyhow::Result<()> {
    let permit = ingest_semaphore().acquire_owned().await?;
    let (artifact, payload) = read_artifact_job_payload(pool, artifact_id).await?;
    set_artifact_status(pool, artifact_id, "extracting", None).await?;

    let extraction: anyhow::Result<(Option<String>, Option<i64>, String)> = async {
        match payload.source_kind {
            IngestSourceKind::Web => {
                let source_uri = payload
                    .source_uri
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("url required"))?;
                let (bytes, text) = extract_text_from_web(source_uri).await?;
                Ok((Some(bytes_checksum(&bytes)), Some(i64::try_from(bytes.len()).unwrap_or(i64::MAX)), text))
            }
            IngestSourceKind::File => {
                let path = payload
                    .file_path
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("file path required"))?;
                let home = company_home(pool, artifact.company_id).await?;
                let (resolved, bytes, text) =
                    extract_text_from_file(home.as_deref(), path, payload.extracted_text.as_deref()).await?;
                sqlx::query("UPDATE memory_artifacts SET storage_uri = $2 WHERE id = $1")
                    .bind(artifact_id)
                    .bind(&resolved)
                    .execute(pool)
                    .await?;
                Ok((
                    if bytes.is_empty() { None } else { Some(bytes_checksum(&bytes)) },
                    if bytes.is_empty() {
                        None
                    } else {
                        Some(i64::try_from(bytes.len()).unwrap_or(i64::MAX))
                    },
                    text,
                ))
            }
            IngestSourceKind::Audio | IngestSourceKind::Image => {
                let text = payload
                    .extracted_text
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("extracted_text required for this modality"))?;
                Ok((None, Some(i64::try_from(text.len()).unwrap_or(i64::MAX)), truncate_chars(text, max_ingest_chars())))
            }
        }
    }
    .await;

    match extraction {
        Ok((checksum, size_bytes, extracted_text)) => {
            set_artifact_status(pool, artifact_id, "chunked", None).await?;
            let (memory_id, chunk_embeddings) = persist_memory_from_artifact(
                pool,
                &artifact,
                &payload,
                &extracted_text,
                checksum.as_deref(),
                size_bytes,
            )
            .await?;
            let title = build_artifact_title(&payload, &extracted_text);
            let body = extracted_text.chars().take(12_000).collect::<String>();
            let pool_embed = pool.clone();
            tokio::spawn(async move {
                hybrid::embed_row_after_write(pool_embed.clone(), memory_id, title, body).await;
                hybrid::embed_chunks_after_write(pool_embed, chunk_embeddings).await;
            });
            crate::telemetry::client().record_technical(
                "company.memory.ingest.completed",
                json!({
                    "artifact_id": artifact_id,
                    "company_id": artifact.company_id,
                    "source_type": artifact.source_type,
                    "media_type": artifact.media_type,
                    "memory_id": memory_id,
                }),
            );
        }
        Err(error) => {
            let next_retry = artifact.retry_count + 1;
            let status = if next_retry >= MAX_RETRIES {
                "dead_letter"
            } else {
                "retry_waiting"
            };
            sqlx::query(
                r#"UPDATE memory_artifacts
                   SET extraction_status = $2,
                       retry_count = retry_count + 1,
                       last_error = $3,
                       updated_at = NOW()
                   WHERE id = $1"#,
            )
            .bind(artifact_id)
            .bind(status)
            .bind(truncate_chars(&error.to_string(), 600))
            .execute(pool)
            .await?;
            crate::telemetry::client().record_technical(
                "company.memory.ingest.failed",
                json!({
                    "artifact_id": artifact_id,
                    "company_id": artifact.company_id,
                    "source_type": artifact.source_type,
                    "media_type": artifact.media_type,
                    "status": status,
                    "retry_count": next_retry,
                }),
            );
        }
    }
    drop(permit);
    Ok(())
}

async fn create_and_enqueue(
    st: &ConsoleState,
    company_id: Uuid,
    payload: IngestArtifactPayload,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let scope = normalize_scope(payload.scope.as_deref())
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
    if scope == "agent" && payload.company_agent_id.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "company_agent_id required for agent scope" })),
        ));
    }
    verify_company_agent(pool, company_id, payload.company_agent_id).await?;
    let artifact = create_artifact_job(pool, company_id, &payload)
        .await
        .map_err(|e| ingest_db_err("create_artifact_job", &e))?;
    enqueue_artifact_processing(pool.clone(), artifact.id).await;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "artifact": artifact,
            "queued": true,
        })),
    ))
}

async fn post_ingest_web(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<IngestWebBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let url = body.url.trim();
    if url.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "url required" }))));
    }
    create_and_enqueue(
        &st,
        company_id,
        IngestArtifactPayload {
            source_kind: IngestSourceKind::Web,
            scope: body.scope,
            company_agent_id: body.company_agent_id,
            title: body.title,
            tags: body.tags,
            kind: body.kind,
            source_uri: Some(url.to_string()),
            file_path: None,
            media_type: "web".to_string(),
            provider: Some("http_fetch".to_string()),
            extracted_text: None,
            metadata: json!({}),
            document_date: body.document_date,
            event_date: body.event_date,
            valid_from: body.valid_from,
            valid_to: body.valid_to,
            entity_type: body.entity_type,
            entity_id: body.entity_id,
        },
    )
    .await
}

async fn post_ingest_file(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<IngestFileBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let raw_path = body.path.trim();
    if raw_path.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "path required" }))));
    }
    let media_type = StdPath::new(raw_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .unwrap_or_else(|| "text".to_string());
    create_and_enqueue(
        &st,
        company_id,
        IngestArtifactPayload {
            source_kind: IngestSourceKind::File,
            scope: body.scope,
            company_agent_id: body.company_agent_id,
            title: body.title,
            tags: body.tags,
            kind: body.kind,
            source_uri: Some(raw_path.to_string()),
            file_path: Some(raw_path.to_string()),
            media_type,
            provider: Some("filesystem".to_string()),
            extracted_text: body.extracted_text,
            metadata: json!({}),
            document_date: body.document_date,
            event_date: body.event_date,
            valid_from: body.valid_from,
            valid_to: body.valid_to,
            entity_type: body.entity_type,
            entity_id: body.entity_id,
        },
    )
    .await
}

async fn post_ingest_audio(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<IngestModalBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if body.extracted_text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "extracted_text required" })),
        ));
    }
    create_and_enqueue(
        &st,
        company_id,
        IngestArtifactPayload {
            source_kind: IngestSourceKind::Audio,
            scope: body.scope,
            company_agent_id: body.company_agent_id,
            title: body.title,
            tags: body.tags,
            kind: body.kind,
            source_uri: body.source_uri,
            file_path: None,
            media_type: "audio".to_string(),
            provider: body.provider,
            extracted_text: Some(body.extracted_text),
            metadata: json!({}),
            document_date: body.document_date,
            event_date: body.event_date,
            valid_from: body.valid_from,
            valid_to: body.valid_to,
            entity_type: body.entity_type,
            entity_id: body.entity_id,
        },
    )
    .await
}

async fn post_ingest_image(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Json(body): Json<IngestModalBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if body.extracted_text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "extracted_text required" })),
        ));
    }
    create_and_enqueue(
        &st,
        company_id,
        IngestArtifactPayload {
            source_kind: IngestSourceKind::Image,
            scope: body.scope,
            company_agent_id: body.company_agent_id,
            title: body.title,
            tags: body.tags,
            kind: body.kind,
            source_uri: body.source_uri,
            file_path: None,
            media_type: "image".to_string(),
            provider: body.provider,
            extracted_text: Some(body.extracted_text),
            metadata: json!({}),
            document_date: body.document_date,
            event_date: body.event_date,
            valid_from: body.valid_from,
            valid_to: body.valid_to,
            entity_type: body.entity_type,
            entity_id: body.entity_id,
        },
    )
    .await
}

async fn list_memory_artifacts(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(query): Query<ArtifactListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    verify_company(pool, company_id).await?;
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query_as::<_, MemoryArtifactRow>(
        r#"SELECT id, company_id, memory_id, media_type, source_type, source_uri, storage_uri,
                  title, checksum, size_bytes, extraction_status, extraction_provider,
                  retry_count, last_error, document_date, event_date, valid_from, valid_to,
                  entity_type, entity_id, contains_pii, redacted_text, extracted_text,
                  metadata, created_at, updated_at
           FROM memory_artifacts
           WHERE company_id = $1
             AND ($2::text IS NULL OR extraction_status = $2)
           ORDER BY created_at DESC
           LIMIT $3"#,
    )
    .bind(company_id)
    .bind(query.status.as_deref())
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| ingest_db_err("list_memory_artifacts", &e))?;
    Ok(Json(json!({ "artifacts": rows })))
}

async fn get_memory_artifact(
    State(st): State<ConsoleState>,
    Path((company_id, artifact_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let artifact = sqlx::query_as::<_, MemoryArtifactRow>(
        r#"SELECT id, company_id, memory_id, media_type, source_type, source_uri, storage_uri,
                  title, checksum, size_bytes, extraction_status, extraction_provider,
                  retry_count, last_error, document_date, event_date, valid_from, valid_to,
                  entity_type, entity_id, contains_pii, redacted_text, extracted_text,
                  metadata, created_at, updated_at
           FROM memory_artifacts
           WHERE company_id = $1 AND id = $2"#,
    )
    .bind(company_id)
    .bind(artifact_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ingest_db_err("get_memory_artifact", &e))?;
    let Some(artifact) = artifact else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "artifact not found" }))));
    };
    let chunks = sqlx::query_as::<_, MemoryChunkRow>(
        r#"SELECT id, artifact_id, memory_id, chunk_index, text, summary_l0, summary_l1, token_count,
                  modality, page_number, time_start_ms, time_end_ms, entity_type, entity_id,
                  document_date, event_date, valid_from, valid_to, source_range, contains_pii, redacted_text
           FROM memory_chunks
           WHERE company_id = $1 AND artifact_id = $2
           ORDER BY chunk_index"#,
    )
    .bind(company_id)
    .bind(artifact_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ingest_db_err("get_memory_artifact_chunks", &e))?;
    Ok(Json(json!({
        "artifact": artifact,
        "chunks": chunks,
    })))
}

async fn post_retry_artifact(
    State(st): State<ConsoleState>,
    Path((company_id, artifact_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let updated = sqlx::query(
        r#"UPDATE memory_artifacts
           SET extraction_status = 'queued',
               last_error = NULL,
               updated_at = NOW()
           WHERE company_id = $1 AND id = $2
             AND extraction_status IN ('retry_waiting', 'failed', 'dead_letter')"#,
    )
    .bind(company_id)
    .bind(artifact_id)
    .execute(pool)
    .await
    .map_err(|e| ingest_db_err("post_retry_artifact", &e))?;
    if updated.rows_affected() == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "artifact not retryable" })),
        ));
    }
    enqueue_artifact_processing(pool.clone(), artifact_id).await;
    Ok(Json(json!({ "ok": true, "artifact_id": artifact_id })))
}

async fn get_memory_inspect(
    State(st): State<ConsoleState>,
    Path((company_id, memory_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let memory = sqlx::query_as::<_, MemoryDetailRow>(
        r#"SELECT id, company_id, scope, company_agent_id, title, body, tags, source, summary_l0, summary_l1, kind,
                  supersedes_memory_id, is_latest, version, document_date, event_date, valid_from, valid_to,
                  entity_type, entity_id, source_type, source_uri, chunk_id, source_range, contains_pii,
                  redacted_body, primary_artifact_id, source_artifact_count, chunk_count, created_at, updated_at
           FROM company_memory_entries
           WHERE company_id = $1 AND id = $2"#,
    )
    .bind(company_id)
    .bind(memory_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ingest_db_err("get_memory_inspect", &e))?;
    let Some(memory) = memory else {
        return Err((StatusCode::NOT_FOUND, Json(json!({ "error": "memory entry not found" }))));
    };
    let artifacts = sqlx::query_as::<_, MemoryArtifactRow>(
        r#"SELECT id, company_id, memory_id, media_type, source_type, source_uri, storage_uri,
                  title, checksum, size_bytes, extraction_status, extraction_provider,
                  retry_count, last_error, document_date, event_date, valid_from, valid_to,
                  entity_type, entity_id, contains_pii, redacted_text, extracted_text,
                  metadata, created_at, updated_at
           FROM memory_artifacts
           WHERE company_id = $1 AND memory_id = $2
           ORDER BY created_at ASC"#,
    )
    .bind(company_id)
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ingest_db_err("get_memory_inspect_artifacts", &e))?;
    let chunks = sqlx::query_as::<_, MemoryChunkRow>(
        r#"SELECT id, artifact_id, memory_id, chunk_index, text, summary_l0, summary_l1, token_count,
                  modality, page_number, time_start_ms, time_end_ms, entity_type, entity_id,
                  document_date, event_date, valid_from, valid_to, source_range, contains_pii, redacted_text
           FROM memory_chunks
           WHERE company_id = $1 AND memory_id = $2
           ORDER BY chunk_index
           LIMIT 120"#,
    )
    .bind(company_id)
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ingest_db_err("get_memory_inspect_chunks", &e))?;
    let lineage: Vec<Value> = sqlx::query_scalar(
        r#"WITH RECURSIVE chain AS (
             SELECT id, supersedes_memory_id, version, is_latest
             FROM company_memory_entries
             WHERE company_id = $1 AND id = $2
           UNION ALL
             SELECT m.id, m.supersedes_memory_id, m.version, m.is_latest
             FROM company_memory_entries m
             JOIN chain c ON m.id = c.supersedes_memory_id
             WHERE m.company_id = $1
           )
           SELECT jsonb_build_object(
               'id', id,
               'version', version,
               'is_latest', is_latest,
               'supersedes_memory_id', supersedes_memory_id
           )
           FROM chain
           ORDER BY version DESC"#,
    )
    .bind(company_id)
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ingest_db_err("get_memory_inspect_lineage", &e))?;
    Ok(Json(json!({
        "memory": memory,
        "artifacts": artifacts,
        "chunks": chunks,
        "lineage": lineage,
    })))
}

fn build_search_options(q: &RetrievalDebugQuery) -> HybridSearchOptions {
    let scope = q.scope.as_deref().unwrap_or("shared");
    let mut opts = HybridSearchOptions::for_scope(scope, q.company_agent_id.unwrap_or_else(Uuid::nil));
    opts.latest_only = q.latest_only.unwrap_or(false);
    opts.entity_type = q.entity_type.clone();
    opts.entity_id = q.entity_id.clone();
    opts.valid_at = q.valid_at;
    opts.document_date_from = q.document_date_from;
    opts.document_date_to = q.document_date_to;
    opts.event_date_from = q.event_date_from;
    opts.event_date_to = q.event_date_to;
    opts.limit = q.limit.unwrap_or(24).clamp(1, 100);
    opts
}

pub async fn build_memory_context_addon(
    pool: &PgPool,
    company_id: Uuid,
    query: &str,
    options: &HybridSearchOptions,
    heading: &str,
) -> Result<MemoryContextAddon, sqlx::Error> {
    let (matches, _meta) = hybrid::hybrid_search_memory_debug(pool, company_id, query, options).await?;
    if matches.is_empty() {
        return Ok(MemoryContextAddon {
            markdown: String::new(),
            match_count: 0,
            matches,
        });
    }
    let ids: Vec<Uuid> = matches.iter().map(|m| m.id).collect();
    let rows: Vec<(
        Uuid,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    )> =
        sqlx::query_as(
            r#"SELECT e.id, e.title, COALESCE(e.redacted_body, e.body), e.summary_l1, e.entity_type, e.event_date, e.document_date
               FROM company_memory_entries e
               JOIN unnest($1::uuid[]) WITH ORDINALITY AS u(id, ord) ON e.id = u.id
               ORDER BY u.ord"#,
        )
        .bind(&ids)
        .fetch_all(pool)
        .await?;
    let row_map: HashMap<Uuid, _> = rows.into_iter().map(|row| (row.0, row)).collect();
    let mut markdown = format!("## {heading}\n\n");
    for matched in &matches {
        let Some((_, title, body, summary_l1, entity_type, event_date, document_date)) = row_map.get(&matched.id) else {
            continue;
        };
        markdown.push_str(&format!("### {title}\n"));
        if !matched.matched_via.is_empty() {
            markdown.push_str(&format!(
                "- matched via: {}\n",
                matched.matched_via.join(", ")
            ));
        }
        if let Some(entity_type) = entity_type {
            markdown.push_str(&format!("- entity: {entity_type}\n"));
        }
        if let Some(event_date) = event_date {
            markdown.push_str(&format!("- event date: {event_date}\n"));
        } else if let Some(document_date) = document_date {
            markdown.push_str(&format!("- document date: {document_date}\n"));
        }
        let body_use = summary_l1.as_ref().filter(|s| !s.trim().is_empty()).unwrap_or(body);
        markdown.push_str(&format!("{}\n", truncate_chars(body_use, 600)));
        if !matched.supporting_chunks.is_empty() {
            markdown.push_str("Supporting chunks:\n");
            for SupportingChunk {
                text,
                modality,
                source_label,
                ..
            } in matched.supporting_chunks.iter().take(2)
            {
                let label = source_label.as_deref().unwrap_or(modality);
                markdown.push_str(&format!(
                    "- [{label}] {}\n",
                    truncate_chars(text, 180)
                ));
            }
        }
        markdown.push('\n');
    }
    Ok(MemoryContextAddon {
        markdown,
        match_count: matches.len(),
        matches,
    })
}

async fn get_retrieval_debug(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
    Query(query): Query<RetrievalDebugQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let q = query.q.trim();
    if q.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": "q required" }))));
    }
    let options = build_search_options(&query);
    let (matches, meta) = hybrid::hybrid_search_memory_debug(pool, company_id, q, &options)
        .await
        .map_err(|e| ingest_db_err("get_retrieval_debug", &e))?;
    Ok(Json(json!({
        "query": q,
        "meta": {
            "mode": meta.mode,
            "channels": meta.channels,
            "reranked": meta.reranked,
            "expansion_terms": meta.expansion_terms,
        },
        "matches": matches,
    })))
}

async fn get_memory_metrics(
    State(st): State<ConsoleState>,
    Path(company_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(ref pool) = st.company_db else {
        return Err(no_db());
    };
    let statuses: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT extraction_status, COUNT(*)::bigint
           FROM memory_artifacts
           WHERE company_id = $1
           GROUP BY extraction_status
           ORDER BY extraction_status"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ingest_db_err("get_memory_metrics_statuses", &e))?;
    let modalities: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT media_type, COUNT(*)::bigint
           FROM memory_artifacts
           WHERE company_id = $1
           GROUP BY media_type
           ORDER BY media_type"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(|e| ingest_db_err("get_memory_metrics_modalities", &e))?;
    let retrieval_ready: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint
           FROM memory_chunks
           WHERE company_id = $1 AND embedding_vec IS NOT NULL"#,
    )
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(|e| ingest_db_err("get_memory_metrics_embeddings", &e))?;
    Ok(Json(json!({
        "company_id": company_id,
        "artifact_status_counts": statuses.into_iter().map(|(k, v)| json!({ "status": k, "count": v })).collect::<Vec<_>>(),
        "artifact_modality_counts": modalities.into_iter().map(|(k, v)| json!({ "media_type": k, "count": v })).collect::<Vec<_>>(),
        "chunk_embeddings_ready": retrieval_ready,
    })))
}
