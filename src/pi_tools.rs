//! Pi Agent Tools Integration
//!
//! This module provides Coder agent tools for file operations:
//! - read: Read file contents with offset/limit support
//! - write: Write content to a file (creates parent directories)
//! - edit: Edit a file by replacing exact text
//! - bash: Execute bash commands with optional timeout
//! - grep: Search file contents
//! - find: Find files by pattern
//! - ls: List directory contents
//!
//! These tools allow Coder agents to explore and modify the codebase,
//! reporting results as beliefs and experiences in the hypergraph.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::memory::{AgentTool, ToolContext, ToolResult, ToolSideEffect};

/// Maximum file size to read (100MB)
const MAX_READ_BYTES: u64 = 100 * 1024 * 1024;
/// Default lines to read
const DEFAULT_READ_LINES: usize = 2000;
/// Maximum lines for grep results
const GREP_MAX_LINES: usize = 100;
/// Maximum line length
const MAX_LINE_LENGTH: usize = 500;
/// Maximum files for find/ls
const MAX_FILES: usize = 1000;

// ============================================================================
// Read Tool
// ============================================================================

pub struct PiReadTool;

impl PiReadTool {
    pub fn new() -> Self {
        Self
    }

    /// Read file with optional offset and limit
    fn read_file(&self, path: &Path, offset: usize, limit: usize) -> Result<String, String> {
        let metadata =
            fs::metadata(path).map_err(|e| format!("Cannot read file metadata: {}", e))?;

        if metadata.len() > MAX_READ_BYTES {
            return Err(format!(
                "File too large: {} bytes (max: {})",
                metadata.len(),
                MAX_READ_BYTES
            ));
        }

        let content = fs::read_to_string(path).map_err(|e| format!("Cannot read file: {}", e))?;

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

    fn parse_input(&self, input: &str) -> Result<(PathBuf, usize, usize), String> {
        if input.trim().starts_with('{') {
            match serde_json::from_str::<serde_json::Value>(input) {
                Ok(json) => {
                    let path = json
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or("Missing 'path' parameter")?;
                    let offset = json.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                    let limit =
                        json.get("limit")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(DEFAULT_READ_LINES as u64) as usize;
                    return Ok((PathBuf::from(path), offset, limit));
                }
                Err(_) => {}
            }
        }

        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        if parts.is_empty() {
            return Err("No path provided".to_string());
        }

        let path = PathBuf::from(parts[0]);
        let offset = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
        let limit = parts
            .get(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_READ_LINES);

        Ok((path, offset, limit))
    }
}

impl AgentTool for PiReadTool {
    fn name(&self) -> &str {
        "pi_read"
    }

    fn description(&self) -> &str {
        "Read file contents.\n\
         Usage: pi_read {\"path\": \"...\", \"offset\": 1, \"limit\": 2000}\n\
         - path: Path to the file\n\
         - offset: Line number to start from (1-indexed, default: 1)\n\
         - limit: Maximum lines to read (default: 2000)"
    }

    fn execute(&self, input: &str, _ctx: &ToolContext) -> ToolResult {
        let (path, offset, limit) = match self.parse_input(input) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    output: format!("Error: {}", e),
                    side_effects: vec![],
                    confidence: 0.0,
                }
            }
        };

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let full_path = if path.is_absolute() {
            if !path.starts_with(&cwd) {
                return ToolResult {
                    output: "Error: Path outside project directory".to_string(),
                    side_effects: vec![],
                    confidence: 0.0,
                };
            }
            path
        } else {
            cwd.join(path)
        };

        match self.read_file(&full_path, offset, limit) {
            Ok(content) => {
                let line_count = content.lines().count();
                ToolResult {
                    output: format!(
                        "File: {} (showing {} lines)\n```\n{}\n```",
                        full_path.display(),
                        line_count,
                        content
                    ),
                    side_effects: vec![ToolSideEffect::LogMessage(format!(
                        "Coder read file: {} (offset: {}, limit: {})",
                        full_path.display(),
                        offset,
                        limit
                    ))],
                    confidence: 1.0,
                }
            }
            Err(e) => ToolResult {
                output: format!("Error reading file: {}", e),
                side_effects: vec![],
                confidence: 0.0,
            },
        }
    }
}

// ============================================================================
// Write Tool
// ============================================================================

