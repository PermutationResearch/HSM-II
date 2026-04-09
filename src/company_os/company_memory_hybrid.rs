//! Hybrid company memory search: chunk-level FTS + pgvector + graph + temporal + recency.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::company_memory::expand_via_graph;

const RRF_K: f64 = 60.0;
fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(s) => {
            let t = s.trim().to_ascii_lowercase();
            matches!(t.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => default,
    }
}

fn ollama_base_url() -> String {
    std::env::var("HSM_OLLAMA_EMBED_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            let raw = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
            let mut h = raw.trim().trim_end_matches('/').to_string();
            if h.ends_with("/v1") {
                h.truncate(h.len().saturating_sub(3));
                h = h.trim_end_matches('/').to_string();
            }
            h
        })
}

fn embed_model() -> String {
    std::env::var("HSM_EMBED_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string())
}

/// Embed text via Ollama `/api/embeddings`.
pub async fn ollama_embed_text(client: &Client, text: &str) -> anyhow::Result<Vec<f32>> {
    let base = ollama_base_url();
    let url = format!("{}/api/embeddings", base.trim_end_matches('/'));
    let payload = json!({ "model": embed_model(), "prompt": text });
    let timeout_ms: u64 = std::env::var("HSM_MEMORY_EMBED_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(15_000);
    let resp = client
        .post(&url)
        .json(&payload)
        .timeout(Duration::from_millis(timeout_ms))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let t = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "embeddings HTTP {}: {}",
            status,
            t.chars().take(200).collect::<String>()
        );
    }
    let value: Value = resp.json().await?;
    let arr = value
        .get("embedding")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("no embedding in response"))?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        if let Some(f) = v.as_f64() {
            out.push(f as f32);
        }
    }
    if out.is_empty() {
        anyhow::bail!("empty embedding");
    }
    Ok(out)
}

