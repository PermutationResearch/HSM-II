//! Optional [**Anthropic Sandbox Runtime**](https://github.com/anthropic-experimental/sandbox-runtime) (`srt`)
//! wrapping for host `bash` / `argv` execution — Seatbelt on macOS, bubblewrap on Linux.
//!
//! Install the CLI (not bundled in this crate):
//! ```text
//! npm install -g @anthropic-ai/sandbox-runtime
//! ```
//!
//! Enable for **host** bash (when Docker bash is off):
//! - `HSM_SRT=1` or `HSM_TOOL_SANDBOX=srt`
//! - Optional: `HSM_SRT_BIN=/path/to/srt`
//! - Optional: `HSM_SRT_ALLOWED_DOMAINS=api.openrouter.ai,github.com` (comma-separated).  
//!   If unset or empty, **no outbound network** inside the sandbox (strict default).
//! - Optional: extra writable roots — `HSM_SRT_EXTRA_ALLOW_WRITE` (comma-separated absolute paths).
//!
//! **Path policy (read/write/edit):** when the same env is set, [`check_srt_write_allowed`] rejects
//! paths outside the collected allow-list (used for writes **and** `read` under SRT), and
//! [`commit_bytes_via_srt`] stages then **`cp`s under `srt`**
//! (in addition to `resolve_tool_fs_path` / thread workspace rules).
//!
//! **Upstream `srt` dependency:** the Anthropic CLI expects **`rg` (ripgrep)** on `PATH` for some
//! Linux/macOS checks. Install ripgrep (`brew install ripgrep`) if `srt` errors with “rg not found”.

use serde_json::json;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use uuid::Uuid;

/// True when host bash should be wrapped with `srt --settings <file> …`.
pub fn srt_sandbox_enabled() -> bool {
    fn truthy(raw: &str) -> bool {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }
    if std::env::var("HSM_SRT")
        .map(|v| truthy(&v))
        .unwrap_or(false)
    {
        return true;
    }
    std::env::var("HSM_TOOL_SANDBOX")
        .map(|v| v.trim().eq_ignore_ascii_case("srt"))
        .unwrap_or(false)
}

pub fn srt_executable() -> PathBuf {
    match std::env::var("HSM_SRT_BIN") {
        Ok(s) if !s.trim().is_empty() => PathBuf::from(s),
        _ => PathBuf::from("srt"),
    }
}

fn is_subpath(base: &Path, candidate: &Path) -> bool {
    let mut bc = base.components();
    let mut cc = candidate.components();
    loop {
        match (bc.next(), cc.next()) {
            (None, None) => return true,
            (None, Some(_)) => return true,
            (Some(_), None) => return false,
            (Some(a), Some(b)) if a == b => continue,
            _ => return false,
        }
    }
}

/// Writable roots for `filesystem.allowWrite` and for [`check_srt_write_allowed`].
///
/// Includes [`super::current_root()`] so Company OS `execute-worker` task workspaces (set via
/// `Message.thread_workspace_root` / `activate_thread_workspace_at`) stay readable/writable when `HSM_SRT=1`.
pub fn collect_allow_write_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Some(r) = super::current_root() {
        out.push(r);
    }
    if let Ok(cwd) = std::env::current_dir() {
        out.push(cwd);
    }
    if let Ok(pwd) = std::env::var("PWD") {
        let pb = PathBuf::from(pwd);
        if !out.iter().any(|x| x == &pb) {
            out.push(pb);
        }
    }
    if let Ok(h) = std::env::var("HSMII_HOME") {
        let pb = PathBuf::from(h);
        if !pb.as_os_str().is_empty() && !out.iter().any(|x| x == &pb) {
            out.push(pb);
        }
    }
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target"));
    if !out.iter().any(|x| x == &target) {
        out.push(target);
    }
    let kb = PathBuf::from("kb");
    if !out.iter().any(|x| x == &kb) {
        out.push(kb);
    }
    let cf = PathBuf::from("company-files");
    if !out.iter().any(|x| x == &cf) {
        out.push(cf);
    }
    if let Ok(extra) = std::env::var("HSM_SRT_EXTRA_ALLOW_WRITE") {
        for p in extra.split(',') {
            let t = p.trim();
            if !t.is_empty() {
                let pb = PathBuf::from(t);
                if !out.iter().any(|x| x == &pb) {
                    out.push(pb);
                }
            }
        }
    }
    // Typical temp for builds / npm
    out.push(PathBuf::from("/tmp"));
    out
}

