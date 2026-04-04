//! API Authentication & Authorization
//!
//! Provides API key management, JWT tokens, and rate limiting.

use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

/// API Key with metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub key_hash: String, // Hashed version of the key
    pub name: String,
    pub owner_id: String,
    pub permissions: Vec<Permission>,
    pub rate_limit: RateLimitConfig,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub usage_count: u64,
    pub is_active: bool,
    /// Tenant ID for multi-tenant keys (None for legacy/single-tenant keys).
    #[serde(default)]
    pub tenant_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Permission {
    Read,
    Write,
    Admin,
    Chat,
    ToolUse,
    WorkflowManage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub requests_per_hour: u32,
    pub requests_per_day: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
            requests_per_hour: 1000,
            requests_per_day: 10000,
        }
    }
}

/// JWT Claims
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,    // Subject (user ID)
    pub key_id: String, // API key ID
    pub permissions: Vec<Permission>,
    pub iat: i64, // Issued at
    pub exp: i64, // Expiration
    /// Tenant ID for multi-tenant claims. None for legacy tokens.
    #[serde(default)]
    pub tenant_id: Option<String>,
}

/// Rate limit tracking for a key
#[derive(Clone, Debug, Default)]
struct RateLimitState {
    minute_window: Vec<DateTime<Utc>>,
    hour_window: Vec<DateTime<Utc>>,
    day_window: Vec<DateTime<Utc>>,
}

impl RateLimitState {
    fn is_allowed(&mut self, config: &RateLimitConfig) -> bool {
        let now = Utc::now();

        // Clean old entries
        let minute_ago = now - Duration::minutes(1);
        let hour_ago = now - Duration::hours(1);
        let day_ago = now - Duration::days(1);

        self.minute_window.retain(|&t| t > minute_ago);
        self.hour_window.retain(|&t| t > hour_ago);
        self.day_window.retain(|&t| t > day_ago);

        // Check limits
        if self.minute_window.len() >= config.requests_per_minute as usize {
            return false;
        }
        if self.hour_window.len() >= config.requests_per_hour as usize {
            return false;
        }
        if self.day_window.len() >= config.requests_per_day as usize {
            return false;
        }

        // Record request
        self.minute_window.push(now);
        self.hour_window.push(now);
        self.day_window.push(now);

        true
    }
}

/// Authentication manager
pub struct AuthManager {
    /// API keys by ID
    keys: Arc<RwLock<HashMap<String, ApiKey>>>,
    /// Rate limit states
    rate_limits: Arc<RwLock<HashMap<String, RateLimitState>>>,
    /// JWT secret
    jwt_secret: String,
}

