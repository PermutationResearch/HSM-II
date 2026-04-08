//! Optional high-risk bash isolation: run commands in Docker with the thread workspace mounted at `/ws`.

use std::process::Stdio;

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

pub fn docker_bash_enabled() -> bool {
    env_truthy("HSM_DOCKER_BASH")
}

/// When true, `docker run` uses `--network none` (no egress). Enable with `HSM_DOCKER_BASH_NETWORK=none`.
fn docker_network_is_none() -> bool {
    std::env::var("HSM_DOCKER_BASH_NETWORK")
        .map(|v| v.trim().eq_ignore_ascii_case("none"))
        .unwrap_or(false)
}

pub async fn run_in_docker(
    command: &str,
    working_dir: Option<&str>,
) -> Result<(String, String, i32), String> {
    let root = super::thread_workspace::current_root().ok_or_else(|| {
        "HSM_DOCKER_BASH requires an active thread workspace (enable HSM_THREAD_WORKSPACE=1)"
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
    if docker_network_is_none() {
        cmd.args(["--network", "none"]);
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
