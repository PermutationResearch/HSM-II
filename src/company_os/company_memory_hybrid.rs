//! Hybrid company memory search: parallel FTS + embedding + recency, RRF fusion, optional HTTP rerank, optional query expansion.
//! Mirrors [`crate::memory`] RRF (`reciprocal_rank_fusion` with weighted channels).

use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

const RRF_K: f64 = 60.0;
const DEFAULT_EMBED_DIM: usize = 768;

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

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

fn json_vec_f32(v: &Value) -> Option<Vec<f32>> {
    let arr = v.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        out.push(x.as_f64()? as f32);
    }
    (!out.is_empty()).then_some(out)
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
        }
        Err(e) => {
            tracing::debug!(target: "hsm.company_memory", %memory_id, ?e, "ollama embed skipped or failed");
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

#[derive(sqlx::FromRow)]
struct IdRow {
    id: Uuid,
}

#[derive(sqlx::FromRow)]
struct IdEmbRow {
    id: Uuid,
    embedding_json: Option<Value>,
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
    mode_key: &str,
    agent_bind: Uuid,
    like_pat: &str,
    limit: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<IdRow> = sqlx::query_as(
        r#"SELECT id FROM company_memory_entries
           WHERE company_id = $1
             AND CASE $4::text
               WHEN 'all' THEN true
               WHEN 'shared' THEN scope = 'shared'
               WHEN 'agent' THEN scope = 'agent' AND company_agent_id = $5
               ELSE false
             END
             AND (
               to_tsvector(
                    'english',
                    coalesce(title, '') || ' ' || coalesce(body, '') || ' ' || coalesce(summary_l1, '') || ' ' || coalesce(summary_l0, '')
                  ) @@ plainto_tsquery('english', trim($2::text))
               OR title ILIKE $3 ESCAPE '\'
               OR body ILIKE $3 ESCAPE '\'
               OR COALESCE(summary_l1, '') ILIKE $3 ESCAPE '\'
               OR COALESCE(summary_l0, '') ILIKE $3 ESCAPE '\'
             )
           ORDER BY
             ts_rank_cd(
                    to_tsvector(
                      'english',
                      coalesce(title, '') || ' ' || coalesce(body, '') || ' ' || coalesce(summary_l1, '') || ' ' || coalesce(summary_l0, '')
                    ),
                    plainto_tsquery('english', trim($2::text))
                  ) DESC,
             CASE WHEN kind = 'broadcast' THEN 0 ELSE 1 END,
             updated_at DESC
           LIMIT $6"#,
    )
    .bind(company_id)
    .bind(q_search)
    .bind(like_pat)
    .bind(mode_key)
    .bind(agent_bind)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

async fn recency_ranked_ids(
    pool: &PgPool,
    company_id: Uuid,
    mode_key: &str,
    agent_bind: Uuid,
    limit: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<IdRow> = sqlx::query_as(
        r#"SELECT id FROM company_memory_entries
           WHERE company_id = $1
             AND CASE $2::text
               WHEN 'all' THEN true
               WHEN 'shared' THEN scope = 'shared'
               WHEN 'agent' THEN scope = 'agent' AND company_agent_id = $3
               ELSE false
             END
           ORDER BY updated_at DESC
           LIMIT $4"#,
    )
    .bind(company_id)
    .bind(mode_key)
    .bind(agent_bind)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| r.id).collect())
}

async fn vector_ranked_ids(
    pool: &PgPool,
    company_id: Uuid,
    query_emb: &[f32],
    mode_key: &str,
    agent_bind: Uuid,
    limit: usize,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<IdEmbRow> = sqlx::query_as(
        r#"SELECT id, embedding_json FROM company_memory_entries
           WHERE company_id = $1
             AND embedding_json IS NOT NULL
             AND CASE $2::text
               WHEN 'all' THEN true
               WHEN 'shared' THEN scope = 'shared'
               WHEN 'agent' THEN scope = 'agent' AND company_agent_id = $3
               ELSE false
             END"#,
    )
    .bind(company_id)
    .bind(mode_key)
    .bind(agent_bind)
    .fetch_all(pool)
    .await?;

    let mut scored: Vec<(Uuid, f32)> = Vec::new();
    for r in rows {
        if let Some(j) = r.embedding_json.as_ref().and_then(json_vec_f32) {
            if j.len() == query_emb.len() || (j.len() == DEFAULT_EMBED_DIM && query_emb.len() == DEFAULT_EMBED_DIM) {
                let s = cosine_similarity(query_emb, &j);
                scored.push((r.id, s));
            }
        }
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored.into_iter().take(limit).map(|x| x.0).collect())
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

/// Parallel: multiple FTS (query expansion) + query embedding + recency; then vector ANN in-process; RRF; optional reranker HTTP.
pub async fn hybrid_search_memory_ids(
    pool: &PgPool,
    company_id: Uuid,
    q_search: &str,
    mode_key: &str,
    agent_bind: Uuid,
) -> Result<(Vec<Uuid>, HybridMeta), sqlx::Error> {
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
        .unwrap_or(200);

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

    let expansion_terms = query_expansion_terms(&client, q_search).await;
    let exp_n = expansion_terms.len();

    let pool_a = pool.clone();
    let pool_b = pool.clone();
    let mk_fts = mode_key.to_string();
    let mk_rec = mode_key.to_string();
    let terms = expansion_terms.clone();

    let fts_parallel = async move {
        let mut channels: Vec<Vec<Uuid>> = Vec::new();
        for term in terms {
            let like = if term.is_empty() {
                "%".to_string()
            } else {
                format!("%{}%", term.replace('%', "\\%").replace('_', "\\_"))
            };
            let ids = fts_ranked_ids(&pool_a, company_id, &term, &mk_fts, agent_bind, &like, limit_fts).await?;
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

    let rec_parallel = async move { recency_ranked_ids(&pool_b, company_id, &mk_rec, agent_bind, limit_rec).await };

    let (fts_channels_result, embed_res, rec_res) = tokio::join!(fts_parallel, embed_parallel, rec_parallel);

    let fts_channels: Vec<Vec<Uuid>> = fts_channels_result.unwrap_or_else(|e| {
        tracing::debug!(target: "hsm.company_memory", ?e, "parallel fts channels failed");
        vec![]
    });
    let recency_ids: Vec<Uuid> = rec_res.unwrap_or_else(|e| {
        tracing::debug!(target: "hsm.company_memory", ?e, "recency channel failed");
        vec![]
    });

    let vec_ids: Vec<Uuid> = if let Some(ref emb) = embed_res {
        vector_ranked_ids(pool, company_id, emb, mode_key, agent_bind, limit_vec).await?
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

    let mut fused = if channels.is_empty() {
        vec![]
    } else {
        reciprocal_rank_fusion_weighted(&channels, &weights, rrf_out)
    };

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

    let meta = HybridMeta {
        mode: "hybrid",
        channels: names,
        reranked,
        expansion_terms: exp_n,
    };

    Ok((fused, meta))
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
}