fn vector_literal(emb: &[f32]) -> String {
    let values = emb
        .iter()
        .map(|v| {
            if v.is_finite() {
                format!("{v}")
            } else {
                "0".to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{values}]")
}

pub async fn store_embedding_json(pool: &PgPool, memory_id: Uuid, emb: &[f32]) -> Result<(), sqlx::Error> {
    let j = serde_json::to_value(emb).unwrap_or(json!([]));
    sqlx::query(
        r#"UPDATE company_memory_entries SET embedding_json = $2::jsonb, updated_at = NOW() WHERE id = $1"#,
    )
    .bind(memory_id)
    .bind(j)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn store_embedding_vec(
    pool: &PgPool,
    memory_id: Uuid,
    emb: &[f32],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE company_memory_entries
           SET embedding_vec = $2::vector,
               updated_at = NOW()
           WHERE id = $1"#,
    )
    .bind(memory_id)
    .bind(vector_literal(emb))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn store_chunk_embedding(
    pool: &PgPool,
    chunk_id: Uuid,
    emb: &[f32],
) -> Result<(), sqlx::Error> {
    let j = serde_json::to_value(emb).unwrap_or(json!([]));
    sqlx::query(
        r#"UPDATE memory_chunks
           SET embedding_json = $2::jsonb,
               embedding_vec = $3::vector
           WHERE id = $1"#,
    )
    .bind(chunk_id)
    .bind(j)
    .bind(vector_literal(emb))
    .execute(pool)
    .await?;
    Ok(())
}

pub fn text_for_embedding(title: &str, body: &str) -> String {
    let t = title.trim();
    let b = body.trim();
    if b.is_empty() {
        t.to_string()
    } else {
        format!("{}\n{}", t, b.chars().take(12_000).collect::<String>())
    }
}

pub async fn embed_row_after_write(pool: PgPool, memory_id: Uuid, title: String, body: String) {
    if !env_bool("HSM_MEMORY_EMBED_ENABLED", true) {
        return;
    }
    let client = Client::new();
    let text = text_for_embedding(&title, &body);
    if text.is_empty() {
        return;
    }
    match ollama_embed_text(&client, &text).await {
        Ok(emb) => {
            if let Err(e) = store_embedding_json(&pool, memory_id, &emb).await {
                tracing::warn!(target: "hsm.company_memory", %memory_id, ?e, "store embedding failed");
            }
            if let Err(e) = store_embedding_vec(&pool, memory_id, &emb).await {
                tracing::warn!(target: "hsm.company_memory", %memory_id, ?e, "store vector embedding failed");
            }
        }
        Err(e) => {
            tracing::debug!(target: "hsm.company_memory", %memory_id, ?e, "ollama embed skipped or failed");
        }
    }
}

pub async fn embed_chunks_after_write(pool: PgPool, chunks: Vec<(Uuid, String)>) {
    if !env_bool("HSM_MEMORY_EMBED_ENABLED", true) || chunks.is_empty() {
        return;
    }
    let client = Client::new();
    for (chunk_id, text) in chunks {
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        match ollama_embed_text(&client, t).await {
            Ok(emb) => {
                if let Err(e) = store_chunk_embedding(&pool, chunk_id, &emb).await {
                    tracing::warn!(target: "hsm.company_memory", %chunk_id, ?e, "store chunk embedding failed");
                }
            }
            Err(e) => {
                tracing::debug!(target: "hsm.company_memory", %chunk_id, ?e, "chunk embed skipped or failed");
            }
        }
    }
}

pub fn reciprocal_rank_fusion_weighted(channels: &[Vec<Uuid>], weights: &[f64], out: usize) -> Vec<Uuid> {
    let mut fused: HashMap<Uuid, f64> = HashMap::new();
    for (ci, ids) in channels.iter().enumerate() {
        let w = weights.get(ci).copied().unwrap_or(1.0);
        for (rank, id) in ids.iter().enumerate() {
            *fused.entry(*id).or_insert(0.0) += w / (RRF_K + rank as f64 + 1.0);
        }
    }
    let mut v: Vec<(Uuid, f64)> = fused.into_iter().collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    v.into_iter().take(out).map(|x| x.0).collect()
}

#[derive(Debug, Clone)]
pub struct HybridSearchOptions {
    pub mode_key: String,
    pub agent_bind: Uuid,
    pub latest_only: bool,
    pub entity_type: Option<String>,
    pub entity_id: Option<String>,
    pub valid_at: Option<DateTime<Utc>>,
    pub document_date_from: Option<DateTime<Utc>>,
    pub document_date_to: Option<DateTime<Utc>>,
    pub event_date_from: Option<DateTime<Utc>>,
    pub event_date_to: Option<DateTime<Utc>>,
    pub limit: usize,
}

impl HybridSearchOptions {
    pub fn for_scope(mode_key: &str, agent_bind: Uuid) -> Self {
        Self {
            mode_key: mode_key.to_string(),
            agent_bind,
            latest_only: false,
            entity_type: None,
            entity_id: None,
            valid_at: None,
            document_date_from: None,
            document_date_to: None,
            event_date_from: None,
            event_date_to: None,
            limit: 200,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SupportingChunk {
    pub chunk_id: Uuid,
    pub chunk_index: i32,
    pub text: String,
    pub modality: String,
    pub source_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HybridMatch {
    pub id: Uuid,
    pub matched_via: Vec<String>,
    pub supporting_chunks: Vec<SupportingChunk>,
    pub lineage_summary: Option<String>,
    pub latest_version_only: bool,
}

async fn query_expansion_terms(client: &Client, query: &str) -> Vec<String> {
    if !env_bool("HSM_MEMORY_QUERY_EXPANSION", false) {
        return vec![query.to_string()];
    }
    let base = ollama_base_url();
    let url = format!("{}/api/chat", base.trim_end_matches('/'));
    let model = std::env::var("HSM_MEMORY_EXPANSION_MODEL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2".to_string()));
    let payload = json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": format!(
                "Given this search query for a company knowledge base, reply with ONLY a JSON array of 1 to 3 short English keyword phrases (no sentences) that help recall related rows. Query: {:?}\nExample: [\"billing\", \"refund policy\"]",
                query.chars().take(400).collect::<String>()
            )
        }],
        "stream": false,
        "options": { "temperature": 0.2 }
    });
    let Ok(resp) = client.post(&url).json(&payload).timeout(Duration::from_secs(20)).send().await else {
        return vec![query.to_string()];
    };
    let Ok(val) = resp.json::<Value>().await else {
        return vec![query.to_string()];
    };
    let text = val
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    let cleaned = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let parsed: Option<Vec<String>> = serde_json::from_str(cleaned).ok();
    let mut terms = vec![query.to_string()];
    if let Some(arr) = parsed {
        for s in arr.into_iter().take(3) {
            let t = s.trim().to_string();
            if t.len() > 2 && !terms.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
                terms.push(t);
            }
        }
    }
    terms
}

