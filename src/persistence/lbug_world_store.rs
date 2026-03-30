//! Primary on-disk world store using **LadybugDB** (`lbug`): canonical [`EmbeddedGraphStoreSnapshot`]
//! payload in `HsmCheckpoint` plus typed nodes for Cypher / future vector+FTS alignment.

use std::path::{Path, PathBuf};

use anyhow::Context;
use lbug::{Connection, Database, SystemConfig, Value};

use crate::embedded_graph_store::EmbeddedGraphStoreSnapshot;
use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;

/// When `1`/`true` together with [`super::ladybug_native::ENV_HSMII_LADYBUG_PATH`], world
/// save/load uses Ladybug as the primary store (bincode optional mirror).
pub const ENV_HSMII_LADYBUG_PRIMARY: &str = "HSMII_LADYBUG_PRIMARY";

pub fn primary_enabled() -> bool {
    let path_ok = std::env::var(super::ladybug_native::ENV_HSMII_LADYBUG_PATH)
        .map(|p| !p.trim().is_empty())
        .unwrap_or(false);
    if !path_ok {
        return false;
    }
    std::env::var(ENV_HSMII_LADYBUG_PRIMARY)
        .map(|v| {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

pub fn primary_path() -> Option<PathBuf> {
    if !primary_enabled() {
        return None;
    }
    std::env::var(super::ladybug_native::ENV_HSMII_LADYBUG_PATH)
        .ok()
        .map(|s| PathBuf::from(s.trim()))
}

/// Run an ad-hoc Cypher string against the primary Ladybug store (for debugging / power users).
pub fn run_cypher_debug(query: &str) -> anyhow::Result<String> {
    let path = primary_path().or_else(|| {
        std::env::var(super::ladybug_native::ENV_HSMII_LADYBUG_PATH)
            .ok()
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from)
    })
    .context("Set HSMII_LADYBUG_PATH to a database path")?;

    let db = Database::new(&path, SystemConfig::default()).map_err(|e| anyhow::anyhow!("{}", e))?;
    let conn = Connection::new(&db).map_err(|e| anyhow::anyhow!("{}", e))?;
    let mut result = conn.query(query).map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(format!("{}", result))
}

/// Save full world: clear HSM subgraph, write checkpoint blob + typed mirror + property graph projection.
pub fn save_world_primary(
    path: &Path,
    snapshot: &EmbeddedGraphStoreSnapshot,
    world: &HyperStigmergicMorphogenesis,
) -> anyhow::Result<usize> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| format!("create_dir_all {:?}", parent))?;
        }
    }

    let payload = bincode::serialize(snapshot).context("bincode snapshot")?;
    let db = Database::new(path, SystemConfig::default()).map_err(|e| anyhow::anyhow!("{}", e))?;
    let conn = Connection::new(&db).map_err(|e| anyhow::anyhow!("{}", e))?;

    super::lbug_hsm_schema::apply_schema(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;
    super::lbug_hsm_schema::clear_hsm_graph(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut chk = conn
        .prepare(
            "CREATE (:HsmCheckpoint {id: $id, saved_at: $saved_at, tick: $tick, format_version: $fv, payload: $payload});",
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    conn.execute(
        &mut chk,
        vec![
            ("id", Value::String("singleton".into())),
            (
                "saved_at",
                Value::Int64(snapshot.metadata.saved_at as i64),
            ),
            ("tick", Value::Int64(snapshot.metadata.tick_count as i64)),
            ("fv", Value::String(snapshot.format_version.clone())),
            ("payload", Value::Blob(payload.clone())),
        ],
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut b_ins = conn
        .prepare(
            "CREATE (:HsmBelief {bid: $bid, content: $content, confidence: $confidence, abstract_l0: $a0, overview_l1: $a1, source_json: $src, created_at: $ca, updated_at: $ua, update_count: $uc});",
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    for belief in &world.beliefs {
        let src = serde_json::to_string(&belief.source).unwrap_or_else(|_| "{}".into());
        conn.execute(
            &mut b_ins,
            vec![
                ("bid", Value::Int64(belief.id as i64)),
                ("content", Value::String(belief.content.clone())),
                ("confidence", Value::Double(belief.confidence)),
                (
                    "a0",
                    Value::String(belief.abstract_l0.clone().unwrap_or_default()),
                ),
                (
                    "a1",
                    Value::String(belief.overview_l1.clone().unwrap_or_default()),
                ),
                ("src", Value::String(src)),
                ("ca", Value::Int64(belief.created_at as i64)),
                ("ua", Value::Int64(belief.updated_at as i64)),
                ("uc", Value::Int64(belief.update_count as i64)),
            ],
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    let mut s_ins = conn
        .prepare(
            "CREATE (:HsmSkill {sid: $sid, title: $title, principle: $principle, confidence: $confidence, status_json: $st, embedding_f32_json: $emb, created_at: $ca, last_evolved: $le});",
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    for skill in world.skill_bank.all_skills() {
        let st = serde_json::to_string(&skill.status).unwrap_or_else(|_| "{}".into());
        let emb = skill
            .embedding
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_default();
        conn.execute(
            &mut s_ins,
            vec![
                ("sid", Value::String(skill.id.clone())),
                ("title", Value::String(skill.title.clone())),
                ("principle", Value::String(skill.principle.clone())),
                ("confidence", Value::Double(skill.confidence)),
                ("st", Value::String(st)),
                ("emb", Value::String(emb)),
                ("ca", Value::Int64(skill.created_at as i64)),
                ("le", Value::Int64(skill.last_evolved as i64)),
            ],
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    let mut e_ins = conn
        .prepare(
            "CREATE (:HsmExperience {eid: $eid, description: $desc, context: $ctx, outcome_json: $out, timestamp: $ts, tick: $tk});",
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    for exp in &world.experiences {
        let out = serde_json::to_string(&exp.outcome).unwrap_or_else(|_| "{}".into());
        conn.execute(
            &mut e_ins,
            vec![
                ("eid", Value::Int64(exp.id as i64)),
                ("desc", Value::String(exp.description.clone())),
                ("ctx", Value::String(exp.context.clone())),
                ("out", Value::String(out)),
                ("ts", Value::Int64(exp.timestamp as i64)),
                ("tk", Value::Int64(exp.tick as i64)),
            ],
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    if let Some(ref fed) = world.federation_config {
        let mut f_ins = conn
            .prepare("CREATE (:HsmFederationMeta {fkey: $k, payload_json: $j});")
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let json = serde_json::to_string(fed).unwrap_or_else(|_| "{}".into());
        conn.execute(
            &mut f_ins,
            vec![
                ("k", Value::String("federation_config".into())),
                ("j", Value::String(json)),
            ],
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    super::ladybug_native::sync_property_graph_on_connection(&conn, &snapshot.property_graph)?;

    super::lbug_hsm_schema::try_apply_analytical_indices(&conn);

    Ok(payload.len())
}

/// Load world from primary Ladybug checkpoint blob.
pub fn load_world_primary(path: &Path) -> anyhow::Result<EmbeddedGraphStoreSnapshot> {
    let db = Database::new(path, SystemConfig::default()).map_err(|e| anyhow::anyhow!("{}", e))?;
    let conn = Connection::new(&db).map_err(|e| anyhow::anyhow!("{}", e))?;
    super::lbug_hsm_schema::apply_schema(&conn).map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut q = conn
        .query("MATCH (c:HsmCheckpoint {id: 'singleton'}) RETURN c.payload AS payload LIMIT 1;")
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let row = q
        .next()
        .ok_or_else(|| anyhow::anyhow!("no HsmCheckpoint row in Ladybug store"))?;
    let blob = match &row[0] {
        Value::Blob(b) => b.clone(),
        _ => anyhow::bail!("checkpoint payload column type mismatch"),
    };

    let snapshot: EmbeddedGraphStoreSnapshot =
        bincode::deserialize(&blob).context("bincode deserialize checkpoint")?;
    Ok(snapshot)
}

pub fn primary_store_exists(path: &Path) -> bool {
    path.exists()
}
