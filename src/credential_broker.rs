//! Ephemeral per-agent credential broker.
//!
//! [`CredentialBroker`] issues TTL-scoped env-var grants backed by [`CredentialVault`].
//! Each grant is a map of `env_var_name → secret_value` valid for a fixed window.
//! Once issued, a grant is applied to a subprocess via [`apply_grant_std`] /
//! [`apply_grant_tokio`]. Expired grants are rejected; they can also be revoked early.
//!
//! # Thread safety
//!
//! `CredentialBroker` is cheaply `Clone` (Arc-backed) and safe to share across tasks.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;

use crate::credential_vault::{CredentialVault, VaultError};

pub const DEFAULT_TTL: Duration = Duration::from_secs(300); // 5 minutes

// ── Public types ──────────────────────────────────────────────────────────────

/// An active credential grant bound to a single agent.
#[derive(Debug, Clone)]
pub struct CredentialGrant {
    pub grant_id: String,
    pub agent_id: String,
    /// Env var name → plaintext secret. Cleared on revoke/expiry.
    pub env_vars: HashMap<String, String>,
    pub expires_at: Instant,
}

impl CredentialGrant {
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
    pub fn remaining(&self) -> Duration {
        self.expires_at.saturating_duration_since(Instant::now())
    }
}

#[derive(Debug)]
pub enum BrokerError {
    Vault(VaultError),
    GrantExpired,
    GrantNotFound,
    /// The credential has no `env_var` configured in the vault.
    NoEnvVar(String),
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrokerError::Vault(e) => write!(f, "vault: {e}"),
            BrokerError::GrantExpired => write!(f, "credential grant has expired"),
            BrokerError::GrantNotFound => write!(f, "grant not found"),
            BrokerError::NoEnvVar(k) => {
                write!(f, "no env_var configured for credential '{k}' in vault")
            }
        }
    }
}

impl std::error::Error for BrokerError {}

impl From<VaultError> for BrokerError {
    fn from(e: VaultError) -> Self {
        BrokerError::Vault(e)
    }
}

// ── Broker ────────────────────────────────────────────────────────────────────

/// Thread-safe credential broker backed by a [`CredentialVault`].
#[derive(Clone)]
pub struct CredentialBroker {
    inner: Arc<Mutex<BrokerState>>,
}

struct BrokerState {
    vault_db: PathBuf,
    vault_salt: PathBuf,
    passphrase: String,
    grants: HashMap<String, CredentialGrant>,
    default_ttl: Duration,
}

impl CredentialBroker {
    /// Construct a broker. The vault is opened on each [`issue`] call to limit
    /// the window during which the derived key is resident in memory.
    pub fn new(
        vault_db: impl Into<PathBuf>,
        vault_salt: impl Into<PathBuf>,
        passphrase: impl Into<String>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BrokerState {
                vault_db: vault_db.into(),
                vault_salt: vault_salt.into(),
                passphrase: passphrase.into(),
                grants: HashMap::new(),
                default_ttl: DEFAULT_TTL,
            })),
        }
    }

    /// Override the default TTL for all future grants.
    pub fn with_ttl(self, ttl: Duration) -> Self {
        self.inner.lock().unwrap().default_ttl = ttl;
        self
    }

    /// Issue a credential grant for `provider_keys` to `agent_id`.
    ///
    /// Opens the vault, retrieves each credential's `env_var` name and plaintext
    /// secret, packages them into a [`CredentialGrant`], and returns the opaque
    /// `grant_id`. The caller applies the grant to a subprocess via
    /// [`apply_grant_std`] / [`apply_grant_tokio`].
    pub fn issue(
        &self,
        agent_id: &str,
        provider_keys: &[&str],
        ttl_override: Option<Duration>,
    ) -> Result<String, BrokerError> {
        let mut state = self.inner.lock().unwrap();
        let vault =
            CredentialVault::open(&state.vault_db, &state.vault_salt, &state.passphrase)?;

        let entries = vault.list()?;
        let mut env_vars = HashMap::new();

        for pk in provider_keys {
            let entry = entries.iter().find(|e| e.provider_key == *pk);
            let env_name = entry
                .and_then(|e| e.env_var.as_deref())
                .ok_or_else(|| BrokerError::NoEnvVar(pk.to_string()))?
                .to_string();
            let secret = vault.get_secret(pk)?;
            env_vars.insert(env_name, secret);
        }

        let ttl = ttl_override.unwrap_or(state.default_ttl);
        let grant_id = make_grant_id(agent_id);
        state.grants.insert(
            grant_id.clone(),
            CredentialGrant {
                grant_id: grant_id.clone(),
                agent_id: agent_id.to_string(),
                env_vars,
                expires_at: Instant::now() + ttl,
            },
        );
        state.evict_expired();
        Ok(grant_id)
    }

    /// Inject the grant's env vars into a `std::process::Command`.
    ///
    /// Returns [`BrokerError::GrantExpired`] if the TTL has passed.
    pub fn apply_grant_std(
        &self,
        grant_id: &str,
        cmd: &mut std::process::Command,
    ) -> Result<(), BrokerError> {
        let state = self.inner.lock().unwrap();
        let grant = state.grants.get(grant_id).ok_or(BrokerError::GrantNotFound)?;
        if grant.is_expired() {
            return Err(BrokerError::GrantExpired);
        }
        for (k, v) in &grant.env_vars {
            cmd.env(k, v);
        }
        Ok(())
    }

    /// Inject the grant's env vars into a `tokio::process::Command`.
    pub fn apply_grant_tokio(
        &self,
        grant_id: &str,
        cmd: &mut tokio::process::Command,
    ) -> Result<(), BrokerError> {
        let state = self.inner.lock().unwrap();
        let grant = state.grants.get(grant_id).ok_or(BrokerError::GrantNotFound)?;
        if grant.is_expired() {
            return Err(BrokerError::GrantExpired);
        }
        for (k, v) in &grant.env_vars {
            cmd.env(k, v);
        }
        Ok(())
    }

    /// Revoke a grant before its TTL expires (e.g. agent finished).
    pub fn revoke(&self, grant_id: &str) {
        self.inner.lock().unwrap().grants.remove(grant_id);
    }

    /// Remove all expired grants from the in-memory table.
    pub fn purge_expired(&self) {
        self.inner.lock().unwrap().evict_expired();
    }

    /// Number of currently live (non-expired) grants.
    pub fn live_count(&self) -> usize {
        let state = self.inner.lock().unwrap();
        state.grants.values().filter(|g| !g.is_expired()).count()
    }

    /// Look up grant metadata (no secrets exposed).
    pub fn grant_info(&self, grant_id: &str) -> Option<(String, bool, Duration)> {
        let state = self.inner.lock().unwrap();
        state.grants.get(grant_id).map(|g| {
            (g.agent_id.clone(), g.is_expired(), g.remaining())
        })
    }
}

