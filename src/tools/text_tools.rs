//! Text Processing Tools - String manipulation and analysis

use serde_json::Value;

use super::{Tool, ToolOutput, object_schema};

// ============================================================================
// Text Replace Tool
// ============================================================================

pub struct TextReplaceTool;

impl TextReplaceTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for TextReplaceTool {
    fn name(&self) -> &str {
        "text_replace"
    }
    
    fn description(&self) -> &str {
        "Replace text in a string. Supports regex if regex=true."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("text", "Source text", true),
            ("search", "Text to search for", true),
            ("replace", "Replacement text", true),
            ("regex", "Use regex matching (default: false)", false),
            ("all", "Replace all occurrences (default: true)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let search = params.get("search").and_then(|v| v.as_str()).unwrap_or("");
        let replace = params.get("replace").and_then(|v| v.as_str()).unwrap_or("");
        let use_regex = params.get("regex").and_then(|v| v.as_bool()).unwrap_or(false);
        let replace_all = params.get("all").and_then(|v| v.as_bool()).unwrap_or(true);
        
        if search.is_empty() {
            return ToolOutput::error("search parameter is required");
        }
        
        let result = if use_regex {
            // Simple regex replace (limited support)
            if replace_all {
                text.split(&search).collect::<Vec<_>>().join(replace)
            } else {
                text.replacen(search, replace, 1)
            }
        } else {
            if replace_all {
                text.replace(search, replace)
            } else {
                text.replacen(search, replace, 1)
            }
        };
        
        let count = text.matches(search).count();
        
        ToolOutput::success(result)
            .with_metadata(serde_json::json!({
                "replacements": count,
            }))
    }
}

impl Default for TextReplaceTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Text Split Tool
// ============================================================================

pub struct TextSplitTool;

impl TextSplitTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for TextSplitTool {
    fn name(&self) -> &str {
        "text_split"
    }
    
    fn description(&self) -> &str {
        "Split text by delimiter or fixed chunk size."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("text", "Text to split", true),
            ("delimiter", "Delimiter to split by (e.g., '\\n' for lines)", false),
            ("chunk_size", "Split into fixed-size chunks (overrides delimiter)", false),
            ("limit", "Maximum number of splits (default: unlimited)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let chunk_size = params.get("chunk_size").and_then(|v| v.as_u64()).map(|v| v as usize);
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        
        let parts: Vec<String> = if let Some(size) = chunk_size {
            text.chars()
                .collect::<Vec<_>>()
                .chunks(size)
                .map(|chunk| chunk.iter().collect())
                .take(if limit > 0 { limit } else { usize::MAX })
                .collect()
        } else if let Some(delimiter) = params.get("delimiter").and_then(|v| v.as_str()) {
            let delim = match delimiter {
                "\\n" => "\n",
                "\\t" => "\t",
                "\\r" => "\r",
                _ => delimiter,
            };
            
            text.split(delim)
                .map(|s| s.to_string())
                .take(if limit > 0 { limit } else { usize::MAX })
                .collect()
        } else {
            text.lines()
                .map(|s| s.to_string())
                .take(if limit > 0 { limit } else { usize::MAX })
                .collect()
        };
        
        ToolOutput::success(format!("Split into {} parts", parts.len()))
            .with_metadata(serde_json::json!({
                "count": parts.len(),
                "parts": parts,
            }))
    }
}

impl Default for TextSplitTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Text Join Tool
// ============================================================================

pub struct TextJoinTool;

impl TextJoinTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for TextJoinTool {
    fn name(&self) -> &str {
        "text_join"
    }
    
    fn description(&self) -> &str {
        "Join text parts with a delimiter."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("parts", "Array of text parts to join", true),
            ("delimiter", "Delimiter to join with (default: newline)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let parts = params.get("parts").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let delimiter = params.get("delimiter").and_then(|v| v.as_str()).unwrap_or("\n");
        
        let delim = match delimiter {
            "\\n" => "\n",
            "\\t" => "\t",
            "\\r" => "\r",
            _ => delimiter,
        };
        
        let strings: Vec<String> = parts
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        
        ToolOutput::success(strings.join(delim))
    }
}

impl Default for TextJoinTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Text Case Tool
// ============================================================================

pub struct TextCaseTool;

impl TextCaseTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for TextCaseTool {
    fn name(&self) -> &str {
        "text_case"
    }
    
