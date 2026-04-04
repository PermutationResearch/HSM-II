use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::columnar_engine::ColumnarGraphStore;
use crate::embedding_index::InMemoryEmbeddingIndex;
use crate::hyper_stigmergy::{HyperStigmergicMorphogenesis, ImprovementEvent};
use crate::property_graph::PropertyGraphSnapshot;
use crate::social_memory::SocialMemory;

pub const EMBEDDED_GRAPH_STORE_FILE: &str = "world_state.ladybug.bincode";
pub const EMBEDDED_GRAPH_WAL_FILE: &str = "world_state.ladybug.wal.bincode";
pub const EMBEDDED_GRAPH_LOCK_FILE: &str = "world_state.ladybug.lock";
pub const LEGACY_WORLD_STATE_FILE: &str = "world_state.bincode";
pub const LEGACY_EMBEDDING_INDEX_FILE: &str = "embedding_index.bincode";

/// When primary Ladybug save is on, still write `world_state.ladybug.bincode` if this env is `1`/`true`.
pub const ENV_HSMII_BINCODE_MIRROR: &str = "HSMII_BINCODE_MIRROR";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddedRuntimeMetadata {
    pub saved_at: u64,
    pub version: String,
    pub tick_count: u64,
    pub decay_rate: f64,
    pub prev_coherence: f64,
    pub improvement_history: Vec<ImprovementEvent>,
    pub current_intent: Option<String>,
    pub avoid_hints: Vec<String>,
    pub social_memory: SocialMemory,
    pub skill_bank: crate::skill::SkillBank,
    pub federation_config: Option<crate::federation::types::FederationConfig>,
    pub rlm_state: Option<crate::rlm::RLMState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddedGraphStoreSnapshot {
    pub metadata: EmbeddedRuntimeMetadata,
    pub embedding_index: InMemoryEmbeddingIndex,
    pub property_graph: PropertyGraphSnapshot,
    pub columnar_graph: ColumnarGraphStore,
    pub tx_id: u64,
    pub format_version: String,
}

pub struct EmbeddedGraphStore;

pub(crate) fn build_snapshot(
    world: &HyperStigmergicMorphogenesis,
    rlm_state: Option<&crate::rlm::RLMState>,
) -> EmbeddedGraphStoreSnapshot {
    let property_graph = world.to_property_graph_snapshot();
    let tx_id = HyperStigmergicMorphogenesis::current_timestamp();
    EmbeddedGraphStoreSnapshot {
        metadata: EmbeddedRuntimeMetadata {
            saved_at: HyperStigmergicMorphogenesis::current_timestamp(),
            version: "0.5.0".to_string(),
            tick_count: world.tick_count,
            decay_rate: world.decay_rate,
            prev_coherence: world.prev_coherence,
            improvement_history: world.improvement_history.clone(),
            current_intent: world.current_intent.clone(),
            avoid_hints: world.avoid_hints.clone(),
            social_memory: world.social_memory.clone(),
            skill_bank: world.skill_bank.clone(),
            federation_config: world.federation_config.clone(),
            rlm_state: rlm_state.cloned(),
        },
        embedding_index: world.embedding_index.clone(),
        property_graph: property_graph.clone(),
        columnar_graph: ColumnarGraphStore::from_snapshot(&property_graph),
        tx_id,
        format_version: "ladybug-single-file-v1".to_string(),
    }
}

/// Reconstruct runtime morphogenesis from a snapshot (shared by bincode and Ladybug checkpoint).
pub fn morph_from_snapshot(
    snapshot: EmbeddedGraphStoreSnapshot,
) -> (HyperStigmergicMorphogenesis, Option<crate::rlm::RLMState>) {
    if snapshot.format_version != "ladybug-single-file-v1" {
        eprintln!(
            "Warning: embedded graph format version mismatch (expected 'ladybug-single-file-v1', got '{}')",
            snapshot.format_version
        );
    }
    let mut morph =
        HyperStigmergicMorphogenesis::from_property_graph_snapshot(&snapshot.property_graph);
    morph.tick_count = snapshot.metadata.tick_count;
    morph.decay_rate = snapshot.metadata.decay_rate;
    morph.prev_coherence = snapshot.metadata.prev_coherence;
    morph.improvement_history = snapshot.metadata.improvement_history.clone();
    morph.current_intent = snapshot.metadata.current_intent.clone();
    morph.avoid_hints = snapshot.metadata.avoid_hints.clone();
    morph.social_memory = snapshot.metadata.social_memory.clone();
    morph.skill_bank = snapshot.metadata.skill_bank.clone();
    morph.federation_config = snapshot.metadata.federation_config.clone();
    morph.embedding_index = snapshot.embedding_index;
    (morph, snapshot.metadata.rlm_state)
}

impl EmbeddedGraphStore {
    /// Read and deserialize the embedded snapshot (bincode primary path).
    /// Used by [`Self::load_world`] and [`Self::load_skill_bank`].
    pub fn read_embedded_snapshot() -> anyhow::Result<EmbeddedGraphStoreSnapshot> {
        let bytes = match fs::read(EMBEDDED_GRAPH_STORE_FILE) {
            Ok(bytes) => bytes,
            Err(err) => {
                if Path::new(EMBEDDED_GRAPH_WAL_FILE).exists() {
                    let wal_bytes = fs::read(EMBEDDED_GRAPH_WAL_FILE)?;
                    fs::write(EMBEDDED_GRAPH_STORE_FILE, &wal_bytes)?;
                    wal_bytes
                } else {
                    return Err(err.into());
                }
            }
        };
        match bincode::deserialize::<EmbeddedGraphStoreSnapshot>(&bytes) {
            Ok(snapshot) => Ok(snapshot),
            Err(main_err) => {
                if Path::new(EMBEDDED_GRAPH_WAL_FILE).exists() {
                    let wal_bytes = fs::read(EMBEDDED_GRAPH_WAL_FILE)?;
                    fs::write(EMBEDDED_GRAPH_STORE_FILE, &wal_bytes)?;
                    bincode::deserialize(&wal_bytes).map_err(|e| e.into())
                } else {
                    Err(main_err.into())
                }
            }
        }
    }

    /// Load only the skill bank from the on-disk embedded store.
    /// Uses the same resolution order as [`load_world`] (Ladybug primary when enabled, else bincode).
    /// Pair with `HSM_SKILL_BANK_RELOAD_SECS` on the personal agent to pick up `hsm_trace2skill apply`
    /// without a full process restart.
    pub fn load_skill_bank() -> anyhow::Result<crate::skill::SkillBank> {
        #[cfg(feature = "lbug")]
        {
            if crate::persistence::lbug_world_store::primary_enabled() {
                if let Some(ref pb) = crate::persistence::lbug_world_store::primary_path() {
                    if pb.exists() {
                        match crate::persistence::lbug_world_store::load_world_primary(pb) {
                            Ok(snap) => return Ok(snap.metadata.skill_bank.clone()),
                            Err(e) => tracing::warn!(
                                target: "hsm",
                                "Ladybug primary load failed ({}); falling back to bincode for skill_bank",
                                e
                            ),
                        }
                    }
                }
            }
        }
        let snapshot = Self::read_embedded_snapshot()?;
        Ok(snapshot.metadata.skill_bank.clone())
    }

    pub fn save_world(
        world: &HyperStigmergicMorphogenesis,
        rlm_state: Option<&crate::rlm::RLMState>,
    ) -> anyhow::Result<usize> {
        let _lock = StoreLock::acquire()?;
        let snapshot = build_snapshot(world, rlm_state);
        let bytes = bincode::serialize(&snapshot)?;

        #[cfg(feature = "lbug")]
        if crate::persistence::lbug_world_store::primary_enabled() {
            if let Some(ref p) = crate::persistence::lbug_world_store::primary_path() {
                crate::persistence::lbug_world_store::save_world_primary(p, &snapshot, world)?;
                let mirror = std::env::var(ENV_HSMII_BINCODE_MIRROR)
                    .map(|v| {
                        let v = v.trim();
                        v == "1" || v.eq_ignore_ascii_case("true")
                    })
                    .unwrap_or(false);
                if mirror {
                    fs::write(EMBEDDED_GRAPH_WAL_FILE, &bytes)?;
                    fs::write(EMBEDDED_GRAPH_STORE_FILE, &bytes)?;
                    let _ = fs::remove_file(EMBEDDED_GRAPH_WAL_FILE);
                }
                return Ok(bytes.len());
            }
        }

        fs::write(EMBEDDED_GRAPH_WAL_FILE, &bytes)?;
        fs::write(EMBEDDED_GRAPH_STORE_FILE, &bytes)?;
        let _ = fs::remove_file(EMBEDDED_GRAPH_WAL_FILE);

        #[cfg(feature = "lbug")]
        {
            let property_graph = world.to_property_graph_snapshot();
            if let Ok(p) = std::env::var(crate::persistence::ladybug_native::ENV_HSMII_LADYBUG_PATH)
            {
                let p = p.trim();
                if !p.is_empty() && !crate::persistence::lbug_world_store::primary_enabled() {
                    if let Err(e) = crate::persistence::ladybug_native::sync_property_graph(
                        Path::new(p),
                        &property_graph,
                    ) {
                        tracing::warn!(
                            target: "hsm",
                            error = %e,
                            path = %p,
                            "native Ladybug (lbug) sync failed; bincode snapshot still saved"
                        );
                    }
                }
            }
        }

        Ok(bytes.len())
    }

    pub fn load_world(
    ) -> anyhow::Result<(HyperStigmergicMorphogenesis, Option<crate::rlm::RLMState>)> {
        #[cfg(feature = "lbug")]
        {
            if crate::persistence::lbug_world_store::primary_enabled() {
                if let Some(ref pb) = crate::persistence::lbug_world_store::primary_path() {
                    if pb.exists() {
                        match crate::persistence::lbug_world_store::load_world_primary(pb) {
                            Ok(snap) => return Ok(morph_from_snapshot(snap)),
                            Err(e) => tracing::warn!(
                                target: "hsm",
                                "Ladybug primary load failed ({}); falling back to bincode",
                                e
                            ),
                        }
                    }
                }
            }
        }

        let snapshot = Self::read_embedded_snapshot()?;
        Ok(morph_from_snapshot(snapshot))
    }

    pub fn exists() -> bool {
        #[cfg(feature = "lbug")]
        {
            if crate::persistence::lbug_world_store::primary_enabled() {
                if let Some(ref pb) = crate::persistence::lbug_world_store::primary_path() {
                    if pb.exists() {
                        return true;
                    }
                }
            }
        }
        Path::new(EMBEDDED_GRAPH_STORE_FILE).exists() || Path::new(EMBEDDED_GRAPH_WAL_FILE).exists()
    }

    pub fn migrate_legacy_files() -> anyhow::Result<bool> {
        if Self::exists() || !Path::new(LEGACY_WORLD_STATE_FILE).exists() {
            return Ok(false);
        }

        let bytes = fs::read(LEGACY_WORLD_STATE_FILE)?;
        let state: crate::hyper_stigmergy::SystemState = bincode::deserialize(&bytes)?;
        let embedding_index = if Path::new(LEGACY_EMBEDDING_INDEX_FILE).exists() {
            let index_bytes = fs::read(LEGACY_EMBEDDING_INDEX_FILE)?;
            bincode::deserialize(&index_bytes).unwrap_or_default()
        } else {
            InMemoryEmbeddingIndex::default()
        };

        let property_graph = state.morphogenesis.to_property_graph_snapshot();
        let columnar_graph = ColumnarGraphStore::from_snapshot(&property_graph);
        let snapshot = EmbeddedGraphStoreSnapshot {
            metadata: EmbeddedRuntimeMetadata {
                saved_at: state.saved_at,
                version: state.version,
                tick_count: state.morphogenesis.tick_count,
                decay_rate: state.morphogenesis.decay_rate,
                prev_coherence: state.morphogenesis.prev_coherence,
                improvement_history: state.morphogenesis.improvement_history.clone(),
                current_intent: state.morphogenesis.current_intent.clone(),
                avoid_hints: state.morphogenesis.avoid_hints.clone(),
                social_memory: state.morphogenesis.social_memory.clone(),
                skill_bank: state.morphogenesis.skill_bank.clone(),
                federation_config: state.morphogenesis.federation_config.clone(),
                rlm_state: state.rlm_state,
            },
            embedding_index,
            property_graph,
            columnar_graph,
            tx_id: HyperStigmergicMorphogenesis::current_timestamp(),
            format_version: "ladybug-single-file-v1".to_string(),
        };

        let new_bytes = bincode::serialize(&snapshot)?;
        fs::write(EMBEDDED_GRAPH_STORE_FILE, new_bytes)?;
        Ok(true)
    }
}

struct StoreLock;

impl StoreLock {
    fn acquire() -> anyhow::Result<Self> {
        if Path::new(EMBEDDED_GRAPH_LOCK_FILE).exists() {
            anyhow::bail!("Embedded graph store is already locked");
        }
        fs::write(EMBEDDED_GRAPH_LOCK_FILE, b"locked")?;
        Ok(Self)
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(EMBEDDED_GRAPH_LOCK_FILE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_store_filename_is_single_file() {
        assert!(EMBEDDED_GRAPH_STORE_FILE.ends_with(".bincode"));
        assert!(EMBEDDED_GRAPH_STORE_FILE.contains("ladybug"));
    }
}
