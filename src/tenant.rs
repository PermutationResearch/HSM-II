//! Multi-Tenant Support for HSM-II SaaS
//!
//! Provides tenant isolation, registry with LRU caching, and file/RooDB
//! persistence for the autonomous business team API.

use crate::autonomous_team::TeamOrchestrator;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

// ═══════════════════════════════════════════════════════════════════
// Section 1: Tenant Model
// ═══════════════════════════════════════════════════════════════════

/// A registered tenant in the SaaS system.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tenant {
    /// Unique tenant identifier (UUID).
    pub id: String,
    /// Human-readable tenant name (e.g. "Acme Corp").
    pub name: String,
    /// Billing plan tier.
    pub plan: TenantPlan,
    /// When the tenant was created.
    pub created_at: DateTime<Utc>,
    /// Tenant-specific settings/limits.
    pub settings: TenantSettings,
}

/// Billing plan tiers. Limits enforced at the API layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TenantPlan {
    Free,
    Starter,
    Pro,
    Enterprise,
}

impl TenantPlan {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Starter => "Starter",
            Self::Pro => "Pro",
            Self::Enterprise => "Enterprise",
        }
    }
}

impl Default for TenantPlan {
    fn default() -> Self {
        Self::Free
    }
}

/// Per-tenant limits and overrides.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TenantSettings {
    /// Maximum active team members (roles that can be enabled).
    pub max_team_members: usize,
    /// Maximum concurrent campaigns.
    pub max_campaigns: usize,
    /// API call quota per day.
    pub max_api_calls_per_day: u32,
    /// If set, forces this LLM provider for the tenant.
    pub llm_provider_override: Option<String>,
}

impl TenantSettings {
    /// Default settings for a given plan.
    pub fn for_plan(plan: TenantPlan) -> Self {
        match plan {
            TenantPlan::Free => Self {
                max_team_members: 5,
                max_campaigns: 2,
                max_api_calls_per_day: 100,
                llm_provider_override: None,
            },
            TenantPlan::Starter => Self {
                max_team_members: 10,
                max_campaigns: 10,
                max_api_calls_per_day: 1_000,
                llm_provider_override: None,
            },
            TenantPlan::Pro => Self {
                max_team_members: 14,
                max_campaigns: 50,
                max_api_calls_per_day: 10_000,
                llm_provider_override: None,
            },
            TenantPlan::Enterprise => Self {
                max_team_members: 14,
                max_campaigns: 500,
                max_api_calls_per_day: 100_000,
                llm_provider_override: None,
            },
        }
    }
}