pub struct PiWriteTool;

impl PiWriteTool {
    pub fn new() -> Self {
        Self
    }

    fn parse_input(&self, input: &str) -> Result<(PathBuf, String), String> {
        if input.trim().starts_with('{') {
            match serde_json::from_str::<serde_json::Value>(input) {
                Ok(json) => {
                    let path = json
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or("Missing 'path' parameter")?;
                    let content = json.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    return Ok((PathBuf::from(path), content.to_string()));
                }
                Err(_) => {}
            }
        }

        let mut lines = input.lines();
        let path_str = lines.next().ok_or("No path provided")?.trim();
        let content = lines.collect::<Vec<_>>().join("\n");

        Ok((PathBuf::from(path_str), content))
    }
}

impl AgentTool for PiWriteTool {
    fn name(&self) -> &str {
        "pi_write"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if needed.\n\
         Usage: pi_write {\"path\": \"...\", \"content\": \"...\"}"
    }

    fn execute(&self, input: &str, _ctx: &ToolContext) -> ToolResult {
        let (path, content) = match self.parse_input(input) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    output: format!("Error: {}", e),
                    side_effects: vec![],
                    confidence: 0.0,
                }
            }
        };

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let full_path = if path.is_absolute() {
            if !path.starts_with(&cwd) {
                return ToolResult {
                    output: "Error: Path outside project directory".to_string(),
                    side_effects: vec![],
                    confidence: 0.0,
                };
            }
            path
        } else {
            cwd.join(path)
        };

        if let Some(parent) = full_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return ToolResult {
                    output: format!("Error creating directories: {}", e),
                    side_effects: vec![],
                    confidence: 0.0,
                };
            }
        }

        match fs::write(&full_path, &content) {
            Ok(_) => {
                let action = if full_path.exists() {
                    "Updated"
                } else {
                    "Created"
                };
                ToolResult {
                    output: format!(
                        "✓ {} {} ({} bytes)",
                        action,
                        full_path.display(),
                        content.len()
                    ),
                    side_effects: vec![
                        ToolSideEffect::AddBelief {
                            content: format!("Coder wrote file: {}", full_path.display()),
                            confidence: 0.95,
                        },
                        ToolSideEffect::RecordExperience {
                            description: format!("{} file: {}", action, full_path.display()),
                            outcome_positive: true,
                        },
                    ],
                    confidence: 1.0,
                }
            }
            Err(e) => ToolResult {
                output: format!("Error writing file: {}", e),
                side_effects: vec![],
                confidence: 0.0,
            },
        }
    }
}

// ============================================================================
// Edit Tool
// ============================================================================

pub struct PiEditTool;

impl PiEditTool {
    pub fn new() -> Self {
        Self
    }

    fn parse_input(&self, input: &str) -> Result<(PathBuf, String, String), String> {
        if input.trim().starts_with('{') {
            match serde_json::from_str::<serde_json::Value>(input) {
                Ok(json) => {
                    let path = json
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or("Missing 'path' parameter")?;
                    let old_text = json
                        .get("oldText")
                        .and_then(|v| v.as_str())
                        .ok_or("Missing 'oldText' parameter")?;
                    let new_text = json.get("newText").and_then(|v| v.as_str()).unwrap_or("");
                    return Ok((
                        PathBuf::from(path),
                        old_text.to_string(),
                        new_text.to_string(),
                    ));
                }
                Err(_) => {}
            }
        }
        Err("Use JSON: {\"path\": \"...\", \"oldText\": \"...\", \"newText\": \"...\"}".to_string())
    }
}

impl AgentTool for PiEditTool {
    fn name(&self) -> &str {
        "pi_edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing exact text. oldText must match exactly (including whitespace).\n\
         Usage: pi_edit {\"path\": \"...\", \"oldText\": \"...\", \"newText\": \"...\"}"
    }

