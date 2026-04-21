//! SQLite-backed credential vault with Argon2id passphrase-derived key and AES-256-GCM encryption.
//!
//! # Fails-closed design
//!
//! The Argon2 salt lives in a **separate file** (default: `<vault_dir>/.vault.salt`, or
//! `$HSM_VAULT_SALT_PATH`). If the salt file is absent, [`CredentialVault::open`] returns
//! [`VaultError::SaltMissing`] and refuses to proceed — it never synthesises a default salt.
//!
//! # Key derivation
//!
//! Argon2id (m=64 MiB, t=3, p=1) over the user passphrase + 32-byte random salt.
//! The derived 32-byte key is used for AES-256-GCM. Each secret gets a fresh random 12-byte nonce;
//! the on-disk format is `enc:v1:<nonce_b64>:<ciphertext_b64>`.

use std::path::{Path, PathBuf};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use rand::RngCore;
use rusqlite::{params, Connection};

const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;

// ── Public types ─────────────────────────────────────────────────────────────

pub struct CredentialVault {
    conn: Connection,
    cipher: Aes256Gcm,
}

/// Metadata row returned by [`CredentialVault::list`]. Secrets are never included.
#[derive(Debug, Clone)]
pub struct VaultEntry {
    pub provider_key: String,
    pub env_var: Option<String>,
    pub label: Option<String>,
    pub masked_preview: String,
    pub notes: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug)]
pub enum VaultError {
    /// Salt file does not exist — vault is locked (fails-closed).
    SaltMissing(PathBuf),
    SaltInvalid(String),
    Kdf(String),
    Cipher(String),
    Db(rusqlite::Error),
    NotFound,
    EncryptionFailed,
    /// Wrong passphrase or corrupted ciphertext.
    DecryptionFailed,
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::SaltMissing(p) => write!(
                f,
                "vault salt file missing: {} — refusing to open vault (fails-closed)",
                p.display()
            ),
            VaultError::SaltInvalid(s) => write!(f, "vault salt invalid: {s}"),
            VaultError::Kdf(s) => write!(f, "key derivation failed: {s}"),
            VaultError::Cipher(s) => write!(f, "cipher error: {s}"),
            VaultError::Db(e) => write!(f, "vault db error: {e}"),
            VaultError::NotFound => write!(f, "credential not found"),
            VaultError::EncryptionFailed => write!(f, "encryption failed"),
            VaultError::DecryptionFailed => {
                write!(f, "decryption failed — wrong passphrase or corrupted data")
            }
        }
    }
}

impl std::error::Error for VaultError {}

impl From<rusqlite::Error> for VaultError {
    fn from(e: rusqlite::Error) -> Self {
        VaultError::Db(e)
    }
}

// ── Core impl ────────────────────────────────────────────────────────────────

impl CredentialVault {
    /// Resolve default `(db_path, salt_path)` for a given base directory.
    ///
    /// Respects `$HSM_VAULT_DIR` and `$HSM_VAULT_SALT_PATH` env overrides.
    pub fn default_paths(base: &Path) -> (PathBuf, PathBuf) {
        let dir = std::env::var("HSM_VAULT_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| base.to_path_buf());
        let db = dir.join("credentials.db");
        let salt = std::env::var("HSM_VAULT_SALT_PATH")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join(".vault.salt"));
        (db, salt)
    }

    /// Create a brand-new vault, generating a fresh salt file.
    ///
    /// Returns an error if `salt_path` already exists (prevents accidental re-key).
    pub fn create(db_path: &Path, salt_path: &Path, passphrase: &str) -> Result<Self, VaultError> {
        if salt_path.exists() {
            return Err(VaultError::SaltInvalid(
                "salt file already exists; use open() to access the existing vault".to_string(),
            ));
        }
        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);
        std::fs::write(salt_path, &salt)
            .map_err(|e| VaultError::SaltInvalid(format!("cannot write salt: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                salt_path,
                std::fs::Permissions::from_mode(0o600),
            );
        }

        let key = derive_key(passphrase, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| VaultError::Cipher(e.to_string()))?;
        let conn = init_db(db_path)?;
        Ok(Self { conn, cipher })
    }