    fn description(&self) -> &str {
        "Change text case: uppercase, lowercase, title case, snake_case, camelCase, kebab-case."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("text", "Text to transform", true),
            ("to", "Target case: upper, lower, title, snake, camel, kebab, pascal", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let to = params.get("to").and_then(|v| v.as_str()).unwrap_or("lower");
        
        let result = match to {
            "upper" => text.to_uppercase(),
            "lower" => text.to_lowercase(),
            "title" => {
                text.split_whitespace()
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                            None => String::new(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            "snake" => {
                text.to_lowercase()
                    .replace(" ", "_")
                    .replace("-", "_")
            }
            "kebab" => {
                text.to_lowercase()
                    .replace(" ", "-")
                    .replace("_", "-")
            }
            "camel" => {
                let words: Vec<&str> = text.split_whitespace().collect();
                if words.is_empty() {
                    String::new()
                } else {
                    let first = words[0].to_lowercase();
                    let rest: String = words[1..]
                        .iter()
                        .map(|word| {
                            let mut chars = word.chars();
                            match chars.next() {
                                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                                None => String::new(),
                            }
                        })
                        .collect();
                    first + &rest
                }
            }
            "pascal" => {
                text.split_whitespace()
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                            None => String::new(),
                        }
                    })
                    .collect::<String>()
            }
            _ => text.to_string(),
        };
        
        ToolOutput::success(result)
    }
}

impl Default for TextCaseTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Text Truncate Tool
// ============================================================================

pub struct TextTruncateTool;

impl TextTruncateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for TextTruncateTool {
    fn name(&self) -> &str {
        "text_truncate"
    }
    
    fn description(&self) -> &str {
        "Truncate text to a maximum length with optional ellipsis."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("text", "Text to truncate", true),
            ("length", "Maximum length", true),
            ("ellipsis", "Add '...' when truncated (default: true)", false),
            ("from_end", "Truncate from end vs start (default: true)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let length = params.get("length").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
        let ellipsis = params.get("ellipsis").and_then(|v| v.as_bool()).unwrap_or(true);
        let from_end = params.get("from_end").and_then(|v| v.as_bool()).unwrap_or(true);
        
        if text.len() <= length {
            return ToolOutput::success(text.to_string());
        }
        
        let result = if from_end {
            if ellipsis && length > 3 {
                format!("{}...", &text[..length - 3])
            } else {
                text[..length].to_string()
            }
        } else {
            if ellipsis && length > 3 {
                format!("...{}", &text[text.len() - length + 3..])
            } else {
                text[text.len() - length..].to_string()
            }
        };
        
        ToolOutput::success(result)
            .with_metadata(serde_json::json!({
                "original_length": text.len(),
                "truncated": true,
            }))
    }
}

impl Default for TextTruncateTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Word Count Tool
// ============================================================================

pub struct WordCountTool;

impl WordCountTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for WordCountTool {
    fn name(&self) -> &str {
        "word_count"
    }
    
    fn description(&self) -> &str {
        "Count words, characters, lines in text."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("text", "Text to analyze", true),
            ("file", "File path to analyze (alternative to text)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let text = if let Some(path) = params.get("file").and_then(|v| v.as_str()) {
            match tokio::fs::read_to_string(path).await {
                Ok(content) => content,
                Err(e) => return ToolOutput::error(format!("Cannot read file: {}", e)),
            }
        } else {
            params.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string()
        };
        
        let words: Vec<&str> = text.split_whitespace().collect();
        let word_count = words.len();
        let char_count = text.len();
        let char_count_no_spaces = text.chars().filter(|c| !c.is_whitespace()).count();
        let line_count = text.lines().count();
        let sentence_count = text.split(['.', '!', '?']).filter(|s| !s.trim().is_empty()).count();
        let paragraph_count = text.split("\n\n").filter(|s| !s.trim().is_empty()).count();
        
        let stats = serde_json::json!({
            "words": word_count,
            "characters": char_count,
            "characters_no_spaces": char_count_no_spaces,
            "lines": line_count,
            "sentences": sentence_count,
            "paragraphs": paragraph_count,
        });
        
        let summary = format!(
            "Words: {} | Characters: {} | Lines: {} | Sentences: {}",
            word_count, char_count, line_count, sentence_count
        );
        
        ToolOutput::success(summary)
            .with_metadata(stats)
    }
}

impl Default for WordCountTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Text Diff Tool
// ============================================================================

pub struct TextDiffTool;

impl TextDiffTool {
    pub fn new() -> Self {
        Self
    }
    
