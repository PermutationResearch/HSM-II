use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConnectorPolicy {
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[serde(default)]
    pub blocked_methods: Vec<String>,
    #[serde(default)]
    pub auth_env_var: Option<String>,
    #[serde(default)]
    pub auth_header: Option<String>,
    #[serde(default)]
    pub auth_prefix: Option<String>,
}

fn policy_map() -> &'static HashMap<String, ConnectorPolicy> {
    static CACHE: OnceLock<HashMap<String, ConnectorPolicy>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let raw = std::env::var("HSM_CONNECTOR_POLICIES").unwrap_or_default();
        if raw.trim().is_empty() {
            return HashMap::new();
        }
        serde_json::from_str::<HashMap<String, ConnectorPolicy>>(&raw).unwrap_or_default()
    })
}

pub fn policy_for(connector_ref: &str) -> Option<ConnectorPolicy> {
    let key = connector_ref.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }
    policy_map().get(&key).cloned()
}

pub fn enforce_policy(
    connector_ref: &str,
    method: &str,
    host: &str,
) -> Result<(), String> {
    let Some(policy) = policy_for(connector_ref) else {
        return Ok(());
    };
    let host_l = host.trim().to_ascii_lowercase();
    let method_l = method.trim().to_ascii_lowercase();
    if policy
        .blocked_methods
        .iter()
        .any(|m| m.trim().eq_ignore_ascii_case(&method_l))
    {
        return Err(format!(
            "connector `{}` policy blocks {}",
            connector_ref, method
        ));
    }
    if !policy.allowed_hosts.is_empty() {
        let ok = policy.allowed_hosts.iter().any(|h| {
            let hh = h.trim().to_ascii_lowercase();
            host_l == hh || host_l.ends_with(&format!(".{hh}"))
        });
        if !ok {
            return Err(format!(
                "connector `{}` policy disallows host `{}`",
                connector_ref, host
            ));
        }
    }
    Ok(())
}

pub fn auth_header(connector_ref: &str) -> Option<(String, String)> {
    let policy = policy_for(connector_ref)?;
    let env_name = policy.auth_env_var.as_deref()?.trim();
    if env_name.is_empty() {
        return None;
    }
    let token = std::env::var(env_name).ok()?.trim().to_string();
    if token.is_empty() {
        return None;
    }
    let header = policy
        .auth_header
        .unwrap_or_else(|| "Authorization".to_string());
    let prefix = policy.auth_prefix.unwrap_or_else(|| "Bearer".to_string());
    Some((header, format!("{} {}", prefix.trim(), token)))
}
