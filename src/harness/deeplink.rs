//! Deep-link parser for terminal and future desktop shells.

use anyhow::{anyhow, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeepLinkAction {
    pub route: String,
    pub value: Option<String>,
}

pub fn parse_hsm_deeplink(uri: &str) -> Result<DeepLinkAction> {
    let prefix = "hsm://";
    let raw = uri
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow!("deep link must start with hsm://"))?;
    let mut parts = raw.splitn(2, '?');
    let route = parts.next().unwrap_or_default().trim_matches('/').to_string();
    if route.is_empty() {
        return Err(anyhow!("deep link route is empty"));
    }
    let value = parts
        .next()
        .and_then(|qs| qs.split('&').find_map(|kv| kv.strip_prefix("value=").map(|v| v.to_string())));
    Ok(DeepLinkAction { route, value })
}