    fn compute_diff(&self, old: &str, new: &str) -> Vec<String> {
        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();
        
        let mut result = Vec::new();
        let mut old_idx = 0;
        let mut new_idx = 0;
        
        while old_idx < old_lines.len() || new_idx < new_lines.len() {
            if old_idx < old_lines.len() && new_idx < new_lines.len() {
                if old_lines[old_idx] == new_lines[new_idx] {
                    result.push(format!("  {}", old_lines[old_idx]));
                    old_idx += 1;
                    new_idx += 1;
                } else {
                    // Find next match or end
                    result.push(format!("- {}", old_lines[old_idx]));
                    old_idx += 1;
                }
            } else if old_idx < old_lines.len() {
                result.push(format!("- {}", old_lines[old_idx]));
                old_idx += 1;
            } else {
                result.push(format!("+ {}", new_lines[new_idx]));
                new_idx += 1;
            }
        }
        
        result
    }
}

#[async_trait::async_trait]
impl Tool for TextDiffTool {
    fn name(&self) -> &str {
        "text_diff"
    }
    
    fn description(&self) -> &str {
        "Show differences between two texts (line-by-line diff)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("old", "Original text", true),
            ("new", "New text", true),
            ("context", "Number of context lines (default: 3)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let old_text = params.get("old").and_then(|v| v.as_str()).unwrap_or("");
        let new_text = params.get("new").and_then(|v| v.as_str()).unwrap_or("");
        
        let diff = self.compute_diff(old_text, new_text);
        
        let changed_lines = diff.iter().filter(|l| l.starts_with('+') || l.starts_with('-')).count();
        
        ToolOutput::success(diff.join("\n"))
            .with_metadata(serde_json::json!({
                "changed_lines": changed_lines,
            }))
    }
}

impl Default for TextDiffTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Regex Extract Tool
// ============================================================================

pub struct RegexExtractTool;

impl RegexExtractTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for RegexExtractTool {
    fn name(&self) -> &str {
        "regex_extract"
    }
    
    fn description(&self) -> &str {
        "Extract patterns from text using simple patterns (* wildcards, ? single char)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("text", "Text to search", true),
            ("pattern", "Pattern to extract (e.g., '*.com' for domains, '+*' for phone numbers)", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let pattern = params.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        
        if pattern.is_empty() {
            return ToolOutput::error("pattern is required");
        }
        
        // Simple pattern matching (not full regex)
        let matches: Vec<String> = text
            .split_whitespace()
            .filter(|word| {
                let mut word_chars = word.chars().peekable();
                let mut pat_chars = pattern.chars().peekable();
                
                while let Some(pat_ch) = pat_chars.next() {
                    match pat_ch {
                        '*' => {
                            // Match any sequence
                            if let Some(next_pat) = pat_chars.peek() {
                                // Find next required char
                                while let Some(word_ch) = word_chars.next() {
                                    if word_ch == *next_pat {
                                        break;
                                    }
                                }
                            }
                        }
                        '?' => {
                            // Match single char
                            if word_chars.next().is_none() {
                                return false;
                            }
                        }
                        c => {
                            if word_chars.next() != Some(c) {
                                return false;
                            }
                        }
                    }
                }
                
                word_chars.next().is_none()
            })
            .map(|s| s.to_string())
            .collect();
        
        ToolOutput::success(format!("Found {} matches", matches.len()))
            .with_metadata(serde_json::json!({
                "matches": matches,
                "count": matches.len(),
            }))
    }
}

impl Default for RegexExtractTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Template Tool
// ============================================================================

pub struct TemplateTool;

impl TemplateTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for TemplateTool {
    fn name(&self) -> &str {
        "template"
    }
    
    fn description(&self) -> &str {
        "Simple template substitution with {{variable}} syntax."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("template", "Template string with {{vars}}", true),
            ("vars", "Object with variable values", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let template = params.get("template").and_then(|v| v.as_str()).unwrap_or("");
        let vars = params.get("vars").and_then(|v| v.as_object()).cloned().unwrap_or_default();
        
        let mut result = template.to_string();
        
        for (key, value) in vars {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = value.as_str().map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string());
            result = result.replace(&placeholder, &replacement);
        }
        
        ToolOutput::success(result)
    }
}

impl Default for TemplateTool {
    fn default() -> Self {
        Self::new()
    }
}
