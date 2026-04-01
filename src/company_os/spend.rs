//! Async spend ledger writes after LLM calls (optional; env-gated).

use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

/// Fire-and-forget insert into `spend_events` when:
/// - `HSM_COMPANY_OS_DATABASE_URL` is set
/// - `HSM_COMPANY_ID` is a valid UUID (company must exist)
///
/// Optional: `HSM_COMPANY_TASK_ID`, `HSM_SPEND_AGENT_REF` (default `llm`).
/// Pricing: `HSM_LLM_PRICE_PER_1K_OUTPUT_TOKENS_USD` (default `0`) × (completion_tokens / 1000).
pub fn spawn_record_llm_spend(
    model: &str,
    text: &str,
    tokens_generated: usize,
    latency_ms: u64,
    timed_out: bool,
    cached: bool,
) {
    if timed_out || text.trim_start().starts_with("[FALLBACK:") {
        return;
    }
    let Ok(url) = std::env::var("HSM_COMPANY_OS_DATABASE_URL") else {
        return;
    };
    let url = url.trim().to_string();
    if url.is_empty() {
        return;
    }
    let Ok(cid) = std::env::var("HSM_COMPANY_ID") else {
        return;
    };
    let Ok(company_id) = Uuid::parse_str(cid.trim()) else {
        tracing::warn!(target: "hsm_company_spend", "HSM_COMPANY_ID is not a valid UUID");
        return;
    };
    let task_id = std::env::var("HSM_COMPANY_TASK_ID")
        .ok()
        .and_then(|s| Uuid::parse_str(s.trim()).ok());
    let agent_ref = std::env::var("HSM_SPEND_AGENT_REF").unwrap_or_else(|_| "llm".to_string());
    let price_per_1k: f64 = std::env::var("HSM_LLM_PRICE_PER_1K_OUTPUT_TOKENS_USD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let tokens = tokens_generated as f64;
    let amount_usd = (tokens / 1000.0) * price_per_1k;
    let model = model.to_string();
    let meta = json!({
        "model": model,
        "latency_ms": latency_ms,
        "completion_tokens": tokens_generated,
        "cached": cached,
    });

    tokio::spawn(async move {
        let Ok(pool) = PgPoolOptions::new().max_connections(1).connect(&url).await else {
            tracing::warn!(target: "hsm_company_spend", "spend pool connect failed");
            return;
        };
        let tid = task_id;
        let res = sqlx::query(
            r#"INSERT INTO spend_events
               (company_id, task_id, agent_ref, kind, amount, unit, meta)
               VALUES ($1, $2, $3, 'llm_output_tokens', $4, 'usd', $5)"#,
        )
        .bind(company_id)
        .bind(tid)
        .bind(&agent_ref)
        .bind(amount_usd)
        .bind(meta)
        .execute(&pool)
        .await;
        if let Err(e) = res {
            tracing::warn!(target: "hsm_company_spend", "spend_events insert failed: {e}");
        }
    });
}
