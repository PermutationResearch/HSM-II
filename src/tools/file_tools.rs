//! File Operation Tools
//!
//! Read, write, and edit files with safety limits.

use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Result;
use serde_json::Value;
use tracing::debug;

use super::{Tool, ToolOutput, object_schema};

/// Maximum file size to read (10MB)
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
/// Default lines to read
const DEFAULT_READ_LINES: usize = 2000;
/// Maximum edit size
const MAX_EDIT_SIZE: usize = 100_000;

// ============================================================================
// Read Tool
// ============================================================================

pub struct ReadTool;

impl ReadTool {
    pub fn new() -> Self {
        Self
    }
    
    fn read_file_internal(&self, path: &Path, offset: usize, limit: usize) -> Result<String, String> {
        let metadata = fs::metadata(path)
            .map_err(|e| format!("Cannot read file metadata: {}", e))?;
        
        if metadata.len() > MAX_FILE_SIZE {
            return Err(format!(
                "File too large: {} bytes (max: {})",
                metadata.len(), MAX_FILE_SIZE
            ));
        }
        
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Cannot read file: {}", e))?;
        
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        
        let start_idx = if offset > 0 { offset - 1 } else { 0 }.min(total_lines);
        let end_idx = (start_idx + limit).min(total_lines);
        
        let selected_lines: Vec<&str> = lines[start_idx..end_idx].to_vec();
        let result = selected_lines.join("\n");
        
        if start_idx > 0 || end_idx < total_lines {
            Ok(format!(
                "{}\n\n[Lines {}-{} of {}]",
                result,
                start_idx + 1,
                end_idx,
                total_lines
            ))
        } else {
            Ok(result)
        }
    }
}

#[async_trait::async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }
    
    fn description(&self) -> &str {
        "Read file contents with optional offset and limit. Shows line numbers if truncated."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("path", "Path to the file to read", true),
            ("offset", "Line number to start from (1-indexed, default: 1)", false),
            ("limit", "Maximum number of lines to read (default: 2000)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let path_str = params.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if path_str.is_empty() {
            return ToolOutput::error("Path parameter is required");
        }
        
        let path = PathBuf::from(path_str);
        
        if !path.exists() {
            return ToolOutput::error(format!("File not found: {}", path_str));
        }
        
        let offset: usize = params.get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize;
        
        let limit: usize = params.get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_READ_LINES as u64) as usize;
        
        debug!("Reading file: {} (offset: {}, limit: {})", path_str, offset, limit);
        
        match self.read_file_internal(&path, offset, limit) {
            Ok(content) => ToolOutput::success(content),
            Err(e) => ToolOutput::error(e),
        }
    }
}

impl Default for ReadTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Write Tool
// ============================================================================

pub struct WriteTool;

impl WriteTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }
    
    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if needed. Overwrites existing files."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("path", "Path to the file to write", true),
            ("content", "Content to write to the file", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let path_str = params.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let content = params.get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if path_str.is_empty() {
            return ToolOutput::error("Path parameter is required");
        }
        
        let path = PathBuf::from(path_str);
        
        // Create parent directories
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return ToolOutput::error(format!("Failed to create directories: {}", e));
            }
        }
        
        let existed = path.exists();
        
        debug!("Writing file: {} ({} bytes)", path_str, content.len());
        
        match fs::write(&path, content) {
            Ok(_) => {
                let action = if existed { "Updated" } else { "Created" };
                ToolOutput::success(format!("{} file: {}", action, path_str))
                    .with_metadata(serde_json::json!({
                        "path": path_str,
                        "bytes_written": content.len(),
                        "existed": existed,
                    }))
            }
            Err(e) => ToolOutput::error(format!("Failed to write file: {}", e)),
        }
    }
}

impl Default for WriteTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Edit Tool
// ============================================================================

pub struct EditTool;

impl EditTool {
    pub fn new() -> Self {
        Self
    }
    
    fn edit_file_internal(&self, path: &Path, old_string: &str, new_string: &str) -> Result<String, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Cannot read file: {}", e))?;
        
        if content.len() > MAX_EDIT_SIZE {
            return Err(format!("File too large for editing: {} bytes", content.len()));
        }
        
        if !content.contains(old_string) {
            return Err(format!("Old string not found in file: '{}'", 
                old_string.chars().take(50).collect::<String>()));
        }
        
        let new_content = content.replacen(old_string, new_string, 1);
        
        fs::write(path, new_content)
            .map_err(|e| format!("Failed to write file: {}", e))?;
        
        Ok(format!("Successfully edited file: {}", path.display()))
    }
}

#[async_trait::async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }
    
    fn description(&self) -> &str {
        "Edit a file by replacing exact text. The old_string must match exactly (including whitespace)."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("path", "Path to the file to edit", true),
            ("old_string", "Exact text to replace (including whitespace)", true),
            ("new_string", "New text to insert", true),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let path_str = params.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let old_string = params.get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let new_string = params.get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if path_str.is_empty() {
            return ToolOutput::error("Path parameter is required");
        }
        
        let path = PathBuf::from(path_str);
        
        if !path.exists() {
            return ToolOutput::error(format!("File not found: {}", path_str));
        }
        
        if old_string.is_empty() {
            return ToolOutput::error("old_string parameter is required");
        }
        
        debug!("Editing file: {} (replacing {} bytes with {} bytes)", 
            path_str, old_string.len(), new_string.len());
        
        match self.edit_file_internal(&path, old_string, new_string) {
            Ok(msg) => ToolOutput::success(msg),
            Err(e) => ToolOutput::error(e),
        }
    }
}

impl Default for EditTool {
    fn default() -> Self {
        Self::new()
    }
}
