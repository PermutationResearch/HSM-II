//! Production LLM Client with Multi-Provider Support
//!
//! Supports OpenAI, OpenRouter (OpenAI-compatible), xAI (OpenAI-compatible), Gemini (OpenAI-compatible), Anthropic, and Ollama with automatic failover,
//! retry logic, and comprehensive observability.
//!
//! **Provider order (fallback chain):** set `HSM_LLM_PROVIDER_ORDER` to a comma-separated
//! list: `openai`, `openrouter`, `xai`, `gemini`, `anthropic`, `ollama` (case-insensitive). Example:
//! `HSM_LLM_PROVIDER_ORDER=ollama,openai` tries local Ollama first, then OpenAI. When unset,
//! available cloud providers are tried first, then Ollama (always included by default).

use anyhow::{anyhow, Result};
use futures_util::Stream;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::{error, info, instrument, warn};

/// HTTP error from a provider response (used for retry / failover policy).
#[derive(Debug)]
struct LlmHttpError {
    status: u16,
    body: String,
}

impl std::fmt::Display for LlmHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status, self.body)
    }
}

impl std::error::Error for LlmHttpError {}

impl LlmHttpError {
    fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status)
    }
}

fn root_http_err(e: &anyhow::Error) -> Option<&LlmHttpError> {
    e.downcast_ref::<LlmHttpError>()
        .or_else(|| e.chain().find_map(|c| c.downcast_ref::<LlmHttpError>()))
}

/// One configured upstream (OpenAI, OpenRouter, Anthropic, Ollama).
#[derive(Clone, Debug)]
struct ProviderSlot {
    /// Human/log label: `openai`, `openrouter`, `xai`, `gemini`, `anthropic`, `ollama`.
    label: &'static str,
    transport: LlmProvider,
    api_key: String,
    base_url: String,
}

/// LLM provider types
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmProvider {
    OpenAi,
    Gemini,
    Anthropic,
    Ollama,
}

impl LlmProvider {
    fn try_openai() -> Option<(Self, String, String)> {
        let api_key = std::env::var("OPENAI_API_KEY").ok()?;
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
        Some((LlmProvider::OpenAi, api_key, base_url))
    }

