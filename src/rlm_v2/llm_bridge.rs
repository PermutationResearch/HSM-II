//! LLM Bridge for RLM sub-queries
//!
//! Handles dispatching parallel sub-queries to Ollama/LLM,
//! with caching and result aggregation.

use super::{SubQuery, SubQueryResponse, RlmError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Bridge to LLM for sub-query execution
pub struct LlmBridge {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    cache: QueryCache,
    config: LlmBridgeConfig,
}

/// Configuration for LLM bridge
#[derive(Clone, Debug)]
pub struct LlmBridgeConfig {
    pub endpoint: String,
    pub model: String,
    pub timeout_secs: u64,
    pub temperature: f64,
    pub max_tokens: usize,
    pub enable_cache: bool,
}

impl Default for LlmBridgeConfig {
    fn default() -> Self {
        let endpoint = std::env::var("OLLAMA_HOST")
            .unwrap_or_else(|_| crate::config::network::DEFAULT_OLLAMA_URL.to_string());
        let model = match std::env::var("OLLAMA_MODEL") {
            Ok(m) if !m.is_empty() && m != "auto" => m,
            _ => "qwen2.5:14b".to_string(),
        };
        Self {
            endpoint,
            model,
            timeout_secs: 60,
            temperature: 0.7,
            max_tokens: 2000,
            enable_cache: true,
        }
    }
}

/// A query to the LLM
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmQuery {
    pub id: String,
    pub chunk_content: String,
    pub instruction: String,
    pub context_preview: String,
}

impl LlmQuery {
    /// Build the full prompt for this query
    pub fn build_prompt(&self) -> String {
        format!(
            r#"You are processing a chunk of a larger document.

CONTEXT CHUNK:
{}

INSTRUCTION: {}

Provide a concise answer based only on this chunk. If the chunk doesn't contain relevant information, say "NO_RELEVANT_INFO".

Answer:"#,
            self.chunk_content,
            self.instruction
        )
    }
}

/// Simple cache for query results
#[derive(Clone, Debug)]
pub struct QueryCache {
    entries: HashMap<String, SubQueryResponse>,
    max_size: usize,
}

impl QueryCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_size,
        }
    }
    
    fn compute_key(query: &LlmQuery) -> String {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        
        let mut hasher = DefaultHasher::new();
        query.chunk_content.hash(&mut hasher);
        query.instruction.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
    
    pub fn get(&self, query: &LlmQuery) -> Option<&SubQueryResponse> {
        let key = Self::compute_key(query);
        self.entries.get(&key)
    }
    
    pub fn insert(&mut self, query: &LlmQuery, response: SubQueryResponse) {
        if self.entries.len() >= self.max_size {
            // Remove oldest entry (simple FIFO)
            if let Some(first_key) = self.entries.keys().next().cloned() {
                self.entries.remove(&first_key);
            }
        }
        let key = Self::compute_key(query);
        self.entries.insert(key, response);
    }
    
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl LlmBridge {
    /// Create new LLM bridge
    pub fn new(config: LlmBridgeConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build HTTP client");
        
        Self {
            client,
            endpoint: config.endpoint.clone(),
            model: config.model.clone(),
            cache: QueryCache::new(1000),
            config,
        }
    }
    
    /// Execute a single sub-query
    pub async fn query(&mut self, query: &LlmQuery) -> Result<SubQueryResponse, RlmError> {
        let start = std::time::Instant::now();
        
        // Check cache
        if self.config.enable_cache {
            if let Some(cached) = self.cache.get(query) {
                return Ok(cached.clone());
            }
        }
        
        // Build prompt
        let prompt = query.build_prompt();
        
        // Call Ollama
        let body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": self.config.temperature,
                "num_predict": self.config.max_tokens,
            }
        });
        
        let response = self.client
            .post(format!("{}/api/generate", self.endpoint))
            .json(&body)
            .send()
            .await
            .map_err(|e| RlmError::LlmQueryFailed(format!("HTTP error: {}", e)))?;
        
        if !response.status().is_success() {
            return Err(RlmError::LlmQueryFailed(
                format!("Ollama returned status {}", response.status())
            ));
        }
        
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RlmError::LlmQueryFailed(format!("JSON parse error: {}", e)))?;
        
        let result = json["response"]
            .as_str()
            .unwrap_or("NO_RESPONSE")
            .to_string();
        
        let duration_ms = start.elapsed().as_millis() as u64;
        let tokens_used = json["eval_count"].as_u64().map(|t| t as usize);
        
        let response = SubQueryResponse {
            query_id: query.id.clone(),
            result,
            tokens_used,
            duration_ms,
        };
        
        // Cache result
        if self.config.enable_cache {
            self.cache.insert(query, response.clone());
        }
        
        Ok(response)
    }
    
    /// Execute multiple sub-queries in parallel
    pub async fn query_parallel(&mut self, queries: Vec<LlmQuery>) -> Vec<Result<SubQueryResponse, RlmError>> {
        let mut handles = Vec::new();
        
        for query in queries {
            // Clone necessary data for the async block
            let client = self.client.clone();
            let endpoint = self.endpoint.clone();
            let model = self.model.clone();
            let temperature = self.config.temperature;
            let max_tokens = self.config.max_tokens;
            
            let handle = tokio::spawn(async move {
                let start = std::time::Instant::now();
                let prompt = query.build_prompt();
                
                let body = serde_json::json!({
                    "model": model,
                    "prompt": prompt,
                    "stream": false,
                    "options": {
                        "temperature": temperature,
                        "num_predict": max_tokens,
                    }
                });
                
                let response = client
                    .post(format!("{}/api/generate", endpoint))
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| RlmError::LlmQueryFailed(format!("HTTP error: {}", e)))?;
                
                if !response.status().is_success() {
                    return Err(RlmError::LlmQueryFailed(
                        format!("Ollama returned status {}", response.status())
                    ));
                }
                
                let json: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| RlmError::LlmQueryFailed(format!("JSON parse error: {}", e)))?;
                
                let result = json["response"]
                    .as_str()
                    .unwrap_or("NO_RESPONSE")
                    .to_string();
                
                let duration_ms = start.elapsed().as_millis() as u64;
                let tokens_used = json["eval_count"].as_u64().map(|t| t as usize);
                
                Ok(SubQueryResponse {
                    query_id: query.id.clone(),
                    result,
                    tokens_used,
                    duration_ms,
                })
            });
            
            handles.push(handle);
        }
        
        // Collect results
        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(RlmError::LlmQueryFailed(format!("Task panicked: {}", e)))),
            }
        }
        
        results
    }
    
    /// Convert RLM SubQuery to LlmQuery
    pub fn to_llm_query(sub_query: &SubQuery, chunk_content: &str) -> LlmQuery {
        LlmQuery {
            id: sub_query.id.clone(),
            chunk_content: chunk_content.to_string(),
            instruction: sub_query.instruction.clone(),
            context_preview: sub_query.context_preview.clone(),
        }
    }
    
    /// Get cache stats
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.cache.len(), self.cache.entries.capacity())
    }
    
    /// Clear cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
    
    /// Execute a direct prompt (for main RLM iterations, not sub-queries)
    pub async fn generate_direct(&mut self, prompt: &str) -> Result<String, RlmError> {
        let body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": self.config.temperature,
                "num_predict": self.config.max_tokens,
            }
        });
        
        let response = self.client
            .post(format!("{}/api/generate", self.endpoint))
            .json(&body)
            .send()
            .await
            .map_err(|e| RlmError::LlmQueryFailed(format!("HTTP error: {}", e)))?;
        
        if !response.status().is_success() {
            return Err(RlmError::LlmQueryFailed(
                format!("Ollama returned status {}", response.status())
            ));
        }
        
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| RlmError::LlmQueryFailed(format!("JSON parse error: {}", e)))?;
        
        let result = json["response"]
            .as_str()
            .unwrap_or("NO_RESPONSE")
            .to_string();
        
        Ok(result)
    }
    
    /// Get current model
    pub fn model(&self) -> &str {
        &self.model
    }
    
    /// Set model
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }
}

