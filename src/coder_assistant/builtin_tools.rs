//! Built-in tool implementations: read, write, edit, bash, grep, find, ls.

use super::tool_executor::{ToolContext, ToolError};
use crate::config::{limits, security};
use std::process::Command;

use super::sandbox::{execute_bash_with_runtime, is_path_allowed};

/// Dispatch a builtin tool by name. Returns (result, native_enforcement_label).
pub async fn execute_builtin_tool(
    context: &ToolContext,
    tool_name: &str,
    args: &serde_json::Value,
) -> (Result<String, ToolError>, Option<String>) {
    match tool_name {
        "read" => (execute_read(context, args).await, None),
        "write" => (execute_write(context, args).await, None),
        "edit" => (execute_edit(context, args).await, None),
        "bash" => execute_bash(context, args).await,
        "grep" => (execute_grep(context, args).await, None),
        "find" => (execute_find(context, args).await, None),
        "ls" => (execute_ls(context, args).await, None),
        _ => (Err(ToolError::UnknownTool(tool_name.to_string())), None),
    }
}

// ── Read ─────────────────────────────────────────────────────────────

async fn execute_read(context: &ToolContext, args: &serde_json::Value) -> Result<String, ToolError> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArgument("path".to_string()))?;
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(limits::DEFAULT_READ_LINES as u64) as usize;

    let full_path = context.cwd.join(path);
    if !is_path_allowed(context, &full_path) {
        return Err(ToolError::SecurityViolation(
            "Path outside project directory".to_string(),
        ));
    }

    let content = tokio::fs::read_to_string(&full_path)
        .await
        .map_err(|e| ToolError::IoError(format!("Cannot read file: {}", e)))?;

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let start_idx = if offset > 0 { offset - 1 } else { 0 }.min(total_lines);
    let end_idx = (start_idx + limit).min(total_lines);

    let selected: Vec<&str> = lines[start_idx..end_idx].to_vec();
    let result = selected.join("\n");

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

// ── Write ────────────────────────────────────────────────────────────

async fn execute_write(
    context: &ToolContext,
    args: &serde_json::Value,
) -> Result<String, ToolError> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArgument("path".to_string()))?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArgument("content".to_string()))?;

    let full_path = context.cwd.join(path);
    if !is_path_allowed(context, &full_path) {
        return Err(ToolError::SecurityViolation(
            "Path outside project directory".to_string(),
        ));
    }

    if let Some(parent) = full_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ToolError::IoError(format!("Cannot create directories: {}", e)))?;
    }

    tokio::fs::write(&full_path, content)
        .await
        .map_err(|e| ToolError::IoError(format!("Cannot write file: {}", e)))?;

    Ok(format!(
        "Wrote {} bytes to {}",
        content.len(),
        full_path.display()
    ))
}

// ── Edit ─────────────────────────────────────────────────────────────

async fn execute_edit(
    context: &ToolContext,
    args: &serde_json::Value,
) -> Result<String, ToolError> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArgument("path".to_string()))?;
    let old_text = args
        .get("oldText")
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArgument("oldText".to_string()))?;
    let new_text = args
        .get("newText")
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArgument("newText".to_string()))?;

    let full_path = context.cwd.join(path);
    if !is_path_allowed(context, &full_path) {
        return Err(ToolError::SecurityViolation(
            "Path outside project directory".to_string(),
        ));
    }

    let content = tokio::fs::read_to_string(&full_path)
        .await
        .map_err(|e| ToolError::IoError(format!("Cannot read file: {}", e)))?;

    if !content.contains(old_text) {
        return Err(ToolError::ValidationError(format!(
            "oldText not found in file (looking for {} chars)",
            old_text.len()
        )));
    }

    let new_content = content.replacen(old_text, new_text, 1);
    tokio::fs::write(&full_path, new_content)
        .await
        .map_err(|e| ToolError::IoError(format!("Cannot write file: {}", e)))?;

    Ok(format!(
        "Replaced {} chars with {} chars in {}",
        old_text.len(),
        new_text.len(),
        full_path.display()
    ))
}

// ── Bash ─────────────────────────────────────────────────────────────