    fn try_anthropic() -> Option<(Self, String, String)> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok()?;
        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com/v1".to_string());
        Some((LlmProvider::Anthropic, api_key, base_url))
    }

    fn try_openrouter() -> Option<(Self, String, String)> {
        let api_key = std::env::var("OPENROUTER_API_KEY").ok()?;
        let base_url = std::env::var("OPENROUTER_API_BASE")
            .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());
        // OpenRouter uses OpenAI-compatible chat/completions paths.
        Some((LlmProvider::OpenAi, api_key, base_url))
    }

    fn try_xai() -> Option<(Self, String, String)> {
        let api_key = std::env::var("XAI_API_KEY").ok()?;
        let base_url =
            std::env::var("XAI_BASE_URL").unwrap_or_else(|_| "https://api.x.ai/v1".to_string());
        // xAI chat endpoint is OpenAI-compatible.
        Some((LlmProvider::OpenAi, api_key, base_url))
    }

    fn try_gemini() -> Option<(Self, String, String)> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .ok()
            .or_else(|| std::env::var("GOOGLE_API_KEY").ok())?;
        let base_url = std::env::var("GEMINI_BASE_URL")
            .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta/openai".to_string());
        Some((LlmProvider::Gemini, api_key, base_url))
    }

    fn ollama_endpoint() -> (Self, String, String) {
        let url =
            std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
        (LlmProvider::Ollama, String::new(), url)
    }

    fn slots_from_env() -> Vec<ProviderSlot> {
        if let Ok(order_str) = std::env::var("HSM_LLM_PROVIDER_ORDER") {
            let tokens: Vec<String> = order_str
                .split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();

            if !tokens.is_empty() {
                let mut providers = Vec::new();
                for t in tokens {
                    match t.as_str() {
                        "openai" => {
                            if let Some((transport, api_key, base_url)) = Self::try_openai() {
                                providers.push(ProviderSlot {
                                    label: "openai",
                                    transport,
                                    api_key,
                                    base_url,
                                });
                            } else {
                                warn!("HSM_LLM_PROVIDER_ORDER lists openai but OPENAI_API_KEY is unset");
                            }
                        }
                        "openrouter" => {
                            if let Some((transport, api_key, base_url)) = Self::try_openrouter() {
                                providers.push(ProviderSlot {
                                    label: "openrouter",
                                    transport,
                                    api_key,
                                    base_url,
                                });
                            } else {
                                warn!("HSM_LLM_PROVIDER_ORDER lists openrouter but OPENROUTER_API_KEY is unset");
                            }
                        }
                        "xai" => {
                            if let Some((transport, api_key, base_url)) = Self::try_xai() {
                                providers.push(ProviderSlot {
                                    label: "xai",
                                    transport,
                                    api_key,
                                    base_url,
                                });
                            } else {
                                warn!(
                                    "HSM_LLM_PROVIDER_ORDER lists xai but XAI_API_KEY is unset"
                                );
                            }
                        }
                        "anthropic" => {
                            if let Some((transport, api_key, base_url)) = Self::try_anthropic() {
                                providers.push(ProviderSlot {
                                    label: "anthropic",
                                    transport,
                                    api_key,
                                    base_url,
                                });
                            } else {
                                warn!("HSM_LLM_PROVIDER_ORDER lists anthropic but ANTHROPIC_API_KEY is unset");
                            }
                        }
                        "gemini" => {
                            if let Some((transport, api_key, base_url)) = Self::try_gemini() {
                                providers.push(ProviderSlot {
                                    label: "gemini",
                                    transport,
                                    api_key,
                                    base_url,
                                });
                            } else {
                                warn!("HSM_LLM_PROVIDER_ORDER lists gemini but GEMINI_API_KEY/GOOGLE_API_KEY is unset");
                            }
                        }
                        "ollama" => {
                            let (transport, api_key, base_url) = Self::ollama_endpoint();
                            providers.push(ProviderSlot {
                                label: "ollama",
                                transport,
                                api_key,
                                base_url,
                            });
                        }
                        _ => warn!("HSM_LLM_PROVIDER_ORDER: unknown provider {:?}, expected openai|openrouter|xai|gemini|anthropic|ollama", t),
                    }
                }
                if !providers.is_empty() {
                    return providers;
                }
                warn!("HSM_LLM_PROVIDER_ORDER produced no usable providers; using default order");
            }
        }

        let mut providers = Vec::new();
        if let Some((transport, api_key, base_url)) = Self::try_openai() {
            providers.push(ProviderSlot {
                label: "openai",
                transport,
                api_key,
                base_url,
            });
        }
        if let Some((transport, api_key, base_url)) = Self::try_openrouter() {
            providers.push(ProviderSlot {
                label: "openrouter",
                transport,
                api_key,
                base_url,
            });
        }
        if let Some((transport, api_key, base_url)) = Self::try_xai() {
            providers.push(ProviderSlot {
                label: "xai",
                transport,
                api_key,
                base_url,
            });
        }
        if let Some((transport, api_key, base_url)) = Self::try_gemini() {
            providers.push(ProviderSlot {
                label: "gemini",
                transport,
                api_key,
                base_url,
            });
        }
        if let Some((transport, api_key, base_url)) = Self::try_anthropic() {
            providers.push(ProviderSlot {
                label: "anthropic",
                transport,
                api_key,
                base_url,
            });
        }
        let (transport, api_key, base_url) = Self::ollama_endpoint();
        providers.push(ProviderSlot {
            label: "ollama",
            transport,
            api_key,
            base_url,
        });
        providers
    }
}

/// LLM request configuration
#[derive(Clone, Debug)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: f64,
    pub max_tokens: Option<usize>,
    pub top_p: Option<f64>,
    pub stream: bool,
}

impl Default for LlmRequest {
    fn default() -> Self {
        Self {
            model: crate::ollama_client::resolve_model_from_env("gpt-4o-mini"),
            messages: Vec::new(),
            temperature: 0.7,
            max_tokens: Some(2000),
            top_p: Some(0.9),
            stream: false,
        }
    }
}

/// Resolve runtime model by workflow + risk band.
///
/// Policy source order:
/// 1) `HSM_MODEL_ROUTING_JSON` (JSON object with `low|medium|high` model ids)
/// 2) built-in defaults
pub fn resolve_risk_based_model(
    current_model: &str,
    _workflow_pack: &str,
    risk_band: &str,
) -> (String, &'static str) {
    // Never downgrade a cloud model (openrouter/, openai/, anthropic/, etc.) to a local one.
    // Cloud models are explicitly configured; auto-routing should not override them.
    if crate::ollama_client::is_cloud_model_pub(current_model) {
        return (current_model.to_string(), "cloud_passthrough");
    }
    if let Ok(raw) = std::env::var("HSM_MODEL_ROUTING_JSON") {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            let key = risk_band.trim().to_ascii_lowercase();
            if let Some(model) = v.get(&key).and_then(|x| x.as_str()).map(str::trim) {
                if !model.is_empty() {
                    return (model.to_string(), "env_policy");
                }
            }
        }
    }
    match risk_band.trim().to_ascii_lowercase().as_str() {
        "low" => ("mimo-v2-pro".to_string(), "builtin"),
        "medium" => ("gpt-4o-mini".to_string(), "builtin"),
        "high" => (current_model.to_string(), "builtin"),
        _ => (current_model.to_string(), "builtin"),
    }
}