impl Default for TenantSettings {
    fn default() -> Self {
        Self::for_plan(TenantPlan::Free)
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 2: LRU Cache for TeamOrchestrator instances
// ═══════════════════════════════════════════════════════════════════

/// A bounded LRU cache for tenant orchestrators.
///
/// When the cache is full and a new tenant is accessed, the least-recently-used
/// entry is evicted. Eviction is safe because all mutations are written through
/// to disk immediately.
struct OrchestratorCache {
    /// Ordered map: most-recently-used at the back.
    entries: Vec<(String, Arc<RwLock<TeamOrchestrator>>)>,
    capacity: usize,
}

impl OrchestratorCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            capacity,
        }
    }

    /// Get an orchestrator, moving it to most-recently-used position.
    fn get(&mut self, tenant_id: &str) -> Option<Arc<RwLock<TeamOrchestrator>>> {
        if let Some(pos) = self.entries.iter().position(|(id, _)| id == tenant_id) {
            let entry = self.entries.remove(pos);
            let orch = entry.1.clone();
            self.entries.push(entry);
            Some(orch)
        } else {
            None
        }
    }

    /// Insert an orchestrator, evicting LRU if at capacity.
    fn insert(&mut self, tenant_id: String, orch: Arc<RwLock<TeamOrchestrator>>) {
        // Remove existing entry if present
        self.entries.retain(|(id, _)| id != &tenant_id);

        // Evict LRU if at capacity
        if self.entries.len() >= self.capacity {
            let evicted = self.entries.remove(0);
            info!(tenant_id = %evicted.0, "Evicted tenant orchestrator from cache");
        }

        self.entries.push((tenant_id, orch));
    }

    fn remove(&mut self, tenant_id: &str) {
        self.entries.retain(|(id, _)| id != tenant_id);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 3: TenantRegistry — manages lifecycle and cached state
// ═══════════════════════════════════════════════════════════════════

/// Registry of all tenants with LRU-cached TeamOrchestrator instances.
///
/// # Persistence Strategy
///
/// - **File-based** (default): `~/.hsmii/auth/tenants.json`
/// - **RooDB** (optional): `tenants` table when `HSM_ROODB_URL` is set
///
/// TeamOrchestrator state is always file-based (one directory per tenant).
pub struct TenantRegistry {
    /// All registered tenants.
    tenants: Arc<RwLock<HashMap<String, Tenant>>>,
    /// LRU cache of loaded orchestrators.
    cache: Arc<RwLock<OrchestratorCache>>,
    /// Base directory for tenant state (`~/.hsmii`).
    base_dir: PathBuf,
}

impl TenantRegistry {
    /// Create a new registry.
    ///
    /// Loads existing tenants from `{base_dir}/auth/tenants.json` if present.
    pub fn new(base_dir: &Path) -> Self {
        let tenants = Self::load_tenants_file(base_dir).unwrap_or_default();
        Self {
            tenants: Arc::new(RwLock::new(tenants)),
            cache: Arc::new(RwLock::new(OrchestratorCache::new(100))),
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// Create a new registry with a custom cache capacity.
    pub fn with_capacity(base_dir: &Path, cache_capacity: usize) -> Self {
        let tenants = Self::load_tenants_file(base_dir).unwrap_or_default();
        Self {
            tenants: Arc::new(RwLock::new(tenants)),
            cache: Arc::new(RwLock::new(OrchestratorCache::new(cache_capacity))),
            base_dir: base_dir.to_path_buf(),
        }
    }

    /// Register a new tenant.
    pub async fn create_tenant(&self, name: &str, plan: TenantPlan) -> Result<Tenant> {
        let tenant = Tenant {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            plan,
            created_at: Utc::now(),
            settings: TenantSettings::for_plan(plan),
        };

        // Create tenant directory
        let tenant_dir = self.tenant_dir(&tenant.id);
        std::fs::create_dir_all(&tenant_dir)?;

        // Store tenant
        {
            let mut tenants = self.tenants.write().await;
            tenants.insert(tenant.id.clone(), tenant.clone());
        }

        // Persist to disk
        self.persist_tenants().await?;

        info!(tenant_id = %tenant.id, name = %tenant.name, plan = %plan.label(), "Tenant created");

        Ok(tenant)
    }

    /// Look up a tenant by ID.
    pub async fn get_tenant(&self, tenant_id: &str) -> Option<Tenant> {
        let tenants = self.tenants.read().await;
        tenants.get(tenant_id).cloned()
    }

    /// List all tenants.
    pub async fn list_tenants(&self) -> Vec<Tenant> {
        let tenants = self.tenants.read().await;
        tenants.values().cloned().collect()
    }

    /// Get (or load) the TeamOrchestrator for a tenant.
    ///
    /// Checks LRU cache first; on miss, loads from disk (~5ms).
    pub async fn get_orchestrator(&self, tenant_id: &str) -> Result<Arc<RwLock<TeamOrchestrator>>> {
        // Check cache
        {
            let mut cache = self.cache.write().await;
            if let Some(orch) = cache.get(tenant_id) {
                return Ok(orch);
            }
        }

        // Verify tenant exists
        {
            let tenants = self.tenants.read().await;
            if !tenants.contains_key(tenant_id) {
                return Err(anyhow!("Tenant not found: {}", tenant_id));
            }
        }

        // Load from disk
        let tenant_dir = self.tenant_dir(tenant_id);
        std::fs::create_dir_all(&tenant_dir)?;

        let mut orch = TeamOrchestrator::new(&tenant_dir);
        // Try to load saved member state
        if let Err(e) = orch.load_members() {
            warn!(tenant_id = %tenant_id, error = %e, "Could not load saved member state");
        }

        let orch = Arc::new(RwLock::new(orch));

        // Insert into cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(tenant_id.to_string(), orch.clone());
        }

        Ok(orch)
    }

    /// Save a tenant's orchestrator state to disk.
    pub async fn save_tenant_state(&self, tenant_id: &str) -> Result<()> {
        let orch = {
            let mut cache = self.cache.write().await;
            cache.get(tenant_id)
        };

        if let Some(orch) = orch {
            let orch = orch.read().await;
            orch.save()?;
            info!(tenant_id = %tenant_id, "Tenant state saved");
        }

        Ok(())
    }

    /// Evict a tenant from the cache (does NOT delete from disk).
    pub async fn evict(&self, tenant_id: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(tenant_id);
    }

    /// Delete a tenant entirely (from registry, cache, and disk).
    pub async fn delete_tenant(&self, tenant_id: &str) -> Result<()> {
        // Remove from cache
        {
            let mut cache = self.cache.write().await;
            cache.remove(tenant_id);
        }

        // Remove from registry
        {
            let mut tenants = self.tenants.write().await;
            tenants.remove(tenant_id);
        }

        // Remove from disk
        let tenant_dir = self.tenant_dir(tenant_id);
        if tenant_dir.exists() {
            std::fs::remove_dir_all(&tenant_dir)?;
        }

        // Persist updated tenant list
        self.persist_tenants().await?;

        info!(tenant_id = %tenant_id, "Tenant deleted");

        Ok(())
    }

    /// Update a tenant's plan (and settings).
    pub async fn update_plan(&self, tenant_id: &str, plan: TenantPlan) -> Result<()> {
        let mut tenants = self.tenants.write().await;
        let tenant = tenants
            .get_mut(tenant_id)
            .ok_or_else(|| anyhow!("Tenant not found: {}", tenant_id))?;
        tenant.plan = plan;
        tenant.settings = TenantSettings::for_plan(plan);
        drop(tenants);

        self.persist_tenants().await?;
        Ok(())
    }

    /// Number of registered tenants.
    pub async fn tenant_count(&self) -> usize {
        let tenants = self.tenants.read().await;
        tenants.len()
    }

    // ── Internal helpers ───────────────────────────────────────────

    fn tenant_dir(&self, tenant_id: &str) -> PathBuf {
        self.base_dir.join("tenants").join(tenant_id)
    }

    fn auth_dir(&self) -> PathBuf {
        self.base_dir.join("auth")
    }

    fn tenants_file(&self) -> PathBuf {
        self.auth_dir().join("tenants.json")
    }

    async fn persist_tenants(&self) -> Result<()> {
        let auth_dir = self.auth_dir();
        std::fs::create_dir_all(&auth_dir)?;

        let tenants = self.tenants.read().await;
        let tenants_vec: Vec<&Tenant> = tenants.values().collect();
        let json = serde_json::to_string_pretty(&tenants_vec)?;
        std::fs::write(self.tenants_file(), json)?;

        Ok(())
    }

    fn load_tenants_file(base_dir: &Path) -> Result<HashMap<String, Tenant>> {
        let path = base_dir.join("auth").join("tenants.json");
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let data = std::fs::read_to_string(&path)?;
        let tenants: Vec<Tenant> = serde_json::from_str(&data)?;
        let map = tenants.into_iter().map(|t| (t.id.clone(), t)).collect();
        Ok(map)
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 4: Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_plan_defaults() {
        let free = TenantSettings::for_plan(TenantPlan::Free);
        assert_eq!(free.max_team_members, 5);
        assert_eq!(free.max_campaigns, 2);
        assert_eq!(free.max_api_calls_per_day, 100);

        let pro = TenantSettings::for_plan(TenantPlan::Pro);
        assert_eq!(pro.max_team_members, 14);
        assert_eq!(pro.max_campaigns, 50);
        assert_eq!(pro.max_api_calls_per_day, 10_000);

        let ent = TenantSettings::for_plan(TenantPlan::Enterprise);
        assert_eq!(ent.max_api_calls_per_day, 100_000);
    }

    #[test]
    fn test_orchestrator_cache() {
        let mut cache = OrchestratorCache::new(2);

        let orch1 = Arc::new(RwLock::new(TeamOrchestrator::new(Path::new("/tmp/t1"))));
        let orch2 = Arc::new(RwLock::new(TeamOrchestrator::new(Path::new("/tmp/t2"))));
        let orch3 = Arc::new(RwLock::new(TeamOrchestrator::new(Path::new("/tmp/t3"))));

        cache.insert("t1".to_string(), orch1.clone());
        cache.insert("t2".to_string(), orch2.clone());
        assert_eq!(cache.len(), 2);

        // Access t1 to make it MRU
        assert!(cache.get("t1").is_some());

        // Insert t3 — should evict t2 (LRU)
        cache.insert("t3".to_string(), orch3);
        assert_eq!(cache.len(), 2);
        assert!(cache.get("t2").is_none()); // evicted
        assert!(cache.get("t1").is_some()); // still present
        assert!(cache.get("t3").is_some()); // newly inserted
    }

    #[tokio::test]
    async fn test_tenant_registry_create_and_get() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = TenantRegistry::new(tmp.path());

        let tenant = registry
            .create_tenant("Acme Corp", TenantPlan::Starter)
            .await
            .unwrap();

        assert_eq!(tenant.name, "Acme Corp");
        assert_eq!(tenant.plan, TenantPlan::Starter);
        assert_eq!(tenant.settings.max_team_members, 10);

        // Can retrieve it
        let loaded = registry.get_tenant(&tenant.id).await.unwrap();
        assert_eq!(loaded.name, "Acme Corp");
    }

    #[tokio::test]
    async fn test_tenant_registry_persistence() {
        let tmp = tempfile::tempdir().unwrap();
        let tenant_id;

        // Create tenant in first registry instance
        {
            let registry = TenantRegistry::new(tmp.path());
            let tenant = registry
                .create_tenant("Persist Test", TenantPlan::Pro)
                .await
                .unwrap();
            tenant_id = tenant.id;
        }

        // Load from disk in new registry instance
        {
            let registry = TenantRegistry::new(tmp.path());
            let tenant = registry.get_tenant(&tenant_id).await.unwrap();
            assert_eq!(tenant.name, "Persist Test");
            assert_eq!(tenant.plan, TenantPlan::Pro);
        }
    }

    #[tokio::test]
    async fn test_tenant_orchestrator_lifecycle() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = TenantRegistry::new(tmp.path());

        let tenant = registry
            .create_tenant("Team Test", TenantPlan::Free)
            .await
            .unwrap();

        // Get orchestrator (loads from disk)
        let orch = registry.get_orchestrator(&tenant.id).await.unwrap();
        {
            let orch = orch.read().await;
            assert_eq!(orch.members.len(), 14); // all roles initialized
        }

        // Save state
        registry.save_tenant_state(&tenant.id).await.unwrap();

        // Evict from cache
        registry.evict(&tenant.id).await;

        // Re-load (should hit disk, not cache)
        let orch2 = registry.get_orchestrator(&tenant.id).await.unwrap();
        {
            let orch2 = orch2.read().await;
            assert_eq!(orch2.members.len(), 14);
        }
    }

    #[tokio::test]
    async fn test_tenant_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = TenantRegistry::new(tmp.path());

        let tenant = registry
            .create_tenant("Delete Me", TenantPlan::Free)
            .await
            .unwrap();

        // Load orchestrator to populate cache
        let _ = registry.get_orchestrator(&tenant.id).await.unwrap();

        // Delete
        registry.delete_tenant(&tenant.id).await.unwrap();

        // Should be gone
        assert!(registry.get_tenant(&tenant.id).await.is_none());
        assert!(registry.get_orchestrator(&tenant.id).await.is_err());
    }

    #[tokio::test]
    async fn test_nonexistent_tenant_orchestrator() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = TenantRegistry::new(tmp.path());

        let result = registry.get_orchestrator("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_plan() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = TenantRegistry::new(tmp.path());

        let tenant = registry
            .create_tenant("Upgrade Me", TenantPlan::Free)
            .await
            .unwrap();
        assert_eq!(tenant.settings.max_api_calls_per_day, 100);

        registry
            .update_plan(&tenant.id, TenantPlan::Enterprise)
            .await
            .unwrap();

        let updated = registry.get_tenant(&tenant.id).await.unwrap();
        assert_eq!(updated.plan, TenantPlan::Enterprise);
        assert_eq!(updated.settings.max_api_calls_per_day, 100_000);
    }

    #[tokio::test]
    async fn test_tenant_count() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = TenantRegistry::new(tmp.path());

        assert_eq!(registry.tenant_count().await, 0);

        registry
            .create_tenant("T1", TenantPlan::Free)
            .await
            .unwrap();
        registry
            .create_tenant("T2", TenantPlan::Free)
            .await
            .unwrap();

        assert_eq!(registry.tenant_count().await, 2);
    }
}