/// Aggregate sub-query results into a coherent response
pub fn aggregate_results(results: &[SubQueryResponse]) -> String {
    let mut aggregated = String::new();
    
    for result in results {
        // Skip results that indicate no relevant info
        if result.result.contains("NO_RELEVANT_INFO") || result.result.contains("NO_RELEVANT") {
            continue;
        }
        
        aggregated.push_str(&format!("[Chunk {}]\n{}\n\n", result.query_id, result.result));
    }
    
    if aggregated.is_empty() {
        "No relevant information found in any chunks.".to_string()
    } else {
        aggregated.trim().to_string()
    }
}

/// Check if Ollama is available
pub async fn check_ollama_available(endpoint: &str) -> bool {
    match reqwest::Client::new()
        .get(format!("{}/api/tags", endpoint))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_query_cache() {
        let mut cache = QueryCache::new(10);
        
        let query = LlmQuery {
            id: "test".to_string(),
            chunk_content: "test content".to_string(),
            instruction: "summarize".to_string(),
            context_preview: "preview".to_string(),
        };
        
        let response = SubQueryResponse {
            query_id: "test".to_string(),
            result: "result".to_string(),
            tokens_used: Some(100),
            duration_ms: 1000,
        };
        
        assert!(cache.get(&query).is_none());
        cache.insert(&query, response.clone());
        assert!(cache.get(&query).is_some());
    }
    
    #[test]
    fn test_aggregate_results() {
        let results = vec![
            SubQueryResponse {
                query_id: "1".to_string(),
                result: "Found A".to_string(),
                tokens_used: None,
                duration_ms: 100,
            },
            SubQueryResponse {
                query_id: "2".to_string(),
                result: "NO_RELEVANT_INFO".to_string(),
                tokens_used: None,
                duration_ms: 100,
            },
            SubQueryResponse {
                query_id: "3".to_string(),
                result: "Found B".to_string(),
                tokens_used: None,
                duration_ms: 100,
            },
        ];
        
        let aggregated = aggregate_results(&results);
        assert!(aggregated.contains("Found A"));
        assert!(aggregated.contains("Found B"));
        assert!(!aggregated.contains("NO_RELEVANT_INFO"));
    }
}
