//! Shell Operation Tools
//!
//! Bash execution, grep search, and file finding.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use serde_json::Value;
use tracing::debug;

use super::{object_schema, Tool, ToolOutput};
use crate::tools::security::sanitize_working_dir_input;
use crate::tools::subprocess_env::{apply_minimal_env_std, warn_host_bash_unsafe_once};

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

    fn argv_allowlist() -> Option<HashSet<String>> {
        let raw = std::env::var("HSM_BASH_ARGV_ALLOWLIST").unwrap_or_default();
        let set: HashSet<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if set.is_empty() {
            None
        } else {
            Some(set)
        }
    }

    fn bash_argv_only_from_env() -> bool {
        std::env::var("HSM_BASH_ARGV_ONLY")
            .map(|v| {
                let s = v.trim();
                s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(false)
    }

    fn execute_argv_blocking(
        argv: Vec<String>,
        working_dir: Option<String>,
    ) -> Result<(String, String, i32), String> {
        if crate::harness::srt_sandbox_enabled() {
            let cwd = working_dir.as_deref().map(Path::new);
            return crate::harness::run_srt_argv_blocking(&argv, cwd);
        }
        if argv.is_empty() || argv[0].is_empty() {
            return Err("argv must be a non-empty array with a program path or name first".into());
        }
        let prog_path = std::path::Path::new(&argv[0]);
        let base = prog_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(argv[0].as_str());
        if let Some(set) = Self::argv_allowlist() {
            if !set.contains(base) {
                return Err(format!(
                    "program `{base}` not allowed by HSM_BASH_ARGV_ALLOWLIST"
                ));
            }
        }
        let cwd = working_dir.as_deref().map(std::path::Path::new);
        warn_host_bash_unsafe_once();
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        if let Some(c) = cwd {
            cmd.current_dir(c);
        }
        apply_minimal_env_std(&mut cmd);
        let output = cmd
            .output()
            .map_err(|e| format!("Failed to execute argv command: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let code = output.status.code().unwrap_or(-1);
        Ok((stdout, stderr, code))
    }

    fn format_bash_triple(stdout: String, stderr: String, code: i32) -> ToolOutput {
        let stdout_len = stdout.len();
        let stderr_len = stderr.len();

        let truncated_stdout = if stdout.len() > MAX_OUTPUT_SIZE {
            format!(
                "{}...\n[Output truncated, total: {} bytes]",
                &stdout[..MAX_OUTPUT_SIZE],
                stdout.len()
            )
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
            format!(
                "Command failed (exit code {})\n\nstdout:\n{}\n\nstderr:\n{}",
                code, truncated_stdout, stderr
            )
        };

        ToolOutput::success(result_text).with_metadata(serde_json::json!({
            "exit_code": code,
            "stdout_bytes": stdout_len,
            "stderr_bytes": stderr_len,
        }))
    }

    fn execute_bash_blocking(
        command: String,
        working_dir: Option<String>,
    ) -> Result<(String, String, i32), String> {
        if crate::harness::srt_sandbox_enabled() {
            let cwd = working_dir.as_deref().map(Path::new);
            return crate::harness::run_srt_bash_blocking(&command, cwd);
        }
        warn_host_bash_unsafe_once();
        let cwd = working_dir.as_deref().map(std::path::Path::new);
        let mut cmd = crate::harness::host_bash_command(&command, cwd);

        let output = cmd
            .output()
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
        "Execute a bash command or (preferred) argv-only: pass `argv` as [\"prog\", \"arg1\", ...]. Use with caution. 30s timeout. HSM_BASH_POLICY=strict, HSM_BASH_ARGV_ONLY=1, HSM_BASH_ARGV_ALLOWLIST=ls,git. Isolation: Docker by default for shell commands (HSM_THREAD_WORKSPACE=1); argv runs on host with minimal env. Host hardening: HSM_BASH_ISOLATE=firejail|unshare, HSM_SRT=1 wraps host bash/argv with Anthropic `srt` (npm i -g @anthropic-ai/sandbox-runtime; see src/harness/srt_sandbox.rs). HSM_DOCKER_BASH=0, HSM_UNSAFE_HOST_BASH=1."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            (
                "command",
                "Shell command (ignored when `argv` is set; required unless argv-only mode)",
                false,
            ),
            (
                "argv",
                "Optional: execute without shell — JSON array of strings, e.g. [\"ls\", \"-la\"]",
                false,
            ),
            (
                "working_dir",
                "Working directory for the command (optional)",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let argv_param: Option<Vec<String>> = params
            .get("argv")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());

        if Self::bash_argv_only_from_env() && argv_param.is_none() {
            return ToolOutput::error(
                "HSM_BASH_ARGV_ONLY is set: provide `argv` as a JSON array of strings (no shell).",
            );
        }

        let cwd: Option<String> = match params.get("working_dir").and_then(|v| v.as_str()) {
            Some(w) => match crate::harness::resolve_tool_fs_path(w) {
                Ok(p) => match sanitize_working_dir_input(&p.to_string_lossy()) {
                    Ok(_) => Some(p.to_string_lossy().into_owned()),
                    Err(e) => return ToolOutput::error(e),
                },
                Err(e) => return ToolOutput::error(e),
            },
            None => {
                if crate::harness::current_root().is_some() {
                    match crate::harness::resolve_tool_fs_path(".") {
                        Ok(p) => match sanitize_working_dir_input(&p.to_string_lossy()) {
                            Ok(_) => Some(p.to_string_lossy().into_owned()),
                            Err(e) => return ToolOutput::error(e),
                        },
                        Err(e) => return ToolOutput::error(e),
                    }
                } else {
                    None
                }
            }
        };

        if let Some(argv) = argv_param {
            if crate::harness::docker_bash_enabled() {
                return ToolOutput::error(
                    "argv runs on the host process; disable container bash first (HSM_DOCKER_BASH=0 or HSM_UNSAFE_HOST_BASH=1), or use the shell command field to run inside Docker when enabled.",
                );
            }
            debug!("Executing argv: {:?}", argv);
            let timeout = tokio::time::Duration::from_secs(CMD_TIMEOUT_SECS);
            let argv_clone = argv.clone();
            let wd = cwd.clone();
            let result = tokio::time::timeout(
                timeout,
                tokio::task::spawn_blocking(move || Self::execute_argv_blocking(argv_clone, wd)),
            )
            .await;
            return match result {
                Err(_) => ToolOutput::error(format!(
                    "argv command timed out after {} seconds",
                    CMD_TIMEOUT_SECS
                )),
                Ok(Err(e)) => ToolOutput::error(format!("argv task failed: {e}")),
                Ok(Ok(Err(e))) => ToolOutput::error(e),
                Ok(Ok(Ok((stdout, stderr, code)))) => {
                    Self::format_bash_triple(stdout, stderr, code)
                }
            };
        }

        let command = params
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if command.is_empty() {
            return ToolOutput::error("Command parameter is required (or pass argv array)");
        }

        let bash_policy = crate::tools::bash_policy::bash_policy_from_env();
        if let Err(e) = crate::tools::bash_policy::validate_bash_command(&command, bash_policy) {
            return ToolOutput::error(e);
        }

        debug!("Executing bash: {}", command);

        let timeout = tokio::time::Duration::from_secs(CMD_TIMEOUT_SECS);
        let cmd_for_docker = command.clone();
        let result = tokio::time::timeout(timeout, async move {
            if crate::harness::docker_bash_enabled() {
                crate::harness::run_in_docker(&cmd_for_docker, cwd.as_deref()).await
            } else {
                let cmd_inner = command;
                let wd = cwd;
                tokio::task::spawn_blocking(move || Self::execute_bash_blocking(cmd_inner, wd))
                    .await
                    .unwrap_or_else(|e| Err(format!("bash task failed: {e}")))
            }
        })
        .await;

        match result {
            Ok(Ok((stdout, stderr, code))) => Self::format_bash_triple(stdout, stderr, code),
            Ok(Err(e)) => ToolOutput::error(e),
            Err(_) => ToolOutput::error(format!(
                "Command timed out after {} seconds",
                CMD_TIMEOUT_SECS
            )),
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

    fn grep_internal(
        &self,
        pattern: &str,
        path: &str,
        file_pattern: Option<&str>,
    ) -> Result<String, String> {
        let mut cmd = Command::new("grep");
        apply_minimal_env_std(&mut cmd);
        cmd.arg("-r")
            .arg("-n")
            .arg("-I") // Ignore binary files
            .arg("--include")
            .arg(file_pattern.unwrap_or("*"))
            .arg(pattern)
            .arg(path);

        let output = cmd
            .output()
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
            Ok(format!(
                "{}\n\n[Showing {} of {} matches]",
                result, MAX_GREP_RESULTS, total
            ))
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
            (
                "path",
                "Directory or file to search in (default: current directory)",
                false,
            ),
            (
                "include",
                "File pattern to include (e.g., '*.rs', default: all files)",
                false,
            ),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let pattern = params.get("pattern").and_then(|v| v.as_str()).unwrap_or("");

        if pattern.is_empty() {
            return ToolOutput::error("Pattern parameter is required");
        }

        let path_raw = params.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let path = match crate::harness::resolve_tool_fs_path(path_raw) {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(e),
        };
        let path_s = path.to_string_lossy().to_string();

        let include = params.get("include").and_then(|v| v.as_str());

        debug!("Grep: {} in {} (include: {:?})", pattern, path_s, include);

        match self.grep_internal(pattern, &path_s, include) {
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

    fn find_internal(
        &self,
        path: &str,
        name_pattern: Option<&str>,
        file_type: Option<&str>,
    ) -> Result<String, String> {
        let mut cmd = Command::new("find");
        apply_minimal_env_std(&mut cmd);
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

        let output = cmd
            .output()
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
            Ok(format!(
                "{}\n\n[Showing {} of {} results]",
                result, MAX_FIND_RESULTS, total
            ))
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
            (
                "path",
                "Directory to search in (default: current directory)",
                false,
            ),
            ("name", "File name pattern (e.g., '*.rs', 'Cargo.*')", false),
            ("type", "Type: 'f' for files, 'd' for directories", false),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let path_raw = params.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let path = match crate::harness::resolve_tool_fs_path(path_raw) {
            Ok(p) => p,
            Err(e) => return ToolOutput::error(e),
        };
        let path_s = path.to_string_lossy().to_string();

        let name = params.get("name").and_then(|v| v.as_str());

        let file_type = params.get("type").and_then(|v| v.as_str());

        debug!(
            "Find: in {} (name: {:?}, type: {:?})",
            path_s, name, file_type
        );

        match self.find_internal(&path_s, name, file_type) {
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
