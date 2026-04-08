//! Unified LLM transport policy from environment (Claude layers 2–3 parity).
//!
//! - **Retries / backoff** — `HSM_LLM_MAX_RETRIES`, `HSM_LLM_RETRY_BASE_MS`, `HSM_LLM_RETRY_MAX_MS`,
//!   `HSM_LLM_RETRY_EXP_BASE`.
//! - **HTTP timeout** — `HSM_LLM_HTTP_TIMEOUT_SECS` (per-request upper bound on the reqwest client).
//!
//! **Streaming:** use [`super::client::LlmClient::chat_stream`] or `POST /api/llm/chat/stream` (SSE).
//! `LlmClient::chat` rejects `stream: true` on the request. Long streams use `http_stream` with
//! `HSM_LLM_STREAM_TIMEOUT_SECS` (default 600).

use std::time::Duration;

use super::client::RetryConfig;

/// Retry / backoff for a single provider attempt (before failover to the next slot).
pub fn retry_config_from_env() -> RetryConfig {
    RetryConfig {
        max_retries: std::env::var("HSM_LLM_MAX_RETRIES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3),
        base_delay_ms: std::env::var("HSM_LLM_RETRY_BASE_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000),
        max_delay_ms: std::env::var("HSM_LLM_RETRY_MAX_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30_000),
        exponential_base: std::env::var("HSM_LLM_RETRY_EXP_BASE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2.0),
    }
}

pub fn http_timeout_secs() -> u64 {
    std::env::var("HSM_LLM_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(120)
}

pub fn http_timeout() -> Duration {
    Duration::from_secs(http_timeout_secs())
}