    /// Open an existing vault. **Fails closed** when `salt_path` is absent.
    pub fn open(db_path: &Path, salt_path: &Path, passphrase: &str) -> Result<Self, VaultError> {
        if !salt_path.exists() {
            return Err(VaultError::SaltMissing(salt_path.to_path_buf()));
        }
        let raw = std::fs::read(salt_path)
            .map_err(|e| VaultError::SaltInvalid(format!("cannot read salt: {e}")))?;
        if raw.len() != SALT_LEN {
            return Err(VaultError::SaltInvalid(format!(
                "salt must be {SALT_LEN} bytes, got {}",
                raw.len()
            )));
        }
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&raw);
        let key = derive_key(passphrase, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| VaultError::Cipher(e.to_string()))?;
        let conn = init_db(db_path)?;
        Ok(Self { conn, cipher })
    }

    // ── CRUD ─────────────────────────────────────────────────────────────────

    /// Insert or update a credential.
    pub fn put(
        &self,
        provider_key: &str,
        secret: &str,
        env_var: Option<&str>,
        label: Option<&str>,
        notes: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted = self.encrypt(secret)?;
        let masked = mask_secret(secret);
        let now = unix_now();
        self.conn.execute(
            r#"INSERT INTO credentials
                   (provider_key, encrypted_secret, masked_preview, env_var, label, notes,
                    status, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'connected', ?7, ?7)
               ON CONFLICT(provider_key) DO UPDATE
                  SET encrypted_secret = excluded.encrypted_secret,
                      masked_preview   = excluded.masked_preview,
                      env_var          = excluded.env_var,
                      label            = excluded.label,
                      notes            = excluded.notes,
                      status           = 'connected',
                      updated_at       = excluded.updated_at"#,
            params![provider_key, encrypted, masked, env_var, label, notes, now],
        )?;
        Ok(())
    }

    /// Retrieve and decrypt a secret. Returns [`VaultError::NotFound`] if absent.
    pub fn get_secret(&self, provider_key: &str) -> Result<String, VaultError> {
        let encrypted: String = self
            .conn
            .query_row(
                "SELECT encrypted_secret FROM credentials WHERE provider_key = ?1",
                params![provider_key],
                |row| row.get(0),
            )
            .map_err(|e| {
                if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                    VaultError::NotFound
                } else {
                    VaultError::Db(e)
                }
            })?;
        self.decrypt(&encrypted)
    }

    /// List all entries without exposing secrets.
    pub fn list(&self) -> Result<Vec<VaultEntry>, VaultError> {
        let mut stmt = self.conn.prepare(
            r#"SELECT provider_key, env_var, label, masked_preview, notes, status,
                      created_at, updated_at
               FROM credentials
               ORDER BY lower(provider_key)"#,
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(VaultEntry {
                    provider_key: row.get(0)?,
                    env_var: row.get(1)?,
                    label: row.get(2)?,
                    masked_preview: row.get(3)?,
                    notes: row.get(4)?,
                    status: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Delete a credential. Returns `true` if a row was removed.
    pub fn delete(&self, provider_key: &str) -> Result<bool, VaultError> {
        let n = self.conn.execute(
            "DELETE FROM credentials WHERE provider_key = ?1",
            params![provider_key],
        )?;
        Ok(n > 0)
    }

    /// Update the `status` field (e.g. `"connected"`, `"expired"`, `"invalid"`).
    pub fn set_status(&self, provider_key: &str, status: &str) -> Result<(), VaultError> {
        self.conn.execute(
            "UPDATE credentials SET status = ?2, updated_at = ?3 WHERE provider_key = ?1",
            params![provider_key, status, unix_now()],
        )?;
        Ok(())
    }

    // ── Crypto ────────────────────────────────────────────────────────────────

    fn encrypt(&self, plaintext: &str) -> Result<String, VaultError> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .map_err(|_| VaultError::EncryptionFailed)?;
        let n = base64::engine::general_purpose::STANDARD.encode(nonce_bytes);
        let c = base64::engine::general_purpose::STANDARD.encode(ct);
        Ok(format!("enc:v1:{n}:{c}"))
    }

    fn decrypt(&self, ciphertext: &str) -> Result<String, VaultError> {
        // Format: enc:v1:<nonce_b64>:<ct_b64>
        let mut parts = ciphertext.splitn(4, ':');
        let tag = parts.next().unwrap_or("");
        let ver = parts.next().unwrap_or("");
        let nonce_b64 = parts.next().unwrap_or("");
        let ct_b64 = parts.next().unwrap_or("");
        if tag != "enc" || ver != "v1" {
            return Err(VaultError::DecryptionFailed);
        }
        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(nonce_b64)
            .map_err(|_| VaultError::DecryptionFailed)?;
        let ct = base64::engine::general_purpose::STANDARD
            .decode(ct_b64)
            .map_err(|_| VaultError::DecryptionFailed)?;
        if nonce_bytes.len() != NONCE_LEN {
            return Err(VaultError::DecryptionFailed);
        }
        let pt = self
            .cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), ct.as_slice())
            .map_err(|_| VaultError::DecryptionFailed)?;
        String::from_utf8(pt).map_err(|_| VaultError::DecryptionFailed)
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn derive_key(passphrase: &str, salt: &[u8; SALT_LEN]) -> Result<[u8; 32], VaultError> {
    // Argon2id: m=64 MiB, t=3 iterations, p=1 lane, output=32 bytes
    let params = Params::new(64 * 1024, 3, 1, Some(32))
        .map_err(|e| VaultError::Kdf(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| VaultError::Kdf(e.to_string()))?;
    Ok(key)
}

fn init_db(path: &Path) -> Result<Connection, VaultError> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"CREATE TABLE IF NOT EXISTS credentials (
            provider_key     TEXT PRIMARY KEY NOT NULL,
            encrypted_secret TEXT NOT NULL,
            masked_preview   TEXT NOT NULL DEFAULT '••••',
            env_var          TEXT,
            label            TEXT,
            notes            TEXT,
            status           TEXT NOT NULL DEFAULT 'connected',
            created_at       INTEGER NOT NULL,
            updated_at       INTEGER NOT NULL
        );"#,
    )?;
    Ok(conn)
}

