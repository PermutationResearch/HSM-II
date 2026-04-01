//! Email client: real IMAP (TLS) when configured, otherwise mock messages for offline dev.

use super::{Email, EmailConfig, OutgoingEmail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Email provider types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EmailProvider {
    Gmail,
    Outlook,
    Yahoo,
    Custom(String),
}

/// IMAP configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImapConfig {
    pub server: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub use_tls: bool,
}

/// SMTP configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SmtpConfig {
    pub server: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub use_tls: bool,
}

/// One message from [`EmailClient::fetch_mailbox_with_raw`]: parsed summary + original bytes (for `.eml` / tools).
#[derive(Clone, Debug)]
pub struct ImapFetchedMessage {
    pub email: Email,
    pub rfc822: Vec<u8>,
}

/// Email client (IMAP settings retained for [`Self::fetch_mailbox`]).
pub struct EmailClient {
    #[allow(dead_code)]
    provider: EmailProvider,
    imap: ImapConfig,
    #[allow(dead_code)]
    smtp: SmtpConfig,
}

impl EmailClient {
    /// Connect (stores config; IMAP runs per fetch on a short-lived session).
    pub async fn connect(config: &EmailConfig) -> anyhow::Result<Self> {
        Ok(Self {
            provider: config.provider.clone(),
            imap: config.imap.clone(),
            smtp: config.smtp.clone(),
        })
    }

    /// Fetch from `INBOX` (mock loopback if `imap.server` is empty).
    pub async fn fetch_recent(&self, limit: usize) -> anyhow::Result<Vec<Email>> {
        self.fetch_mailbox("INBOX", limit).await
    }

    /// Fetch up to `limit` messages: prefers `UNSEEN`, else most recent UIDs in `mailbox`.
    pub async fn fetch_mailbox(&self, mailbox: &str, limit: usize) -> anyhow::Result<Vec<Email>> {
        Ok(self
            .fetch_mailbox_with_raw(mailbox, limit)
            .await?
            .into_iter()
            .map(|m| m.email)
            .collect())
    }

