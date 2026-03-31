//! SQLite-backed persistence for 8 HSM subsystems.
//!
//! Uses rusqlite with bundled SQLite and serde_json for complex field serialization.
//! All public methods return `anyhow::Result` for ergonomic error propagation.
//!
//! **Naming:** [`HsmSqliteStore`] is SQLite (subsystem audit / CRUD). The real **LadybugDB**
//! embedded engine is the optional `lbug` crate — see [`ladybug_native`] and [`lbug_world_store`].

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use anyhow::{Context, Result};

#[cfg(feature = "lbug")]
pub mod ladybug_native;
#[cfg(feature = "lbug")]
pub mod lbug_hsm_schema;
#[cfg(feature = "lbug")]
pub mod lbug_world_store;

use crate::hyper_stigmergy::{Belief, BeliefSource, Experience, ExperienceOutcome};
use crate::skill::Skill;
use crate::scenario_simulator::PredictionReport;
use crate::real::api::WorldSnapshot;
use crate::federation::trust::TrustEdge;

// ═══════════════════════════════════════════════════════════════════════════════
// Row types for tables that don't map 1:1 to existing structs
// ═══════════════════════════════════════════════════════════════════════════════

/// A persisted council decision row.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CouncilDecisionRow {
    pub id: String,
    pub proposal_id: String,
    pub mode: String,
    pub decision: serde_json::Value,
    pub participants: serde_json::Value,
    pub timestamp: u64,
}

/// A persisted trust edge row with system identifiers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrustEdgeRow {
    pub from_system: String,
    pub to_system: String,
    pub score: f64,
    pub successful_imports: u64,
    pub failed_imports: u64,
    pub last_interaction: u64,
}

/// A persisted context snapshot row.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextSnapshotRow {
    pub id: i64,
    pub query: String,
    pub ranked_skills: serde_json::Value,
    pub context_summary: String,
    pub timestamp: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// HsmSqliteStore — SQLite persistence (not LadybugDB / lbug)
// ═══════════════════════════════════════════════════════════════════════════════

/// SQLite-backed persistence layer for HSM subsystems (beliefs, skills, council rows, etc.).
/// This is **not** [LadybugDB](https://github.com/LadybugDB/ladybug); use `lbug` + [`lbug_world_store`] for the graph engine.
pub struct HsmSqliteStore {
    conn: Connection,
}

/// Deprecated alias — the old name collided with LadybugDB branding.
#[deprecated(note = "Renamed to HsmSqliteStore — this type is SQLite, not the lbug graph database.")]
pub type LadybugDb = HsmSqliteStore;

impl HsmSqliteStore {
    /// Open (or create) a SQLite database at `path` and run all migrations.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open SQLite database at {}", path))?;

        // Enable WAL mode for better concurrent read performance.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .context("Failed to set PRAGMA options")?;

