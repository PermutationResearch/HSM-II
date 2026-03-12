//! Shell Operation Tools
//!
//! Bash execution, grep search, and file finding.

use std::process::Command;

use anyhow::Result;
use serde_json::Value;
use tracing::debug;

use super::{Tool, ToolOutput, object_schema};

/// Maximum output size (100KB)
const MAX_OUTPUT_SIZE: usize = 100 * 1024;
/// Command timeout in seconds
const CMD_TIMEOUT_SECS: u64 = 30;
/// Maximum grep results
const MAX_GREP_RESULTS: usize = 100;
/// Maximum find results
const MAX_FIND_RESULTS: usize = 1000;

// ============================================================================
// Bash Tool
// ============================================================================

pub struct BashTool;

impl BashTool {
    pub fn new() -> Self {
        Self
    }
    
    fn execute_bash(&self, command: &str, working_dir: Option<&str>) -> Result<(String, String, i32), String> {
        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(command);
        
        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }
        
        let output = cmd.output()
            .map_err(|e| format!("Failed to execute command: {}", e))?;
        
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let code = output.status.code().unwrap_or(-1);
        
        Ok((stdout, stderr, code))
    }
}

#[async_trait::async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }
    
    fn description(&self) -> &str {
        "Execute a bash command. Use with caution. Has 30 second timeout."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("command", "The bash command to execute", true),
            ("working_dir", "Working directory for the command (optional)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let command = params.get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if command.is_empty() {
            return ToolOutput::error("Command parameter is required");
        }
        
        let working_dir = params.get("working_dir")
            .and_then(|v| v.as_str());
        
        debug!("Executing bash: {}", command);
        
        // Execute with timeout
        let timeout = tokio::time::Duration::from_secs(CMD_TIMEOUT_SECS);
        let result = tokio::time::timeout(timeout, async {
            self.execute_bash(command, working_dir)
        }).await;
        
        match result {
            Ok(Ok((stdout, stderr, code))) => {
                let stdout_len = stdout.len();
                let stderr_len = stderr.len();
                
                let truncated_stdout = if stdout.len() > MAX_OUTPUT_SIZE {
                    format!("{}...\n[Output truncated, total: {} bytes]", 
                        &stdout[..MAX_OUTPUT_SIZE], stdout.len())
                } else {
                    stdout
                };
                
                let result_text = if code == 0 {
                    if truncated_stdout.is_empty() && !stderr.is_empty() {
                        format!("Command completed (exit code 0)\n\nstderr:\n{}", stderr)
                    } else {
                        truncated_stdout
                    }
                } else {
                    format!("Command failed (exit code {})\n\nstdout:\n{}\n\nstderr:\n{}", 
                        code, truncated_stdout, stderr)
                };
                
                ToolOutput::success(result_text)
                    .with_metadata(serde_json::json!({
                        "exit_code": code,
                        "stdout_bytes": stdout_len,
                        "stderr_bytes": stderr_len,
                    }))
            }
            Ok(Err(e)) => ToolOutput::error(e),
            Err(_) => ToolOutput::error(format!("Command timed out after {} seconds", CMD_TIMEOUT_SECS)),
        }
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Grep Tool
// ============================================================================

pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
    
    fn grep_internal(&self, pattern: &str, path: &str, file_pattern: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("grep");
        cmd.arg("-r")
            .arg("-n")
            .arg("-I")  // Ignore binary files
            .arg("--include")
            .arg(file_pattern.unwrap_or("*"))
            .arg(pattern)
            .arg(path);
        
        let output = cmd.output()
            .map_err(|e| format!("Failed to execute grep: {}", e))?;
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        
        if lines.is_empty() {
            return Ok("No matches found".to_string());
        }
        
        let total = lines.len();
        let truncated = if lines.len() > MAX_GREP_RESULTS {
            &lines[..MAX_GREP_RESULTS]
        } else {
            &lines[..]
        };
        
        let result = truncated.join("\n");
        
        if total > MAX_GREP_RESULTS {
            Ok(format!("{}\n\n[Showing {} of {} matches]", result, MAX_GREP_RESULTS, total))
        } else {
            Ok(format!("{} matches:\n{}", total, result))
        }
    }
}

#[async_trait::async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    
    fn description(&self) -> &str {
        "Search file contents using grep. Returns matching lines with line numbers."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("pattern", "Pattern to search for", true),
            ("path", "Directory or file to search in (default: current directory)", false),
            ("include", "File pattern to include (e.g., '*.rs', default: all files)", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let pattern = params.get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        if pattern.is_empty() {
            return ToolOutput::error("Pattern parameter is required");
        }
        
        let path = params.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        
        let include = params.get("include")
            .and_then(|v| v.as_str());
        
        debug!("Grep: {} in {} (include: {:?})", pattern, path, include);
        
        match self.grep_internal(pattern, path, include) {
            Ok(result) => ToolOutput::success(result),
            Err(e) => ToolOutput::error(e),
        }
    }
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Find Tool
// ============================================================================

pub struct FindTool;

impl FindTool {
    pub fn new() -> Self {
        Self
    }
    
    fn find_internal(&self, path: &str, name_pattern: Option<&str>, file_type: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("find");
        cmd.arg(path);
        
        // Max depth to prevent hanging on large directories
        cmd.arg("-maxdepth").arg("5");
        
        if let Some(name) = name_pattern {
            cmd.arg("-name").arg(name);
        }
        
        if let Some(ftype) = file_type {
            match ftype {
                "f" | "file" => cmd.arg("-type").arg("f"),
                "d" | "directory" => cmd.arg("-type").arg("d"),
                _ => &mut cmd,
            };
        }
        
        let output = cmd.output()
            .map_err(|e| format!("Failed to execute find: {}", e))?;
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        
        if lines.is_empty() {
            return Ok("No files found".to_string());
        }
        
        let total = lines.len();
        let truncated = if lines.len() > MAX_FIND_RESULTS {
            &lines[..MAX_FIND_RESULTS]
        } else {
            &lines[..]
        };
        
        let result = truncated.join("\n");
        
        if total > MAX_FIND_RESULTS {
            Ok(format!("{}\n\n[Showing {} of {} results]", result, MAX_FIND_RESULTS, total))
        } else {
            Ok(format!("{} results:\n{}", total, result))
        }
    }
}

#[async_trait::async_trait]
impl Tool for FindTool {
    fn name(&self) -> &str {
        "find"
    }
    
    fn description(&self) -> &str {
        "Find files by name pattern. Searches up to 5 levels deep."
    }
    
    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            ("path", "Directory to search in (default: current directory)", false),
            ("name", "File name pattern (e.g., '*.rs', 'Cargo.*')", false),
            ("type", "Type: 'f' for files, 'd' for directories", false),
        ])
    }
    
    async fn execute(&self, params: Value) -> ToolOutput {
        let path = params.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        
        let name = params.get("name")
            .and_then(|v| v.as_str());
        
        let file_type = params.get("type")
            .and_then(|v| v.as_str());
        
        debug!("Find: in {} (name: {:?}, type: {:?})", path, name, file_type);
        
        match self.find_internal(path, name, file_type) {
            Ok(result) => ToolOutput::success(result),
            Err(e) => ToolOutput::error(e),
        }
    }
}

impl Default for FindTool {
    fn default() -> Self {
        Self::new()
    }
}