fn mask_secret(secret: &str) -> String {
    let t = secret.trim();
    if t.chars().count() <= 4 {
        return "••••".to_string();
    }
    let tail: String = t
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••{tail}")
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("hsm_vault_test_{}_{}", name, std::process::id()))
    }

    #[test]
    fn create_open_roundtrip() {
        let db = tmp("rt.sqlite");
        let salt = tmp("rt.salt");
        let pass = "correct-horse-battery-staple";

        let vault = CredentialVault::create(&db, &salt, pass).unwrap();
        vault
            .put("github", "ghp_supersecret", Some("GITHUB_TOKEN"), Some("GitHub"), None)
            .unwrap();
        drop(vault);

        let vault2 = CredentialVault::open(&db, &salt, pass).unwrap();
        assert_eq!(vault2.get_secret("github").unwrap(), "ghp_supersecret");

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }

    #[test]
    fn fails_closed_on_missing_salt() {
        let db = tmp("fc.sqlite");
        let salt = tmp("fc_missing.salt"); // deliberately not created
        match CredentialVault::open(&db, &salt, "pass") {
            Err(VaultError::SaltMissing(_)) => {}
            Err(e) => panic!("expected SaltMissing, got VaultError: {e}"),
            Ok(_) => panic!("expected SaltMissing, vault opened unexpectedly"),
        }
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn wrong_passphrase_yields_decryption_error() {
        let db = tmp("wp.sqlite");
        let salt = tmp("wp.salt");

        let vault = CredentialVault::create(&db, &salt, "right-pass").unwrap();
        vault.put("svc", "top_secret", None, None, None).unwrap();
        drop(vault);

        let vault2 = CredentialVault::open(&db, &salt, "wrong-pass").unwrap();
        assert!(matches!(vault2.get_secret("svc"), Err(VaultError::DecryptionFailed)));

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }

    #[test]
    fn list_never_exposes_plaintext_secret() {
        let db = tmp("ls.sqlite");
        let salt = tmp("ls.salt");

        let vault = CredentialVault::create(&db, &salt, "pass123").unwrap();
        vault
            .put("stripe", "sk_live_ACTUAL_SECRET_VALUE", Some("STRIPE_KEY"), None, None)
            .unwrap();

        let entries = vault.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].masked_preview.contains("sk_live_ACTUAL_SECRET_VALUE"));
        assert!(entries[0].masked_preview.starts_with("••••"));

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }

    #[test]
    fn delete_removes_entry() {
        let db = tmp("del.sqlite");
        let salt = tmp("del.salt");

        let vault = CredentialVault::create(&db, &salt, "pass").unwrap();
        vault.put("foo", "bar", None, None, None).unwrap();
        assert!(vault.delete("foo").unwrap());
        assert!(matches!(vault.get_secret("foo"), Err(VaultError::NotFound)));

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }

    #[test]
    fn create_refuses_existing_salt() {
        let db = tmp("dup.sqlite");
        let salt = tmp("dup.salt");

        CredentialVault::create(&db, &salt, "pass").unwrap();
        let result = CredentialVault::create(&db, &salt, "pass");
        assert!(matches!(result, Err(VaultError::SaltInvalid(_))));

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&salt);
    }
}
