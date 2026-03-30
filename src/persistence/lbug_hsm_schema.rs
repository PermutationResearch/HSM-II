//! Cypher DDL for HSM’s **LadybugDB** (`lbug`) graph — typed nodes for agents/beliefs/skills/etc.,
//! plus the generic `HsmNode`/`HsmRel` projection of [`crate::property_graph::PropertyGraphSnapshot`].
//!
//! ## Vector + full-text (next step)
//!
//! Ladybug exposes vector and FTS via extension functions such as `CREATE_VECTOR_INDEX` and
//! `CREATE_FTS_INDEX` (see the [LadybugDB repo](https://github.com/LadybugDB/ladybug)). Once your
//! deployment’s Cypher `CALL` syntax is pinned, add matching statements in
//! [`try_apply_analytical_indices`] after nodes are populated. Until then, embeddings are stored as
//! JSON on `HsmSkill` / vertex rows for portability.

use lbug::Connection;

/// Ordered DDL applied idempotently at store open.
pub static SCHEMA_DDL: &[&str] = &[
    "CREATE NODE TABLE IF NOT EXISTS HsmCheckpoint(id STRING, saved_at INT64, tick INT64, format_version STRING, payload BLOB, PRIMARY KEY(id));",
    "CREATE NODE TABLE IF NOT EXISTS HsmBelief(bid INT64, content STRING, confidence DOUBLE, abstract_l0 STRING, overview_l1 STRING, source_json STRING, created_at INT64, updated_at INT64, update_count INT64, PRIMARY KEY(bid));",
    "CREATE NODE TABLE IF NOT EXISTS HsmSkill(sid STRING, title STRING, principle STRING, confidence DOUBLE, status_json STRING, embedding_f32_json STRING, created_at INT64, last_evolved INT64, PRIMARY KEY(sid));",
    "CREATE NODE TABLE IF NOT EXISTS HsmExperience(eid INT64, description STRING, context STRING, outcome_json STRING, timestamp INT64, tick INT64, PRIMARY KEY(eid));",
    "CREATE NODE TABLE IF NOT EXISTS HsmFederationMeta(fkey STRING, payload_json STRING, PRIMARY KEY(fkey));",
    "CREATE NODE TABLE IF NOT EXISTS HsmNode(id STRING, labels_json STRING, props_json STRING, PRIMARY KEY(id));",
    "CREATE REL TABLE IF NOT EXISTS HsmRel(FROM HsmNode TO HsmNode, rid STRING, rel_type STRING, props_json STRING);",
];

/// Documented examples for aligning CASS / embedding retrieval with Ladybug (run manually or from
/// [`try_apply_analytical_indices`] when syntax matches your `lbug` version).
pub const FTS_VECTOR_HINTS: &str = r#"-- Example (verify against your LadybugDB version / docs):
-- CALL CREATE_FTS_INDEX('HsmSkill', 'title', ...);
-- CALL CREATE_VECTOR_INDEX('HsmSkill', 'embedding_f32_json', ...);
"#;

pub fn apply_schema(conn: &Connection<'_>) -> Result<(), lbug::Error> {
    for stmt in SCHEMA_DDL {
        conn.query(stmt)?;
    }
    Ok(())
}

/// Clear all HSM-owned node labels (relationships attached to these nodes are removed).
pub static CLEAR_HSM_CYPHER: &[&str] = &[
    "MATCH (n:HsmCheckpoint) DETACH DELETE n;",
    "MATCH (n:HsmBelief) DETACH DELETE n;",
    "MATCH (n:HsmSkill) DETACH DELETE n;",
    "MATCH (n:HsmExperience) DETACH DELETE n;",
    "MATCH (n:HsmFederationMeta) DETACH DELETE n;",
    "MATCH (n:HsmNode) DETACH DELETE n;",
];

pub fn clear_hsm_graph(conn: &Connection<'_>) -> Result<(), lbug::Error> {
    for q in CLEAR_HSM_CYPHER {
        conn.query(q)?;
    }
    Ok(())
}

/// Hook for `CREATE_FTS_INDEX` / `CREATE_VECTOR_INDEX` once Cypher matches your deployed `lbug`
/// version. See [`FTS_VECTOR_HINTS`].
pub fn try_apply_analytical_indices(_conn: &Connection<'_>) {
    // Intentionally empty — uncomment validated CALL statements from Ladybug docs when ready.
}
