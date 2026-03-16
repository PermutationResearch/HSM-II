//! Persistent JSON storage for autocontext knowledge base.
//!
//! Stores playbooks, hints, and generation records to ~/.hsmii/autocontext/.
//! Uses atomic writes (temp file + rename) for crash safety.

use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use super::{Generation, KnowledgeBase};

/// Configuration for autocontext storage.
#[derive(Clone, Debug)]
pub struct StorageConfig {
    pub base_path: PathBuf,
}

impl StorageConfig {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Default storage path: ~/.hsmii/autocontext/
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .expect("Could not find home directory")
            .join(".hsmii")
            .join("autocontext")
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            base_path: Self::default_path(),
        }
    }
}

/// Persistent JSON storage for autocontext data.
pub struct AutoContextStore {
    config: StorageConfig,
}

impl AutoContextStore {
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            config: StorageConfig::new(base_path),
        }
    }

    pub fn with_default_path() -> Self {
        Self {
            config: StorageConfig::default(),
        }
    }

    pub fn base_path(&self) -> &Path {
        &self.config.base_path
    }

    // ── Directory management ─────────────────────────────────────────────

    /// Ensure the directory structure exists.
    pub async fn ensure_dirs(&self) -> anyhow::Result<()> {
        let dirs = [
            self.config.base_path.clone(),
            self.config.base_path.join("generations"),
            self.config.base_path.join("models"),
            self.config.base_path.join("models").join("training_data"),
        ];

        for dir in &dirs {
            tokio::fs::create_dir_all(dir).await?;
        }

        debug!("AutoContext storage dirs ensured at {:?}", self.config.base_path);
        Ok(())
    }

    // ── Knowledge base load/save ─────────────────────────────────────────

    /// Load the full knowledge base from disk.
    /// Returns empty KnowledgeBase if files don't exist.
    pub async fn load(&self) -> anyhow::Result<KnowledgeBase> {
        let mut kb = KnowledgeBase::new();

        // Load playbooks
        let playbooks_path = self.config.base_path.join("playbooks.json");
        if playbooks_path.exists() {
            let data = tokio::fs::read_to_string(&playbooks_path).await?;
            kb.playbooks = serde_json::from_str(&data).unwrap_or_else(|e| {
                warn!("Failed to parse playbooks.json: {}, starting fresh", e);
                vec![]
            });
        }

        // Load hints
        let hints_path = self.config.base_path.join("hints.json");
        if hints_path.exists() {
            let data = tokio::fs::read_to_string(&hints_path).await?;
            kb.hints = serde_json::from_str(&data).unwrap_or_else(|e| {
                warn!("Failed to parse hints.json: {}, starting fresh", e);
                vec![]
            });
        }

        info!(
            "AutoContext loaded: {} playbooks, {} hints",
            kb.playbooks.len(),
            kb.hints.len()
        );
        Ok(kb)
    }

    /// Save the knowledge base to disk (atomic write).
    pub async fn save(&self, kb: &KnowledgeBase) -> anyhow::Result<()> {
        self.ensure_dirs().await?;

        // Save playbooks atomically
        let playbooks_path = self.config.base_path.join("playbooks.json");
        self.atomic_write(&playbooks_path, &kb.playbooks).await?;

        // Save hints atomically
        let hints_path = self.config.base_path.join("hints.json");
        self.atomic_write(&hints_path, &kb.hints).await?;

        debug!(
            "AutoContext saved: {} playbooks, {} hints",
            kb.playbooks.len(),
            kb.hints.len()
        );
        Ok(())
    }

    // ── Generation records ───────────────────────────────────────────────

    /// Save a generation record.
    pub async fn save_generation(&self, gen: &Generation) -> anyhow::Result<()> {
        self.ensure_dirs().await?;
        let path = self
            .config
            .base_path
            .join("generations")
            .join(format!("gen_{:06}.json", gen.id));
        self.atomic_write(&path, gen).await?;
        debug!("Saved generation {} to {:?}", gen.id, path);
        Ok(())
    }

    /// Load all generation records (sorted by ID).
    pub async fn load_generations(&self) -> anyhow::Result<Vec<Generation>> {
        let gen_dir = self.config.base_path.join("generations");
        if !gen_dir.exists() {
            return Ok(vec![]);
        }

        let mut generations = Vec::new();
        let mut entries = tokio::fs::read_dir(&gen_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                match tokio::fs::read_to_string(&path).await {
                    Ok(data) => match serde_json::from_str::<Generation>(&data) {
                        Ok(gen) => generations.push(gen),
                        Err(e) => warn!("Failed to parse {:?}: {}", path, e),
                    },
                    Err(e) => warn!("Failed to read {:?}: {}", path, e),
                }
            }
        }

        generations.sort_by_key(|g| g.id);
        Ok(generations)
    }

    // ── Training data export ─────────────────────────────────────────────

    /// Save training data for model distillation.
    pub async fn save_training_data(
        &self,
        examples: &[super::TrainingExample],
    ) -> anyhow::Result<PathBuf> {
        self.ensure_dirs().await?;
        let filename = format!(
            "export_{}.jsonl",
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        );
        let path = self
            .config
            .base_path
            .join("models")
            .join("training_data")
            .join(&filename);

        let mut content = String::new();
        for example in examples {
            content.push_str(&serde_json::to_string(example)?);
            content.push('\n');
        }
        tokio::fs::write(&path, content).await?;

        info!("Exported {} training examples to {:?}", examples.len(), path);
        Ok(path)
    }

    // ── Internal helpers ─────────────────────────────────────────────────

    /// Atomic write: serialize to temp file, then rename.
    async fn atomic_write<T: serde::Serialize>(
        &self,
        path: &Path,
        data: &T,
    ) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(data)?;
        let tmp_path = path.with_extension("json.tmp");
        tokio::fs::write(&tmp_path, &json).await?;
        tokio::fs::rename(&tmp_path, path).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autocontext::{Hint, Playbook};

    #[tokio::test]
    async fn test_storage_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = AutoContextStore::new(tmp.path().join("autocontext"));
        store.ensure_dirs().await.unwrap();

        let mut kb = KnowledgeBase::new();
        kb.upsert_playbook(Playbook::new("Test PB", "desc", "test pattern"));
        kb.upsert_hint(Hint::new("Use grep first", "code search", 0.8));

        store.save(&kb).await.unwrap();

        let loaded = store.load().await.unwrap();
        assert_eq!(loaded.playbooks.len(), 1);
        assert_eq!(loaded.hints.len(), 1);
        assert_eq!(loaded.playbooks[0].name, "Test PB");
        assert_eq!(loaded.hints[0].content, "Use grep first");
    }

    #[tokio::test]
    async fn test_generation_save_load() {
        let tmp = tempfile::tempdir().unwrap();
        let store = AutoContextStore::new(tmp.path().join("autocontext"));

        let gen = Generation::new(1, "test scenario");
        store.save_generation(&gen).await.unwrap();

        let loaded = store.load_generations().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].scenario, "test scenario");
    }

    #[tokio::test]
    async fn test_load_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let store = AutoContextStore::new(tmp.path().join("autocontext"));
        store.ensure_dirs().await.unwrap();

        let kb = store.load().await.unwrap();
        assert_eq!(kb.playbooks.len(), 0);
        assert_eq!(kb.hints.len(), 0);
    }
}
