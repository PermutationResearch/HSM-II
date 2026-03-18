//! macOS sandbox execution and environment curation for bash tools.

use super::security_policy::NetworkBoundary;
use super::tool_executor::{ToolContext, ToolError};
use std::time::Duration;
use tokio::time::timeout;

/// Check if a path is within the allowed working directory.
pub fn is_path_allowed(context: &ToolContext, path: &std::path::Path) -> bool {
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        context.cwd.join(path)
    };
    abs_path.starts_with(&context.cwd)
}

/// Run a bash command, choosing native macOS sandbox when available.
pub async fn execute_bash_with_runtime(
    context: &ToolContext,
    command: &str,
    timeout_secs: u64,
) -> Result<(std::process::Output, Option<String>), ToolError> {
    if should_use_native_macos_sandbox(context) {
        match execute_bash_with_macos_sandbox(context, command, timeout_secs).await {
            Ok(output) => {
                if is_nested_sandbox_rejection(&output) {
                    let fallback = execute_bash_unsandboxed(context, command, timeout_secs).await?;
                    return Ok((
                        fallback,
                        Some("sandbox-exec:fallback-nested-sandbox".to_string()),
                    ));
                }
                return Ok((output, Some("sandbox-exec:file-sandbox".to_string())));
            }
            Err(ToolError::IoError(message))
                if message.contains("sandbox_apply: Operation not permitted") =>
            {
                let output = execute_bash_unsandboxed(context, command, timeout_secs).await?;
                return Ok((
                    output,
                    Some("sandbox-exec:fallback-nested-sandbox".to_string()),
                ));
            }
            Err(err) => return Err(err),
        }
    }

    let output = execute_bash_unsandboxed(context, command, timeout_secs).await?;
    Ok((output, Some("direct-process".to_string())))
}

async fn execute_bash_unsandboxed(
    context: &ToolContext,
    command: &str,
    timeout_secs: u64,
) -> Result<std::process::Output, ToolError> {
    let mut process = tokio::process::Command::new("/bin/bash");
    process
        .arg("-c")
        .arg(command)
        .current_dir(&context.cwd);
    apply_curated_environment(context, &mut process);
    timeout(Duration::from_secs(timeout_secs), process.output())
        .await
        .map_err(|_| ToolError::Timeout)?
        .map_err(|e| ToolError::IoError(format!("Failed to execute: {}", e)))
}

async fn execute_bash_with_macos_sandbox(
    context: &ToolContext,
    command: &str,
    timeout_secs: u64,
) -> Result<std::process::Output, ToolError> {
    let profile = build_macos_sandbox_profile(
        &context.cwd,
        &context.execution_policy.network_boundary,
    );
    let mut process = tokio::process::Command::new("/usr/bin/sandbox-exec");
    process
        .arg("-p")
        .arg(profile)
        .arg("/bin/bash")
        .arg("-c")
        .arg(command)
        .current_dir(&context.cwd);
    apply_curated_environment(context, &mut process);
    timeout(Duration::from_secs(timeout_secs), process.output())
        .await
        .map_err(|_| ToolError::Timeout)?
        .map_err(|e| ToolError::IoError(format!("Failed to execute in native sandbox: {}", e)))
}

fn apply_curated_environment(context: &ToolContext, process: &mut tokio::process::Command) {
    process.env_clear();
    for (key, value) in curated_passthrough_env() {
        process.env(key, value);
    }
    for key in &context.execution_policy.secret_boundary.injected_env_keys {
        if let Some(value) = context.env_vars.get(key) {
            process.env(key, value);
        }
    }
}

fn should_use_native_macos_sandbox(context: &ToolContext) -> bool {
    cfg!(target_os = "macos")
        && matches!(
            context.execution_policy.sandbox_mode,
            super::security_policy::SandboxMode::WorkspaceWrite
        )
        && std::path::Path::new("/usr/bin/sandbox-exec").exists()
}

fn build_macos_sandbox_profile(
    cwd: &std::path::Path,
    network_boundary: &NetworkBoundary,
) -> String {
    let mut rules = vec![
        "(version 1)".to_string(),
        "(deny default)".to_string(),
        "(allow process*)".to_string(),
        "(allow sysctl-read)".to_string(),
        "(allow file-read*)".to_string(),
        format!(
            "(allow file-write* (subpath \"{}\"))",
            escape_sandbox_path(cwd)
        ),
    ];

    if let Ok(tmpdir) = std::env::var("TMPDIR") {
        rules.push(format!(
            "(allow file-write* (subpath \"{}\"))",
            escape_sandbox_path(std::path::Path::new(&tmpdir))
        ));
    }
    rules.push("(allow file-write* (subpath \"/private/tmp\"))".to_string());

    if network_boundary.block_network_for_bash || network_boundary.allowed_hosts.is_empty() {
        rules.push("(deny network*)".to_string());
    } else {
        rules.push("(allow network*)".to_string());
    }

    rules.join(" ")
}

fn escape_sandbox_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn curated_passthrough_env() -> Vec<(String, String)> {
    let keys = [
        "PATH", "HOME", "TMPDIR", "USER", "LOGNAME", "LANG", "LC_ALL",
    ];
    keys.iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect()
}

fn is_nested_sandbox_rejection(output: &std::process::Output) -> bool {
    output.status.code() == Some(71)
        && String::from_utf8_lossy(&output.stderr)
            .contains("sandbox_apply: Operation not permitted")
}
