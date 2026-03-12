//! Ollama LLM Client with latency budgeting and request batching.
//!
//! This module provides:
//! - Actual Ollama API calls with configurable timeout
//! - Request batching for efficiency
//! - Latency budget enforcement with fallback

use ollama_rs::generation::chat::{ChatMessage, MessageRole};
use ollama_rs::generation::chat::request::ChatMessageRequest;
use ollama_rs::generation::completion::request::GenerationRequest;
use ollama_rs::Ollama;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{timeout, Duration, Instant};

/// Configuration for Ollama client
#[derive(Clone, Debug)]
pub struct OllamaConfig {
    /// Host address for Ollama server
    pub host: String,
    /// Port for Ollama server
    pub port: u16,
    /// Model to use
    pub model: String,
    /// Latency budget per request in milliseconds
    pub latency_budget_ms: u64,
    /// Whether to use request batching
    pub enable_batching: bool,
    /// Batch size for batched requests
    pub batch_size: usize,
    /// Batch timeout in milliseconds (max time to wait for batch to fill)
    pub batch_timeout_ms: u64,
    /// Temperature for generation
    pub temperature: f64,
    /// Maximum tokens to generate
    pub max_tokens: u32,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            host: std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost".to_string()),
            port: std::env::var("OLLAMA_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(11434),
            model: std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "auto".to_string()),
            latency_budget_ms: 60000,
            enable_batching: false,
            batch_size: 4,
            batch_timeout_ms: 100,
            temperature: 0.7,
            max_tokens: 1024,
        }
    }
}

impl OllamaConfig {
    /// Detect the best available model from Ollama.
    /// Preference order: env OLLAMA_MODEL > largest installed model > "llama3.2" fallback.
    pub async fn detect_model(host: &str, port: u16) -> String {
        // If user explicitly set a model, use it
        if let Ok(model) = std::env::var("OLLAMA_MODEL") {
            if model != "auto" {
                return model;
            }
        }

        // Query Ollama for installed models
        let url = format!("{}:{}/api/tags", host, port);
        match reqwest::get(&url).await {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(models) = json.get("models").and_then(|m| m.as_array()) {
                        if !models.is_empty() {
                            // Pick the first available model
                            if let Some(name) = models[0].get("name").and_then(|n| n.as_str()) {
                                eprintln!("[HSM-II] Auto-detected Ollama model: {}", name);
                                return name.to_string();
                            }
                        }
                    }
                }
                eprintln!("[HSM-II] No models found in Ollama. Run: ollama pull llama3.2");
                "llama3.2".to_string()
            }
            Err(_) => {
                eprintln!("[HSM-II] Cannot reach Ollama at {}:{}. Is it running?", host, port);
                "llama3.2".to_string()
            }
        }
    }
}

/// Result of an LLM call with metadata
#[derive(Clone, Debug)]
pub struct LlmResult {
    pub text: String,
    pub latency_ms: u64,
    pub tokens_generated: usize,
    pub cached: bool,
    pub timed_out: bool,
}

/// Ollama client with latency budgeting
///
/// Uses Arc<Mutex<_>> wrapper to ensure Send + Sync for use across threads
pub struct OllamaClient {
    ollama: Arc<Mutex<Ollama>>,
    config: OllamaConfig,
    /// Latency history for adaptive budgeting
    latency_history: Arc<RwLock<Vec<u64>>>,
}

/// Request for batching
struct BatchedRequest {
    prompt: String,
    response_tx: oneshot::Sender<LlmResult>,
}

use tokio::sync::oneshot;

