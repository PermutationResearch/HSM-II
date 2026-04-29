//! Hardened subprocess environments: do not inherit API keys / `HSM_*` / DB URLs into child processes.

use std::ffi::OsString;

pub fn subprocess_inherit_full_env() -> bool {
    std::env::var("HSM_SUBPROCESS_INHERIT_FULL_ENV")
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn default_path() -> String {
    if let Ok(explicit) = std::env::var("HSM_SUBPROCESS_PATH") {
        return explicit;
    }
    let base = if cfg!(target_os = "macos") {
        "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin"
    } else {
        "/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin"
    };
    let current = std::env::var("PATH").unwrap_or_default();
    let mut seen = std::collections::BTreeSet::<String>::new();
    let mut out = Vec::<String>::new();
    for part in current.split(':').chain(base.split(':')) {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if seen.insert(p.to_string()) {
            out.push(p.to_string());
        }
    }
    if out.is_empty() {
        base.to_string()
    } else {
        out.join(":")
    }
}

fn minimal_env_pairs() -> Vec<(OsString, OsString)> {
    let path: OsString = default_path().into();
    let tmpdir: OsString = std::env::var("HSM_SUBPROCESS_TMPDIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(OsString::from)
        .unwrap_or_else(|| "/tmp".into());
    let home: OsString = std::env::var("HSM_SUBPROCESS_HOME")
        .ok()
        .map(OsString::from)
        .or_else(|| {
            crate::harness::current_root().map(|p| p.as_os_str().to_owned())
        })
        .unwrap_or_else(|| tmpdir.clone());
    let mut out = vec![
        ("PATH".into(), path),
        ("HOME".into(), home),
        ("TMPDIR".into(), tmpdir.clone()),
        ("TEMP".into(), tmpdir.clone()),
        ("LANG".into(), std::env::var_os("LANG").unwrap_or_else(|| "C.UTF-8".into())),
    ];
    if let Ok(lc) = std::env::var("LC_ALL") {
        if !lc.trim().is_empty() {
            out.push(("LC_ALL".into(), lc.into()));
        }
    } else {
        out.push(("LC_ALL".into(), "C.UTF-8".into()));
    }
    out.push(("GIT_CONFIG_NOSYSTEM".into(), "1".into()));
    out.push(("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()));
    out
}

/// Apply [`env_clear`](std::process::Command::env_clear) + tiny allowlist unless `HSM_SUBPROCESS_INHERIT_FULL_ENV=1`.
pub fn apply_minimal_env_std(cmd: &mut std::process::Command) {
    if subprocess_inherit_full_env() {
        return;
    }
    cmd.env_clear();
    for (k, v) in minimal_env_pairs() {
        cmd.env(k, v);
    }
}

/// Tokio variant.
pub fn apply_minimal_env_tokio(cmd: &mut tokio::process::Command) {
    if subprocess_inherit_full_env() {
        return;
    }
    cmd.env_clear();
    for (k, v) in minimal_env_pairs() {
        cmd.env(k, v);
    }
}

/// One-time warning when falling back to host `bash` without Docker isolation.
pub fn warn_host_bash_unsafe_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        tracing::warn!(
            target: "hsm.security",
            "bash/argv is running on the HOST (Docker wrap is default when HSM_UNSAFE_HOST_BASH is unset). Subprocess env is MINIMAL unless HSM_SUBPROCESS_INHERIT_FULL_ENV=1 — never enable full inherit for untrusted agent code."
        );
    });
}
