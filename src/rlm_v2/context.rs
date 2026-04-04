//! Context management for RLM
//!
//! Handles loading, chunking, and metadata extraction from large contexts.
//! Supports files, directories, URLs, and raw text.

use super::RlmError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Context loaded for RLM processing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Context {
    /// Raw content
    pub content: String,
    /// Metadata about the context
    pub metadata: ContextMetadata,
    /// Pre-computed chunks for parallel processing
    pub chunks: Vec<ContextChunk>,
    /// Additional metadata extracted during loading
    pub extracted_meta: HashMap<String, String>,
}

/// Metadata about loaded context
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ContextMetadata {
    pub source: String,
    pub content_type: ContentType,
    pub total_bytes: usize,
    pub total_lines: usize,
    pub line_count: usize,
    pub preview_lines: usize,
}

/// Type of content loaded
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    File,
    Directory,
    Url,
    Text,
    Multiple,
}

impl Default for ContentType {
    fn default() -> Self {
        ContentType::Text
    }
}

/// A chunk of context for sub-query processing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextChunk {
    pub index: usize,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub byte_range: (usize, usize),
    pub metadata: ChunkMetadata,
}

/// Metadata for a context chunk
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Semantic topic/title if extracted
    pub topic: Option<String>,
    /// Entities mentioned in chunk
    pub entities: Vec<String>,
    /// Whether chunk contains code
    pub is_code: bool,
    /// Programming language if detected
    pub language: Option<String>,
}

impl Context {
    /// Load context from a file
    pub async fn from_file(path: impl AsRef<Path>) -> Result<Self, RlmError> {
        let path = path.as_ref();
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| RlmError::StorageError(format!("Failed to read file: {}", e)))?;

        let metadata = ContextMetadata {
            source: path.to_string_lossy().to_string(),
            content_type: ContentType::File,
            total_bytes: content.len(),
            total_lines: content.lines().count(),
            line_count: content.lines().count(),
            preview_lines: content.lines().take(20).count(),
        };

        let mut context = Self {
            content,
            metadata,
            chunks: Vec::new(),
            extracted_meta: HashMap::new(),
        };