/// Chat message
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

/// LLM response
#[derive(Clone, Debug)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub usage: Usage,
    pub provider: LlmProvider,
    pub latency_ms: u64,
}

/// Token usage
#[derive(Clone, Debug, Default, Serialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// Retry configuration
#[derive(Clone, Debug)]
pub struct RetryConfig {
    pub max_retries: usize,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub exponential_base: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1000,
            max_delay_ms: 30000,
            exponential_base: 2.0,
        }
    }
}

/// Production LLM client with failover and retry
pub struct LlmClient {
    http: Client,
    /// Long-timeout client for streaming bodies (whole response can exceed default `http` timeout).
    http_stream: Client,
    providers: Vec<ProviderSlot>,
    retry_config: RetryConfig,
    default_model: String,
    metrics: Arc<LlmMetrics>,
    /// Consecutive full-chain failures; opens breaker when ≥ threshold.
    breaker_failures: AtomicU32,
    /// Unix millis until which `chat` returns early (bounded degradation).
    breaker_open_until_ms: AtomicU64,
}

/// Metrics for observability
#[derive(Default)]
struct LlmMetrics {
    requests_total: std::sync::atomic::AtomicU64,
    requests_failed: std::sync::atomic::AtomicU64,
    tokens_total: std::sync::atomic::AtomicU64,
    latency_ms_sum: std::sync::atomic::AtomicU64,
}

