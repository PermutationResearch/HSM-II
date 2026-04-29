//! Credential validity checker with provider-specific adapters.
//!
//! [`verify_credential`] performs a live HTTP probe for a given provider and returns
//! a [`CredentialStatus`] indicating whether the secret is still valid and whether
//! re-auth is recommended.
//!
//! [`verify_all`] runs multiple checks concurrently via `futures::future::join_all`.
//!
//! # Adapters
//!
//! | Adapter | Probe endpoint |
//! |---------|---------------|
//! | `BearerHttp { url }` | GET `url` — expects 2xx |
//! | `Supabase { project_url }` | GET `{url}/rest/v1/` with `apikey` header |
//! | `OpenAI` | GET `https://api.openai.com/v1/models` |
//! | `GitHub` | GET `https://api.github.com/user` |
//! | `Slack` | POST `https://slack.com/api/auth.test` |
//! | `NoOp` | Always valid — use for offline/airgapped setups |

use std::time::Duration;

// ── Public types ──────────────────────────────────────────────────────────────

/// Result of a credential liveness check.
#[derive(Debug, Clone)]
pub struct CredentialStatus {
    pub provider_key: String,
    pub valid: bool,
    pub http_status: Option<u16>,
    /// True when the probe indicates the token must be rotated (e.g. HTTP 401).
    pub needs_reauth: bool,
    pub error: Option<String>,
}

/// Selects the HTTP probe strategy for a given provider.
#[derive(Debug, Clone)]
pub enum ProviderAdapter {
    /// Generic authenticated GET: expects any 2xx response.
    BearerHttp { url: String },
    /// Supabase REST API: GET `{project_url}/rest/v1/` with `apikey` + `Authorization` headers.
    Supabase { project_url: String },
    /// OpenAI: GET `/v1/models`.
    OpenAI,
    /// GitHub: GET `/user`.
    GitHub,
    /// Slack: POST `/api/auth.test` (Slack-specific JSON `ok` field check).
    Slack,
    /// Skip the live probe — assume valid. Use for offline or airgapped setups.
    NoOp,
}

// ── Core ──────────────────────────────────────────────────────────────────────

/// Verify a single credential against its provider.
pub async fn verify_credential(
    provider_key: &str,
    secret: &str,
    adapter: &ProviderAdapter,
) -> CredentialStatus {
    match adapter {
        ProviderAdapter::NoOp => CredentialStatus {
            provider_key: provider_key.to_string(),
            valid: true,
            http_status: None,
            needs_reauth: false,
            error: None,
        },
        ProviderAdapter::BearerHttp { url } => {
            bearer_check(provider_key, secret, url).await
        }
        ProviderAdapter::Supabase { project_url } => {
            supabase_check(provider_key, secret, project_url).await
        }
        ProviderAdapter::OpenAI => openai_check(provider_key, secret).await,
        ProviderAdapter::GitHub => github_check(provider_key, secret).await,
        ProviderAdapter::Slack => slack_check(provider_key, secret).await,
    }
}

/// Verify multiple credentials concurrently.
pub async fn verify_all(
    checks: Vec<(String, String, ProviderAdapter)>,
) -> Vec<CredentialStatus> {
    futures::future::join_all(checks.into_iter().map(|(pk, secret, adapter)| async move {
        verify_credential(&pk, &secret, &adapter).await
    }))
    .await
}

// ── Provider adapters ─────────────────────────────────────────────────────────

async fn bearer_check(provider_key: &str, secret: &str, url: &str) -> CredentialStatus {
    match client()
        .get(url)
        .header("Authorization", format!("Bearer {secret}"))
        .send()
        .await
    {
        Ok(resp) => {
            let code = resp.status().as_u16();
            CredentialStatus {
                provider_key: provider_key.to_string(),
                valid: resp.status().is_success(),
                http_status: Some(code),
                needs_reauth: code == 401 || code == 403,
                error: if !resp.status().is_success() {
                    Some(format!("HTTP {code}"))
                } else {
                    None
                },
            }
        }
        Err(e) => net_err(provider_key, e),
    }
}