impl AuthManager {
    pub fn new() -> Self {
        let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
            let random = Uuid::new_v4().to_string();
            warn!(
                "JWT_SECRET not set, using random value. Sessions won't persist across restarts!"
            );
            random
        });

        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
            jwt_secret,
        }
    }

    /// Create a new API key
    pub async fn create_key(
        &self,
        name: String,
        owner_id: String,
        permissions: Vec<Permission>,
        rate_limit: Option<RateLimitConfig>,
        expires_days: Option<i64>,
    ) -> Result<String> {
        let key_id = Uuid::new_v4().to_string();
        let key_plain = format!("hsk_{}", Uuid::new_v4().to_string().replace("-", ""));

        // Hash the key
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let key_hash = argon2
            .hash_password(key_plain.as_bytes(), &salt)
            .map_err(|e| anyhow!("Failed to hash key: {}", e))?
            .to_string();

        let api_key = ApiKey {
            id: key_id.clone(),
            key_hash,
            name,
            owner_id,
            permissions,
            rate_limit: rate_limit.unwrap_or_default(),
            created_at: Utc::now(),
            expires_at: expires_days.map(|d| Utc::now() + Duration::days(d)),
            last_used_at: None,
            usage_count: 0,
            is_active: true,
            tenant_id: None,
        };

        let key_id_log = key_id.clone();
        self.keys.write().await.insert(key_id, api_key);

        info!(key_id = %key_id_log, "Created new API key");

        // Return the plain key (only time it's visible)
        Ok(key_plain)
    }

    /// Validate an API key and return JWT token
    pub async fn validate_key(&self, key_plain: &str) -> Result<String> {
        // Extract key ID from prefix if present
        let keys = self.keys.read().await;

        for (key_id, api_key) in keys.iter() {
            if !api_key.is_active {
                continue;
            }

            // Check expiration
            if let Some(expires) = api_key.expires_at {
                if Utc::now() > expires {
                    continue;
                }
            }

            // Verify hash
            let parsed_hash =
                PasswordHash::new(&api_key.key_hash).map_err(|e| anyhow!("Invalid hash: {}", e))?;

            if Argon2::default()
                .verify_password(key_plain.as_bytes(), &parsed_hash)
                .is_ok()
            {
                // Clone what we need before releasing lock
                let key_id_clone = key_id.clone();
                let permissions_clone = api_key.permissions.clone();
                let tenant_id_clone = api_key.tenant_id.clone();
                drop(keys); // Release read lock
                return self.generate_jwt(
                    &key_id_clone,
                    &permissions_clone,
                    tenant_id_clone.as_deref(),
                );
            }
        }

        Err(anyhow!("Invalid API key"))
    }

    /// Generate JWT token
    fn generate_jwt(
        &self,
        key_id: &str,
        permissions: &[Permission],
        tenant_id: Option<&str>,
    ) -> Result<String> {
        let now = Utc::now();
        let exp = now + Duration::hours(24); // 24 hour expiry

        let claims = Claims {
            sub: key_id.to_string(),
            key_id: key_id.to_string(),
            permissions: permissions.to_vec(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            tenant_id: tenant_id.map(|s| s.to_string()),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|e| anyhow!("Failed to encode JWT: {}", e))
    }

    /// Validate JWT token
    pub fn validate_jwt(&self, token: &str) -> Result<Claims> {
        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map(|data| data.claims)
        .map_err(|e| anyhow!("Invalid token: {}", e))
    }

    /// Check rate limit
    pub async fn check_rate_limit(&self, key_id: &str) -> Result<()> {
        let keys = self.keys.read().await;
        let key = keys.get(key_id).ok_or_else(|| anyhow!("Key not found"))?;

        let config = key.rate_limit.clone();
        drop(keys);

        let mut rate_limits = self.rate_limits.write().await;
        let state = rate_limits.entry(key_id.to_string()).or_default();

        if state.is_allowed(&config) {
            Ok(())
        } else {
            Err(anyhow!("Rate limit exceeded"))
        }
    }

    /// Revoke an API key
    pub async fn revoke_key(&self, key_id: &str) -> Result<()> {
        let mut keys = self.keys.write().await;
        if let Some(key) = keys.get_mut(key_id) {
            key.is_active = false;
            info!(key_id = %key_id, "API key revoked");
            Ok(())
        } else {
            Err(anyhow!("Key not found"))
        }
    }

    /// Get key info
    pub async fn get_key(&self, key_id: &str) -> Option<ApiKey> {
        self.keys.read().await.get(key_id).cloned()
    }

    /// List all keys
    pub async fn list_keys(&self) -> Vec<ApiKey> {
        self.keys.read().await.values().cloned().collect()
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Authentication state for Axum
#[derive(Clone)]
pub struct AuthState {
    pub auth: Arc<AuthManager>,
}

/// Extractor for authenticated requests
#[derive(Clone, Debug)]
pub struct AuthenticatedUser {
    pub key_id: String,
    pub permissions: Vec<Permission>,
    /// Tenant ID if this is a multi-tenant key.
    pub tenant_id: Option<String>,
}

/// Tenant context extracted from JWT for multi-tenant API routes.
/// Only present when the token was issued with a tenant_id.
#[derive(Clone, Debug)]
pub struct TenantContext {
    pub tenant_id: String,
    pub permissions: Vec<Permission>,
}

/// Axum middleware: Require authentication
pub async fn require_auth(
    State(state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract token from header
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    let token = match token {
        Some(t) => t,
        None => {
            warn!("Missing authorization header");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Validate JWT
    let claims = match state.auth.validate_jwt(token) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Invalid JWT");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Check rate limit
    if let Err(e) = state.auth.check_rate_limit(&claims.key_id).await {
        warn!(error = %e, key_id = %claims.key_id, "Rate limit exceeded");
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    // Add user to request extensions
    let user = AuthenticatedUser {
        key_id: claims.key_id,
        permissions: claims.permissions,
        tenant_id: claims.tenant_id,
    };
    request.extensions_mut().insert(user);

    Ok(next.run(request).await)
}

/// Check if user has required permission
pub fn has_permission(user: &AuthenticatedUser, permission: Permission) -> bool {
    user.permissions.contains(&Permission::Admin) || user.permissions.contains(&permission)
}

/// API Key creation request
#[derive(Clone, Debug, Deserialize)]
pub struct CreateKeyRequest {
    pub name: String,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub expires_days: Option<i64>,
}

/// API Key creation response
#[derive(Clone, Debug, Serialize)]
pub struct CreateKeyResponse {
    pub key: String,
    pub key_id: String,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Handler: Create API key
pub async fn create_api_key(
    State(state): State<AuthState>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, StatusCode> {
    let permissions: Vec<Permission> = req
        .permissions
        .iter()
        .filter_map(|p| match p.as_str() {
            "read" => Some(Permission::Read),
            "write" => Some(Permission::Write),
            "admin" => Some(Permission::Admin),
            "chat" => Some(Permission::Chat),
            "tool" => Some(Permission::ToolUse),
            "workflow" => Some(Permission::WorkflowManage),
            _ => None,
        })
        .collect();

    let key = state
        .auth
        .create_key(
            req.name,
            "default_owner".to_string(), // TODO: Get from authenticated user
            permissions,
            None,
            req.expires_days,
        )
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to create API key");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(CreateKeyResponse {
        key_id: Uuid::new_v4().to_string(),
        key,
        expires_at: None,
    }))
}

/// Handler: Authenticate with API key (get JWT)
pub async fn authenticate(
    State(state): State<AuthState>,
    axum::extract::Json(body): axum::extract::Json<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let api_key = body.get("api_key").ok_or(StatusCode::BAD_REQUEST)?;

    match state.auth.validate_key(api_key).await {
        Ok(token) => Ok(Json(json!({
            "token": token,
            "token_type": "Bearer",
            "expires_in": 86400,
        }))),
        Err(e) => {
            warn!(error = %e, "Authentication failed");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Multi-Tenant Auth Extensions
// ═══════════════════════════════════════════════════════════════════

/// Authentication state for multi-tenant Axum routes.
#[derive(Clone)]
pub struct TenantAuthState {
    pub auth: Arc<PersistentAuthManager>,
}

/// Persistent auth manager that wraps AuthManager with file-based storage.
///
/// On every key create/revoke, writes through to `{storage_path}/api_keys.json`.
/// On startup, loads existing keys from disk.
pub struct PersistentAuthManager {
    inner: AuthManager,
    storage_path: std::path::PathBuf,
}

impl PersistentAuthManager {
    /// Load or create a persistent auth manager.
    ///
    /// If `{storage_path}/api_keys.json` exists, loads all keys into memory.
    pub fn load(storage_path: &std::path::Path) -> Result<Self> {
        std::fs::create_dir_all(storage_path)?;
        let inner = AuthManager::new();

        let keys_file = storage_path.join("api_keys.json");
        if keys_file.exists() {
            let data = std::fs::read_to_string(&keys_file)?;
            let keys: Vec<ApiKey> = serde_json::from_str(&data)?;
            // Inject keys directly into the inner AuthManager's map
            let keys_map: HashMap<String, ApiKey> =
                keys.into_iter().map(|k| (k.id.clone(), k)).collect();
            // We need to get past the RwLock — use try_write since we're in startup
            let rt = tokio::runtime::Handle::try_current();
            if let Ok(handle) = rt {
                handle.block_on(async {
                    let mut guard = inner.keys.write().await;
                    *guard = keys_map;
                });
            }
        }

        Ok(Self {
            inner,
            storage_path: storage_path.to_path_buf(),
        })
    }

    /// Create a new persistent auth manager without loading from disk.
    pub fn new(storage_path: &std::path::Path) -> Self {
        std::fs::create_dir_all(storage_path).ok();
        Self {
            inner: AuthManager::new(),
            storage_path: storage_path.to_path_buf(),
        }
    }

    /// Create an API key for a specific tenant.
    pub async fn create_tenant_key(
        &self,
        tenant_id: &str,
        name: String,
        permissions: Vec<Permission>,
        rate_limit: Option<RateLimitConfig>,
    ) -> Result<(String, String)> {
        // Return (key_plain, key_id)
        let key_id = Uuid::new_v4().to_string();
        let key_plain = format!("hsk_{}", Uuid::new_v4().to_string().replace("-", ""));

        // Hash the key
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let key_hash = argon2
            .hash_password(key_plain.as_bytes(), &salt)
            .map_err(|e| anyhow!("Failed to hash key: {}", e))?
            .to_string();

        let api_key = ApiKey {
            id: key_id.clone(),
            key_hash,
            name,
            owner_id: tenant_id.to_string(),
            permissions,
            rate_limit: rate_limit.unwrap_or_default(),
            created_at: Utc::now(),
            expires_at: None,
            last_used_at: None,
            usage_count: 0,
            is_active: true,
            tenant_id: Some(tenant_id.to_string()),
        };

        self.inner
            .keys
            .write()
            .await
            .insert(key_id.clone(), api_key);
        self.persist().await?;

        info!(key_id = %key_id, tenant_id = %tenant_id, "Created tenant API key");

        Ok((key_plain, key_id))
    }

    /// Validate an API key and return a JWT token.
    pub async fn validate_key(&self, key_plain: &str) -> Result<String> {
        self.inner.validate_key(key_plain).await
    }

    /// Validate a JWT token.
    pub fn validate_jwt(&self, token: &str) -> Result<Claims> {
        self.inner.validate_jwt(token)
    }

    /// Check rate limit for a key.
    pub async fn check_rate_limit(&self, key_id: &str) -> Result<()> {
        self.inner.check_rate_limit(key_id).await
    }

    /// Revoke a key and persist.
    pub async fn revoke_key(&self, key_id: &str) -> Result<()> {
        self.inner.revoke_key(key_id).await?;
        self.persist().await?;
        Ok(())
    }

    /// List all keys for a tenant.
    pub async fn list_tenant_keys(&self, tenant_id: &str) -> Vec<ApiKey> {
        let keys = self.inner.keys.read().await;
        keys.values()
            .filter(|k| k.tenant_id.as_deref() == Some(tenant_id))
            .cloned()
            .collect()
    }

    /// Persist all keys to disk.
    async fn persist(&self) -> Result<()> {
        let keys = self.inner.keys.read().await;
        let keys_vec: Vec<&ApiKey> = keys.values().collect();
        let json = serde_json::to_string_pretty(&keys_vec)?;
        std::fs::write(self.storage_path.join("api_keys.json"), json)?;
        Ok(())
    }
}

/// Axum middleware: Require authentication WITH a tenant context.
///
/// Rejects requests that:
/// - Have no Bearer token
/// - Have an invalid JWT
/// - Have a token without a tenant_id
/// - Exceed rate limits
pub async fn require_tenant_auth(
    State(state): State<TenantAuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract token from header
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    let token = match token {
        Some(t) => t,
        None => {
            warn!("Missing authorization header");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Validate JWT
    let claims = match state.auth.validate_jwt(token) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "Invalid JWT");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Require tenant_id
    let tenant_id = match &claims.tenant_id {
        Some(t) => t.clone(),
        None => {
            warn!(key_id = %claims.key_id, "Token missing tenant_id");
            return Err(StatusCode::FORBIDDEN);
        }
    };

    // Check rate limit
    if let Err(e) = state.auth.check_rate_limit(&claims.key_id).await {
        warn!(error = %e, key_id = %claims.key_id, "Rate limit exceeded");
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    // Insert TenantContext for handlers
    let ctx = TenantContext {
        tenant_id,
        permissions: claims.permissions,
    };
    request.extensions_mut().insert(ctx);

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_api_key_lifecycle() {
        let auth = AuthManager::new();

        // Create key
        let key = auth
            .create_key(
                "Test Key".to_string(),
                "user1".to_string(),
                vec![Permission::Read, Permission::Chat],
                None,
                None,
            )
            .await
            .unwrap();

        assert!(key.starts_with("hsk_"));

        // Validate key
        let jwt = auth.validate_key(&key).await.unwrap();
        assert!(!jwt.is_empty());

        // Validate JWT
        let claims = auth.validate_jwt(&jwt).unwrap();
        assert!(claims.permissions.contains(&Permission::Read));
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let auth = AuthManager::new();

        let key = auth
            .create_key(
                "Rate Limit Test".to_string(),
                "user1".to_string(),
                vec![Permission::Read],
                Some(RateLimitConfig {
                    requests_per_minute: 2,
                    requests_per_hour: 100,
                    requests_per_day: 1000,
                }),
                None,
            )
            .await
            .unwrap();

        let jwt = auth.validate_key(&key).await.unwrap();
        let claims = auth.validate_jwt(&jwt).unwrap();

        // First two requests should pass
        assert!(auth.check_rate_limit(&claims.key_id).await.is_ok());
        assert!(auth.check_rate_limit(&claims.key_id).await.is_ok());

        // Third should fail
        assert!(auth.check_rate_limit(&claims.key_id).await.is_err());
    }

    #[tokio::test]
    async fn test_persistent_auth_tenant_key() {
        let tmp = tempfile::tempdir().unwrap();
        let auth = PersistentAuthManager::new(tmp.path());

        // Create a tenant key
        let (key_plain, key_id) = auth
            .create_tenant_key(
                "tenant-123",
                "Test Tenant Key".to_string(),
                vec![Permission::Read, Permission::Write],
                None,
            )
            .await
            .unwrap();

        assert!(key_plain.starts_with("hsk_"));
        assert!(!key_id.is_empty());

        // Validate key → JWT should contain tenant_id
        let jwt = auth.validate_key(&key_plain).await.unwrap();
        let claims = auth.validate_jwt(&jwt).unwrap();
        assert_eq!(claims.tenant_id, Some("tenant-123".to_string()));
        assert!(claims.permissions.contains(&Permission::Read));
        assert!(claims.permissions.contains(&Permission::Write));
    }

    #[tokio::test]
    async fn test_persistent_auth_list_tenant_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let auth = PersistentAuthManager::new(tmp.path());

        // Create keys for two tenants
        auth.create_tenant_key(
            "tenant-a",
            "Key A1".to_string(),
            vec![Permission::Read],
            None,
        )
        .await
        .unwrap();

        auth.create_tenant_key(
            "tenant-a",
            "Key A2".to_string(),
            vec![Permission::Write],
            None,
        )
        .await
        .unwrap();

        auth.create_tenant_key(
            "tenant-b",
            "Key B1".to_string(),
            vec![Permission::Admin],
            None,
        )
        .await
        .unwrap();

        // List tenant A keys
        let a_keys = auth.list_tenant_keys("tenant-a").await;
        assert_eq!(a_keys.len(), 2);

        // List tenant B keys
        let b_keys = auth.list_tenant_keys("tenant-b").await;
        assert_eq!(b_keys.len(), 1);
    }

    #[test]
    fn test_claims_backward_compat() {
        // Existing tokens without tenant_id should deserialize fine
        let json = r#"{
            "sub": "key1",
            "key_id": "key1",
            "permissions": ["Read"],
            "iat": 1000000,
            "exp": 2000000
        }"#;
        let claims: Claims = serde_json::from_str(json).unwrap();
        assert!(claims.tenant_id.is_none());
    }
}
