//! Plugin lifecycle primitives: manifest validation, enable/disable, registry wiring.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::schemas::{ToolProviderMetadata, ToolRegistry, ToolSchema, ValidationError};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub provider: ToolProviderMetadata,
    #[serde(default)]
    pub tools: Vec<ToolSchema>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub checksum_sha256: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginStateIndex {
    pub enabled: HashMap<String, bool>,
}

pub struct PluginManager {
    manifest_dir: PathBuf,
    state_path: PathBuf,
    allow_unsigned: bool,
}

impl PluginManager {
    pub fn new(manifest_dir: PathBuf, state_path: PathBuf, allow_unsigned: bool) -> Self {
        Self {
            manifest_dir,
            state_path,
            allow_unsigned,
        }
    }

    pub fn from_env() -> Self {
        let cfg = crate::harness::RuntimeConfig::from_env();
        Self::new(
            cfg.plugins.manifest_dir,
            cfg.state_dir.join("plugin_state.json"),
            cfg.plugins.allow_unsigned,
        )
    }

    fn load_state(&self) -> Result<PluginStateIndex> {
        if !self.state_path.exists() {
            return Ok(PluginStateIndex::default());
        }
        Ok(serde_json::from_slice(&fs::read(&self.state_path)?)?)
    }

    fn save_state(&self, state: &PluginStateIndex) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.state_path, serde_json::to_vec_pretty(state)?)?;
        Ok(())
    }

    pub fn set_enabled(&self, plugin_id: &str, enabled: bool) -> Result<()> {
        let mut state = self.load_state()?;
        state.enabled.insert(plugin_id.to_string(), enabled);
        self.save_state(&state)
    }

    pub fn list_manifests(&self) -> Result<Vec<PluginManifest>> {
        if !self.manifest_dir.exists() {
            return Ok(Vec::new());
        }
        let state = self.load_state()?;
        let mut manifests = Vec::new();
        for entry in fs::read_dir(&self.manifest_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let raw = fs::read(&path)?;
            let mut manifest: PluginManifest = serde_json::from_slice(&raw)
                .map_err(|e| anyhow!("invalid manifest {}: {}", path.display(), e))?;
            self.verify_checksum(&path, &raw, &manifest)?;
            if let Some(v) = state.enabled.get(&manifest.id) {
                manifest.enabled = *v;
            }
            manifests.push(manifest);
        }
        manifests.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(manifests)
    }

    fn verify_checksum(&self, path: &Path, raw: &[u8], manifest: &PluginManifest) -> Result<()> {
        let Some(expected) = manifest.checksum_sha256.as_deref() else {
            if self.allow_unsigned {
                return Ok(());
            }
            return Err(anyhow!(
                "plugin {} missing checksum_sha256: {}",
                manifest.id,
                path.display()
            ));
        };
        let got = format!("{:x}", Sha256::digest(raw));
        if got != expected {
            return Err(anyhow!(
                "plugin {} checksum mismatch: expected {}, got {}",
                manifest.id,
                expected,
                got
            ));
        }
        Ok(())
    }

    pub fn register_enabled_into_registry(&self, registry: &mut ToolRegistry) -> Result<()> {
        for manifest in self.list_manifests()? {
            if !manifest.enabled {
                continue;
            }
            registry.register_provider(manifest.provider.clone());
            for tool in manifest.tools {
                registry
                    .register_external_tool(tool, &manifest.provider.id)
                    .map_err(|e| map_validation(e, &manifest.id))?;
            }
        }
        Ok(())
    }
}

fn map_validation(err: ValidationError, plugin_id: &str) -> anyhow::Error {
    anyhow!("plugin {} registration failed: {}", plugin_id, err)
}