    fn execute(&self, input: &str, _ctx: &ToolContext) -> ToolResult {
        let (path, old_text, new_text) = match self.parse_input(input) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    output: format!("Error: {}", e),
                    side_effects: vec![],
                    confidence: 0.0,
                }
            }
        };

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let full_path = if path.is_absolute() {
            if !path.starts_with(&cwd) {
                return ToolResult {
                    output: "Error: Path outside project directory".to_string(),
                    side_effects: vec![],
                    confidence: 0.0,
                };
            }
            path
        } else {
            cwd.join(path)
        };

        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => {
                return ToolResult {
                    output: format!("Error reading file: {}", e),
                    side_effects: vec![],
                    confidence: 0.0,
                }
            }
        };

        if !content.contains(&old_text) {
            return ToolResult {
                output: format!(
                    "Error: oldText not found in file.\n\nLooking for ({} chars):\n```\n{}\n```",
                    old_text.len(),
                    &old_text[..old_text.len().min(100)]
                ),
                side_effects: vec![],
                confidence: 0.0,
            };
        }

        let new_content = content.replacen(&old_text, &new_text, 1);

        match fs::write(&full_path, new_content) {
            Ok(_) => ToolResult {
                output: format!(
                    "✓ Edited {}\n\nReplaced ({} chars) with ({} chars)",
                    full_path.display(),
                    old_text.len(),
                    new_text.len()
                ),
                side_effects: vec![
                    ToolSideEffect::AddBelief {
                        content: format!("Coder edited file: {}", full_path.display()),
                        confidence: 0.9,
                    },
                    ToolSideEffect::RecordExperience {
                        description: format!("Successfully edited {}", full_path.display()),
                        outcome_positive: true,
                    },
                ],
                confidence: 1.0,
            },
            Err(e) => ToolResult {
                output: format!("Error writing file: {}", e),
                side_effects: vec![],
                confidence: 0.0,
            },
        }
    }
}

// ============================================================================
// Bash Tool
// ============================================================================

pub struct PiBashTool;

impl PiBashTool {
    pub fn new() -> Self {
        Self
    }

    fn parse_input(&self, input: &str) -> Result<(String, Option<u64>), String> {
        if input.trim().starts_with('{') {
            match serde_json::from_str::<serde_json::Value>(input) {
                Ok(json) => {
                    let command = json
                        .get("command")
                        .and_then(|v| v.as_str())
                        .ok_or("Missing 'command' parameter")?;
                    let timeout = json.get("timeout").and_then(|v| v.as_u64());
                    return Ok((command.to_string(), timeout));
                }
                Err(_) => {}
            }
        }
        Ok((input.trim().to_string(), None))
    }

    fn run_command(
        &self,
        command: &str,
        _timeout_secs: Option<u64>,
        cwd: &Path,
    ) -> Result<(String, i32), String> {
        let blocked = [
            "rm -rf /",
            "rm -rf ~",
            "> /dev/sda",
            "mkfs",
            "dd if=/dev/zero",
        ];
        for dangerous in &blocked {
            if command.contains(dangerous) {
                return Err(format!("Blocked dangerous command: {}", dangerous));
            }
        }

        let output = Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("Failed to execute: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let result = if stderr.is_empty() {
            stdout.to_string()
        } else if stdout.is_empty() {
            stderr.to_string()
        } else {
            format!("stdout:\n{}\n\nstderr:\n{}", stdout, stderr)
        };

        let truncated = if result.len() > 10_000 {
            format!(
                "{}... [truncated, total: {} chars]",
                &result[..10_000],
                result.len()
            )
        } else {
            result
        };

        Ok((truncated, output.status.code().unwrap_or(-1)))
    }
}

impl AgentTool for PiBashTool {
    fn name(&self) -> &str {
        "pi_bash"
    }

    fn description(&self) -> &str {
        "Execute a bash command.\n\
         Usage: pi_bash {\"command\": \"...\", \"timeout\": 120}"
    }

    fn execute(&self, input: &str, _ctx: &ToolContext) -> ToolResult {
        let (command, timeout) = match self.parse_input(input) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    output: format!("Error: {}", e),
                    side_effects: vec![],
                    confidence: 0.0,
                }
            }
        };

        if command.is_empty() {
            return ToolResult {
                output: "Error: No command provided".to_string(),
                side_effects: vec![],
                confidence: 0.0,
            };
        }

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        match self.run_command(&command, timeout, &cwd) {
            Ok((output, exit_code)) => {
                let success = exit_code == 0;
                ToolResult {
                    output: format!("Exit code: {}\n```\n{}\n```", exit_code, output),
                    side_effects: vec![ToolSideEffect::RecordExperience {
                        description: format!("Coder executed: {}", command),
                        outcome_positive: success,
                    }],
                    confidence: if success { 1.0 } else { 0.5 },
                }
            }
            Err(e) => ToolResult {
                output: format!("Error: {}", e),
                side_effects: vec![],
                confidence: 0.0,
            },
        }
    }
}