async fn execute_bash(
    context: &ToolContext,
    args: &serde_json::Value,
) -> (Result<String, ToolError>, Option<String>) {
    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArgument("command".to_string()));
    let command = match command {
        Ok(command) => command,
        Err(err) => return (Err(err), None),
    };

    let timeout_secs = args
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(context.timeout_ms / 1000);

    // Block dangerous commands
    for dangerous in security::DANGEROUS_COMMANDS {
        if command.contains(dangerous) {
            return (
                Err(ToolError::SecurityViolation(format!(
                    "Blocked dangerous command: {}",
                    dangerous
                ))),
                None,
            );
        }
    }

    let (output, native_enforcement) =
        match execute_bash_with_runtime(context, command, timeout_secs).await {
            Ok(value) => value,
            Err(err) => return (Err(err), None),
        };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let result = if stderr.is_empty() {
        stdout.to_string()
    } else if stdout.is_empty() {
        stderr.to_string()
    } else {
        format!("stdout:\n{}\n\nstderr:\n{}", stdout, stderr)
    };

    let truncated = if result.len() > limits::MAX_OUTPUT_CHARS {
        format!(
            "{}... [truncated, total: {} chars]",
            &result[..limits::MAX_OUTPUT_CHARS],
            result.len()
        )
    } else {
        result
    };

    if output.status.success() {
        (Ok(truncated), native_enforcement)
    } else {
        (
            Err(ToolError::CommandFailed {
                exit_code: output.status.code().unwrap_or(-1),
                stderr: truncated,
            }),
            native_enforcement,
        )
    }
}

// ── Grep ─────────────────────────────────────────────────────────────

async fn execute_grep(
    context: &ToolContext,
    args: &serde_json::Value,
) -> Result<String, ToolError> {
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArgument("pattern".to_string()))?;
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let full_path = context.cwd.join(path);

    if !is_path_allowed(context, &full_path) {
        return Err(ToolError::SecurityViolation(
            "Path outside project directory".to_string(),
        ));
    }

    let output = Command::new("rg")
        .args(&["-n", "--max-count", "100", pattern])
        .current_dir(&full_path)
        .output();

    let result = match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
        Err(_) => {
            let output = Command::new("grep")
                .args(&["-rn", "--max-count=100", pattern, "."])
                .current_dir(&full_path)
                .output()
                .map_err(|e| ToolError::IoError(format!("grep failed: {}", e)))?;
            String::from_utf8_lossy(&output.stdout).to_string()
        }
    };

    if result.is_empty() {
        Ok(format!("No matches found for '{}'", pattern))
    } else {
        Ok(result)
    }
}

// ── Find ─────────────────────────────────────────────────────────────

async fn execute_find(
    context: &ToolContext,
    args: &serde_json::Value,
) -> Result<String, ToolError> {
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or(ToolError::MissingArgument("pattern".to_string()))?;

    let output = Command::new("find")
        .args(&[".", "-name", pattern, "-type", "f", "-maxdepth", "5"])
        .current_dir(&context.cwd)
        .output()
        .map_err(|e| ToolError::IoError(format!("find failed: {}", e)))?;

    let result = String::from_utf8_lossy(&output.stdout).to_string();
    if result.is_empty() {
        Ok(format!("No files found matching '{}'", pattern))
    } else {
        Ok(result)
    }
}

// ── Ls ───────────────────────────────────────────────────────────────

async fn execute_ls(
    context: &ToolContext,
    args: &serde_json::Value,
) -> Result<String, ToolError> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let full_path = context.cwd.join(path);

    if !is_path_allowed(context, &full_path) {
        return Err(ToolError::SecurityViolation(
            "Path outside project directory".to_string(),
        ));
    }

    let mut entries = tokio::fs::read_dir(&full_path)
        .await
        .map_err(|e| ToolError::IoError(format!("Cannot read directory: {}", e)))?;

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = entry.metadata().await?;
        if metadata.is_dir() {
            dirs.push(format!("{}/", name));
        } else {
            files.push(name);
        }
    }

    dirs.sort();
    files.sort();

    let mut output = format!("Directory: {}\n\n", full_path.display());
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

    Ok(output)
}