async fn fts_ranked_ids(
    pool: &PgPool,
    company_id: Uuid,
    q_search: &str,
    options: &HybridSearchOptions,
    like_pat: &str,
    limit: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT c.memory_id
           FROM memory_chunks c
           JOIN company_memory_entries e ON e.id = c.memory_id
           WHERE c.company_id = $1
             AND CASE $4::text
               WHEN 'all' THEN true
               WHEN 'shared' THEN e.scope = 'shared'
               WHEN 'agent' THEN e.scope = 'agent' AND e.company_agent_id = $5
               ELSE false
             END
             AND ($6::bool = false OR e.is_latest = true)
             AND ($7::text IS NULL OR COALESCE(c.entity_type, e.entity_type) = $7)
             AND ($8::text IS NULL OR COALESCE(c.entity_id, e.entity_id) = $8)
             AND ($9::timestamptz IS NULL OR COALESCE(c.valid_from, e.valid_from) IS NULL OR COALESCE(c.valid_from, e.valid_from) <= $9)
             AND ($9::timestamptz IS NULL OR COALESCE(c.valid_to, e.valid_to) IS NULL OR COALESCE(c.valid_to, e.valid_to) >= $9)
             AND ($10::timestamptz IS NULL OR COALESCE(c.document_date, e.document_date) >= $10)
             AND ($11::timestamptz IS NULL OR COALESCE(c.document_date, e.document_date) <= $11)
             AND ($12::timestamptz IS NULL OR COALESCE(c.event_date, e.event_date) >= $12)
             AND ($13::timestamptz IS NULL OR COALESCE(c.event_date, e.event_date) <= $13)
             AND (
               to_tsvector(
                    'english',
                    coalesce(c.text, '') || ' ' || coalesce(c.summary_l1, '') || ' ' || coalesce(c.summary_l0, '')
                  ) @@ plainto_tsquery('english', trim($2::text))
               OR c.text ILIKE $3 ESCAPE '\'
               OR COALESCE(c.summary_l1, '') ILIKE $3 ESCAPE '\'
               OR COALESCE(c.summary_l0, '') ILIKE $3 ESCAPE '\'
             )
           GROUP BY c.memory_id
           ORDER BY MAX(
               ts_rank_cd(
                   to_tsvector(
                       'english',
                       coalesce(c.text, '') || ' ' || coalesce(c.summary_l1, '') || ' ' || coalesce(c.summary_l0, '')
                   ),
                   plainto_tsquery('english', trim($2::text))
               )
           ) DESC,
           MAX(e.updated_at) DESC
           LIMIT $14"#,
    )
    .bind(company_id)
    .bind(q_search)
    .bind(like_pat)
    .bind(&options.mode_key)
    .bind(options.agent_bind)
    .bind(options.latest_only)
    .bind(options.entity_type.as_deref())
    .bind(options.entity_id.as_deref())
    .bind(options.valid_at)
    .bind(options.document_date_from)
    .bind(options.document_date_to)
    .bind(options.event_date_from)
    .bind(options.event_date_to)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