// ============================================================================
// Grep Tool
// ============================================================================

pub struct PiGrepTool;

impl PiGrepTool {
    pub fn new() -> Self {
        Self
    }

    fn grep(&self, pattern: &str, path: &Path) -> Result<Vec<String>, String> {
        let mut results = Vec::new();
        self.grep_recursive(pattern, path, &mut results)?;
        Ok(results)
    }

    fn grep_recursive(
        &self,
        pattern: &str,
        path: &Path,
        results: &mut Vec<String>,
    ) -> Result<(), String> {
        if results.len() >= GREP_MAX_LINES {
            return Ok(());
        }

        if path.is_file() {
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_lowercase();
                if ["exe", "dll", "so", "dylib", "bin", "o", "a"].contains(&ext.as_str()) {
                    return Ok(());
                }
            }

            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => return Ok(()),
            };

            for (line_num, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    let truncated_line = if line.len() > MAX_LINE_LENGTH {
                        format!("{}...", &line[..MAX_LINE_LENGTH])
                    } else {
                        line.to_string()
                    };
                    results.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        line_num + 1,
                        truncated_line
                    ));

                    if results.len() >= GREP_MAX_LINES {
                        return Ok(());
                    }
                }
            }
        } else if path.is_dir() {
            let entries = match fs::read_dir(path) {
                Ok(e) => e,
                Err(_) => return Ok(()),
            };

            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                if name_str.starts_with('.')
                    || ["node_modules", "target", "__pycache__", "dist", "build"]
                        .contains(&name_str.as_ref())
                {
                    continue;
                }

                self.grep_recursive(pattern, &entry.path(), results)?;
                if results.len() >= GREP_MAX_LINES {
                    return Ok(());
                }
            }
        }

        Ok(())
    }
}

impl AgentTool for PiGrepTool {
    fn name(&self) -> &str {
        "pi_grep"
    }

    fn description(&self) -> &str {
        "Search for pattern in files.\n\
         Usage: pi_grep <pattern>"
    }

    fn execute(&self, input: &str, _ctx: &ToolContext) -> ToolResult {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let pattern = input.trim();

        if pattern.is_empty() {
            return ToolResult {
                output: "Error: No pattern provided".to_string(),
                side_effects: vec![],
                confidence: 0.0,
            };
        }

        match self.grep(pattern, &cwd) {
            Ok(results) => {
                if results.is_empty() {
                    ToolResult {
                        output: format!("No matches found for '{}'", pattern),
                        side_effects: vec![],
                        confidence: 1.0,
                    }
                } else {
                    let truncated = if results.len() >= GREP_MAX_LINES {
                        format!("{}\n... ({}+ matches)", results.join("\n"), GREP_MAX_LINES)
                    } else {
                        results.join("\n")
                    };

                    ToolResult {
                        output: format!(
                            "Found {} matches:\n```\n{}\n```",
                            results.len(),
                            truncated
                        ),
                        side_effects: vec![ToolSideEffect::LogMessage(format!(
                            "Coder grep'd for '{}' ({} matches)",
                            pattern,
                            results.len()
                        ))],
                        confidence: 1.0,
                    }
                }
            }
            Err(e) => ToolResult {
                output: format!("Error: {}", e),
                side_effects: vec![],
                confidence: 0.0,
            },
        }
    }
}

// ============================================================================
// Find Tool
// ============================================================================

pub struct PiFindTool;

impl PiFindTool {
    pub fn new() -> Self {
        Self
    }

    fn find(&self, pattern: &str, path: &Path) -> Result<Vec<String>, String> {
        let mut results = Vec::new();
        self.find_recursive(pattern, path, &mut results)?;
        Ok(results)
    }