fn normalize_root(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Serialize paths for `srt-settings.json` — prefer absolute, canonical when possible.
fn allow_write_strings() -> Vec<String> {
    collect_allow_write_paths()
        .into_iter()
        .map(|p| normalize_root(&p).display().to_string())
        .collect()
}

fn allowed_network_domains() -> Vec<String> {
    std::env::var("HSM_SRT_ALLOWED_DOMAINS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Build JSON compatible with Anthropic's documented `~/.srt-settings.json` shape.
pub fn srt_settings_value() -> serde_json::Value {
    let allow_write: Vec<String> = allow_write_strings();
    json!({
        "network": {
            "allowedDomains": allowed_network_domains(),
            "deniedDomains": [],
            "allowLocalBinding": false
        },
        "filesystem": {
            "denyRead": ["~/.ssh", "~/.aws", "~/.gnupg", "~/.config/gcloud"],
            "allowRead": [],
            "allowWrite": allow_write,
            "denyWrite": [".env", ".env.local", ".git/config", ".git/hooks"]
        },
        "ignoreViolations": {},
        "enableWeakerNestedSandbox": false,
        "enableWeakerNetworkIsolation": false
    })
}

fn write_ephemeral_srt_settings_path() -> io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("hsm-srt-settings-{}.json", Uuid::new_v4()));
    let mut f = fs::File::create(&path)?;
    let v = srt_settings_value();
    let body =
        serde_json::to_string_pretty(&v).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    f.write_all(body.as_bytes())?;
    f.flush()?;
    Ok(path)
}

/// When [`srt_sandbox_enabled`], path (or its parent for new files) must sit under one allow-write root.
pub fn check_srt_write_allowed(path: &Path) -> Result<(), String> {
    if !srt_sandbox_enabled() {
        return Ok(());
    }
    let roots: Vec<PathBuf> = collect_allow_write_paths()
        .into_iter()
        .map(|p| normalize_root(&p))
        .collect();

    let candidate = if path.exists() {
        normalize_root(path)
    } else if let Some(parent) = path.parent() {
        normalize_root(parent)
    } else {
        return Err("invalid path (no parent)".into());
    };

    for base in &roots {
        if is_subpath(base, &candidate) {
            return Ok(());
        }
    }
    Err(format!(
        "path outside HSM_SRT allow-write roots (set HSM_SRT_EXTRA_ALLOW_WRITE or disable HSM_SRT): {}",
        path.display()
    ))
}

/// Run `bash -c` under `srt` (blocking). Caller must not use Docker bash simultaneously.
pub fn run_srt_bash_blocking(
    command: &str,
    cwd: Option<&Path>,
) -> Result<(String, String, i32), String> {
    let settings_path = write_ephemeral_srt_settings_path().map_err(|e: io::Error| e.to_string())?;

    let mut cmd = Command::new(srt_executable());
    cmd.arg("--settings").arg(&settings_path);
    cmd.arg("bash").arg("-c").arg(command);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    crate::tools::subprocess_env::apply_minimal_env_std(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("srt bash failed to spawn (is `srt` installed? npm i -g @anthropic-ai/sandbox-runtime): {e}"))?;
    let _ = fs::remove_file(&settings_path);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    Ok((stdout, stderr, code))
}

/// Run argv under `srt` (blocking).
pub fn run_srt_argv_blocking(
    argv: &[String],
    cwd: Option<&Path>,
) -> Result<(String, String, i32), String> {
    if argv.is_empty() || argv[0].is_empty() {
        return Err("argv must be non-empty".into());
    }
    let settings_path = write_ephemeral_srt_settings_path().map_err(|e: io::Error| e.to_string())?;

    let mut cmd = Command::new(srt_executable());
    cmd.arg("--settings").arg(&settings_path);
    for a in argv {
        cmd.arg(a);
    }
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    crate::tools::subprocess_env::apply_minimal_env_std(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("srt argv failed to spawn (is `srt` installed?): {e}"))?;
    let _ = fs::remove_file(&settings_path);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    Ok((stdout, stderr, code))
}

/// Stage bytes in host `/tmp`, then `cp` into `dest` **inside** `srt` (same Seatbelt/bubblewrap as bash).
/// Used by `write` / `edit` when [`srt_sandbox_enabled`] so file bytes hit disk only from a sandboxed process.
pub fn commit_bytes_via_srt(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("hsm-srt-staging-{}", Uuid::new_v4()));
    fs::write(&tmp, bytes).map_err(|e| format!("host staging write failed: {e}"))?;
    let from = fs::canonicalize(&tmp).map_err(|e| format!("canonicalize staging file: {e}"))?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create_dir_all {}: {e}", parent.display()))?;
    }
    let dest_canon = if dest.exists() {
        fs::canonicalize(dest).map_err(|e| format!("canonicalize dest: {e}"))?
    } else if let Some(parent) = dest.parent() {
        let pn = fs::canonicalize(parent).map_err(|e| format!("canonicalize parent: {e}"))?;
        pn.join(
            dest.file_name()
                .ok_or_else(|| "destination path has no file name".to_string())?,
        )
    } else {
        return Err("destination path has no parent".into());
    };

    let argv = vec![
        "cp".to_string(),
        from.to_string_lossy().into_owned(),
        dest_canon.to_string_lossy().into_owned(),
    ];
    let (stdout, stderr, code) = run_srt_argv_blocking(&argv, None)?;
    let _ = fs::remove_file(&tmp);
    if code != 0 {
        return Err(format!(
            "srt cp failed (exit {code}): stderr={stderr} stdout={stdout}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_settings_serializes() {
        let v = srt_settings_value();
        let s = serde_json::to_string(&v).expect("serialize");
        assert!(s.contains("filesystem"));
        assert!(s.contains("network"));
    }
}