async fn recency_ranked_ids(
    pool: &PgPool,
    company_id: Uuid,
    options: &HybridSearchOptions,
    limit: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT id FROM company_memory_entries
           WHERE company_id = $1
             AND CASE $2::text
               WHEN 'all' THEN true
               WHEN 'shared' THEN scope = 'shared'
               WHEN 'agent' THEN scope = 'agent' AND company_agent_id = $3
               ELSE false
             END
             AND ($4::bool = false OR is_latest = true)
             AND ($5::text IS NULL OR entity_type = $5)
             AND ($6::text IS NULL OR entity_id = $6)
             AND ($7::timestamptz IS NULL OR valid_from IS NULL OR valid_from <= $7)
             AND ($7::timestamptz IS NULL OR valid_to IS NULL OR valid_to >= $7)
             AND ($8::timestamptz IS NULL OR document_date >= $8)
             AND ($9::timestamptz IS NULL OR document_date <= $9)
             AND ($10::timestamptz IS NULL OR event_date >= $10)
             AND ($11::timestamptz IS NULL OR event_date <= $11)
           ORDER BY updated_at DESC
           LIMIT $12"#,
    )
    .bind(company_id)
    .bind(&options.mode_key)
    .bind(options.agent_bind)
    .bind(options.latest_only)
    .bind(options.entity_type.as_deref())
    .bind(options.entity_id.as_deref())
    .bind(options.valid_at)
    .bind(options.document_date_from)
    .bind(options.document_date_to)
    .bind(options.event_date_from)
    .bind(options.event_date_to)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

async fn temporal_ranked_ids(
    pool: &PgPool,
    company_id: Uuid,
    options: &HybridSearchOptions,
    limit: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let anchor = options
        .valid_at
        .or(options.event_date_to)
        .or(options.event_date_from)
        .or(options.document_date_to)
        .or(options.document_date_from)
        .unwrap_or_else(Utc::now);
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT id
           FROM company_memory_entries
           WHERE company_id = $1
             AND CASE $2::text
               WHEN 'all' THEN true
               WHEN 'shared' THEN scope = 'shared'
               WHEN 'agent' THEN scope = 'agent' AND company_agent_id = $3
               ELSE false
             END
             AND ($4::bool = false OR is_latest = true)
             AND ($5::text IS NULL OR entity_type = $5)
             AND ($6::text IS NULL OR entity_id = $6)
             AND ($7::timestamptz IS NULL OR valid_from IS NULL OR valid_from <= $7)
             AND ($7::timestamptz IS NULL OR valid_to IS NULL OR valid_to >= $7)
             AND ($8::timestamptz IS NULL OR document_date >= $8)
             AND ($9::timestamptz IS NULL OR document_date <= $9)
             AND ($10::timestamptz IS NULL OR event_date >= $10)
             AND ($11::timestamptz IS NULL OR event_date <= $11)
           ORDER BY ABS(EXTRACT(EPOCH FROM (COALESCE(event_date, document_date, updated_at) - $12::timestamptz))) ASC,
                    updated_at DESC
           LIMIT $13"#,
    )
    .bind(company_id)
    .bind(&options.mode_key)
    .bind(options.agent_bind)
    .bind(options.latest_only)
    .bind(options.entity_type.as_deref())
    .bind(options.entity_id.as_deref())
    .bind(options.valid_at)
    .bind(options.document_date_from)
    .bind(options.document_date_to)
    .bind(options.event_date_from)
    .bind(options.event_date_to)
    .bind(anchor)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

