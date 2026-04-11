use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn private_egress_allowed() -> bool {
    env_truthy("HSM_ALLOW_PRIVATE_EGRESS")
}

fn blocked_hostname(host: &str) -> bool {
    let h = host.trim().to_ascii_lowercase();
    if h.is_empty() {
        return true;
    }
    if h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") {
        return true;
    }
    if h == "host.docker.internal" || h == "gateway.docker.internal" {
        return true;
    }
    false
}

fn blocked_ip(ip: IpAddr) -> bool {
    if private_egress_allowed() {
        return false;
    }
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
    }
}

pub fn validate_outbound_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|e| format!("invalid URL: {e}"))?;
    let scheme = url.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err("only http/https URLs are allowed".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "URL must include a host".to_string())?;
    if blocked_hostname(host) && !private_egress_allowed() {
        return Err("URL host is blocked by SSRF policy".to_string());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if blocked_ip(ip) {
            return Err("private or local IPs are blocked by SSRF policy".to_string());
        }
    }
    Ok(url)
}

pub fn sanitize_working_dir_input(raw: &str) -> Result<&str, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Err("working_dir is empty".to_string());
    }
    if t.contains('\0') || t.contains('\n') || t.contains('\r') {
        return Err("working_dir contains disallowed control characters".to_string());
    }
    Ok(t)
}

/// When set, [`deny_sensitive_tool_path`] allows home-dir secret locations (not recommended).
pub fn sensitive_paths_allowed() -> bool {
    env_truthy("HSM_ALLOW_SENSITIVE_PATHS")
}

/// Expand `~/` using the process home dir; otherwise return `path` as [`PathBuf`].
pub fn expand_user_path_hint(raw: &str) -> PathBuf {
    let t = raw.trim();
    if let Some(rest) = t.strip_prefix("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    }
    PathBuf::from(t)
}

/// Block obvious operator-secret paths (`.ssh`, cloud CLIs, `.env`) when tools touch the host FS.
pub fn deny_sensitive_tool_path(p: &Path) -> Result<(), String> {
    if sensitive_paths_allowed() {
        return Ok(());
    }
    let Some(home) = dirs::home_dir() else {
        return Ok(());
    };
    let canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let Ok(rel) = canon.strip_prefix(&home) else {
        return Ok(());
    };
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    let first = rel
        .components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .unwrap_or("");
    let blocked_prefixes = [
        ".ssh",
        ".kube",
        ".aws",
        ".docker",
        ".config/gcloud",
        ".azure",
        ".local/share/keyrings",
    ];
    if blocked_prefixes
        .iter()
        .any(|pre| rel_s == *pre || rel_s.starts_with(&format!("{pre}/")))
    {
        return Err(format!(
            "refusing path under home secret/config area: {} (set HSM_ALLOW_SENSITIVE_PATHS=1 to override — not recommended)",
            canon.display()
        ));
    }
    if first == ".env" || rel_s.ends_with("/.env") || rel_s.contains("/.env/") {
        return Err(format!(
            "refusing .env under home: {} (set HSM_ALLOW_SENSITIVE_PATHS=1 to override)",
            canon.display()
        ));
    }
    if rel_s == ".git-credentials" || rel_s.ends_with("/.git-credentials") {
        return Err(format!(
            "refusing git credentials file: {}",
            canon.display()
        ));
    }
    if rel_s == ".netrc" || rel_s.ends_with("/.netrc") {
        return Err(format!("refusing .netrc: {}", canon.display()));
    }
    Ok(())
}

pub fn validate_archive_member_path(entry: &str) -> Result<(), String> {
    let e = entry.trim();
    if e.is_empty() {
        return Ok(());
    }
    let p = Path::new(e);
    if p.is_absolute() {
        return Err(format!("archive entry is absolute path: {e}"));
    }
    for c in p.components() {
        if matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
            return Err(format!("archive entry escapes destination: {e}"));
        }
    }
    Ok(())
}