impl LlmMetrics {
    fn record_request(&self, success: bool, tokens: usize, latency_ms: u64) {
        self.requests_total
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if !success {
            self.requests_failed
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.tokens_total
            .fetch_add(tokens as u64, std::sync::atomic::Ordering::Relaxed);
        self.latency_ms_sum
            .fetch_add(latency_ms, std::sync::atomic::Ordering::Relaxed);
    }

    fn get_stats(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_total: self
                .requests_total
                .load(std::sync::atomic::Ordering::Relaxed),
            requests_failed: self
                .requests_failed
                .load(std::sync::atomic::Ordering::Relaxed),
            tokens_total: self.tokens_total.load(std::sync::atomic::Ordering::Relaxed),
            avg_latency_ms: {
                let total = self
                    .requests_total
                    .load(std::sync::atomic::Ordering::Relaxed);
                let sum = self
                    .latency_ms_sum
                    .load(std::sync::atomic::Ordering::Relaxed);
                if total > 0 {
                    sum / total
                } else {
                    0
                }
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct MetricsSnapshot {
    pub requests_total: u64,
    pub requests_failed: u64,
    pub tokens_total: u64,
    pub avg_latency_ms: u64,
}

impl LlmClient {
    fn infer_context_window(model: &str) -> &'static str {
        let m = model.to_ascii_lowercase();
        if m.contains("gemini-2.5-pro") {
            "1M"
        } else if m.contains("gemini-2.5-flash") || m.contains("gemini-1.5") {
            "1M"
        } else if m.contains("gpt-4o") || m.contains("gpt-4") {
            "128k"
        } else if m.contains("claude") {
            "200k"
        } else if m.contains("mimo") {
            "256k"
        } else {
            "auto"
        }
    }

    /// Create new LLM client from environment variables
    pub fn new() -> Result<Self> {
        let providers = LlmProvider::slots_from_env();

        if providers.is_empty() {
            return Err(anyhow!(
                "No LLM providers configured. Set one of: OPENAI_API_KEY, OPENROUTER_API_KEY, ANTHROPIC_API_KEY, or OLLAMA_URL"
            ));
        }

        info!(
            "Initialized LLM client with {} provider(s)",
            providers.len()
        );

        let http = Client::builder()
            .timeout(crate::llm::policy::http_timeout())
            .pool_max_idle_per_host(10)
            .build()?;

        let stream_secs: u64 = std::env::var("HSM_LLM_STREAM_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600);
        let http_stream = Client::builder()
            .timeout(Duration::from_secs(stream_secs.max(30)))
            .pool_max_idle_per_host(10)
            .build()?;

        Ok(Self {
            http,
            http_stream,
            providers,
            retry_config: crate::llm::policy::retry_config_from_env(),
            default_model: std::env::var("DEFAULT_LLM_MODEL")
                .unwrap_or_else(|_| crate::ollama_client::resolve_model_from_env("gpt-4o-mini")),
            metrics: Arc::new(LlmMetrics::default()),
            breaker_failures: AtomicU32::new(0),
            breaker_open_until_ms: AtomicU64::new(0),
        })
    }

    /// Create with custom retry config
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Send chat completion request with automatic failover
    #[instrument(skip(self, request))]
    pub async fn chat(&self, mut request: LlmRequest) -> Result<LlmResponse> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if now_ms < self.breaker_open_until_ms.load(Ordering::SeqCst) {
            return Err(anyhow!(
                "LLM circuit breaker cooling down (set HSM_LLM_BREAKER_* or wait)"
            ));
        }

        if request.stream {
            return Err(anyhow!(
                "LlmClient::chat is non-streaming; use LlmClient::chat_stream() for stream: true semantics"
            ));
        }

        if let Ok(cap) = std::env::var("HSM_LLM_MAX_OUTPUT_TOKENS") {
            if let Ok(cap_n) = cap.parse::<usize>() {
                if let Some(ref mut mt) = request.max_tokens {
                    *mt = (*mt).min(cap_n);
                }
            }
        }

        let start = std::time::Instant::now();
        let mut last_error = None;

        // Try each provider in order
        for slot in &self.providers {
            match self
                .try_provider(
                    request.clone(),
                    &slot.transport,
                    slot.label,
                    &slot.api_key,
                    &slot.base_url,
                )
                .await
            {
                Ok(response) => {
                    self.breaker_failures.store(0, Ordering::SeqCst);
                    let latency_ms = start.elapsed().as_millis() as u64;
                    let tokens = response.usage.total_tokens;
                    self.metrics.record_request(true, tokens, latency_ms);

                    info!(
                        provider = %slot.label,
                        model = %response.model,
                        context_window = %Self::infer_context_window(&response.model),
                        latency_ms = latency_ms,
                        tokens = tokens,
                        "LLM request succeeded"
                    );

                    crate::telemetry::client().record_technical(
                        "llm.chat.success",
                        json!({
                            "provider": slot.label,
                            "model": response.model,
                            "context_window": Self::infer_context_window(&response.model),
                            "latency_ms": latency_ms,
                            "total_tokens": tokens,
                        }),
                    );

                    return Ok(LlmResponse {
                        latency_ms,
                        ..response
                    });
                }
                Err(e) => {
                    warn!(provider = %slot.label, error = %e, "Provider failed, trying next");
                    last_error = Some(e);
                }
            }
        }

        // All providers failed
        let latency_ms = start.elapsed().as_millis() as u64;
        self.metrics.record_request(false, 0, latency_ms);

        let fails = self.breaker_failures.fetch_add(1, Ordering::SeqCst) + 1;
        let thresh: u32 = std::env::var("HSM_LLM_BREAKER_FAILURES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        if fails >= thresh {
            let cool_ms: u64 = std::env::var("HSM_LLM_BREAKER_COOL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30_000);
            let now2 = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            self.breaker_open_until_ms
                .store(now2.saturating_add(cool_ms), Ordering::SeqCst);
            warn!(
                cool_ms,
                "LLM circuit breaker opened after repeated failures"
            );
        }

        error!("All LLM providers failed");

        let breaker_opened = fails >= thresh;
        crate::telemetry::client().record_technical(
            "llm.chat.failure",
            json!({
                "latency_ms": latency_ms,
                "breaker_opened": breaker_opened,
            }),
        );

        Err(anyhow!(
            "All providers failed. Last error: {}",
            last_error.unwrap_or_else(|| anyhow!("Unknown error"))
        ))
    }

    /// Default chat model from env (`DEFAULT_LLM_MODEL` / Ollama resolution).
    pub fn default_model(&self) -> &str {
        self.default_model.as_str()
    }

    /// Streaming chat with provider failover (no per-chunk retries; tries next slot on HTTP/open errors).
    #[instrument(skip(self, request))]
    pub async fn chat_stream(
        &self,
        mut request: LlmRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = anyhow::Result<crate::llm::streaming::LlmStreamEvent>> + Send>>,
    > {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if now_ms < self.breaker_open_until_ms.load(Ordering::SeqCst) {
            return Err(anyhow!(
                "LLM circuit breaker cooling down (set HSM_LLM_BREAKER_* or wait)"
            ));
        }

        request.stream = true;

        if let Ok(cap) = std::env::var("HSM_LLM_MAX_OUTPUT_TOKENS") {
            if let Ok(cap_n) = cap.parse::<usize>() {
                if let Some(ref mut mt) = request.max_tokens {
                    *mt = (*mt).min(cap_n);
                }
            }
        }

        let mut last_error = None;
        for slot in &self.providers {
            match self
                .post_stream_request(
                    &request,
                    &slot.transport,
                    slot.label,
                    &slot.api_key,
                    &slot.base_url,
                )
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
                        warn!(
                            provider = %slot.label,
                            status = %status,
                            "streaming request rejected"
                        );
                        last_error = Some(anyhow!("HTTP {}: {}", status, body));
                        continue;
                    }
                    self.breaker_failures.store(0, Ordering::SeqCst);
                    let stream: Pin<
                        Box<
                            dyn Stream<Item = anyhow::Result<crate::llm::streaming::LlmStreamEvent>>
                                + Send,
                        >,
                    > = match slot.transport {
                        LlmProvider::OpenAi | LlmProvider::Gemini => crate::llm::streaming::openai_sse_stream(response, slot.transport.clone()),
                        LlmProvider::Anthropic => {
                            crate::llm::streaming::anthropic_sse_stream(response)
                        }
                        LlmProvider::Ollama => {
                            crate::llm::streaming::ollama_ndjson_stream(response)
                        }
                    };
                    return Ok(stream);
                }
                Err(e) => {
                    warn!(provider = %slot.label, error = %e, "streaming open failed, trying next");
                    last_error = Some(e);
                }
            }
        }

        let fails = self.breaker_failures.fetch_add(1, Ordering::SeqCst) + 1;
        let thresh: u32 = std::env::var("HSM_LLM_BREAKER_FAILURES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        if fails >= thresh {
            let cool_ms: u64 = std::env::var("HSM_LLM_BREAKER_COOL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30_000);
            let now2 = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            self.breaker_open_until_ms
                .store(now2.saturating_add(cool_ms), Ordering::SeqCst);
        }

        Err(anyhow!(
            "All streaming providers failed. Last error: {}",
            last_error.unwrap_or_else(|| anyhow!("Unknown error"))
        ))
    }

    async fn post_stream_request(
        &self,
        request: &LlmRequest,
        provider: &LlmProvider,
        provider_label: &str,
        api_key: &str,
        base_url: &str,
    ) -> Result<Response> {
        let _ = provider_label;
        match provider {
            LlmProvider::OpenAi | LlmProvider::Gemini => {
                let url = format!("{}/chat/completions", base_url);
                let model = Self::openai_compatible_model_id(request.model.as_str(), base_url);
                let mut messages = request
                    .messages
                    .iter()
                    .map(|m| {
                        json!({
                            "role": m.role,
                            "content": m.content
                        })
                    })
                    .collect::<Vec<_>>();
                if provider_label.eq_ignore_ascii_case("xai") {
                    if let Ok(prefill) = std::env::var("HSM_XAI_THINKING_PREFILL") {
                        let trimmed = prefill.trim();
                        if !trimmed.is_empty() {
                            messages.push(json!({
                                "role": "assistant",
                                "content": trimmed,
                            }));
                        }
                    }
                }
                let body = json!({
                    "model": model,
                    "messages": messages,
                    "temperature": request.temperature,
                    "max_tokens": request.max_tokens,
                    "top_p": request.top_p,
                    "stream": true,
                });
                let mut req = self
                    .http_stream
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("Content-Type", "application/json");
                if provider_label.eq_ignore_ascii_case("xai")
                    && std::env::var("HSM_XAI_PROMPT_CACHE").ok().as_deref() == Some("1")
                {
                    req = req.header("x-ai-prompt-cache", "true");
                    if let Ok(cache_key) = std::env::var("XAI_PROMPT_CACHE_KEY") {
                        let trimmed = cache_key.trim();
                        if !trimmed.is_empty() {
                            req = req.header("x-ai-prompt-cache-key", trimmed);
                        }
                    }
                }
                req.json(&body).send().await.map_err(anyhow::Error::from)
            }
            LlmProvider::Anthropic => {
                let url = format!("{}/messages", base_url);
                let system_msg = request
                    .messages
                    .iter()
                    .find(|m| m.role == "system")
                    .map(|m| m.content.clone());
                let messages: Vec<_> = request
                    .messages
                    .iter()
                    .filter(|m| m.role != "system")
                    .map(|m| {
                        json!({
                            "role": if m.role == "user" { "user" } else { "assistant" },
                            "content": m.content
                        })
                    })
                    .collect();
                let mut body = json!({
                    "model": request.model,
                    "messages": messages,
                    "temperature": request.temperature,
                    "max_tokens": request.max_tokens.unwrap_or(4096),
                    "stream": true,
                });
                if let Some(system) = system_msg {
                    body["system"] = json!(system);
                }
                self.http_stream
                    .post(&url)
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01")
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(anyhow::Error::from)
            }
            LlmProvider::Ollama => {
                let url = format!("{}/api/chat", base_url);
                let model = Self::ollama_model_id(&request.model);
                let body = json!({
                    "model": model,
                    "messages": request.messages.iter().map(|m| json!({
                        "role": m.role,
                        "content": m.content
                    })).collect::<Vec<_>>(),
                    "options": {
                        "temperature": request.temperature,
                        "top_p": request.top_p,
                    },
                    "stream": true,
                });
                self.http_stream
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(anyhow::Error::from)
            }
        }
    }

    /// Try a single provider with retry logic
    async fn try_provider(
        &self,
        request: LlmRequest,
        provider: &LlmProvider,
        provider_label: &str,
        api_key: &str,
        base_url: &str,
    ) -> Result<LlmResponse> {
        let mut last_error = None;

        for attempt in 0..=self.retry_config.max_retries {
            if attempt > 0 {
                let delay = self.calculate_backoff(attempt);
                warn!(
                    provider = %provider_label,
                    attempt = attempt,
                    delay_ms = delay,
                    "Retrying LLM request"
                );
                sleep(Duration::from_millis(delay)).await;
            }

            match self
                .send_request(request.clone(), provider, provider_label, api_key, base_url)
                .await
            {
                Ok(response) => return Ok(response),
                Err(e) => {
                    // Don't retry on HTTP 4xx — quota/rate-limit/model-not-found won't heal by waiting.
                    if let Some(http) = root_http_err(&e) {
                        if http.is_client_error() {
                            warn!(
                                provider = %provider_label,
                                status = http.status,
                                "LLM HTTP client error; not retrying this provider"
                            );
                            return Err(e);
                        }
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(anyhow!(
            "Max retries exceeded. Last error: {}",
            last_error.unwrap_or_else(|| anyhow!("Unknown error"))
        ))
    }

    /// Calculate exponential backoff delay
    fn calculate_backoff(&self, attempt: usize) -> u64 {
        let delay = (self.retry_config.base_delay_ms as f64
            * self
                .retry_config
                .exponential_base
                .powi(attempt.saturating_sub(1) as i32)) as u64;
        delay.min(self.retry_config.max_delay_ms)
    }

    /// Send actual HTTP request to provider
    async fn send_request(
        &self,
        request: LlmRequest,
        provider: &LlmProvider,
        provider_label: &str,
        api_key: &str,
        base_url: &str,
    ) -> Result<LlmResponse> {
        match provider {
            LlmProvider::OpenAi | LlmProvider::Gemini => {
                self.send_openai_request(request, api_key, base_url, provider_label)
                    .await
            }
            LlmProvider::Anthropic => {
                self.send_anthropic_request(request, api_key, base_url)
                    .await
            }
            LlmProvider::Ollama => self.send_ollama_request(request, base_url).await,
        }
    }

    /// OpenRouter rejects `openrouter/openai/...` (400); their IDs are `openai/...`, `anthropic/...`, etc.
    fn openai_compatible_model_id(model: &str, base_url: &str) -> String {
        let m = model.trim();
        if base_url.to_ascii_lowercase().contains("openrouter") {
            m.strip_prefix("openrouter/")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(m)
                .to_string()
        } else {
            m.to_string()
        }
    }

    /// If a cloud-only model id leaks into an Ollama call, swap to a local fallback.
    fn ollama_model_id(model: &str) -> String {
        let m = model.trim();
        let lowered = m.to_ascii_lowercase();
        let looks_cloud = lowered.contains("claude")
            || lowered.contains("gpt-")
            || lowered.contains("gemini")
            || lowered.contains("anthropic/")
            || lowered.contains("openai/")
            || lowered.contains("xai/")
            || lowered.contains("openrouter/");
        if looks_cloud {
            crate::ollama_client::resolve_model_from_env("llama3.2")
        } else {
            m.to_string()
        }
    }

    /// OpenAI API request
    async fn send_openai_request(
        &self,
        request: LlmRequest,
        api_key: &str,
        base_url: &str,
        provider_label: &str,
    ) -> Result<LlmResponse> {
        let url = format!("{}/chat/completions", base_url);
        let model = Self::openai_compatible_model_id(request.model.as_str(), base_url);
        let mut messages = request
            .messages
            .iter()
            .map(|m| {
                json!({
                    "role": m.role,
                    "content": m.content
                })
            })
            .collect::<Vec<_>>();
        if provider_label.eq_ignore_ascii_case("xai") {
            if let Ok(prefill) = std::env::var("HSM_XAI_THINKING_PREFILL") {
                let trimmed = prefill.trim();
                if !trimmed.is_empty() {
                    messages.push(json!({
                        "role": "assistant",
                        "content": trimmed
                    }));
                }
            }
        }

        let body = json!({
            "model": model,
            "messages": messages,
            "temperature": request.temperature,
            "max_tokens": request.max_tokens,
            "top_p": request.top_p,
            "stream": request.stream,
        });

        let mut req = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json");
        if provider_label.eq_ignore_ascii_case("xai")
            && std::env::var("HSM_XAI_PROMPT_CACHE").ok().as_deref() == Some("1")
        {
            req = req.header("x-ai-prompt-cache", "true");
            if let Ok(cache_key) = std::env::var("XAI_PROMPT_CACHE_KEY") {
                let trimmed = cache_key.trim();
                if !trimmed.is_empty() {
                    req = req.header("x-ai-prompt-cache-key", trimmed);
                }
            }
        }
        let response = req.json(&body).send().await?;

        self.handle_response(response, LlmProvider::OpenAi).await
    }

    /// Anthropic API request
    async fn send_anthropic_request(
        &self,
        request: LlmRequest,
        api_key: &str,
        base_url: &str,
    ) -> Result<LlmResponse> {
        let url = format!("{}/messages", base_url);

        // Convert messages to Anthropic format
        let system_msg = request
            .messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone());

        let messages: Vec<_> = request
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                json!({
                    "role": if m.role == "user" { "user" } else { "assistant" },
                    "content": m.content
                })
            })
            .collect();

        // Anthropic does not allow both temperature and top_p simultaneously.
        // Send only temperature (the more commonly used parameter).
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "temperature": request.temperature,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "stream": request.stream,
        });

        if let Some(system) = system_msg {
            body["system"] = json!(system);
        }

        let response = self
            .http
            .post(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        self.handle_response(response, LlmProvider::Anthropic).await
    }

    /// Ollama API request
    async fn send_ollama_request(
        &self,
        request: LlmRequest,
        base_url: &str,
    ) -> Result<LlmResponse> {
        let url = format!("{}/api/chat", base_url);
        let model = Self::ollama_model_id(&request.model);

        let body = json!({
            "model": model,
            "messages": request.messages.iter().map(|m| json!({
                "role": m.role,
                "content": m.content
            })).collect::<Vec<_>>(),
            "options": {
                "temperature": request.temperature,
                "top_p": request.top_p,
            },
            "stream": request.stream,
        });

        let response = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        self.handle_response(response, LlmProvider::Ollama).await
    }

    /// Handle HTTP response
    async fn handle_response(
        &self,
        response: Response,
        provider: LlmProvider,
    ) -> Result<LlmResponse> {
        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::Error::new(LlmHttpError {
                status: status.as_u16(),
                body: error_text,
            }));
        }

        let json: Value = response.json().await?;

        match provider {
            LlmProvider::OpenAi | LlmProvider::Gemini => self.parse_openai_response(json, provider),
            LlmProvider::Anthropic => self.parse_anthropic_response(json),
            LlmProvider::Ollama => self.parse_ollama_response(json),
        }
    }

    fn parse_openai_response(&self, json: Value, provider: LlmProvider) -> Result<LlmResponse> {
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let model = json["model"].as_str().unwrap_or("unknown").to_string();

        let usage = Usage {
            prompt_tokens: json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as usize,
            completion_tokens: json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as usize,
            total_tokens: json["usage"]["total_tokens"].as_u64().unwrap_or(0) as usize,
        };

        Ok(LlmResponse {
            content,
            model,
            usage,
            provider,
            latency_ms: 0,
        })
    }

    fn parse_anthropic_response(&self, json: Value) -> Result<LlmResponse> {
        let content = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let model = json["model"].as_str().unwrap_or("unknown").to_string();

        let usage = Usage {
            prompt_tokens: json["usage"]["input_tokens"].as_u64().unwrap_or(0) as usize,
            completion_tokens: json["usage"]["output_tokens"].as_u64().unwrap_or(0) as usize,
            total_tokens: json["usage"]["input_tokens"].as_u64().unwrap_or(0) as usize
                + json["usage"]["output_tokens"].as_u64().unwrap_or(0) as usize,
        };

        Ok(LlmResponse {
            content,
            model,
            usage,
            provider: LlmProvider::Anthropic,
            latency_ms: 0,
        })
    }

    fn parse_ollama_response(&self, json: Value) -> Result<LlmResponse> {
        let content = json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let model = json["model"].as_str().unwrap_or("unknown").to_string();

        // Ollama doesn't provide token counts in the same format
        let usage = Usage::default();

        Ok(LlmResponse {
            content,
            model,
            usage,
            provider: LlmProvider::Ollama,
            latency_ms: 0,
        })
    }

    /// Simple completion helper
    pub async fn complete(&self, prompt: impl Into<String>) -> Result<String> {
        let request = LlmRequest {
            model: self.default_model.clone(),
            messages: vec![Message::user(prompt)],
            ..Default::default()
        };

        let response = self.chat(request).await?;
        Ok(response.content)
    }

    /// Get current metrics
    pub fn metrics(&self) -> MetricsSnapshot {
        self.metrics.get_stats()
    }

    /// Current breaker state for observability.
    pub fn breaker_state(&self) -> (u32, u64) {
        (
            self.breaker_failures.load(Ordering::SeqCst),
            self.breaker_open_until_ms.load(Ordering::SeqCst),
        )
    }

    /// True when breaker is in cooldown window.
    pub fn breaker_is_open_now(&self) -> bool {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms < self.breaker_open_until_ms.load(Ordering::SeqCst)
    }

    /// Health check all providers
    pub async fn health_check(&self) -> Vec<(LlmProvider, bool, String)> {
        let mut results = Vec::new();

        for slot in &self.providers {
            let result = match &slot.transport {
                LlmProvider::OpenAi => self.check_openai(&slot.api_key, &slot.base_url).await,
                LlmProvider::Gemini => self.check_openai(&slot.api_key, &slot.base_url).await,
                LlmProvider::Anthropic => self.check_anthropic(&slot.api_key, &slot.base_url).await,
                LlmProvider::Ollama => self.check_ollama(&slot.base_url).await,
            };

            results.push((
                slot.transport.clone(),
                result.is_ok(),
                result.unwrap_or_else(|e| format!("{}: {}", slot.label, e)),
            ));
        }

        results
    }

    async fn check_openai(&self, api_key: &str, base_url: &str) -> Result<String> {
        let url = format!("{}/models", base_url);
        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await?;

        if response.status().is_success() {
            Ok("OK".to_string())
        } else {
            Err(anyhow!("HTTP {}", response.status()))
        }
    }

    async fn check_anthropic(&self, api_key: &str, base_url: &str) -> Result<String> {
        let url = format!("{}/models", base_url);
        let response = self
            .http
            .get(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await?;

        if response.status().is_success() {
            Ok("OK".to_string())
        } else {
            Err(anyhow!("HTTP {}", response.status()))
        }
    }

    async fn check_ollama(&self, base_url: &str) -> Result<String> {
        let url = format!("{}/api/tags", base_url);
        let response = self.http.get(&url).send().await?;

        if response.status().is_success() {
            Ok("OK".to_string())
        } else {
            Err(anyhow!("HTTP {}", response.status()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_client() -> LlmClient {
        LlmClient {
            http: Client::new(),
            http_stream: Client::new(),
            providers: Vec::<ProviderSlot>::new(),
            retry_config: RetryConfig::default(),
            default_model: "test".to_string(),
            metrics: Arc::new(LlmMetrics::default()),
            breaker_failures: AtomicU32::new(0),
            breaker_open_until_ms: AtomicU64::new(0),
        }
    }

    #[test]
    fn test_message_creation() {
        let user_msg = Message::user("Hello");
        assert_eq!(user_msg.role, "user");
        assert_eq!(user_msg.content, "Hello");

        let sys_msg = Message::system("You are a helpful assistant");
        assert_eq!(sys_msg.role, "system");
    }

    #[test]
    fn test_retry_backoff() {
        let client = test_client();

        assert_eq!(client.calculate_backoff(1), 1000);
        assert_eq!(client.calculate_backoff(2), 2000);
        assert_eq!(client.calculate_backoff(3), 4000);
        assert_eq!(client.calculate_backoff(10), 30000); // capped at max
    }

    #[test]
    fn test_breaker_state_accessor() {
        let client = test_client();
        client.breaker_failures.store(3, Ordering::SeqCst);
        client.breaker_open_until_ms.store(12345, Ordering::SeqCst);
        let (fails, open_until) = client.breaker_state();
        assert_eq!(fails, 3);
        assert_eq!(open_until, 12345);
    }

    #[test]
    fn test_llm_http_error_in_chain_for_retry_policy() {
        let e = anyhow::Error::new(LlmHttpError {
            status: 429,
            body: "{\"error\":\"quota\"}".to_string(),
        });
        let he = root_http_err(&e).expect("root http");
        assert!(he.is_client_error());
        assert_eq!(he.status, 429);
    }

    #[test]
    fn root_http_err_finds_llm_http_under_context() {
        let inner = anyhow::Error::new(LlmHttpError {
            status: 429,
            body: "{}".into(),
        });
        let e: anyhow::Error = inner.context("outer wrap");
        let he = root_http_err(&e).expect("nested");
        assert!(he.is_client_error());
        assert_eq!(he.status, 429);
    }
}