    fn find_recursive(
        &self,
        pattern: &str,
        path: &Path,
        results: &mut Vec<String>,
    ) -> Result<(), String> {
        if results.len() >= MAX_FILES {
            return Ok(());
        }

        let entries = match fs::read_dir(path) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with('.')
                || ["node_modules", "target", "__pycache__", "dist", "build"]
                    .contains(&name_str.as_ref())
            {
                continue;
            }

            let entry_path = entry.path();

            if name_str.contains(pattern) {
                results.push(entry_path.display().to_string());
                if results.len() >= MAX_FILES {
                    return Ok(());
                }
            }

            if entry_path.is_dir() {
                self.find_recursive(pattern, &entry_path, results)?;
            }
        }

        Ok(())
    }
}

impl AgentTool for PiFindTool {
    fn name(&self) -> &str {
        "pi_find"
    }

    fn description(&self) -> &str {
        "Find files by name pattern.\n\
         Usage: pi_find <pattern>"
    }

    fn execute(&self, input: &str, _ctx: &ToolContext) -> ToolResult {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let pattern = input.trim();

        if pattern.is_empty() {
            return ToolResult {
                output: "Error: No pattern provided".to_string(),
                side_effects: vec![],
                confidence: 0.0,
            };
        }

        match self.find(pattern, &cwd) {
            Ok(results) => {
                if results.is_empty() {
                    ToolResult {
                        output: format!("No files found matching '{}'", pattern),
                        side_effects: vec![],
                        confidence: 1.0,
                    }
                } else {
                    let truncated = if results.len() >= MAX_FILES {
                        format!("{}\n... ({}+ files)", results.join("\n"), MAX_FILES)
                    } else {
                        results.join("\n")
                    };

                    ToolResult {
                        output: format!("Found {} files:\n```\n{}\n```", results.len(), truncated),
                        side_effects: vec![ToolSideEffect::LogMessage(format!(
                            "Coder found {} files matching '{}'",
                            results.len(),
                            pattern
                        ))],
                        confidence: 1.0,
                    }
                }
            }
            Err(e) => ToolResult {
                output: format!("Error: {}", e),
                side_effects: vec![],
                confidence: 0.0,
            },
        }
    }
}

// ============================================================================
// Ls Tool
// ============================================================================

pub struct PiLsTool;

impl PiLsTool {
    pub fn new() -> Self {
        Self
    }
}

impl AgentTool for PiLsTool {
    fn name(&self) -> &str {
        "pi_ls"
    }

    fn description(&self) -> &str {
        "List directory contents.\n\
         Usage: pi_ls [path]"
    }

    fn execute(&self, input: &str, _ctx: &ToolContext) -> ToolResult {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let path_str = input.trim();

        let path = if path_str.is_empty() {
            cwd
        } else {
            cwd.join(path_str)
        };

        match fs::read_dir(&path) {
            Ok(entries) => {
                let mut files = Vec::new();
                let mut dirs = Vec::new();

                for entry in entries.flatten() {
                    let metadata = match entry.metadata() {
                        Ok(m) => m,
                        Err(_) => continue,
                    };

                    let name = entry.file_name().to_string_lossy().to_string();
                    if metadata.is_dir() {
                        dirs.push(format!("{}/", name));
                    } else {
                        files.push(name);
                    }
                }

                dirs.sort();
                files.sort();

                let mut output = format!("Directory: {}\n\n", path.display());

                if !dirs.is_empty() {
                    output.push_str(&format!(
                        "Directories ({}):\n  {}\n\n",
                        dirs.len(),
                        dirs.join("\n  ")
                    ));
                }

                if !files.is_empty() {
                    output.push_str(&format!(
                        "Files ({}):\n  {}",
                        files.len(),
                        files.join("\n  ")
                    ));
                }

                ToolResult {
                    output,
                    side_effects: vec![ToolSideEffect::LogMessage(format!(
                        "Coder listed directory: {}",
                        path.display()
                    ))],
                    confidence: 1.0,
                }
            }
            Err(e) => ToolResult {
                output: format!("Error listing directory: {}", e),
                side_effects: vec![],
                confidence: 0.0,
            },
        }
    }
}

// ============================================================================
// Tool Collection
// ============================================================================

pub fn create_pi_tools() -> Vec<Box<dyn AgentTool>> {
    vec![
        Box::new(PiReadTool::new()),
        Box::new(PiWriteTool::new()),
        Box::new(PiEditTool::new()),
        Box::new(PiBashTool::new()),
        Box::new(PiGrepTool::new()),
        Box::new(PiFindTool::new()),
        Box::new(PiLsTool::new()),
    ]
}