async fn supabase_check(provider_key: &str, secret: &str, project_url: &str) -> CredentialStatus {
    // Supabase: a valid anon/service key returns 200 (or 400 on bad params) on the schema endpoint.
    // An invalid key returns 401.
    let url = format!("{}/rest/v1/", project_url.trim_end_matches('/'));
    match client()
        .get(&url)
        .header("apikey", secret)
        .header("Authorization", format!("Bearer {secret}"))
        .send()
        .await
    {
        Ok(resp) => {
            let code = resp.status().as_u16();
            let valid = matches!(code, 200 | 206 | 400); // 400 = valid key, bad query params
            CredentialStatus {
                provider_key: provider_key.to_string(),
                valid,
                http_status: Some(code),
                needs_reauth: code == 401,
                error: if !valid {
                    Some(format!("Supabase returned HTTP {code}"))
                } else {
                    None
                },
            }
        }
        Err(e) => net_err(provider_key, e),
    }
}

async fn openai_check(provider_key: &str, secret: &str) -> CredentialStatus {
    match client()
        .get("https://api.openai.com/v1/models")
        .header("Authorization", format!("Bearer {secret}"))
        .send()
        .await
    {
        Ok(resp) => {
            let code = resp.status().as_u16();
            CredentialStatus {
                provider_key: provider_key.to_string(),
                valid: resp.status().is_success(),
                http_status: Some(code),
                needs_reauth: code == 401,
                error: if !resp.status().is_success() {
                    Some(format!("HTTP {code}"))
                } else {
                    None
                },
            }
        }
        Err(e) => net_err(provider_key, e),
    }
}

async fn github_check(provider_key: &str, secret: &str) -> CredentialStatus {
    match client()
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {secret}"))
        .header("User-Agent", "hsm-credential-verifier/1.0")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(resp) => {
            let code = resp.status().as_u16();
            CredentialStatus {
                provider_key: provider_key.to_string(),
                valid: resp.status().is_success(),
                http_status: Some(code),
                needs_reauth: code == 401,
                error: if !resp.status().is_success() {
                    Some(format!("HTTP {code}"))
                } else {
                    None
                },
            }
        }
        Err(e) => net_err(provider_key, e),
    }
}

async fn slack_check(provider_key: &str, secret: &str) -> CredentialStatus {
    // Slack returns HTTP 200 even for invalid tokens; the `ok` field is the truth.
    let params = [("token", secret)];
    match client()
        .post("https://slack.com/api/auth.test")
        .form(&params)
        .send()
        .await
    {
        Ok(resp) => {
            let code = resp.status().as_u16();
            let ok = resp
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v.get("ok")?.as_bool())
                .unwrap_or(false);
            CredentialStatus {
                provider_key: provider_key.to_string(),
                valid: ok,
                http_status: Some(code),
                needs_reauth: !ok,
                error: if !ok {
                    Some("Slack auth.test returned ok=false".to_string())
                } else {
                    None
                },
            }
        }
        Err(e) => net_err(provider_key, e),
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("hsm-credential-verifier/1.0")
        .build()
        .expect("reqwest client init failed")
}

fn net_err(provider_key: &str, e: reqwest::Error) -> CredentialStatus {
    CredentialStatus {
        provider_key: provider_key.to_string(),
        valid: false,
        http_status: None,
        needs_reauth: false,
        error: Some(e.to_string()),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_adapter_always_valid() {
        let status =
            verify_credential("internal", "any-secret", &ProviderAdapter::NoOp).await;
        assert!(status.valid);
        assert!(!status.needs_reauth);
        assert!(status.error.is_none());
    }

    #[tokio::test]
    async fn verify_all_empty_list() {
        let results = verify_all(vec![]).await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn verify_all_noop_entries() {
        let checks = vec![
            ("svc-a".to_string(), "secret-a".to_string(), ProviderAdapter::NoOp),
            ("svc-b".to_string(), "secret-b".to_string(), ProviderAdapter::NoOp),
        ];
        let results = verify_all(checks).await;
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.valid));
    }
}
