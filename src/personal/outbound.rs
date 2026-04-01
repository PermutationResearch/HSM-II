//! Outbound integrations — fire JSON to a URL (Zapier, Make, Monday incoming webhooks, etc.).

use anyhow::{Context, Result};
use serde_json::Value;
/// POST `application/json` to `url`. Short timeout; suitable for background `tokio::spawn`.
pub async fn post_json_webhook(url: &str, payload: &Value) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .context("reqwest client")?;
    let resp = client
        .post(url)
        .json(payload)
        .send()
        .await
        .context("webhook HTTP send")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("webhook HTTP {status}: {}", body.chars().take(200).collect::<String>());
    }
    Ok(())
}
