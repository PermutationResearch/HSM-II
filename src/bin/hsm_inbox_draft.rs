//! Poll IMAP, save `.eml` under `outbound/inbox_staging/`, run [`hyper_stigmergy::personal::EnhancedPersonalAgent::draft_email_reply`],
//! write drafts under `outbound/email_drafts/`. Use with cron/LaunchAgent after setting `HSM_IMAP_*` (Outlook: `outlook.office365.com:993` when IMAP is enabled).
//!
//! Example:
//! ```text
//! export HSM_IMAP_SERVER=outlook.office365.com HSM_IMAP_USER='you@company.com' HSM_IMAP_PASSWORD='app-password'
//! cargo run -p hyper-stigmergy --bin hsm_inbox_draft -- --limit 3
//! ```

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing::info;

use hyper_stigmergy::email::email_config_from_env;
use hyper_stigmergy::personal::{resolve_hsmii_home, EnhancedPersonalAgent};
use hyper_stigmergy::EmailClient;

#[derive(Parser)]
#[command(name = "hsm-inbox-draft")]
struct Cli {
    /// Max messages to pull (prefers UNSEEN, then newest in mailbox).
    #[arg(short, long, default_value_t = 5)]
    limit: usize,

    #[arg(short, long, default_value = "INBOX")]
    mailbox: String,

    /// Fetch and save `.eml` only (no LLM).
    #[arg(long)]
    dry_run: bool,

    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[arg(short = 'p', long = "profile", global = true)]
    profile: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let home = resolve_hsmii_home(cli.config, cli.profile.as_deref());

    let email_cfg = email_config_from_env()?.ok_or_else(|| {
        anyhow!(
            "set HSM_IMAP_SERVER (or HSM_IMAP_HOST), HSM_IMAP_USER, HSM_IMAP_PASSWORD — see .env.example"
        )
    })?;

    let client = EmailClient::connect(&email_cfg).await?;
    let fetched = client
        .fetch_mailbox_with_raw(&cli.mailbox, cli.limit)
        .await
        .with_context(|| format!("IMAP fetch failed for mailbox {:?}", cli.mailbox))?;

    let staging = home.join("outbound/inbox_staging");
    let drafts_dir = home.join("outbound/email_drafts");
    tokio::fs::create_dir_all(&staging).await?;
    tokio::fs::create_dir_all(&drafts_dir).await?;

    if cli.dry_run {
        info!(count = fetched.len(), "dry-run: wrote .eml only");
    } else {
        info!(count = fetched.len(), "fetched messages; drafting replies");
    }

    let mut agent = EnhancedPersonalAgent::initialize(&home).await?;

    for item in fetched {
        let uid = item.email.id.clone();
        let eml_path = staging.join(format!("{uid}.eml"));
        tokio::fs::write(&eml_path, &item.rfc822).await?;

        let rel = eml_path
            .strip_prefix(&home)
            .unwrap_or(&eml_path)
            .to_string_lossy();

        if cli.dry_run {
            info!(path = %eml_path.display(), "saved .eml");
            continue;
        }

        let inbound = format!(
            "Draft a reply to this message. Summary:\nFrom: {}\nSubject: {}\n\nFull parse + paperclip context is in the saved RFC822 file:\n@{}",
            item.email.from,
            item.email.subject,
            rel.trim_start_matches('/')
        );

        let draft = agent
            .draft_email_reply(&inbound)
            .await
            .with_context(|| format!("draft_email_reply failed for uid {}", uid))?;

        let slug: String = item
            .email
            .subject
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(48)
            .collect();
        let slug = if slug.is_empty() {
            "message".into()
        } else {
            slug
        };
        let out_name = format!("{}_{}.txt", uid, slug);
        let out_path = drafts_dir.join(out_name);
        let header = format!(
            "# IMAP UID: {}\n# From: {}\n# Subject: {}\n# Source: {}\n\n",
            uid,
            item.email.from,
            item.email.subject,
            eml_path.display()
        );
        tokio::fs::write(&out_path, format!("{header}{draft}")).await?;
        info!(path = %out_path.display(), "wrote draft");
    }

    Ok(())
}
