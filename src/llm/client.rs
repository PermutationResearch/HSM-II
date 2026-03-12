//! Production LLM Client with Multi-Provider Support
//!
//! Supports OpenAI, Anthropic, and Ollama with automatic failover,
//! retry logic, and comprehensive observability.

use std::sync::Arc;
use std::time::Duration;
use anyhow::{anyhow, Result};
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::sleep;
use tracing::{error, info, instrument, warn};

/// LLM provider types
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LlmProvider {
    OpenAi,
    Anthropic,
    Ollama,
}

impl LlmProvider {
    fn from_env() -> Vec<(Self, String, String)> {
        let mut providers = Vec::new();

        // Check OpenAI
        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            let base_url = std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            providers.push((LlmProvider::OpenAi, api_key, base_url));
        }

        // Check Anthropic
        if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
            let base_url = std::env::var("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com/v1".to_string());
            providers.push((LlmProvider::Anthropic, api_key, base_url));
        }

        // Check Ollama (no API key needed)
        if let Ok(url) = std::env::var("OLLAMA_URL") {
            providers.push((LlmProvider::Ollama, "".to_string(), url));
        } else {
            // Default Ollama
            providers.push((LlmProvider::Ollama, "".to_string(), "http://localhost:11434".to_string()));
        }

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
            model: "gpt-4o-mini".to_string(),
            messages: Vec::new(),
            temperature: 0.7,
            max_tokens: Some(2000),
            top_p: Some(0.9),
            stream: false,
        }
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
#[derive(Clone, Debug, Default)]
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
    providers: Vec<(LlmProvider, String, String)>,
    retry_config: RetryConfig,
    default_model: String,
    metrics: Arc<LlmMetrics>,
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
        self.requests_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if !success {
            self.requests_failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.tokens_total.fetch_add(tokens as u64, std::sync::atomic::Ordering::Relaxed);
        self.latency_ms_sum.fetch_add(latency_ms, std::sync::atomic::Ordering::Relaxed);
    }

    fn get_stats(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_total: self.requests_total.load(std::sync::atomic::Ordering::Relaxed),
            requests_failed: self.requests_failed.load(std::sync::atomic::Ordering::Relaxed),
            tokens_total: self.tokens_total.load(std::sync::atomic::Ordering::Relaxed),
            avg_latency_ms: {
                let total = self.requests_total.load(std::sync::atomic::Ordering::Relaxed);
                let sum = self.latency_ms_sum.load(std::sync::atomic::Ordering::Relaxed);
                if total > 0 { sum / total } else { 0 }
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
    /// Create new LLM client from environment variables
    pub fn new() -> Result<Self> {
        let providers = LlmProvider::from_env();
        
        if providers.is_empty() {
            return Err(anyhow!(
                "No LLM providers configured. Set one of: OPENAI_API_KEY, ANTHROPIC_API_KEY, or OLLAMA_URL"
            ));
        }

        info!("Initialized LLM client with {} provider(s)", providers.len());

        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .pool_max_idle_per_host(10)
            .build()?;

        Ok(Self {
            http,
            providers,
            retry_config: RetryConfig::default(),
            default_model: std::env::var("DEFAULT_LLM_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            metrics: Arc::new(LlmMetrics::default()),
        })
    }

    /// Create with custom retry config
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Send chat completion request with automatic failover
    #[instrument(skip(self, request))]
    pub async fn chat(&self, request: LlmRequest) -> Result<LlmResponse> {
        let start = std::time::Instant::now();
        let mut last_error = None;

        // Try each provider in order
        for (provider, api_key, base_url) in &self.providers {
            match self.try_provider(request.clone(), provider, api_key, base_url).await {
                Ok(response) => {
                    let latency_ms = start.elapsed().as_millis() as u64;
                    let tokens = response.usage.total_tokens;
                    self.metrics.record_request(true, tokens, latency_ms);
                    
                    info!(
                        provider = ?provider,
                        model = %response.model,
                        latency_ms = latency_ms,
                        tokens = tokens,
                        "LLM request succeeded"
                    );
                    
                    return Ok(LlmResponse {
                        latency_ms,
                        ..response
                    });
                }
                Err(e) => {
                    warn!(provider = ?provider, error = %e, "Provider failed, trying next");
                    last_error = Some(e);
                }
            }
        }

        // All providers failed
        let latency_ms = start.elapsed().as_millis() as u64;
        self.metrics.record_request(false, 0, latency_ms);
        
        error!("All LLM providers failed");
        Err(anyhow!(
            "All providers failed. Last error: {}",
            last_error.unwrap_or_else(|| anyhow!("Unknown error"))
        ))
    }

    /// Try a single provider with retry logic
    async fn try_provider(
        &self,
        request: LlmRequest,
        provider: &LlmProvider,
        api_key: &str,
        base_url: &str,
    ) -> Result<LlmResponse> {
        let mut last_error = None;

        for attempt in 0..=self.retry_config.max_retries {
            if attempt > 0 {
                let delay = self.calculate_backoff(attempt);
                warn!(attempt = attempt, delay_ms = delay, "Retrying LLM request");
                sleep(Duration::from_millis(delay)).await;
            }

            match self.send_request(request.clone(), provider, api_key, base_url).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    // Don't retry on 4xx errors (client errors)
                    if let Some(status) = e.downcast_ref::<reqwest::StatusCode>() {
                        if status.is_client_error() {
                            return Err(anyhow!("Client error ({}): {}", status, e));
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
            * self.retry_config.exponential_base.powi(attempt as i32)) as u64;
        delay.min(self.retry_config.max_delay_ms)
    }

    /// Send actual HTTP request to provider
    async fn send_request(
        &self,
        request: LlmRequest,
        provider: &LlmProvider,
        api_key: &str,
        base_url: &str,
    ) -> Result<LlmResponse> {
        match provider {
            LlmProvider::OpenAi => {
                self.send_openai_request(request, api_key, base_url).await
            }
            LlmProvider::Anthropic => {
                self.send_anthropic_request(request, api_key, base_url).await
            }
            LlmProvider::Ollama => {
                self.send_ollama_request(request, base_url).await
            }
        }
    }

    /// OpenAI API request
    async fn send_openai_request(
        &self,
        request: LlmRequest,
        api_key: &str,
        base_url: &str,
    ) -> Result<LlmResponse> {
        let url = format!("{}/chat/completions", base_url);
        
        let body = json!({
            "model": request.model,
            "messages": request.messages.iter().map(|m| json!({
                "role": m.role,
                "content": m.content
            })).collect::<Vec<_>>(),
            "temperature": request.temperature,
            "max_tokens": request.max_tokens,
            "top_p": request.top_p,
            "stream": request.stream,
        });

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

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
        let system_msg = request.messages.iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone());

        let messages: Vec<_> = request.messages.iter()
            .filter(|m| m.role != "system")
            .map(|m| json!({
                "role": if m.role == "user" { "user" } else { "assistant" },
                "content": m.content
            }))
            .collect();

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "temperature": request.temperature,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "top_p": request.top_p,
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

        let body = json!({
            "model": request.model,
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
            return Err(anyhow!("HTTP {}: {}", status, error_text));
        }

        let json: Value = response.json().await?;

        match provider {
            LlmProvider::OpenAi => self.parse_openai_response(json),
            LlmProvider::Anthropic => self.parse_anthropic_response(json),
            LlmProvider::Ollama => self.parse_ollama_response(json),
        }
    }

    fn parse_openai_response(&self, json: Value) -> Result<LlmResponse> {
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
            provider: LlmProvider::OpenAi,
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

    /// Health check all providers
    pub async fn health_check(&self) -> Vec<(LlmProvider, bool, String)> {
        let mut results = Vec::new();

        for (provider, api_key, base_url) in &self.providers {
            let result = match provider {
                LlmProvider::OpenAi => self.check_openai(api_key, base_url).await,
                LlmProvider::Anthropic => self.check_anthropic(api_key, base_url).await,
                LlmProvider::Ollama => self.check_ollama(base_url).await,
            };

            results.push((provider.clone(), result.is_ok(), result.unwrap_or_else(|e| e.to_string())));
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
        let client = LlmClient {
            http: Client::new(),
            providers: Vec::new(),
            retry_config: RetryConfig::default(),
            default_model: "test".to_string(),
            metrics: Arc::new(LlmMetrics::default()),
        };

        assert_eq!(client.calculate_backoff(1), 1000);
        assert_eq!(client.calculate_backoff(2), 2000);
        assert_eq!(client.calculate_backoff(3), 4000);
        assert_eq!(client.calculate_backoff(10), 30000); // capped at max
    }
}