impl OllamaClient {
    /// Create a new Ollama client
    pub fn new(config: OllamaConfig) -> Self {
        let ollama = Ollama::new(&config.host, config.port);

        Self {
            ollama: Arc::new(Mutex::new(ollama)),
            config,
            latency_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get the current model name
    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// Generate text with latency budget enforcement
    pub async fn generate(&self, prompt: &str) -> LlmResult {
        let start = Instant::now();

        // Create generation request
        let request = GenerationRequest::new(self.config.model.clone(), prompt.to_string())
            .options(
                ollama_rs::generation::options::GenerationOptions::default()
                    .temperature(self.config.temperature as f32)
                    .num_predict(self.config.max_tokens as i32),
            );

        // No timeout - let LLM take as long as it needs
        let ollama = self.ollama.clone();

        let result = match async move {
            let ollama = ollama.lock().await;
            ollama.generate(request).await
        }
        .await
        {
            Ok(response) => {
                let latency = start.elapsed().as_millis() as u64;

                // Record latency for adaptive budgeting
                self.record_latency(latency).await;

                LlmResult {
                    text: response.response,
                    latency_ms: latency,
                    tokens_generated: response.eval_count.unwrap_or(0) as usize,
                    cached: false,
                    timed_out: false,
                }
            }
            Err(e) => {
                eprintln!("Ollama error: {}", e);
                self.fallback_result("Error calling LLM")
            }
        };

        result
    }

    /// Chat with proper system/user message roles (uses /api/chat endpoint)
    /// `history` contains prior (user, assistant) message pairs for conversation continuity.
    pub async fn chat(&self, system_prompt: &str, user_message: &str, history: &[(String, String)]) -> LlmResult {
        let start = Instant::now();

        let mut messages = vec![
            ChatMessage::new(MessageRole::System, system_prompt.to_string()),
        ];

        // Add conversation history
        for (user_msg, assistant_msg) in history {
            messages.push(ChatMessage::new(MessageRole::User, user_msg.clone()));
            messages.push(ChatMessage::new(MessageRole::Assistant, assistant_msg.clone()));
        }

        // Add current user message
        messages.push(ChatMessage::new(MessageRole::User, user_message.to_string()));

        let request = ChatMessageRequest::new(self.config.model.clone(), messages);

        // No timeout - let LLM take as long as it needs
        let ollama = self.ollama.clone();

        let result = match async move {
            let ollama = ollama.lock().await;
            ollama.send_chat_messages(request).await
        }
        .await
        {
            Ok(response) => {
                let latency = start.elapsed().as_millis() as u64;
                self.record_latency(latency).await;

                LlmResult {
                    text: response.message.content,
                    latency_ms: latency,
                    tokens_generated: response.final_data.as_ref().map(|d| d.eval_count as usize).unwrap_or(0),
                    cached: false,
                    timed_out: false,
                }
            }
            Err(e) => {
                eprintln!("Ollama chat error: {}", e);
                self.fallback_result("Error calling LLM")
            }
        };

        result
    }

    /// Generate with fallback on timeout
    pub async fn generate_with_fallback(
        &self,
        prompt: &str,
        fallback_fn: impl FnOnce() -> String,
    ) -> LlmResult {
        let result = self.generate(prompt).await;

        if result.timed_out || result.text.is_empty() {
            LlmResult {
                text: fallback_fn(),
                latency_ms: result.latency_ms,
                tokens_generated: 0,
                cached: false,
                timed_out: true,
            }
        } else {
            result
        }
    }

    /// Check if Ollama server is available
    pub async fn check_available(&self) -> bool {
        self.is_available().await
    }

    /// Check if Ollama server is available
    pub async fn is_available(&self) -> bool {
        let ollama = self.ollama.clone();
        match timeout(Duration::from_secs(2), async move {
            let ollama = ollama.lock().await;
            ollama.list_local_models().await
        })
        .await
        {
            Ok(Ok(_)) => true,
            _ => false,
        }
    }

    /// Get average latency from history
    pub async fn average_latency(&self) -> f64 {
        let history = self.latency_history.read().await;
        if history.is_empty() {
            0.0
        } else {
            history.iter().sum::<u64>() as f64 / history.len() as f64
        }
    }

    /// Adjust latency budget based on observed latencies
    pub async fn adjust_budget(&mut self, percentile: f64) {
        let history = self.latency_history.read().await;
        if history.len() < 10 {
            return;
        }

        let mut sorted = history.clone();
        sorted.sort_unstable();

        let idx = ((percentile / 100.0) * sorted.len() as f64) as usize;
        let new_budget = sorted[idx.min(sorted.len() - 1)];

        // Add 20% headroom
        self.config.latency_budget_ms = (new_budget as f64 * 1.2) as u64;
    }

    async fn record_latency(&self, latency: u64) {
        let mut history = self.latency_history.write().await;
        history.push(latency);
        // Keep last 100 measurements
        if history.len() > 100 {
            history.remove(0);
        }
    }

    fn fallback_result(&self, message: &str) -> LlmResult {
        LlmResult {
            text: format!("[FALLBACK: {}]", message),
            latency_ms: 0,
            tokens_generated: 0,
            cached: false,
            timed_out: true,
        }
    }
}

/// Batched Ollama client for efficient request processing
pub struct BatchedOllamaClient {
    inner: Arc<OllamaClient>,
    config: OllamaConfig,
    /// Request queue for batching
    request_queue: Arc<Mutex<Vec<BatchedRequest>>>,
    /// Background batch processing task
    _batch_task: tokio::task::JoinHandle<()>,
}

impl BatchedOllamaClient {
    /// Create a new batched client with background processing
    pub async fn new(config: OllamaConfig) -> Self {
        let inner = Arc::new(OllamaClient::new(config.clone()));
        let request_queue = Arc::new(Mutex::new(Vec::new()));

        // Start background batch processor
        let queue_clone = request_queue.clone();
        let inner_clone = inner.clone();
        let batch_size = config.batch_size;
        let batch_timeout = Duration::from_millis(config.batch_timeout_ms);

        let _batch_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(batch_timeout);

            loop {
                interval.tick().await;

                let batch: Vec<BatchedRequest> = {
                    let mut queue = queue_clone.lock().await;
                    if queue.len() >= batch_size {
                        queue.drain(..batch_size).collect()
                    } else if !queue.is_empty() {
                        // Process partial batch on timeout
                        queue.drain(..).collect()
                    } else {
                        continue;
                    }
                };

                // Process batch concurrently
                for request in batch {
                    let inner = inner_clone.clone();
                    tokio::spawn(async move {
                        let result = inner.generate(&request.prompt).await;
                        let _ = request.response_tx.send(result);
                    });
                }
            }
        });

        Self {
            inner,
            config,
            request_queue,
            _batch_task,
        }
    }