        let db = Self { conn };
        db.migrate().context("Failed to run database migrations")?;
        Ok(db)
    }

    /// Run all CREATE TABLE IF NOT EXISTS statements in a single transaction.
    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "BEGIN;

            CREATE TABLE IF NOT EXISTS beliefs (
                id              INTEGER PRIMARY KEY,
                content         TEXT    NOT NULL,
                confidence      REAL    NOT NULL,
                source          TEXT    NOT NULL,
                supporting_evidence   TEXT NOT NULL,
                contradicting_evidence TEXT NOT NULL,
                created_at      INTEGER NOT NULL,
                updated_at      INTEGER NOT NULL,
                update_count    INTEGER NOT NULL,
                abstract_l0     TEXT,
                overview_l1     TEXT
            );

            CREATE TABLE IF NOT EXISTS skills (
                id              TEXT    PRIMARY KEY,
                title           TEXT    NOT NULL,
                principle       TEXT    NOT NULL,
                level           TEXT    NOT NULL,
                source          TEXT    NOT NULL,
                confidence      REAL    NOT NULL,
                usage_count     INTEGER NOT NULL,
                success_count   INTEGER NOT NULL,
                failure_count   INTEGER NOT NULL,
                created_at      INTEGER NOT NULL,
                last_evolved    INTEGER NOT NULL,
                status          TEXT    NOT NULL,
                credit_ema      REAL    NOT NULL DEFAULT 0.0,
                embedding       BLOB
            );

            CREATE TABLE IF NOT EXISTS experiences (
                id              INTEGER PRIMARY KEY,
                description     TEXT    NOT NULL,
                context         TEXT    NOT NULL,
                outcome         TEXT    NOT NULL,
                timestamp       INTEGER NOT NULL,
                tick            INTEGER NOT NULL,
                abstract_l0     TEXT,
                overview_l1     TEXT
            );

            CREATE TABLE IF NOT EXISTS predictions (
                topic           TEXT    NOT NULL,
                seed_summary    TEXT    NOT NULL,
                branches        TEXT    NOT NULL,
                synthesis       TEXT    NOT NULL,
                timestamp       INTEGER PRIMARY KEY
            );

            CREATE TABLE IF NOT EXISTS council_decisions (
                id              TEXT    PRIMARY KEY,
                proposal_id     TEXT    NOT NULL,
                mode            TEXT    NOT NULL,
                decision        TEXT    NOT NULL,
                participants    TEXT    NOT NULL,
                timestamp       INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS trust_edges (
                from_system         TEXT    NOT NULL,
                to_system           TEXT    NOT NULL,
                score               REAL    NOT NULL,
                successful_imports  INTEGER NOT NULL,
                failed_imports      INTEGER NOT NULL,
                last_interaction    INTEGER NOT NULL,
                PRIMARY KEY (from_system, to_system)
            );

            CREATE TABLE IF NOT EXISTS context_snapshots (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                query           TEXT    NOT NULL,
                ranked_skills   TEXT    NOT NULL,
                context_summary TEXT    NOT NULL,
                timestamp       INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS world_snapshots (
                tick                INTEGER PRIMARY KEY,
                coherence           REAL    NOT NULL,
                agents              TEXT    NOT NULL,
                edge_count          INTEGER NOT NULL,
                emergent_edge_count INTEGER NOT NULL
            );

            COMMIT;"
        ).context("Migration transaction failed")?;

        Ok(())
    }

    // ─── Beliefs CRUD ────────────────────────────────────────────────────────

    pub fn insert_belief(&self, belief: &Belief) -> Result<()> {
        let source_json = serde_json::to_string(&belief.source)
            .context("Failed to serialize belief source")?;
        let supporting_json = serde_json::to_string(&belief.supporting_evidence)
            .context("Failed to serialize supporting evidence")?;
        let contradicting_json = serde_json::to_string(&belief.contradicting_evidence)
            .context("Failed to serialize contradicting evidence")?;

        self.conn.execute(
            "INSERT INTO beliefs (id, content, confidence, source,
                supporting_evidence, contradicting_evidence,
                created_at, updated_at, update_count, abstract_l0, overview_l1)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                belief.id as i64,
                belief.content,
                belief.confidence,
                source_json,
                supporting_json,
                contradicting_json,
                belief.created_at as i64,
                belief.updated_at as i64,
                belief.update_count as i64,
                belief.abstract_l0,
                belief.overview_l1,
            ],
        ).context("Failed to insert belief")?;

        Ok(())
    }

    pub fn get_belief(&self, id: usize) -> Result<Option<Belief>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, confidence, source,
                    supporting_evidence, contradicting_evidence,
                    created_at, updated_at, update_count, abstract_l0, overview_l1
             FROM beliefs WHERE id = ?1"
        ).context("Failed to prepare get_belief query")?;

        let mut rows = stmt.query_map(params![id as i64], |row| {
            Ok(BeliefRow {
                id: row.get::<_, i64>(0)?,
                content: row.get(1)?,
                confidence: row.get(2)?,
                source: row.get(3)?,
                supporting_evidence: row.get(4)?,
                contradicting_evidence: row.get(5)?,
                created_at: row.get::<_, i64>(6)?,
                updated_at: row.get::<_, i64>(7)?,
                update_count: row.get::<_, i64>(8)?,
                abstract_l0: row.get(9)?,
                overview_l1: row.get(10)?,
            })
        }).context("Failed to execute get_belief query")?;

        match rows.next() {
            Some(row) => {
                let r = row.context("Failed to read belief row")?;
                Ok(Some(belief_from_row(r)?))
            }
            None => Ok(None),
        }
    }

    pub fn list_beliefs(&self) -> Result<Vec<Belief>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, content, confidence, source,
                    supporting_evidence, contradicting_evidence,
                    created_at, updated_at, update_count, abstract_l0, overview_l1
             FROM beliefs ORDER BY id"
        ).context("Failed to prepare list_beliefs query")?;

        let rows = stmt.query_map([], |row| {
            Ok(BeliefRow {
                id: row.get::<_, i64>(0)?,
                content: row.get(1)?,
                confidence: row.get(2)?,
                source: row.get(3)?,
                supporting_evidence: row.get(4)?,
                contradicting_evidence: row.get(5)?,
                created_at: row.get::<_, i64>(6)?,
                updated_at: row.get::<_, i64>(7)?,
                update_count: row.get::<_, i64>(8)?,
                abstract_l0: row.get(9)?,
                overview_l1: row.get(10)?,
            })
        }).context("Failed to execute list_beliefs query")?;

        let mut beliefs = Vec::new();
        for row in rows {
            let r = row.context("Failed to read belief row")?;
            beliefs.push(belief_from_row(r)?);
        }
        Ok(beliefs)
    }

    pub fn update_belief(&self, belief: &Belief) -> Result<()> {
        let source_json = serde_json::to_string(&belief.source)
            .context("Failed to serialize belief source")?;
        let supporting_json = serde_json::to_string(&belief.supporting_evidence)
            .context("Failed to serialize supporting evidence")?;
        let contradicting_json = serde_json::to_string(&belief.contradicting_evidence)
            .context("Failed to serialize contradicting evidence")?;

        self.conn.execute(
            "UPDATE beliefs SET content = ?1, confidence = ?2, source = ?3,
                supporting_evidence = ?4, contradicting_evidence = ?5,
                updated_at = ?6, update_count = ?7, abstract_l0 = ?8, overview_l1 = ?9
             WHERE id = ?10",
            params![
                belief.content,
                belief.confidence,
                source_json,
                supporting_json,
                contradicting_json,
                belief.updated_at as i64,
                belief.update_count as i64,
                belief.abstract_l0,
                belief.overview_l1,
                belief.id as i64,
            ],
        ).context("Failed to update belief")?;

        Ok(())
    }

    pub fn delete_belief(&self, id: usize) -> Result<()> {
        self.conn.execute("DELETE FROM beliefs WHERE id = ?1", params![id as i64])
            .context("Failed to delete belief")?;
        Ok(())
    }

    // ─── Skills CRUD ─────────────────────────────────────────────────────────

    pub fn insert_skill(&self, skill: &Skill) -> Result<()> {
        let level_json = serde_json::to_string(&skill.level)
            .context("Failed to serialize skill level")?;
        let source_json = serde_json::to_string(&skill.source)
            .context("Failed to serialize skill source")?;
        let status_json = serde_json::to_string(&skill.status)
            .context("Failed to serialize skill status")?;
        let embedding_blob = skill.embedding.as_ref().map(|v| {
            v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>()
        });

        self.conn.execute(
            "INSERT INTO skills (id, title, principle, level, source, confidence,
                usage_count, success_count, failure_count, created_at, last_evolved,
                status, credit_ema, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                skill.id,
                skill.title,
                skill.principle,
                level_json,
                source_json,
                skill.confidence,
                skill.usage_count as i64,
                skill.success_count as i64,
                skill.failure_count as i64,
                skill.created_at as i64,
                skill.last_evolved as i64,
                status_json,
                skill.credit_ema,
                embedding_blob,
            ],
        ).context("Failed to insert skill")?;

        Ok(())
    }

    pub fn get_skill(&self, id: &str) -> Result<Option<Skill>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, principle, level, source, confidence,
                    usage_count, success_count, failure_count, created_at, last_evolved,
                    status, credit_ema, embedding
             FROM skills WHERE id = ?1"
        ).context("Failed to prepare get_skill query")?;

        let mut rows = stmt.query_map(params![id], |row| {
            Ok(SkillRow {
                id: row.get(0)?,
                title: row.get(1)?,
                principle: row.get(2)?,
                level: row.get(3)?,
                source: row.get(4)?,
                confidence: row.get(5)?,
                usage_count: row.get::<_, i64>(6)?,
                success_count: row.get::<_, i64>(7)?,
                failure_count: row.get::<_, i64>(8)?,
                created_at: row.get::<_, i64>(9)?,
                last_evolved: row.get::<_, i64>(10)?,
                status: row.get(11)?,
                credit_ema: row.get(12)?,
                embedding: row.get::<_, Option<Vec<u8>>>(13)?,
            })
        }).context("Failed to execute get_skill query")?;

        match rows.next() {
            Some(row) => {
                let r = row.context("Failed to read skill row")?;
                Ok(Some(skill_from_row(r)?))
            }
            None => Ok(None),
        }
    }

    pub fn list_skills(&self) -> Result<Vec<Skill>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, principle, level, source, confidence,
                    usage_count, success_count, failure_count, created_at, last_evolved,
                    status, credit_ema, embedding
             FROM skills ORDER BY id"
        ).context("Failed to prepare list_skills query")?;

        let rows = stmt.query_map([], |row| {
            Ok(SkillRow {
                id: row.get(0)?,
                title: row.get(1)?,
                principle: row.get(2)?,
                level: row.get(3)?,
                source: row.get(4)?,
                confidence: row.get(5)?,
                usage_count: row.get::<_, i64>(6)?,
                success_count: row.get::<_, i64>(7)?,
                failure_count: row.get::<_, i64>(8)?,
                created_at: row.get::<_, i64>(9)?,
                last_evolved: row.get::<_, i64>(10)?,
                status: row.get(11)?,
                credit_ema: row.get(12)?,
                embedding: row.get::<_, Option<Vec<u8>>>(13)?,
            })
        }).context("Failed to execute list_skills query")?;

        let mut skills = Vec::new();
        for row in rows {
            let r = row.context("Failed to read skill row")?;
            skills.push(skill_from_row(r)?);
        }
        Ok(skills)
    }

    pub fn update_skill(&self, skill: &Skill) -> Result<()> {
        let level_json = serde_json::to_string(&skill.level)
            .context("Failed to serialize skill level")?;
        let source_json = serde_json::to_string(&skill.source)
            .context("Failed to serialize skill source")?;
        let status_json = serde_json::to_string(&skill.status)
            .context("Failed to serialize skill status")?;
        let embedding_blob = skill.embedding.as_ref().map(|v| {
            v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>()
        });

        self.conn.execute(
            "UPDATE skills SET title = ?1, principle = ?2, level = ?3, source = ?4,
                confidence = ?5, usage_count = ?6, success_count = ?7, failure_count = ?8,
                last_evolved = ?9, status = ?10, credit_ema = ?11, embedding = ?12
             WHERE id = ?13",
            params![
                skill.title,
                skill.principle,
                level_json,
                source_json,
                skill.confidence,
                skill.usage_count as i64,
                skill.success_count as i64,
                skill.failure_count as i64,
                skill.last_evolved as i64,
                status_json,
                skill.credit_ema,
                embedding_blob,
                skill.id,
            ],
        ).context("Failed to update skill")?;

        Ok(())
    }

    pub fn delete_skill(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM skills WHERE id = ?1", params![id])
            .context("Failed to delete skill")?;
        Ok(())
    }

    // ─── Experiences CRUD ────────────────────────────────────────────────────

    pub fn insert_experience(&self, exp: &Experience) -> Result<()> {
        let outcome_json = serde_json::to_string(&exp.outcome)
            .context("Failed to serialize experience outcome")?;

        self.conn.execute(
            "INSERT INTO experiences (id, description, context, outcome, timestamp, tick,
                abstract_l0, overview_l1)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                exp.id as i64,
                exp.description,
                exp.context,
                outcome_json,
                exp.timestamp as i64,
                exp.tick as i64,
                exp.abstract_l0,
                exp.overview_l1,
            ],
        ).context("Failed to insert experience")?;

        Ok(())
    }

    pub fn get_experience(&self, id: usize) -> Result<Option<Experience>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, description, context, outcome, timestamp, tick,
                    abstract_l0, overview_l1
             FROM experiences WHERE id = ?1"
        ).context("Failed to prepare get_experience query")?;

        let mut rows = stmt.query_map(params![id as i64], |row| {
            Ok(ExperienceRow {
                id: row.get::<_, i64>(0)?,
                description: row.get(1)?,
                context: row.get(2)?,
                outcome: row.get(3)?,
                timestamp: row.get::<_, i64>(4)?,
                tick: row.get::<_, i64>(5)?,
                abstract_l0: row.get(6)?,
                overview_l1: row.get(7)?,
            })
        }).context("Failed to execute get_experience query")?;

        match rows.next() {
            Some(row) => {
                let r = row.context("Failed to read experience row")?;
                Ok(Some(experience_from_row(r)?))
            }
            None => Ok(None),
        }
    }

    pub fn list_experiences(&self) -> Result<Vec<Experience>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, description, context, outcome, timestamp, tick,
                    abstract_l0, overview_l1
             FROM experiences ORDER BY id"
        ).context("Failed to prepare list_experiences query")?;

        let rows = stmt.query_map([], |row| {
            Ok(ExperienceRow {
                id: row.get::<_, i64>(0)?,
                description: row.get(1)?,
                context: row.get(2)?,
                outcome: row.get(3)?,
                timestamp: row.get::<_, i64>(4)?,
                tick: row.get::<_, i64>(5)?,
                abstract_l0: row.get(6)?,
                overview_l1: row.get(7)?,
            })
        }).context("Failed to execute list_experiences query")?;

        let mut exps = Vec::new();
        for row in rows {
            let r = row.context("Failed to read experience row")?;
            exps.push(experience_from_row(r)?);
        }
        Ok(exps)
    }

    pub fn update_experience(&self, exp: &Experience) -> Result<()> {
        let outcome_json = serde_json::to_string(&exp.outcome)
            .context("Failed to serialize experience outcome")?;

        self.conn.execute(
            "UPDATE experiences SET description = ?1, context = ?2, outcome = ?3,
                timestamp = ?4, tick = ?5, abstract_l0 = ?6, overview_l1 = ?7
             WHERE id = ?8",
            params![
                exp.description,
                exp.context,
                outcome_json,
                exp.timestamp as i64,
                exp.tick as i64,
                exp.abstract_l0,
                exp.overview_l1,
                exp.id as i64,
            ],
        ).context("Failed to update experience")?;

        Ok(())
    }

    pub fn delete_experience(&self, id: usize) -> Result<()> {
        self.conn.execute("DELETE FROM experiences WHERE id = ?1", params![id as i64])
            .context("Failed to delete experience")?;
        Ok(())
    }

    // ─── Predictions ─────────────────────────────────────────────────────────

    pub fn insert_prediction(&self, report: &PredictionReport) -> Result<()> {
        let branches_json = serde_json::to_string(&report.branches)
            .context("Failed to serialize prediction branches")?;
        let seed_summary = report.seeds.join("\n");

        self.conn.execute(
            "INSERT INTO predictions (topic, seed_summary, branches, synthesis, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                report.topic,
                seed_summary,
                branches_json,
                report.synthesis,
                report.generated_at as i64,
            ],
        ).context("Failed to insert prediction")?;

        Ok(())
    }

    pub fn list_predictions(&self) -> Result<Vec<PredictionReport>> {
        let mut stmt = self.conn.prepare(
            "SELECT topic, seed_summary, branches, synthesis, timestamp
             FROM predictions ORDER BY timestamp"
        ).context("Failed to prepare list_predictions query")?;

        let rows = stmt.query_map([], |row| {
            Ok(PredictionRow {
                topic: row.get(0)?,
                seed_summary: row.get(1)?,
                branches: row.get(2)?,
                synthesis: row.get(3)?,
                timestamp: row.get::<_, i64>(4)?,
            })
        }).context("Failed to execute list_predictions query")?;

        let mut reports = Vec::new();
        for row in rows {
            let r = row.context("Failed to read prediction row")?;
            reports.push(prediction_from_row(r)?);
        }
        Ok(reports)
    }

    // ─── Council Decisions ───────────────────────────────────────────────────

    pub fn insert_council_decision(
        &self,
        id: &str,
        proposal_id: &str,
        mode: &str,
        decision_json: &str,
        participants_json: &str,
        timestamp: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO council_decisions (id, proposal_id, mode, decision, participants, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, proposal_id, mode, decision_json, participants_json, timestamp as i64],
        ).context("Failed to insert council decision")?;

        Ok(())
    }

    pub fn list_council_decisions(&self) -> Result<Vec<CouncilDecisionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, proposal_id, mode, decision, participants, timestamp
             FROM council_decisions ORDER BY timestamp"
        ).context("Failed to prepare list_council_decisions query")?;

        let rows = stmt.query_map([], |row| {
            let decision_str: String = row.get(3)?;
            let participants_str: String = row.get(4)?;
            Ok(CouncilDecisionRow {
                id: row.get(0)?,
                proposal_id: row.get(1)?,
                mode: row.get(2)?,
                decision: serde_json::from_str(&decision_str).unwrap_or(serde_json::Value::Null),
                participants: serde_json::from_str(&participants_str).unwrap_or(serde_json::Value::Null),
                timestamp: row.get::<_, i64>(5)? as u64,
            })
        }).context("Failed to execute list_council_decisions query")?;

        let mut decisions = Vec::new();
        for row in rows {
            decisions.push(row.context("Failed to read council decision row")?);
        }
        Ok(decisions)
    }

    // ─── Trust Edges ─────────────────────────────────────────────────────────

    pub fn upsert_trust_edge(&self, from: &str, to: &str, edge: &TrustEdge) -> Result<()> {
        self.conn.execute(
            "INSERT INTO trust_edges (from_system, to_system, score, successful_imports,
                failed_imports, last_interaction)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(from_system, to_system) DO UPDATE SET
                score = excluded.score,
                successful_imports = excluded.successful_imports,
                failed_imports = excluded.failed_imports,
                last_interaction = excluded.last_interaction",
            params![
                from,
                to,
                edge.score,
                edge.successful_imports as i64,
                edge.failed_imports as i64,
                edge.last_interaction as i64,
            ],
        ).context("Failed to upsert trust edge")?;

        Ok(())
    }

    pub fn get_trust_edges(&self) -> Result<Vec<TrustEdgeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT from_system, to_system, score, successful_imports,
                    failed_imports, last_interaction
             FROM trust_edges ORDER BY from_system, to_system"
        ).context("Failed to prepare get_trust_edges query")?;

        let rows = stmt.query_map([], |row| {
            Ok(TrustEdgeRow {
                from_system: row.get(0)?,
                to_system: row.get(1)?,
                score: row.get(2)?,
                successful_imports: row.get::<_, i64>(3)? as u64,
                failed_imports: row.get::<_, i64>(4)? as u64,
                last_interaction: row.get::<_, i64>(5)? as u64,
            })
        }).context("Failed to execute get_trust_edges query")?;

        let mut edges = Vec::new();
        for row in rows {
            edges.push(row.context("Failed to read trust edge row")?);
        }
        Ok(edges)
    }

    // ─── Context Snapshots ───────────────────────────────────────────────────

    pub fn insert_context_snapshot(
        &self,
        query: &str,
        ranked_skills_json: &str,
        context_summary: &str,
        timestamp: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO context_snapshots (query, ranked_skills, context_summary, timestamp)
             VALUES (?1, ?2, ?3, ?4)",
            params![query, ranked_skills_json, context_summary, timestamp as i64],
        ).context("Failed to insert context snapshot")?;

        Ok(())
    }

    // ─── World Snapshots ─────────────────────────────────────────────────────

    pub fn insert_world_snapshot(&self, snapshot: &WorldSnapshot) -> Result<()> {
        let agents_json = serde_json::to_string(&snapshot.agents)
            .context("Failed to serialize world snapshot agents")?;

        self.conn.execute(
            "INSERT INTO world_snapshots (tick, coherence, agents, edge_count, emergent_edge_count)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                snapshot.tick as i64,
                snapshot.coherence,
                agents_json,
                snapshot.edge_count as i64,
                snapshot.emergent_edge_count as i64,
            ],
        ).context("Failed to insert world snapshot")?;

        Ok(())
    }

    pub fn get_world_snapshot(&self, tick: u64) -> Result<Option<WorldSnapshot>> {
        let mut stmt = self.conn.prepare(
            "SELECT tick, coherence, agents, edge_count, emergent_edge_count
             FROM world_snapshots WHERE tick = ?1"
        ).context("Failed to prepare get_world_snapshot query")?;

        let mut rows = stmt.query_map(params![tick as i64], |row| {
            Ok(WorldSnapshotRow {
                tick: row.get::<_, i64>(0)?,
                coherence: row.get(1)?,
                agents: row.get(2)?,
                edge_count: row.get::<_, i64>(3)?,
                emergent_edge_count: row.get::<_, i64>(4)?,
            })
        }).context("Failed to execute get_world_snapshot query")?;

        match rows.next() {
            Some(row) => {
                let r = row.context("Failed to read world snapshot row")?;
                Ok(Some(world_snapshot_from_row(r)?))
            }
            None => Ok(None),
        }
    }

    pub fn latest_world_snapshot(&self) -> Result<Option<WorldSnapshot>> {
        let mut stmt = self.conn.prepare(
            "SELECT tick, coherence, agents, edge_count, emergent_edge_count
             FROM world_snapshots ORDER BY tick DESC LIMIT 1"
        ).context("Failed to prepare latest_world_snapshot query")?;

        let mut rows = stmt.query_map([], |row| {
            Ok(WorldSnapshotRow {
                tick: row.get::<_, i64>(0)?,
                coherence: row.get(1)?,
                agents: row.get(2)?,
                edge_count: row.get::<_, i64>(3)?,
                emergent_edge_count: row.get::<_, i64>(4)?,
            })
        }).context("Failed to execute latest_world_snapshot query")?;

        match rows.next() {
            Some(row) => {
                let r = row.context("Failed to read world snapshot row")?;
                Ok(Some(world_snapshot_from_row(r)?))
            }
            None => Ok(None),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Internal row types and conversion helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Internal row type for beliefs read from SQLite.
struct BeliefRow {
    id: i64,
    content: String,
    confidence: f64,
    source: String,
    supporting_evidence: String,
    contradicting_evidence: String,
    created_at: i64,
    updated_at: i64,
    update_count: i64,
    abstract_l0: Option<String>,
    overview_l1: Option<String>,
}

fn belief_from_row(r: BeliefRow) -> Result<Belief> {
    let source: BeliefSource = serde_json::from_str(&r.source)
        .context("Failed to deserialize belief source")?;
    let supporting: Vec<String> = serde_json::from_str(&r.supporting_evidence)
        .context("Failed to deserialize supporting evidence")?;
    let contradicting: Vec<String> = serde_json::from_str(&r.contradicting_evidence)
        .context("Failed to deserialize contradicting evidence")?;

    Ok(Belief {
        id: r.id as usize,
        content: r.content,
        confidence: r.confidence,
        source,
        supporting_evidence: supporting,
        contradicting_evidence: contradicting,
        created_at: r.created_at as u64,
        updated_at: r.updated_at as u64,
        update_count: r.update_count as u32,
        abstract_l0: r.abstract_l0,
        overview_l1: r.overview_l1,
        owner_namespace: None,
        supersedes_belief_id: None,
        evidence_belief_ids: Vec::new(),
        human_committed: false,
    })
}

/// Internal row type for skills read from SQLite.
struct SkillRow {
    id: String,
    title: String,
    principle: String,
    level: String,
    source: String,
    confidence: f64,
    usage_count: i64,
    success_count: i64,
    failure_count: i64,
    created_at: i64,
    last_evolved: i64,
    status: String,
    credit_ema: f64,
    embedding: Option<Vec<u8>>,
}

fn skill_from_row(r: SkillRow) -> Result<Skill> {
    use crate::consensus::{BayesianConfidence, SkillStatus};
    use crate::skill::{SkillLevel, SkillSource};

    let level: SkillLevel = serde_json::from_str(&r.level)
        .context("Failed to deserialize skill level")?;
    let source: SkillSource = serde_json::from_str(&r.source)
        .context("Failed to deserialize skill source")?;
    let status: SkillStatus = serde_json::from_str(&r.status)
        .context("Failed to deserialize skill status")?;

    let embedding = r.embedding.map(|bytes| {
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect::<Vec<f32>>()
    });

    Ok(Skill {
        id: r.id,
        title: r.title,
        principle: r.principle,
        when_to_apply: Vec::new(), // Not persisted in this table; loaded separately if needed
        level,
        source,
        confidence: r.confidence,
        usage_count: r.usage_count as u64,
        success_count: r.success_count as u64,
        failure_count: r.failure_count as u64,
        embedding,
        created_at: r.created_at as u64,
        last_evolved: r.last_evolved as u64,
        status,
        bayesian: BayesianConfidence::default(), // Not persisted in this table
        credit_ema: r.credit_ema,
        credit_count: 0,
        last_credit_tick: 0,
        curation: Default::default(),
        scope: Default::default(),
        delegation_ema: 0.0,
        delegation_count: 0,
        hired_count: 0,
    })
}

/// Internal row type for experiences read from SQLite.
struct ExperienceRow {
    id: i64,
    description: String,
    context: String,
    outcome: String,
    timestamp: i64,
    tick: i64,
    abstract_l0: Option<String>,
    overview_l1: Option<String>,
}

fn experience_from_row(r: ExperienceRow) -> Result<Experience> {
    let outcome: ExperienceOutcome = serde_json::from_str(&r.outcome)
        .context("Failed to deserialize experience outcome")?;

    Ok(Experience {
        id: r.id as usize,
        description: r.description,
        context: r.context,
        outcome,
        timestamp: r.timestamp as u64,
        tick: r.tick as u64,
        embedding: None, // Not persisted in this table
        abstract_l0: r.abstract_l0,
        overview_l1: r.overview_l1,
    })
}

/// Internal row type for predictions read from SQLite.
struct PredictionRow {
    topic: String,
    seed_summary: String,
    branches: String,
    synthesis: String,
    timestamp: i64,
}

fn prediction_from_row(r: PredictionRow) -> Result<PredictionReport> {
    use crate::scenario_simulator::ScenarioBranch;

    let branches: Vec<ScenarioBranch> = serde_json::from_str(&r.branches)
        .context("Failed to deserialize prediction branches")?;

    Ok(PredictionReport {
        topic: r.topic,
        seeds: if r.seed_summary.is_empty() {
            Vec::new()
        } else {
            vec![r.seed_summary]
        },
        variables: Vec::new(),
        branches,
        synthesis: r.synthesis,
        overall_confidence: 0.0,
        generated_at: r.timestamp as u64,
    })
}

/// Internal row type for world snapshots read from SQLite.
struct WorldSnapshotRow {
    tick: i64,
    coherence: f64,
    agents: String,
    edge_count: i64,
    emergent_edge_count: i64,
}

fn world_snapshot_from_row(r: WorldSnapshotRow) -> Result<WorldSnapshot> {
    use crate::real::api::AgentSnapshot;

    let agents: Vec<AgentSnapshot> = serde_json::from_str(&r.agents)
        .context("Failed to deserialize world snapshot agents")?;

    Ok(WorldSnapshot {
        tick: r.tick as u64,
        coherence: r.coherence,
        agents,
        edge_count: r.edge_count as usize,
        emergent_edge_count: r.emergent_edge_count as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_and_migrate() {
        let db = HsmSqliteStore::open(":memory:").expect("should open in-memory db");

        // Verify tables exist by inserting into each one.
        db.conn
            .query_row("SELECT count(*) FROM beliefs", [], |_r| Ok(()))
            .expect("beliefs table should exist");
        db.conn
            .query_row("SELECT count(*) FROM skills", [], |_r| Ok(()))
            .expect("skills table should exist");
        db.conn
            .query_row("SELECT count(*) FROM experiences", [], |_r| Ok(()))
            .expect("experiences table should exist");
        db.conn
            .query_row("SELECT count(*) FROM predictions", [], |_r| Ok(()))
            .expect("predictions table should exist");
        db.conn
            .query_row("SELECT count(*) FROM council_decisions", [], |_r| Ok(()))
            .expect("council_decisions table should exist");
        db.conn
            .query_row("SELECT count(*) FROM trust_edges", [], |_r| Ok(()))
            .expect("trust_edges table should exist");
        db.conn
            .query_row("SELECT count(*) FROM context_snapshots", [], |_r| Ok(()))
            .expect("context_snapshots table should exist");
        db.conn
            .query_row("SELECT count(*) FROM world_snapshots", [], |_r| Ok(()))
            .expect("world_snapshots table should exist");
    }

    #[test]
    fn test_belief_crud() {
        let db = HsmSqliteStore::open(":memory:").unwrap();

        let belief = Belief {
            id: 1,
            content: "Test belief".into(),
            confidence: 0.85,
            source: BeliefSource::Observation,
            supporting_evidence: vec!["evidence1".into()],
            contradicting_evidence: vec![],
            created_at: 1000,
            updated_at: 1000,
            update_count: 0,
            abstract_l0: Some("abstract".into()),
            overview_l1: None,
            owner_namespace: None,
            supersedes_belief_id: None,
            evidence_belief_ids: Vec::new(),
            human_committed: false,
        };

        db.insert_belief(&belief).unwrap();

        let fetched = db.get_belief(1).unwrap().expect("should find belief");
        assert_eq!(fetched.content, "Test belief");
        assert!((fetched.confidence - 0.85).abs() < f64::EPSILON);

        let all = db.list_beliefs().unwrap();
        assert_eq!(all.len(), 1);

        db.delete_belief(1).unwrap();
        assert!(db.get_belief(1).unwrap().is_none());
    }

    #[test]
    fn test_council_decision_roundtrip() {
        let db = HsmSqliteStore::open(":memory:").unwrap();

        db.insert_council_decision(
            "cd-1",
            "prop-1",
            "debate",
            r#"{"verdict":"approved"}"#,
            r#"["agent-a","agent-b"]"#,
            42,
        )
        .unwrap();

        let decisions = db.list_council_decisions().unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].id, "cd-1");
        assert_eq!(decisions[0].timestamp, 42);
    }

    #[test]
    fn test_trust_edge_upsert() {
        let db = HsmSqliteStore::open(":memory:").unwrap();

        let edge = TrustEdge {
            score: 0.9,
            successful_imports: 10,
            failed_imports: 1,
            last_interaction: 500,
        };

        db.upsert_trust_edge("sys-a", "sys-b", &edge).unwrap();

        let updated_edge = TrustEdge {
            score: 0.95,
            successful_imports: 15,
            failed_imports: 1,
            last_interaction: 600,
        };
        db.upsert_trust_edge("sys-a", "sys-b", &updated_edge).unwrap();

        let edges = db.get_trust_edges().unwrap();
        assert_eq!(edges.len(), 1);
        assert!((edges[0].score - 0.95).abs() < f64::EPSILON);
        assert_eq!(edges[0].successful_imports, 15);
    }

    #[test]
    fn test_world_snapshot_roundtrip() {
        let db = HsmSqliteStore::open(":memory:").unwrap();

        let snapshot = WorldSnapshot {
            tick: 100,
            coherence: 0.75,
            agents: vec![],
            edge_count: 42,
            emergent_edge_count: 7,
        };

        db.insert_world_snapshot(&snapshot).unwrap();

        let fetched = db.get_world_snapshot(100).unwrap().expect("should find snapshot");
        assert_eq!(fetched.tick, 100);
        assert!((fetched.coherence - 0.75).abs() < f64::EPSILON);
        assert_eq!(fetched.edge_count, 42);

        let latest = db.latest_world_snapshot().unwrap().expect("should find latest");
        assert_eq!(latest.tick, 100);
    }

    #[test]
    fn test_context_snapshot_insert() {
        let db = HsmSqliteStore::open(":memory:").unwrap();

        db.insert_context_snapshot(
            "how to debug",
            r#"["skill-1","skill-2"]"#,
            "debugging context",
            999,
        )
        .unwrap();

        // Verify row exists.
        let count: i64 = db
            .conn
            .query_row("SELECT count(*) FROM context_snapshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
