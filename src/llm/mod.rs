//! Production LLM Integration
//!
//! Multi-provider LLM client with failover, retry logic, and observability.
//! Supports OpenAI, Anthropic, and Ollama.

use std::collections::VecDeque;

pub mod cache;
pub mod client;
pub mod engine;
pub mod model;
pub mod tokenizer;

pub use cache::{CacheManager, KvCache};
pub use client::{LlmClient, LlmRequest, LlmResponse, Message, Usage, RetryConfig, MetricsSnapshot, LlmProvider};
pub use engine::{GenerationParams, InferenceConfig, LlmEngine};
pub use model::{ModelLoader, ModelType, Quantization};
pub use tokenizer::{EncodingOptions, TokenEncoder};

/// FrankenTorch-style hybrid inference engine
///
/// Combines:
/// - Candle for pure Rust inference (no Python dependencies)
/// - Optional PyTorch bridge for complex models
/// - Quantization for memory efficiency
/// - KV caching for generation speed
pub struct FrankenTorch {
    engine: LlmEngine,
    cache_manager: CacheManager,
    _config: FrankenConfig,
}

/// Configuration for FrankenTorch
#[derive(Clone, Debug)]
pub struct FrankenConfig {
    /// Model to use
    pub model_id: String,
    /// Quantization level
    pub quantization: Quantization,
    /// Context window size
    pub context_size: usize,
    /// Use GPU if available
    pub use_gpu: bool,
    /// Number of layers to keep in memory
    pub cache_layers: usize,
    /// Thread pool size for inference
    pub threads: usize,
}

impl Default for FrankenConfig {
    fn default() -> Self {
        Self {
            model_id: "microsoft/phi-2".to_string(),
            quantization: Quantization::Q4_0,
            context_size: 2048,
            use_gpu: true,
            cache_layers: 32,
            threads: 4,
        }
    }
}

impl FrankenTorch {
    /// Create new FrankenTorch instance
    pub async fn new(config: FrankenConfig) -> anyhow::Result<Self> {
        let engine = LlmEngine::load(&config.model_id, config.clone()).await?;
        let cache_manager = CacheManager::new(config.cache_layers);

        Ok(Self {
            engine,
            cache_manager,
            _config: config,
        })
    }

    /// Generate text completion
    pub async fn generate(&mut self, prompt: &str, max_tokens: usize) -> anyhow::Result<String> {
        let params = GenerationParams {
            max_tokens,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            repetition_penalty: 1.1,
        };

        self.engine
            .generate(prompt, params, &mut self.cache_manager)
            .await
    }

    /// Generate with custom parameters
    pub async fn generate_with_params(
        &mut self,
        prompt: &str,
        params: GenerationParams,
    ) -> anyhow::Result<String> {
        self.engine
            .generate(prompt, params, &mut self.cache_manager)
            .await
    }

    /// Get embeddings for text
    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        self.engine.embed(text).await
    }

    /// Batch embed multiple texts
    pub async fn embed_batch(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let mut embeddings = Vec::with_capacity(texts.len());
        for text in texts {
            embeddings.push(self.embed(text).await?);
        }
        Ok(embeddings)
    }

    /// Analyze code and return structured output
    pub async fn analyze_code(
        &mut self,
        code: &str,
        language: &str,
    ) -> anyhow::Result<CodeAnalysis> {
        let prompt = format!(
            "Analyze the following {} code and provide a structured analysis:\n\n{}\n\nAnalysis:",
            language, code
        );

        let response = self.generate(&prompt, 500).await?;

        Ok(CodeAnalysis {
            summary: response.lines().next().unwrap_or("").to_string(),
            complexity: self.estimate_complexity(code),
            suggestions: response.lines().skip(1).map(|s| s.to_string()).collect(),
        })
    }

    /// Generate skill from experience
    pub async fn distill_skill(
        &mut self,
        experience: &str,
        outcome: &str,
    ) -> anyhow::Result<DistilledSkill> {
        let prompt = format!(
            "Based on this experience:\n{}\n\nWith outcome: {}\n\nDistill a reusable skill (title, principle, when_to_apply):",
            experience, outcome
        );

        let response = self.generate(&prompt, 300).await?;

        // Parse response into structured skill
        let lines: Vec<&str> = response.lines().collect();
        Ok(DistilledSkill {
            title: lines.get(0).unwrap_or(&"").to_string(),
            principle: lines.get(1).unwrap_or(&"").to_string(),
            when_to_apply: lines.get(2).unwrap_or(&"").to_string(),
        })
    }

    /// Clear KV cache
    pub fn clear_cache(&mut self) {
        self.cache_manager.clear();
    }

    fn estimate_complexity(&self, code: &str) -> f64 {
        // Simple heuristic: lines of code + nesting depth
        let lines = code.lines().count();
        let nesting = code.matches('{').count();
        ((lines as f64) * 0.1 + (nesting as f64) * 0.05).min(1.0)
    }
}

/// Code analysis result
#[derive(Clone, Debug)]
pub struct CodeAnalysis {
    pub summary: String,
    pub complexity: f64,
    pub suggestions: Vec<String>,
}

/// Distilled skill from experience
#[derive(Clone, Debug)]
pub struct DistilledSkill {
    pub title: String,
    pub principle: String,
    pub when_to_apply: String,
}

/// Model serving for multi-agent system
pub struct ModelServer {
    models: Vec<FrankenTorch>,
    request_queue: VecDeque<InferenceRequest>,
}

#[derive(Clone, Debug)]
pub struct InferenceRequest {
    pub id: String,
    pub prompt: String,
    pub params: GenerationParams,
    pub priority: RequestPriority,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestPriority {
    Critical = 0,
    High = 1,
    Normal = 2,
    Low = 3,
}

impl ModelServer {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            request_queue: VecDeque::new(),
        }
    }

    pub async fn add_model(&mut self, config: FrankenConfig) -> anyhow::Result<()> {
        let model = FrankenTorch::new(config).await?;
        self.models.push(model);
        Ok(())
    }

    pub fn submit_request(&mut self, request: InferenceRequest) -> String {
        let id = request.id.clone();
        self.request_queue.push_back(request);
        id
    }

    pub async fn process_queue(&mut self) {
        // Sort by priority
        let mut requests: Vec<_> = self.request_queue.drain(..).collect();
        requests.sort_by_key(|r| r.priority);

        // Process with available models
        for request in requests {
            if let Some(model) = self.models.first_mut() {
                let _ = model
                    .generate_with_params(&request.prompt, request.params)
                    .await;
            }
        }
    }
}
