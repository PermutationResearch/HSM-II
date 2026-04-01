//! Email agent integration (ReachInBox/MCP Agent Mail inspired).
//!
//! Provides AI-powered email management with:
//! - Automated categorization and prioritization
//! - Smart responses using local LLMs
//! - Integration with hyper-stigmergic memory system

use serde::{Deserialize, Serialize};

pub mod classifier;
pub mod client;
pub mod ladybug_storage;
pub mod memory;
pub mod responder;

pub use classifier::{Category, EmailClassifier, Priority};
pub use ladybug_storage::{
    EmailClassification, LadybugEmailStorage, StoredEmail, StorageStats, SuggestedAction,
    VacuumResult,
};
pub use client::{EmailClient, EmailProvider, ImapConfig, ImapFetchedMessage, SmtpConfig};

/// Load [`EmailConfig`] from `HSM_IMAP_*` for CLI/cron (Outlook often: `outlook.office365.com`, port `993`).
///
/// Returns `Ok(None)` if `HSM_IMAP_SERVER` / `HSM_IMAP_HOST` is unset (mock IMAP in tests).
pub fn email_config_from_env() -> anyhow::Result<Option<EmailConfig>> {
    let server = std::env::var("HSM_IMAP_SERVER")
        .or_else(|_| std::env::var("HSM_IMAP_HOST"))
        .unwrap_or_default();
    if server.trim().is_empty() {
        return Ok(None);
    }

    let username = std::env::var("HSM_IMAP_USER").or_else(|_| std::env::var("HSM_IMAP_USERNAME")).map_err(|_| {
        anyhow::anyhow!("set HSM_IMAP_USER when HSM_IMAP_SERVER is set")
    })?;

    let password = std::env::var("HSM_IMAP_PASSWORD")
        .or_else(|_| std::env::var("HSM_IMAP_PASS"))
        .map_err(|_| anyhow::anyhow!("set HSM_IMAP_PASSWORD when HSM_IMAP_SERVER is set"))?;

    let port: u16 = std::env::var("HSM_IMAP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(993);

    let use_tls = std::env::var("HSM_IMAP_TLS")
        .map(|v| {
            let s = v.trim();
            !(s == "0" || s.eq_ignore_ascii_case("false") || s.eq_ignore_ascii_case("no"))
        })
        .unwrap_or(true);

    let provider = match std::env::var("HSM_IMAP_PROVIDER")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "gmail" => EmailProvider::Gmail,
        "outlook" | "office365" | "m365" | "365" => EmailProvider::Outlook,
        "yahoo" => EmailProvider::Yahoo,
        x if !x.is_empty() => EmailProvider::Custom(x.to_string()),
        _ => EmailProvider::Custom(server.clone()),
    };

    Ok(Some(EmailConfig {
        provider,
        imap: ImapConfig {
            server,
            port,
            username: username.clone(),
            password,
            use_tls,
        },
        smtp: SmtpConfig {
            server: String::new(),
            port: 587,
            username,
            password: String::new(),
            use_tls: true,
        },
        auto_reply: false,
        digest_mode: false,
    }))
}

pub use memory::{ConversationThread, EmailMemory};
pub use responder::{ResponseGenerator, ResponseTemplate, Tone};

/// Email agent that manages inbox with AI
pub struct EmailAgent {
    client: EmailClient,
    classifier: EmailClassifier,
    responder: ResponseGenerator,
    memory: EmailMemory,
}

impl EmailAgent {
    /// Create new email agent
    pub async fn new(config: EmailConfig) -> anyhow::Result<Self> {
        let client = EmailClient::connect(&config).await?;
        let classifier = EmailClassifier::new();
        let responder = ResponseGenerator::new();
        let memory = EmailMemory::new();

        Ok(Self {
            client,
            classifier,
            responder,
            memory,
        })
    }

    /// Process inbox - categorize and suggest actions
    pub async fn process_inbox(&mut self, limit: usize) -> anyhow::Result<Vec<EmailAction>> {
        let emails = self.client.fetch_recent(limit).await?;
        let mut actions = Vec::new();

        for email in emails {
            let email_id = email.id.clone();

            // Classify email
            let classification = self.classifier.classify(&email).await;

            // Check if we have context in memory
            let thread = self.memory.get_thread(&email.thread_id);

            // Generate action
            let action = match classification.category {
                Category::Spam => EmailAction::Delete(email_id),
                Category::Newsletter => EmailAction::Archive(email_id),
                Category::Important => {
                    // Generate response suggestion
                    let suggestion = if classification.needs_response {
                        self.responder.generate_response(&email, thread).await.ok()
                    } else {
                        None
                    };

                    EmailAction::Review {
                        email: email.clone(),
                        priority: classification.priority,
                        suggested_response: suggestion,
                    }
                }
                Category::Social => EmailAction::Label(email_id, "Social".to_string()),
                Category::Notification => EmailAction::Label(email_id, "Notifications".to_string()),
            };

            // Store in memory
            self.memory.store_email(email);

            actions.push(action);
        }

        // Sort by priority
        actions.sort_by_key(|a| match a {
            EmailAction::Review { priority, .. } => *priority as u8,
            _ => 255,
        });

        Ok(actions)
    }

    /// Send an email
    pub async fn send_email(&self, to: &str, subject: &str, body: &str) -> anyhow::Result<()> {
        let email = OutgoingEmail {
            to: to.to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            timestamp: current_timestamp(),
        };

        self.client.send(email).await
    }

    /// Generate smart reply
    pub async fn smart_reply(&mut self, email_id: &str) -> anyhow::Result<String> {
        let email = self
            .memory
            .get_email(email_id)
            .ok_or_else(|| anyhow::anyhow!("Email not found"))?;

        let thread = self.memory.get_thread(&email.thread_id);

        self.responder.generate_response(&email, thread).await
    }

    /// Search emails by semantic query
    pub async fn semantic_search(&self, query: &str) -> anyhow::Result<Vec<Email>> {
        self.memory.semantic_search(query).await
    }

    /// Get email statistics
    pub fn stats(&self) -> EmailStats {
        self.memory.stats()
    }
}

/// Email configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmailConfig {
    pub provider: EmailProvider,
    pub imap: ImapConfig,
    pub smtp: SmtpConfig,
    pub auto_reply: bool,
    pub digest_mode: bool,
}

/// Incoming email
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Email {
    pub id: String,
    pub thread_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub timestamp: u64,
    pub labels: Vec<String>,
    pub attachments: Vec<Attachment>,
}

/// Outgoing email
#[derive(Clone, Debug)]
pub struct OutgoingEmail {
    pub to: String,
    pub subject: String,
    pub body: String,
    pub timestamp: u64,
}

/// Email attachment
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub filename: String,
    pub content_type: String,
    pub size: usize,
}

/// Actions that can be taken on emails
#[derive(Clone, Debug)]
pub enum EmailAction {
    Delete(String),
    Archive(String),
    Label(String, String),
    Review {
        email: Email,
        priority: Priority,
        suggested_response: Option<String>,
    },
}

/// Email statistics
#[derive(Clone, Debug, Default)]
pub struct EmailStats {
    pub total_processed: usize,
    pub categorized: std::collections::HashMap<String, usize>,
    pub avg_response_time: f64,
}

fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
