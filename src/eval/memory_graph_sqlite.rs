//! SQLite DDL and upserts for [`super::memory_graph::BipartiteMemoryGraph`].
//!
//! Three tables mirror the JSON export: entities, reified facts, incidence (bipartite links).
//! Use `upsert_bipartite_graph` after `init_schema` (idempotent).

use std::path::Path;

use rusqlite::Connection;

use super::memory_graph::{BipartiteMemoryGraph, MemoryEntity, ReifiedFact};

/// Full DDL (enables foreign keys; safe to run multiple times).
pub const MEMORY_GRAPH_DDL: &str = r#"
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS hsm_memory_entity (
  id TEXT PRIMARY KEY NOT NULL,
  layer TEXT NOT NULL,
  kind TEXT NOT NULL,
  label TEXT,
  properties TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS hsm_memory_fact (
  id TEXT PRIMARY KEY NOT NULL,
  layer TEXT NOT NULL,
  relation TEXT NOT NULL,
  properties TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS hsm_memory_incidence (
  entity_id TEXT NOT NULL,
  fact_id TEXT NOT NULL,
  role TEXT NOT NULL,
  PRIMARY KEY (entity_id, fact_id, role)
);
CREATE INDEX IF NOT EXISTS idx_hsm_incidence_fact ON hsm_memory_incidence(fact_id);
CREATE INDEX IF NOT EXISTS idx_hsm_incidence_entity ON hsm_memory_incidence(entity_id);
CREATE INDEX IF NOT EXISTS idx_hsm_fact_relation ON hsm_memory_fact(relation);
"#;

/// Apply schema to an open connection.
pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(MEMORY_GRAPH_DDL)
}

/// Remove all graph rows (use before a full re-ingest if you need exact mirror of JSON).
pub fn delete_all_graph_rows(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DELETE FROM hsm_memory_incidence;
         DELETE FROM hsm_memory_fact;
         DELETE FROM hsm_memory_entity;",
    )
}

fn props_json(v: &serde_json::Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "{}".into())
}

/// Insert or replace all rows from a projected graph (batched in one transaction).
pub fn upsert_bipartite_graph(
    conn: &mut Connection,
    g: &BipartiteMemoryGraph,
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;

    {
        let mut stmt = tx.prepare(
            "INSERT INTO hsm_memory_entity (id, layer, kind, label, properties)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
               layer = excluded.layer,
               kind = excluded.kind,
               label = excluded.label,
               properties = excluded.properties",
        )?;
        for MemoryEntity {
            id,
            layer,
            kind,
            label,
            properties,
        } in &g.entities
        {
            stmt.execute((
                id.as_str(),
                layer.as_sql(),
                kind.as_str(),
                label.as_deref(),
                props_json(properties).as_str(),
            ))?;
        }
    }

    {
        let mut stmt = tx.prepare(
            "INSERT INTO hsm_memory_fact (id, layer, relation, properties)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
               layer = excluded.layer,
               relation = excluded.relation,
               properties = excluded.properties",
        )?;
        for ReifiedFact {
            id,
            layer,
            relation,
            properties,
        } in &g.facts
        {
            stmt.execute((
                id.as_str(),
                layer.as_sql(),
                relation.as_str(),
                props_json(properties).as_str(),
            ))?;
        }
    }

    {
        let mut stmt = tx.prepare(
            "INSERT INTO hsm_memory_incidence (entity_id, fact_id, role)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(entity_id, fact_id, role) DO NOTHING",
        )?;
        for inc in &g.incidence {
            stmt.execute((
                inc.entity_id.as_str(),
                inc.fact_id.as_str(),
                inc.role.as_str(),
            ))?;
        }
    }

    tx.commit()
}

/// Parse JSON (e.g. `memory_graph.json` from `hsm-eval`) and upsert into `db_path`.
pub fn ingest_json_file(db_path: &Path, json_path: &Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(json_path)?;
    let g: BipartiteMemoryGraph = serde_json::from_str(&text)?;
    let mut conn = Connection::open(db_path)?;
    init_schema(&conn)?;
    upsert_bipartite_graph(&mut conn, &g)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::memory_graph::{BeliefSnapshot, BipartiteMemoryGraph, HsmMemorySnapshot};
    use super::super::trace::{BeliefRankEntry, HsmTurnTrace};
    use super::*;

    #[test]
    fn upsert_roundtrip_in_memory() {
        let snap = HsmMemorySnapshot {
            beliefs: vec![BeliefSnapshot {
                index: 0,
                content: "x".into(),
                confidence: 1.0,
                domain: None,
                source_task: "t1".into(),
                source_turn: 0,
                created_at: 0,
                keywords: vec![],
            }],
            session_summaries: vec![],
            skills: vec![],
        };
        let traces = vec![HsmTurnTrace {
            task_id: "t1".into(),
            turn_index: 1,
            session: 1,
            requires_recall: true,
            selected_skill_id: Some("s1".into()),
            selected_skill_domain: Some("software_engineering".into()),
            belief_ranks: vec![BeliefRankEntry {
                belief_index: 0,
                score: 0.9,
                source_task: "t1".into(),
                preview: "preview".into(),
            }],
            session_summaries_injected: vec![1],
            injected_char_len: 10,
            injected_preview: "hi".into(),
            session_compaction_applied: false,
            session_history_len: 0,
        }];
        let g = BipartiteMemoryGraph::project_from_snapshot_with_traces(&snap, &traces);

        let mut conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        upsert_bipartite_graph(&mut conn, &g).unwrap();

        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM hsm_memory_entity", [], |r| r.get(0))
            .unwrap();
        assert!(n >= 3);
        let f: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM hsm_memory_fact WHERE relation = 'retrieval_turn'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(f, 1);
        let r: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM hsm_memory_fact WHERE relation = 'ranked_belief_at_turn'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(r, 1);
    }
}
