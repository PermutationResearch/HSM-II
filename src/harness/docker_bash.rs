//! Default `bash` tool isolation: `docker run` with the active thread workspace mounted at `/ws` (opt out via env).

use std::process::Stdio;

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// Prefer `docker run` for the `bash` tool when an active thread workspace exists.
///
/// - Default: **on** (set `HSM_DOCKER_BASH=0` to disable).
/// - `HSM_UNSAFE_HOST_BASH=1` disables Docker and forces host `bash` (local dev escape hatch).
pub fn docker_bash_enabled() -> bool {
    if env_truthy("HSM_UNSAFE_HOST_BASH") {
        return false;
    }
    match std::env::var("HSM_DOCKER_BASH") {
        Ok(v) => {
            let s = v.trim();
            if s.is_empty() {
                return true;
            }
            let l = s.to_ascii_lowercase();
            if l == "0" || l == "false" || l == "no" || l == "off" {
                return false;
            }
            true
        }
        Err(_) => true,
    }
}

/// Extra `docker run` network args. Default / empty: **`--network none`**. Set `HSM_DOCKER_BASH_NETWORK=bridge`
/// (or `host`, …) for egress; `default` omits `--network` (Docker daemon default).
fn docker_network_run_args() -> Vec<String> {
    let raw = std::env::var("HSM_DOCKER_BASH_NETWORK").unwrap_or_default();
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() || s == "none" {
        return vec!["--network".into(), "none".into()];
    }
    if s == "default" {
        return vec![];
    }
    vec!["--network".into(), raw.trim().to_string()]
}

pub async fn run_in_docker(
    command: &str,
    working_dir: Option<&str>,
) -> Result<(String, String, i32), String> {
    let root = super::thread_workspace::current_root().ok_or_else(|| {
        "Docker bash requires an active thread workspace (enable HSM_THREAD_WORKSPACE=1) or disable container wrap (HSM_DOCKER_BASH=0 / HSM_UNSAFE_HOST_BASH=1 for local dev)."
            .to_string()
    })?;

    let image = std::env::var("HSM_DOCKER_IMAGE").unwrap_or_else(|_| "alpine:3.20".into());
    let host_root = root.to_string_lossy().to_string();

    let work_in_container = match working_dir {
        Some(w) => {
            let p = super::thread_workspace::resolve_tool_fs_path(w)?;
            let rel = p
                .strip_prefix(&root)
                .map_err(|_| "working_dir must be under the active thread workspace".to_string())?;
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            format!("/ws/{}", rel_s.trim_start_matches('/'))
        }
        None => "/ws".to_string(),
    };

    let mut cmd = tokio::process::Command::new("docker");
    cmd.arg("run").arg("--rm");
    for a in docker_network_run_args() {
        cmd.arg(a);
    }
    cmd.args([
        "-v",
        &format!("{}:/ws:rw", host_root),
        "-w",
        &work_in_container,
        &image,
        "sh",
        "-c",
        command,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("docker run failed: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    Ok((stdout, stderr, code))
}