async fn vector_ranked_ids(
    pool: &PgPool,
    company_id: Uuid,
    query_emb: &[f32],
    options: &HybridSearchOptions,
    limit: usize,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r#"SELECT c.memory_id
           FROM memory_chunks c
           JOIN company_memory_entries e ON e.id = c.memory_id
           WHERE c.company_id = $1
             AND c.embedding_vec IS NOT NULL
             AND CASE $2::text
               WHEN 'all' THEN true
               WHEN 'shared' THEN e.scope = 'shared'
               WHEN 'agent' THEN e.scope = 'agent' AND e.company_agent_id = $3
               ELSE false
             END
             AND ($4::bool = false OR e.is_latest = true)
             AND ($5::text IS NULL OR COALESCE(c.entity_type, e.entity_type) = $5)
             AND ($6::text IS NULL OR COALESCE(c.entity_id, e.entity_id) = $6)
             AND ($7::timestamptz IS NULL OR COALESCE(c.valid_from, e.valid_from) IS NULL OR COALESCE(c.valid_from, e.valid_from) <= $7)
             AND ($7::timestamptz IS NULL OR COALESCE(c.valid_to, e.valid_to) IS NULL OR COALESCE(c.valid_to, e.valid_to) >= $7)
             AND ($8::timestamptz IS NULL OR COALESCE(c.document_date, e.document_date) >= $8)
             AND ($9::timestamptz IS NULL OR COALESCE(c.document_date, e.document_date) <= $9)
             AND ($10::timestamptz IS NULL OR COALESCE(c.event_date, e.event_date) >= $10)
             AND ($11::timestamptz IS NULL OR COALESCE(c.event_date, e.event_date) <= $11)
           GROUP BY c.memory_id
           ORDER BY MIN(c.embedding_vec <=> $12::vector) ASC
           LIMIT $13"#,
    )
    .bind(company_id)
    .bind(&options.mode_key)
    .bind(options.agent_bind)
    .bind(options.latest_only)
    .bind(options.entity_type.as_deref())
    .bind(options.entity_id.as_deref())
    .bind(options.valid_at)
    .bind(options.document_date_from)
    .bind(options.document_date_to)
    .bind(options.event_date_from)
    .bind(options.event_date_to)
    .bind(vector_literal(query_emb))
    .bind(limit as i64)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

#[derive(Deserialize)]
struct RerankResp {
    scores: Vec<RerankScore>,
}

#[derive(Deserialize)]
struct RerankScore {
    id: Uuid,
    score: f64,
}

async fn optional_http_rerank(
    client: &Client,
    query: &str,
    ids: &[Uuid],
    id_to_text: &HashMap<Uuid, String>,
) -> Option<Vec<Uuid>> {
    let url = std::env::var("HSM_MEMORY_RERANK_URL").ok()?;
    if url.trim().is_empty() || ids.len() < 2 {
        return None;
    }
    let candidates: Vec<Value> = ids
        .iter()
        .filter_map(|id| {
            id_to_text.get(id).map(|t| {
                json!({
                    "id": id,
                    "text": t.chars().take(4000).collect::<String>()
                })
            })
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let body = json!({ "query": query, "candidates": candidates });
    let resp = client
        .post(url.trim())
        .json(&body)
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let r: RerankResp = resp.json().await.ok()?;
    let mut pairs: Vec<(Uuid, f64)> = r.scores.into_iter().map(|s| (s.id, s.score)).collect();
    pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Some(pairs.into_iter().map(|x| x.0).collect())
}

pub struct HybridMeta {
    pub mode: &'static str,
    pub channels: Vec<String>,
    pub reranked: bool,
    pub expansion_terms: usize,
}

async fn fetch_supporting_chunks(
    pool: &PgPool,
    company_id: Uuid,
    memory_id: Uuid,
) -> Result<Vec<SupportingChunk>, sqlx::Error> {
    let rows: Vec<(Uuid, i32, String, String, Option<String>)> = sqlx::query_as(
        r#"SELECT c.id, c.chunk_index, COALESCE(c.redacted_text, c.text), c.modality, a.source_uri
           FROM memory_chunks c
           LEFT JOIN memory_artifacts a ON a.id = c.artifact_id
           WHERE c.company_id = $1 AND c.memory_id = $2
           ORDER BY c.chunk_index
           LIMIT 3"#,
    )
    .bind(company_id)
    .bind(memory_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(chunk_id, chunk_index, text, modality, source_label)| SupportingChunk {
            chunk_id,
            chunk_index,
            text: text.chars().take(320).collect(),
            modality,
            source_label,
        })
        .collect())
}

async fn fetch_lineage_summary(
    pool: &PgPool,
    company_id: Uuid,
    memory_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(i32, bool, Option<Uuid>)> = sqlx::query_as(
        r#"SELECT version, is_latest, supersedes_memory_id
           FROM company_memory_entries
           WHERE company_id = $1 AND id = $2"#,
    )
    .bind(company_id)
    .bind(memory_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(version, is_latest, supersedes)| {
        if supersedes.is_some() {
            format!("v{version}{}", if is_latest { " (latest)" } else { "" })
        } else if is_latest {
            "v1 (latest)".to_string()
        } else {
            format!("v{version}")
        }
    }))
}

