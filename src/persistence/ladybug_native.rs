//! Optional sync of [`crate::property_graph::PropertyGraphSnapshot`] into an on-disk **Ladybug**
//! (`lbug`) database. This is separate from [`super::HsmSqliteStore`] (SQLite subsystem store).
//!
//! Enable with `--features lbug` and set [`ENV_HSMII_LADYBUG_PATH`] to the database path (same
//! style as `Database::new` in the `lbug` crate: a filesystem path prefix for the store).

/// Environment variable: path passed to `lbug::Database::new` for native graph sync.
pub const ENV_HSMII_LADYBUG_PATH: &str = "HSMII_LADYBUG_PATH";

use std::path::Path;

use anyhow::Context;
use lbug::{Connection, Database, SystemConfig, Value};

use crate::property_graph::{PropertyGraphSnapshot, PropertyValue};

fn props_json(props: &std::collections::HashMap<String, PropertyValue>) -> anyhow::Result<String> {
    serde_json::to_string(props).context("serialize node/rel properties")
}

/// Writes `graph` on an existing connection (schema must exist). Replaces `HsmNode` / `HsmRel` rows.
pub fn sync_property_graph_on_connection(
    conn: &Connection<'_>,
    graph: &PropertyGraphSnapshot,
) -> anyhow::Result<()> {
    super::lbug_hsm_schema::apply_schema(conn).map_err(|e| anyhow::anyhow!("{}", e))?;

    conn.query("MATCH (n:HsmNode) DETACH DELETE n;")
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut ins = conn
        .prepare("CREATE (:HsmNode {id: $id, labels_json: $labels_json, props_json: $props_json});")
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    for node in &graph.nodes {
        let labels = serde_json::to_string(&node.labels).context("labels json")?;
        let pj = props_json(&node.properties)?;
        conn.execute(
            &mut ins,
            vec![
                ("id", Value::String(node.id.clone())),
                ("labels_json", Value::String(labels)),
                ("props_json", Value::String(pj)),
            ],
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    let mut rel_ins = conn
        .prepare(
            "MATCH (a:HsmNode), (b:HsmNode) WHERE a.id = $sid AND b.id = $eid \
             CREATE (a)-[:HsmRel {rid: $rid, rel_type: $rtype, props_json: $pjson}]->(b);",
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    for rel in &graph.relationships {
        let pj = props_json(&rel.properties)?;
        conn.execute(
            &mut rel_ins,
            vec![
                ("sid", Value::String(rel.start_node.clone())),
                ("eid", Value::String(rel.end_node.clone())),
                ("rid", Value::String(rel.id.clone())),
                ("rtype", Value::String(rel.rel_type.clone())),
                ("pjson", Value::String(pj)),
            ],
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    }

    Ok(())
}

/// Writes `graph` to a Ladybug (`lbug`) database at `path`, replacing previous `HsmNode` /
/// `HsmRel` content.
pub fn sync_property_graph(path: &Path, graph: &PropertyGraphSnapshot) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| format!("create_dir_all {:?}", parent))?;
        }
    }

    let db = Database::new(path, SystemConfig::default()).map_err(|e| anyhow::anyhow!("{}", e))?;
    let conn = Connection::new(&db).map_err(|e| anyhow::anyhow!("{}", e))?;
    sync_property_graph_on_connection(&conn, graph)
}
