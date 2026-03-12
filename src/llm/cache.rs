//! KV cache management for efficient generation.

/// Key-Value cache for transformer layers
pub struct KvCache {
    /// Cached keys per layer [layer, batch, head, seq, dim]
    k_cache: Vec<Vec<f32>>,
    /// Cached values per layer [layer, batch, head, seq, dim]
    v_cache: Vec<Vec<f32>>,
    max_seq_len: usize,
    current_len: usize,
}

impl KvCache {
    pub fn new(n_layers: usize, max_seq_len: usize, head_dim: usize) -> Self {
        let k_cache = vec![Vec::with_capacity(max_seq_len * head_dim); n_layers];
        let v_cache = vec![Vec::with_capacity(max_seq_len * head_dim); n_layers];

        Self {
            k_cache,
            v_cache,
            max_seq_len,
            current_len: 0,
        }
    }

    /// Append new key-value pairs to cache
    pub fn append(&mut self, layer: usize, k: &[f32], v: &[f32]) {
        if layer < self.k_cache.len() && self.current_len < self.max_seq_len {
            self.k_cache[layer].extend_from_slice(k);
            self.v_cache[layer].extend_from_slice(v);
        }
    }

    /// Get cached keys for a layer
    pub fn get_keys(&self, layer: usize) -> Option<&[f32]> {
        self.k_cache.get(layer).map(|v| v.as_slice())
    }

    /// Get cached values for a layer
    pub fn get_values(&self, layer: usize) -> Option<&[f32]> {
        self.v_cache.get(layer).map(|v| v.as_slice())
    }

    /// Get current sequence length
    pub fn current_len(&self) -> usize {
        self.current_len
    }

    /// Clear cache
    pub fn clear(&mut self) {
        for k in &mut self.k_cache {
            k.clear();
        }
        for v in &mut self.v_cache {
            v.clear();
        }
        self.current_len = 0;
    }

    /// Trim to specific length
    pub fn trim(&mut self, len: usize) {
        let trim_len = len.min(self.max_seq_len);
        for k in &mut self.k_cache {
            k.truncate(trim_len);
        }
        for v in &mut self.v_cache {
            v.truncate(trim_len);
        }
        self.current_len = trim_len;
    }
}

/// Cache manager for multiple sequences
pub struct CacheManager {
    caches: std::collections::HashMap<String, KvCache>,
    max_layers: usize,
    max_seq_len: usize,
    head_dim: usize,
}

impl CacheManager {
    pub fn new(max_layers: usize) -> Self {
        Self {
            caches: std::collections::HashMap::new(),
            max_layers,
            max_seq_len: 2048,
            head_dim: 128,
        }
    }

    /// Get or create cache for a sequence
    pub fn get_cache(&mut self, sequence_id: &str) -> &mut KvCache {
        self.caches
            .entry(sequence_id.to_string())
            .or_insert_with(|| KvCache::new(self.max_layers, self.max_seq_len, self.head_dim))
    }

    /// Remove cache for a sequence
    pub fn remove_cache(&mut self, sequence_id: &str) {
        self.caches.remove(sequence_id);
    }

    /// Clear all caches
    pub fn clear(&mut self) {
        self.caches.clear();
    }

    /// Get memory usage in bytes
    pub fn memory_usage(&self) -> usize {
        let mut total = 0;
        for cache in self.caches.values() {
            for k in &cache.k_cache {
                total += k.len() * std::mem::size_of::<f32>();
            }
            for v in &cache.v_cache {
                total += v.len() * std::mem::size_of::<f32>();
            }
        }
        total
    }

    /// Evict oldest caches if memory limit exceeded
    pub fn evict_if_needed(&mut self, max_memory_mb: usize) {
        let max_bytes = max_memory_mb * 1024 * 1024;

        while self.memory_usage() > max_bytes && !self.caches.is_empty() {
            // Remove oldest (first) entry
            let oldest = self.caches.keys().next().cloned();
            if let Some(key) = oldest {
                self.caches.remove(&key);
            }
        }
    }
}

/// Sliding window cache for long sequences
pub struct SlidingWindowCache {
    window_size: usize,
    k_buffer: Vec<f32>,
    v_buffer: Vec<f32>,
}

impl SlidingWindowCache {
    pub fn new(window_size: usize, head_dim: usize) -> Self {
        Self {
            window_size,
            k_buffer: Vec::with_capacity(window_size * head_dim),
            v_buffer: Vec::with_capacity(window_size * head_dim),
        }
    }

    pub fn append(&mut self, k: &[f32], v: &[f32], head_dim: usize) {
        self.k_buffer.extend_from_slice(k);
        self.v_buffer.extend_from_slice(v);

        // Trim to window size
        if self.k_buffer.len() > self.window_size * head_dim {
            let excess = self.k_buffer.len() - self.window_size * head_dim;
            self.k_buffer.drain(0..excess);
            self.v_buffer.drain(0..excess);
        }
    }

    pub fn get(&self) -> (&[f32], &[f32]) {
        (&self.k_buffer, &self.v_buffer)
    }
}