pub async fn hybrid_search_memory_debug(
    pool: &PgPool,
    company_id: Uuid,
    q_search: &str,
    options: &HybridSearchOptions,
) -> Result<(Vec<HybridMatch>, HybridMeta), sqlx::Error> {
    let client = Client::new();
    let client_embed = client.clone();
    let q_owned = q_search.to_string();
    let limit_fts: i64 = std::env::var("HSM_MEMORY_HYBRID_FTS_K")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let limit_vec: usize = std::env::var("HSM_MEMORY_HYBRID_VEC_K")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let limit_rec: i64 = std::env::var("HSM_MEMORY_HYBRID_RECENCY_K")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let rrf_out: usize = std::env::var("HSM_MEMORY_HYBRID_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(options.limit.max(1));

    let w_fts: f64 = std::env::var("HSM_MEMORY_RRF_WEIGHT_FTS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let w_vec: f64 = std::env::var("HSM_MEMORY_RRF_WEIGHT_VEC")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.9);
    let w_rec: f64 = std::env::var("HSM_MEMORY_RRF_WEIGHT_RECENCY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.45);

    let w_graph: f64 = std::env::var("HSM_MEMORY_RRF_WEIGHT_GRAPH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.8);
    let w_temporal: f64 = std::env::var("HSM_MEMORY_RRF_WEIGHT_TEMPORAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.65);

    let expansion_terms = query_expansion_terms(&client, q_search).await;
    let exp_n = expansion_terms.len();

    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let terms = expansion_terms.clone();
    let opts_fts = options.clone();
    let opts_rec = options.clone();
    let opts_temporal = options.clone();

    let fts_parallel = async move {
        let mut channels: Vec<Vec<Uuid>> = Vec::new();
        for term in terms {
            let like = if term.is_empty() {
                "%".to_string()
            } else {
                format!("%{}%", term.replace('%', "\\%").replace('_', "\\_"))
            };
            let ids = fts_ranked_ids(&pool_a, company_id, &term, &opts_fts, &like, limit_fts).await?;
            if !ids.is_empty() {
                channels.push(ids);
            }
        }
        Ok::<_, sqlx::Error>(channels)
    };

    let embed_parallel = async move {
        if !env_bool("HSM_MEMORY_EMBED_ENABLED", true) {
            return None;
        }
        ollama_embed_text(&client_embed, &q_owned).await.ok()
    };

    let rec_parallel = async move { recency_ranked_ids(&pool_b, company_id, &opts_rec, limit_rec).await };
    let temporal_parallel =
        async move { temporal_ranked_ids(pool, company_id, &opts_temporal, limit_rec).await };

    let (fts_channels_result, embed_res, rec_res, temporal_res) =
        tokio::join!(fts_parallel, embed_parallel, rec_parallel, temporal_parallel);

    let fts_channels: Vec<Vec<Uuid>> = fts_channels_result.unwrap_or_else(|e| {
        tracing::debug!(target: "hsm.company_memory", ?e, "parallel fts channels failed");
        vec![]
    });
    let recency_ids: Vec<Uuid> = rec_res.unwrap_or_else(|e| {
        tracing::debug!(target: "hsm.company_memory", ?e, "recency channel failed");
        vec![]
    });
    let temporal_ids: Vec<Uuid> = temporal_res.unwrap_or_else(|e| {
        tracing::debug!(target: "hsm.company_memory", ?e, "temporal channel failed");
        vec![]
    });

    let vec_ids: Vec<Uuid> = if let Some(ref emb) = embed_res {
        vector_ranked_ids(pool, company_id, emb, options, limit_vec).await?
    } else {
        vec![]
    };

    let mut channels: Vec<Vec<Uuid>> = Vec::new();
    let mut weights: Vec<f64> = Vec::new();
    let mut names: Vec<String> = Vec::new();

    for (i, fts) in fts_channels.into_iter().enumerate() {
        if !fts.is_empty() {
            channels.push(fts);
            weights.push(w_fts);
            names.push(format!("fts_{}", i));
        }
    }
    if !vec_ids.is_empty() {
        channels.push(vec_ids);
        weights.push(w_vec);
        names.push("vector".to_string());
    }
    if !recency_ids.is_empty() {
        channels.push(recency_ids);
        weights.push(w_rec);
        names.push("recency".to_string());
    }
    if !temporal_ids.is_empty() {
        channels.push(temporal_ids);
        weights.push(w_temporal);
        names.push("temporal".to_string());
    }

    let mut fused = if channels.is_empty() {
        vec![]
    } else {
        reciprocal_rank_fusion_weighted(&channels, &weights, rrf_out)
    };

    let seed_ids: Vec<Uuid> = fused.iter().take(12).copied().collect();
    let graph_ids: Vec<Uuid> = if seed_ids.is_empty() {
        vec![]
    } else {
        expand_via_graph(pool, company_id, &seed_ids, 2)
            .await?
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    };
    if !graph_ids.is_empty() {
        channels.push(graph_ids);
        weights.push(w_graph);
        names.push("graph".to_string());
        fused = reciprocal_rank_fusion_weighted(&channels, &weights, rrf_out);
    }

    let mut reranked = false;
    if fused.len() > 2 && std::env::var("HSM_MEMORY_RERANK_URL").map(|s| !s.trim().is_empty()).unwrap_or(false) {
        let take = fused.len().min(80);
        let slice: Vec<Uuid> = fused.iter().take(take).copied().collect();
        let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            r#"SELECT id, title, body FROM company_memory_entries WHERE id = ANY($1::uuid[])"#,
        )
        .bind(&slice[..])
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        let fetch_text: HashMap<Uuid, String> = rows
            .into_iter()
            .map(|(id, title, body)| (id, format!("{}\n{}", title, body)))
            .collect();
        if let Some(re) = optional_http_rerank(&client, q_search, &fused, &fetch_text).await {
            if !re.is_empty() {
                fused = re;
                reranked = true;
            }
        }
    }

    let mut matched_via: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (channel_name, ids) in names.iter().zip(channels.iter()) {
        let prefix = if channel_name.starts_with("fts_") {
            "fts".to_string()
        } else {
            channel_name.clone()
        };
        for id in ids {
            let entry = matched_via.entry(*id).or_default();
            if !entry.iter().any(|x| x == &prefix) {
                entry.push(prefix.clone());
            }
        }
    }

    let meta = HybridMeta {
        mode: "hybrid",
        channels: names,
        reranked,
        expansion_terms: exp_n,
    };
    let latest_only = options.latest_only;
    let mut matches = Vec::new();
    for id in fused.iter().take(options.limit.max(1)) {
        matches.push(HybridMatch {
            id: *id,
            matched_via: matched_via.remove(id).unwrap_or_default(),
            supporting_chunks: fetch_supporting_chunks(pool, company_id, *id).await?,
            lineage_summary: fetch_lineage_summary(pool, company_id, *id).await?,
            latest_version_only: latest_only,
        });
    }
    Ok((matches, meta))
}

/// Parallel: multiple FTS (query expansion) + pgvector + graph + temporal + recency.
pub async fn hybrid_search_memory_ids_with_options(
    pool: &PgPool,
    company_id: Uuid,
    q_search: &str,
    options: &HybridSearchOptions,
) -> Result<(Vec<Uuid>, HybridMeta), sqlx::Error> {
    let (matches, meta) = hybrid_search_memory_debug(pool, company_id, q_search, options).await?;
    Ok((matches.into_iter().map(|m| m.id).collect(), meta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_intersection_boosts_both_lists() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let ch1 = vec![a, b];
        let ch2 = vec![b, c];
        let fused = reciprocal_rank_fusion_weighted(&[ch1, ch2], &[1.0, 1.0], 10);
        assert!(fused.len() >= 2);
        assert_eq!(fused[0], b);
    }

    #[test]
    fn vector_literal_formats_pgvector_input() {
        let lit = vector_literal(&[0.25, -0.5, 1.0]);
        assert_eq!(lit, "[0.25,-0.5,1]");
    }
}