    /// Submit a request to the batch queue
    pub async fn generate(&self, prompt: &str) -> LlmResult {
        if !self.config.enable_batching {
            // Direct call if batching disabled
            return self.inner.generate(prompt).await;
        }

        let (tx, rx) = oneshot::channel();
        let request = BatchedRequest {
            prompt: prompt.to_string(),
            response_tx: tx,
        };

        {
            let mut queue = self.request_queue.lock().await;
            queue.push(request);
        }

        // Wait for response with timeout
        match timeout(Duration::from_millis(self.config.latency_budget_ms * 2), rx).await {
            Ok(Ok(result)) => result,
            _ => self.inner.fallback_result("Batch processing timeout"),
        }
    }

    /// Check if underlying client is available
    pub async fn is_available(&self) -> bool {
        self.inner.is_available().await
    }
}

/// Simple council decision prompt builder
pub struct CouncilPromptBuilder;

impl CouncilPromptBuilder {
    /// Build prompt for council mode selection
    pub fn mode_selection_prompt(complexity: f64, urgency: f64, proposal_title: &str) -> String {
        format!(
            "You are a council routing system. Given a proposal with:\n\
             - Complexity: {:.2} (0=simple, 1=complex)\n\
             - Urgency: {:.2} (0=low, 1=high)\n\
             - Title: {}\n\n\
             Select the best council mode:\n\
             - Simple: For routine, low-complexity proposals (fast, efficient)\n\
             - Orchestrate: For urgent proposals requiring coordination\n\
             - LLM: For complex proposals requiring deep reasoning\n\n\
             Respond with ONLY one word: Simple, Orchestrate, or LLM",
            complexity, urgency, proposal_title
        )
    }

    /// Build prompt for council decision
    pub fn decision_prompt(
        mode: &str,
        complexity: f64,
        urgency: f64,
        proposal_title: &str,
        proposal_desc: &str,
    ) -> String {
        format!(
            "You are a council member in {} mode.\n\n\
             PROPOSAL: {}\n\
             Description: {}\n\
             Complexity: {:.2}\n\
             Urgency: {:.2}\n\n\
             Based on the mode's strengths and the proposal characteristics,\n\
             make a decision: Approve, Reject, or Defer\n\n\
             Respond with ONLY one word: Approve, Reject, or Defer",
            mode, proposal_title, proposal_desc, complexity, urgency
        )
    }

    /// Parse mode from LLM response
    pub fn parse_mode(response: &str) -> &str {
        let normalized = response.trim().to_lowercase();
        if normalized.contains("orchestrate") {
            "Orchestrate"
        } else if normalized.contains("llm") || normalized.contains("complex") {
            "LLM"
        } else {
            "Simple"
        }
    }

    /// Parse decision from LLM response
    pub fn parse_decision(response: &str) -> &str {
        let normalized = response.trim().to_lowercase();
        if normalized.contains("approve") {
            "Approve"
        } else if normalized.contains("reject") {
            "Reject"
        } else {
            "Defer"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode() {
        assert_eq!(CouncilPromptBuilder::parse_mode("Simple"), "Simple");
        assert_eq!(
            CouncilPromptBuilder::parse_mode("Orchestrate"),
            "Orchestrate"
        );
        assert_eq!(CouncilPromptBuilder::parse_mode("LLM mode"), "LLM");
        assert_eq!(CouncilPromptBuilder::parse_mode("unknown"), "Simple");
    }

    #[test]
    fn test_parse_decision() {
        assert_eq!(CouncilPromptBuilder::parse_decision("Approve"), "Approve");
        assert_eq!(CouncilPromptBuilder::parse_decision("Reject"), "Reject");
        assert_eq!(CouncilPromptBuilder::parse_decision("Defer"), "Defer");
        assert_eq!(CouncilPromptBuilder::parse_decision("unknown"), "Defer");
    }
}