    /// Same as [`Self::fetch_mailbox`], plus RFC822 bytes (save as `.eml`, or for tooling).
    pub async fn fetch_mailbox_with_raw(
        &self,
        mailbox: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ImapFetchedMessage>> {
        if self.imap.server.trim().is_empty() {
            return Ok(mock_fetch(limit)
                .into_iter()
                .map(|e| {
                    let rfc822 = format!(
                        "Subject: {}\r\nFrom: {}\r\n\r\n{}",
                        e.subject, e.from, e.body
                    )
                    .into_bytes();
                    ImapFetchedMessage { email: e, rfc822 }
                })
                .collect());
        }
        if !self.imap.use_tls {
            anyhow::bail!("IMAP requires TLS for now; set imap.use_tls=true (e.g. port 993)");
        }
        let imap = self.imap.clone();
        let mailbox = mailbox.to_string();
        tokio::task::spawn_blocking(move || fetch_imap_pairs(&imap, &mailbox, limit))
            .await
            .map_err(|e| anyhow::anyhow!("IMAP task failed: {e}"))?
    }

    /// Send an email
    pub async fn send(&self, email: OutgoingEmail) -> anyhow::Result<()> {
        tracing::info!(to = %email.to, "SMTP send not implemented; configure relay separately");
        Ok(())
    }

    /// Mark email as read
    pub async fn mark_read(&self, email_id: &str) -> anyhow::Result<()> {
        if self.imap.server.trim().is_empty() {
            tracing::debug!(email_id, "mark_read (noop mock)");
            return Ok(());
        }
        let imap = self.imap.clone();
        let id = email_id.to_string();
        tokio::task::spawn_blocking(move || imap_store_flags(&imap, &id, r"\Seen"))
            .await
            .map_err(|e| anyhow::anyhow!("IMAP task failed: {e}"))?
    }

    /// Archive email
    pub async fn archive(&self, email_id: &str) -> anyhow::Result<()> {
        tracing::info!(email_id, "archive not implemented for IMAP");
        Ok(())
    }

    /// Delete email
    pub async fn delete(&self, email_id: &str) -> anyhow::Result<()> {
        tracing::info!(email_id, "delete not implemented for IMAP");
        Ok(())
    }

    /// Add label to email
    pub async fn add_label(&self, email_id: &str, label: &str) -> anyhow::Result<()> {
        tracing::info!(email_id, label, "add_label not implemented for IMAP");
        Ok(())
    }

}

fn mock_fetch(limit: usize) -> Vec<Email> {
    let mut emails = Vec::new();
    for i in 0..limit {
        emails.push(Email {
            id: format!("email_{i}"),
            thread_id: format!("thread_{}", i / 3),
            from: format!("sender{i}@example.com"),
            to: vec!["me@example.com".to_string()],
            subject: format!("Test email {i}"),
            body: format!("This is the body of email {i}"),
            timestamp: current_timestamp() - (i as u64 * 3600),
            labels: Vec::new(),
            attachments: Vec::new(),
        });
    }
    emails
}

fn fetch_imap_pairs(
    imap: &ImapConfig,
    mailbox: &str,
    limit: usize,
) -> anyhow::Result<Vec<ImapFetchedMessage>> {
    let tls = native_tls::TlsConnector::builder().build()?;
    let host = imap.server.trim();
    let client = imap::connect((host, imap.port), host, &tls)?;
    let mut session = client
        .login(&imap.username, &imap.password)
        .map_err(|(e, _)| anyhow::anyhow!("IMAP login failed: {e}"))?;

    session.select(mailbox)?;

    let mut uids: Vec<u32> = session
        .uid_search("UNSEEN")?
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    uids.sort_unstable();
    if uids.len() > limit {
        uids = uids[uids.len().saturating_sub(limit)..].to_vec();
    }
    if uids.is_empty() {
        let mut all: Vec<u32> = session.uid_search("ALL")?.into_iter().collect();
        all.sort_unstable();
        if all.len() > limit {
            uids = all[all.len().saturating_sub(limit)..].to_vec();
        } else {
            uids = all;
        }
    }

    let mut out = Vec::new();
    for uid in uids {
        let fetch = session.uid_fetch(format!("{uid}"), "RFC822")?;
        for msg in fetch.iter() {
            if let Some(body) = msg.body() {
                let raw = body.to_vec();
                let email = raw_uid_to_email(uid, &raw)?;
                out.push(ImapFetchedMessage {
                    email,
                    rfc822: raw,
                });
            }
        }
    }

    let _ = session.logout();
    Ok(out)
}

fn imap_store_flags(imap: &ImapConfig, uid: &str, flags: &str) -> anyhow::Result<()> {
    let tls = native_tls::TlsConnector::builder().build()?;
    let host = imap.server.trim();
    let client = imap::connect((host, imap.port), host, &tls)?;
    let mut session = client
        .login(&imap.username, &imap.password)
        .map_err(|(e, _)| anyhow::anyhow!("IMAP login failed: {e}"))?;
    session.select("INBOX")?;
    session.uid_store(uid, format!("+FLAGS ({flags})"))?;
    let _ = session.logout();
    Ok(())
}

fn raw_uid_to_email(uid: u32, raw: &[u8]) -> anyhow::Result<Email> {
    let parsed = mailparse::parse_mail(raw)?;
    let from = header_val(&parsed, "From");
    let to = header_val(&parsed, "To");
    let subject = header_val(&parsed, "Subject");
    let thread_id = header_val(&parsed, "Message-ID");
    let thread_id = if thread_id.is_empty() {
        format!("uid-{uid}")
    } else {
        thread_id
    };

    let body = crate::tools::email_tools::summarize_eml_raw(raw).unwrap_or_else(|e| {
        format!(
            "(could not summarize RFC822: {e})\n\n{}",
            String::from_utf8_lossy(raw).chars().take(4000).collect::<String>()
        )
    });
    let body = truncate_chars(&body, 24_000);

    Ok(Email {
        id: uid.to_string(),
        thread_id,
        from,
        to: if to.is_empty() {
            Vec::new()
        } else {
            vec![to]
        },
        subject,
        body,
        timestamp: current_timestamp(),
        labels: Vec::new(),
        attachments: Vec::new(),
    })
}

fn header_val(parsed: &mailparse::ParsedMail<'_>, name: &str) -> String {
    parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case(name))
        .map(|h| h.get_value())
        .unwrap_or_default()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "\n… (truncated)"
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