        context.compute_chunks(1000); // Default 1000 lines per chunk
        Ok(context)
    }

    /// Load context from raw text
    pub fn from_text(text: impl Into<String>, source: impl Into<String>) -> Self {
        let content = text.into();
        let metadata = ContextMetadata {
            source: source.into(),
            content_type: ContentType::Text,
            total_bytes: content.len(),
            total_lines: content.lines().count(),
            line_count: content.lines().count(),
            preview_lines: content.lines().take(20).count(),
        };

        let mut context = Self {
            content,
            metadata,
            chunks: Vec::new(),
            extracted_meta: HashMap::new(),
        };

        context.compute_chunks(1000);
        context
    }

    /// Load context from multiple files matching a glob pattern
    pub async fn from_glob(pattern: &str) -> Result<Self, RlmError> {
        use glob::glob;

        let mut combined = String::new();
        let mut file_count = 0;
        let mut total_bytes = 0;

        for entry in glob(pattern).map_err(|e| RlmError::StorageError(e.to_string()))? {
            if let Ok(path) = entry {
                if path.is_file() {
                    match tokio::fs::read_to_string(&path).await {
                        Ok(content) => {
                            combined.push_str(&format!("\n\n=== {} ===\n\n", path.display()));
                            combined.push_str(&content);
                            file_count += 1;
                            total_bytes += content.len();
                        }
                        Err(_) => continue, // Skip binary files
                    }
                }
            }
        }

        let metadata = ContextMetadata {
            source: format!("glob:{}", pattern),
            content_type: ContentType::Multiple,
            total_bytes,
            total_lines: combined.lines().count(),
            line_count: combined.lines().count(),
            preview_lines: combined.lines().take(20).count(),
        };

        let mut extracted_meta = HashMap::new();
        extracted_meta.insert("file_count".to_string(), file_count.to_string());

        let mut context = Self {
            content: combined,
            metadata,
            chunks: Vec::new(),
            extracted_meta,
        };

        context.compute_chunks(1000);
        Ok(context)
    }

    /// Load context from a directory (recursive)
    pub async fn from_directory(dir: impl AsRef<Path>) -> Result<Self, RlmError> {
        let dir = dir.as_ref();
        let mut combined = String::new();
        let mut file_count = 0;
        let mut total_bytes = 0;

        let mut entries = tokio::fs::read_dir(dir)
            .await
            .map_err(|e| RlmError::StorageError(format!("Failed to read directory: {}", e)))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_file() {
                // Skip common non-text files
                if let Some(ext) = path.extension() {
                    let ext = ext.to_string_lossy().to_lowercase();
                    if matches!(
                        ext.as_str(),
                        "exe"
                            | "dll"
                            | "so"
                            | "dylib"
                            | "bin"
                            | "o"
                            | "a"
                            | "png"
                            | "jpg"
                            | "jpeg"
                            | "gif"
                            | "pdf"
                            | "zip"
                            | "tar"
                            | "gz"
                    ) {
                        continue;
                    }
                }

                match tokio::fs::read_to_string(&path).await {
                    Ok(content) => {
                        combined.push_str(&format!("\n\n=== {} ===\n\n", path.display()));
                        combined.push_str(&content);
                        file_count += 1;
                        total_bytes += content.len();
                    }
                    Err(_) => continue,
                }
            }
        }

        let metadata = ContextMetadata {
            source: dir.to_string_lossy().to_string(),
            content_type: ContentType::Directory,
            total_bytes,
            total_lines: combined.lines().count(),
            line_count: combined.lines().count(),
            preview_lines: combined.lines().take(20).count(),
        };

        let mut extracted_meta = HashMap::new();
        extracted_meta.insert("file_count".to_string(), file_count.to_string());

        let mut context = Self {
            content: combined,
            metadata,
            chunks: Vec::new(),
            extracted_meta,
        };

        context.compute_chunks(1000);
        Ok(context)
    }

    /// Fetch context from URL
    pub async fn from_url(url: &str) -> Result<Self, RlmError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| RlmError::StorageError(e.to_string()))?;

        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| RlmError::StorageError(format!("HTTP error: {}", e)))?;

        let content = response
            .text()
            .await
            .map_err(|e| RlmError::StorageError(format!("Failed to read response: {}", e)))?;

        let metadata = ContextMetadata {
            source: url.to_string(),
            content_type: ContentType::Url,
            total_bytes: content.len(),
            total_lines: content.lines().count(),
            line_count: content.lines().count(),
            preview_lines: content.lines().take(20).count(),
        };

        let mut context = Self {
            content,
            metadata,
            chunks: Vec::new(),
            extracted_meta: HashMap::new(),
        };

        context.compute_chunks(1000);
        Ok(context)
    }

    /// Compute chunks based on line count
    fn compute_chunks(&mut self, lines_per_chunk: usize) {
        let lines: Vec<&str> = self.content.lines().collect();
        let mut chunks = Vec::new();
        let mut byte_offset = 0;

        for (chunk_idx, chunk_lines) in lines.chunks(lines_per_chunk).enumerate() {
            let chunk_content = chunk_lines.join("\n");
            let start_line = chunk_idx * lines_per_chunk;
            let end_line = (start_line + chunk_lines.len()).saturating_sub(1);

            let chunk = ContextChunk {
                index: chunk_idx,
                content: chunk_content.clone(),
                start_line,
                end_line,
                byte_range: (byte_offset, byte_offset + chunk_content.len()),
                metadata: ChunkMetadata::default(),
            };

            byte_offset += chunk_content.len() + 1; // +1 for newline
            chunks.push(chunk);
        }

        self.chunks = chunks;
    }

    /// Get a specific chunk by index
    pub fn get_chunk(&self, index: usize) -> Option<&ContextChunk> {
        self.chunks.get(index)
    }

    /// Get metadata for LLM consumption
    pub fn to_llm_metadata(&self) -> String {
        let mut meta = format!(
            "Context Source: {}\nType: {:?}\nTotal Bytes: {}\nTotal Lines: {}\nChunks: {}\n\nFirst {} lines:\n",
            self.metadata.source,
            self.metadata.content_type,
            self.metadata.total_bytes,
            self.metadata.total_lines,
            self.chunks.len(),
            self.metadata.preview_lines
        );

        let preview: String = self
            .content
            .lines()
            .take(self.metadata.preview_lines)
            .collect::<Vec<_>>()
            .join("\n");
        meta.push_str(&preview);

        if self.content.lines().count() > self.metadata.preview_lines {
            meta.push_str("\n\n[... content continues ...]");
        }

        // Add extracted metadata
        if !self.extracted_meta.is_empty() {
            meta.push_str("\n\n### Extracted Metadata\n");
            for (key, value) in &self.extracted_meta {
                meta.push_str(&format!("{}: {}\n", key, value));
            }
        }

        meta
    }

    /// Truncate content for LLM consumption
    pub fn truncate_for_llm(&self, max_len: usize) -> String {
        super::truncate_for_llm(&self.content, max_len)
    }

    /// Get chunk summary for sub-query dispatch
    pub fn get_chunk_summary(&self) -> Vec<(usize, String, usize)> {
        self.chunks
            .iter()
            .map(|c| {
                let preview: String = c.content.chars().take(100).collect();
                (c.index, preview, c.content.len())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_from_text() {
        let text = "Line 1\nLine 2\nLine 3\n...\nLine 100";
        let context = Context::from_text(text, "test");

        assert_eq!(context.metadata.total_lines, 5);
        assert!(!context.chunks.is_empty());
    }

    #[test]
    fn test_chunking() {
        let mut lines = Vec::new();
        for i in 0..2500 {
            lines.push(format!("Line {}", i));
        }
        let text = lines.join("\n");
        let context = Context::from_text(text, "test");

        // Should have roughly 3 chunks for 2500 lines at 1000 lines per chunk
        assert!(context.chunks.len() >= 2);
    }
}