impl BrokerState {
    fn evict_expired(&mut self) {
        let now = Instant::now();
        self.grants.retain(|_, g| g.expires_at > now);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_grant_id(agent_id: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    let mut rng_bytes = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rng_bytes);
    let rand_tag =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rng_bytes);
    // Truncate agent_id for readability
    let short_agent: String = agent_id.chars().take(16).collect();
    format!("grant-{short_agent}-{ts}-{rand_tag}")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential_vault::CredentialVault;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("hsm_broker_{name}_{}", std::process::id()))
    }

    fn setup_vault(db: &PathBuf, salt: &PathBuf) {
        let v = CredentialVault::create(db, salt, "broker-test-pass").unwrap();
        v.put(
            "stripe",
            "sk_test_abc123xyz",
            Some("STRIPE_SECRET_KEY"),
            Some("Stripe Test"),
            None,
        )
        .unwrap();
        v.put("openai", "sk-test-open", Some("OPENAI_API_KEY"), None, None)
            .unwrap();
    }

    #[test]
    fn issue_and_apply() {
        let db = tmp("ia.sqlite");
        let salt = tmp("ia.salt");
        setup_vault(&db, &salt);

        let broker =
            CredentialBroker::new(&db, &salt, "broker-test-pass").with_ttl(Duration::from_secs(60));

        let gid = broker.issue("agent-1", &["stripe"], None).unwrap();
        assert!(!gid.is_empty());

        let mut cmd = std::process::Command::new("env");
        broker.apply_grant_std(&gid, &mut cmd).unwrap();

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }

    #[test]
    fn expired_grant_rejected() {
        let db = tmp("exp.sqlite");
        let salt = tmp("exp.salt");
        setup_vault(&db, &salt);

        let broker = CredentialBroker::new(&db, &salt, "broker-test-pass")
            .with_ttl(Duration::from_millis(1));

        let gid = broker.issue("agent-2", &["stripe"], None).unwrap();
        std::thread::sleep(Duration::from_millis(5)); // let it expire

        let mut cmd = std::process::Command::new("true");
        assert!(matches!(
            broker.apply_grant_std(&gid, &mut cmd),
            Err(BrokerError::GrantExpired)
        ));

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }

    #[test]
    fn revoke_before_expiry() {
        let db = tmp("rev.sqlite");
        let salt = tmp("rev.salt");
        setup_vault(&db, &salt);

        let broker = CredentialBroker::new(&db, &salt, "broker-test-pass");
        let gid = broker.issue("agent-3", &["openai"], None).unwrap();

        broker.revoke(&gid);

        let mut cmd = std::process::Command::new("true");
        assert!(matches!(
            broker.apply_grant_std(&gid, &mut cmd),
            Err(BrokerError::GrantNotFound)
        ));

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }

    #[test]
    fn missing_env_var_returns_error() {
        let db = tmp("nev.sqlite");
        let salt = tmp("nev.salt");
        // Store credential without env_var
        let v = CredentialVault::create(&db, &salt, "broker-test-pass").unwrap();
        v.put("bare", "secret", None, None, None).unwrap();
        drop(v);

        let broker = CredentialBroker::new(&db, &salt, "broker-test-pass");
        assert!(matches!(
            broker.issue("agent-4", &["bare"], None),
            Err(BrokerError::NoEnvVar(_))
        ));

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }

    #[test]
    fn purge_expired_reduces_live_count() {
        let db = tmp("purge.sqlite");
        let salt = tmp("purge.salt");
        setup_vault(&db, &salt);

        let broker = CredentialBroker::new(&db, &salt, "broker-test-pass");

        // One long-lived grant
        broker.issue("agent-long", &["openai"], Some(Duration::from_secs(600))).unwrap();
        // One immediately-expiring grant
        broker
            .issue("agent-short", &["stripe"], Some(Duration::from_millis(1)))
            .unwrap();

        std::thread::sleep(Duration::from_millis(5));
        broker.purge_expired();

        assert_eq!(broker.live_count(), 1);

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }
}
