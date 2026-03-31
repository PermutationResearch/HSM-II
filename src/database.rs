//! RooDB / MySQL-compatible persistence layer.
//!
//! RooDB (<https://github.com/jgarzik/roodb>) exposes a MySQL wire-protocol
//! interface, so we use `mysql_async` as the client driver.
//!
//! The module stores the full `SystemState` as a versioned binary snapshot
//! in a single `system_snapshots` table, plus relational tables for agents,
//! edges, beliefs, experiences, and improvement events for queryability.

use crate::hyper_stigmergy::{HyperStigmergicMorphogenesis, SystemState};
use crate::rlm::RLMState;
use mysql_async::prelude::*;
use mysql_async::{OptsBuilder, Pool, SslOpts};
use serde::{Deserialize, Serialize};

/// Configuration for connecting to a RooDB instance.
#[derive(Clone, Debug)]
pub struct RooDbConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub database: String,
    /// Enable TLS (RooDB defaults to TLS-required connections).
    pub tls: bool,
}

impl Default for RooDbConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3307,
            user: "root".to_string(),
            password: Some("secret".to_string()),
            database: "hyper_stigmergy".to_string(),
            tls: true, // RooDB requires TLS
        }
    }
}

impl RooDbConfig {
    /// Parse from a connection string: `user:pass@host:port/database`
    /// or just `host:port` (uses defaults for user/db).
    pub fn from_url(url: &str) -> Self {
        let mut cfg = Self::default();

        // Strip optional mysql:// prefix
        let url = url.strip_prefix("mysql://").unwrap_or(url);

        // Split user:pass@host:port/database
        if let Some((userinfo, rest)) = url.split_once('@') {
            if let Some((user, pass)) = userinfo.split_once(':') {
                cfg.user = user.to_string();
                cfg.password = Some(pass.to_string());
            } else {
                cfg.user = userinfo.to_string();
            }
            Self::parse_host_port_db(&mut cfg, rest);
        } else {
            Self::parse_host_port_db(&mut cfg, url);
        }

        cfg
    }

    fn parse_host_port_db(cfg: &mut RooDbConfig, s: &str) {
        if let Some((hostport, db)) = s.split_once('/') {
            cfg.database = db.to_string();
            Self::parse_host_port(cfg, hostport);
        } else {
            Self::parse_host_port(cfg, s);
        }
    }

    fn parse_host_port(cfg: &mut RooDbConfig, s: &str) {
        if let Some((host, port)) = s.rsplit_once(':') {
            cfg.host = host.to_string();
            if let Ok(p) = port.parse::<u16>() {
                cfg.port = p;
            }
        } else if !s.is_empty() {
            cfg.host = s.to_string();
        }
    }
}

/// Handle to a RooDB connection pool.
pub struct RooDb {
    pool: Pool,
}

impl RooDb {
    /// Create a new connection pool to RooDB.
    pub fn new(config: &RooDbConfig) -> Self {
        let mut opts = OptsBuilder::default()
            .ip_or_hostname(&config.host)
            .tcp_port(config.port)
            .user(Some(&config.user))
            .db_name(Some(&config.database));

        if let Some(ref pass) = config.password {
            opts = opts.pass(Some(pass));
        }

        if config.tls {
            // RooDB requires TLS; accept self-signed certs for local dev
            let ssl = SslOpts::default()
                .with_danger_accept_invalid_certs(true)
                .with_danger_skip_domain_validation(true);
            opts = opts.ssl_opts(Some(ssl));
        }

        let pool = Pool::new(opts);
        Self { pool }
    }

    /// Acquire a pooled connection.
    pub async fn get_conn(&self) -> anyhow::Result<mysql_async::Conn> {
        let conn = self.pool.get_conn().await?;
        Ok(conn)
    }

    /// Initialize the schema (idempotent).
    pub async fn init_schema(&self) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;

        // Main snapshot table: stores the full serialized SystemState
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS system_snapshots (
                id          BIGINT AUTO_INCREMENT PRIMARY KEY,
                version     VARCHAR(32) NOT NULL,
                saved_at    BIGINT NOT NULL,
                tick_count  BIGINT NOT NULL,
                state_data  BLOB NOT NULL
            )",
        )
        .await?;

        // Agents table for queryable agent data
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS agents (
                snapshot_id BIGINT NOT NULL,
                agent_id    BIGINT NOT NULL,
                role        VARCHAR(32) NOT NULL,
                description TEXT,
                curiosity   DOUBLE NOT NULL,
                harmony     DOUBLE NOT NULL,
                growth      DOUBLE NOT NULL,
                transcendence DOUBLE NOT NULL,
                learning_rate DOUBLE NOT NULL,
                bid_bias    DOUBLE NOT NULL,
                jw          DOUBLE NOT NULL DEFAULT 0,
                PRIMARY KEY (snapshot_id, agent_id)
            )",
        )
        .await?;

        // Edges table
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS hyper_edges (
                snapshot_id  BIGINT NOT NULL,
                edge_idx     BIGINT NOT NULL,
                participants TEXT NOT NULL,
                weight       DOUBLE NOT NULL,
                emergent     TINYINT NOT NULL,
                age          BIGINT NOT NULL,
                created_at   BIGINT NOT NULL,
                scope        VARCHAR(32),
                origin_system VARCHAR(128),
                PRIMARY KEY (snapshot_id, edge_idx)
            )",
        )
        .await?;

        // Beliefs table
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS beliefs (
                snapshot_id  BIGINT NOT NULL,
                belief_id    BIGINT NOT NULL,
                content      TEXT NOT NULL,
                confidence   DOUBLE NOT NULL,
                source       VARCHAR(32) NOT NULL,
                created_at   BIGINT NOT NULL,
                updated_at   BIGINT NOT NULL,
                update_count INT NOT NULL,
                PRIMARY KEY (snapshot_id, belief_id)
            )",
        )
        .await?;

        // Experiences table
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS experiences (
                snapshot_id   BIGINT NOT NULL,
                experience_id BIGINT NOT NULL,
                description   TEXT NOT NULL,
                context       TEXT NOT NULL,
                outcome_type  VARCHAR(32) NOT NULL,
                coherence_delta DOUBLE,
                timestamp     BIGINT NOT NULL,
                tick          BIGINT NOT NULL,
                PRIMARY KEY (snapshot_id, experience_id)
            )",
        )
        .await?;

        // Improvement events table
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS improvement_events (
                snapshot_id      BIGINT NOT NULL,
                event_idx        BIGINT NOT NULL,
                timestamp        BIGINT NOT NULL,
                intent           TEXT NOT NULL,
                mutation_type    VARCHAR(32) NOT NULL,
                coherence_before DOUBLE NOT NULL,
                coherence_after  DOUBLE NOT NULL,
                novelty_score    FLOAT NOT NULL,
                applied          TINYINT NOT NULL,
                PRIMARY KEY (snapshot_id, event_idx)
            )",
        )
        .await?;

        // Ontology table
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS ontology (
                snapshot_id    BIGINT NOT NULL,
                concept        VARCHAR(255) NOT NULL,
                instances      TEXT NOT NULL,
                confidence     FLOAT NOT NULL,
                parent_concepts TEXT NOT NULL,
                created_epoch  BIGINT NOT NULL,
                last_modified  BIGINT NOT NULL,
                PRIMARY KEY (snapshot_id, concept)
            )",
        )
        .await?;

        // Code Agent Sessions table - full audit trail for coding assistant
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS code_agent_sessions (
                id              BIGINT AUTO_INCREMENT PRIMARY KEY,
                session_id      VARCHAR(64) NOT NULL UNIQUE,
                query           TEXT NOT NULL,
                model           VARCHAR(128) NOT NULL,
                started_at      BIGINT NOT NULL,
                completed_at    BIGINT,
                final_response  LONGTEXT,
                quality_score   DOUBLE,
                turn_count      INT NOT NULL DEFAULT 0,
                status          VARCHAR(32) NOT NULL,
                working_dir     VARCHAR(512) NOT NULL,
                error_message   TEXT,
                INDEX idx_started_at (started_at),
                INDEX idx_status (status)
            )",
        )
        .await?;

        // Code Agent Tool Calls table - individual tool execution audit
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS code_agent_tool_calls (
                id              BIGINT AUTO_INCREMENT PRIMARY KEY,
                session_id      VARCHAR(64) NOT NULL,
                turn_number     INT NOT NULL,
                tool_name       VARCHAR(32) NOT NULL,
                arguments       JSON NOT NULL,
                result          LONGTEXT,
                error           TEXT,
                execution_time_ms BIGINT,
                executed_at     BIGINT NOT NULL,
                file_path       VARCHAR(512),
                INDEX idx_session (session_id),
                INDEX idx_turn (session_id, turn_number),
                INDEX idx_tool (tool_name),
                INDEX idx_file (file_path)
            )",
        )
        .await?;

        // Code Agent Messages table - full conversation transcript
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS code_agent_messages (
                id              BIGINT AUTO_INCREMENT PRIMARY KEY,
                session_id      VARCHAR(64) NOT NULL,
                turn_number     INT NOT NULL,
                role            VARCHAR(32) NOT NULL,
                content         LONGTEXT NOT NULL,
                timestamp       BIGINT NOT NULL,
                has_tool_calls  TINYINT NOT NULL DEFAULT 0,
                INDEX idx_session (session_id),
                INDEX idx_turn (session_id, turn_number)
            )",
        )
        .await?;

        // Vault embeddings table (semantic search)
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS vault_embeddings (
                note_id      VARCHAR(255) NOT NULL PRIMARY KEY,
                title        TEXT NOT NULL,
                tags         TEXT,
                path         TEXT,
                preview      TEXT,
                content_hash VARCHAR(64) NOT NULL,
                embedding    LONGTEXT NOT NULL,
                metadata     TEXT,
                updated_at   BIGINT NOT NULL
            )",
        )
        .await?;

        // SkillBank tables
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS skills (
                skill_id    VARCHAR(128) NOT NULL PRIMARY KEY,
                title       TEXT NOT NULL,
                principle   TEXT NOT NULL,
                level       VARCHAR(64) NOT NULL,
                role        VARCHAR(64),
                task        VARCHAR(128),
                confidence  DOUBLE NOT NULL,
                usage_count BIGINT NOT NULL DEFAULT 0,
                success_count BIGINT NOT NULL DEFAULT 0,
                failure_count BIGINT NOT NULL DEFAULT 0,
                status      VARCHAR(64),
                created_at  BIGINT NOT NULL,
                updated_at  BIGINT NOT NULL
            )",
        )
        .await?;

        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS skill_evidence (
                id        BIGINT AUTO_INCREMENT PRIMARY KEY,
                skill_id  VARCHAR(128) NOT NULL,
                msg_id    VARCHAR(128) NOT NULL,
                edge_id   BIGINT NOT NULL DEFAULT -1,
                outcome   VARCHAR(64),
                created_at BIGINT NOT NULL,
                UNIQUE KEY uniq_skill_evidence (skill_id, msg_id, edge_id)
            )",
        )
        .await?;

        // Inter-agent messages (evidence store)
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS messages (
                msg_id    VARCHAR(128) NOT NULL PRIMARY KEY,
                sender    BIGINT NOT NULL,
                target    VARCHAR(64) NOT NULL,
                kind      VARCHAR(64) NOT NULL,
                content   TEXT NOT NULL,
                created_at BIGINT NOT NULL
            )",
        )
        .await?;

        // Council claims with evidence bindings
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS council_claims (
                id        BIGINT AUTO_INCREMENT PRIMARY KEY,
                question  TEXT NOT NULL,
                claim     TEXT NOT NULL,
                evidence_msgs TEXT NOT NULL,
                evidence_edges TEXT NOT NULL,
                confidence DOUBLE NOT NULL,
                coverage   DOUBLE NOT NULL,
                mode       VARCHAR(64) NOT NULL,
                created_at BIGINT NOT NULL
            )",
        )
        .await?;

        // Reward logs (GRPO / RLM)
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS reward_logs (
                id        BIGINT AUTO_INCREMENT PRIMARY KEY,
                tick      BIGINT NOT NULL,
                agent_id  BIGINT NOT NULL,
                reward    DOUBLE NOT NULL,
                source    VARCHAR(64) NOT NULL,
                created_at BIGINT NOT NULL
            )",
        )
        .await?;

        // Plan steps — persisted for audit/replay of council plan evidence
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS plan_steps (
                id           BIGINT AUTO_INCREMENT PRIMARY KEY,
                step_index   INT NOT NULL,
                claim        TEXT NOT NULL,
                plan_text    TEXT NOT NULL,
                evidence_msg_ids TEXT NOT NULL,
                qmd_ids      TEXT NOT NULL,
                skill_refs   TEXT NOT NULL,
                has_task_msg BOOLEAN NOT NULL DEFAULT FALSE,
                workflow_msg_id VARCHAR(128),
                created_at   BIGINT NOT NULL
            )",
        )
        .await?;

        // Skill hires — delegation tree records for recursive orchestration
        conn.query_drop(
            // ─── DSPy Optimizer Tables ───
            "CREATE TABLE IF NOT EXISTS dspy_traces (
                id               BIGINT AUTO_INCREMENT PRIMARY KEY,
                signature_name   VARCHAR(64) NOT NULL,
                input_question   TEXT NOT NULL,
                input_context_hash VARCHAR(64),
                output           TEXT NOT NULL,
                score            DOUBLE NOT NULL DEFAULT 0.0,
                semantic_ok      BOOLEAN NOT NULL DEFAULT FALSE,
                repair_count     INT NOT NULL DEFAULT 0,
                model            VARCHAR(64),
                latency_ms       INT NOT NULL DEFAULT 0,
                created_at       BIGINT NOT NULL,
                failure_code     VARCHAR(64) NULL,
                failure_detail   TEXT NULL,
                signals_json     TEXT NULL,
                INDEX idx_sig_score (signature_name, score),
                INDEX idx_created (created_at)
            )",
        )
        .await?;

        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS dspy_demonstrations (
                id               BIGINT AUTO_INCREMENT PRIMARY KEY,
                signature_name   VARCHAR(64) NOT NULL,
                input_summary    VARCHAR(600) NOT NULL,
                output           TEXT NOT NULL,
                score            DOUBLE NOT NULL DEFAULT 0.0,
                source           VARCHAR(32) NOT NULL DEFAULT 'bootstrapped',
                source_trace_id  BIGINT,
                active           BOOLEAN NOT NULL DEFAULT TRUE,
                promoted_by      VARCHAR(64),
                created_at       BIGINT NOT NULL,
                INDEX idx_sig_active (signature_name, active, score)
            )",
        )
        .await?;

        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS dspy_optimized_configs (
                id               BIGINT AUTO_INCREMENT PRIMARY KEY,
                signature_name   VARCHAR(64) NOT NULL,
                system_text      TEXT NOT NULL,
                prompt_template  TEXT NOT NULL,
                demo_ids         TEXT NOT NULL,
                demo_count       INT NOT NULL DEFAULT 0,
                eval_score       DOUBLE NOT NULL DEFAULT 0.0,
                eval_set_size    INT NOT NULL DEFAULT 0,
                trials_run       INT NOT NULL DEFAULT 0,
                version          INT NOT NULL DEFAULT 1,
                created_at       BIGINT NOT NULL,
                active           BOOLEAN NOT NULL DEFAULT FALSE,
                INDEX idx_sig_active (signature_name, active)
            )",
        )
        .await?;

        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS skill_hires (
                id               BIGINT AUTO_INCREMENT PRIMARY KEY,
                hire_id          VARCHAR(128) NOT NULL,
                parent_skill_id  VARCHAR(128) NOT NULL,
                child_skill_id   VARCHAR(128) NOT NULL,
                plan_step_index  INT NOT NULL,
                subproblem       TEXT NOT NULL,
                subproblem_domains TEXT NOT NULL,
                skill_briefing   TEXT NOT NULL,
                signature_id     VARCHAR(128) NOT NULL,
                signature_claim  TEXT NOT NULL,
                signature_evidence TEXT NOT NULL,
                parent_sig_id    VARCHAR(128),
                depth            TINYINT NOT NULL DEFAULT 0,
                budget           DOUBLE NOT NULL DEFAULT 1.0,
                status           VARCHAR(32) NOT NULL DEFAULT 'Active',
                outcome_score    DOUBLE,
                created_at       BIGINT NOT NULL,
                completed_at     BIGINT
            )",
        )
        .await?;

        // Ouroboros compatibility: gate decisions and event-sourced memory log
        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS ouroboros_gate_audits (
                id               BIGINT AUTO_INCREMENT PRIMARY KEY,
                action_id        VARCHAR(128) NOT NULL,
                action_kind      VARCHAR(64) NOT NULL,
                risk_level       VARCHAR(32) NOT NULL,
                policy_decision  VARCHAR(32) NOT NULL,
                council_required BOOLEAN NOT NULL DEFAULT FALSE,
                council_mode     VARCHAR(64),
                approved         BOOLEAN NOT NULL DEFAULT FALSE,
                reason           TEXT,
                created_at       BIGINT NOT NULL,
                INDEX idx_action_id (action_id),
                INDEX idx_created_at (created_at)
            )",
        )
        .await?;

        conn.query_drop(
            "CREATE TABLE IF NOT EXISTS ouroboros_memory_events (
                id               BIGINT AUTO_INCREMENT PRIMARY KEY,
                event_id         VARCHAR(128) NOT NULL,
                event_kind       VARCHAR(64) NOT NULL,
                payload          LONGTEXT NOT NULL,
                created_at       BIGINT NOT NULL,
                UNIQUE KEY uniq_event_id (event_id),
                INDEX idx_created_at (created_at)
            )",
        )
        .await?;

        drop(conn);
        self.upgrade_dspy_gepa_columns().await?;

        Ok(())
    }

    /// Add GEPA / failure-analysis columns to `dspy_traces` (idempotent for existing DBs).
    async fn upgrade_dspy_gepa_columns(&self) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        let alters = [
            "ALTER TABLE dspy_traces ADD COLUMN failure_code VARCHAR(64) NULL",
            "ALTER TABLE dspy_traces ADD COLUMN failure_detail TEXT NULL",
            "ALTER TABLE dspy_traces ADD COLUMN signals_json TEXT NULL",
        ];
        for sql in alters {
            if let Err(e) = conn.query_drop(sql).await {
                let msg = e.to_string();
                if msg.contains("1060") || msg.contains("Duplicate column") {
                    continue;
                }
                return Err(e.into());
            }
        }
        Ok(())
    }

    /// Save the full system state to RooDB.
    pub async fn save(
        &self,
        world: &HyperStigmergicMorphogenesis,
        rlm_state: Option<&RLMState>,
    ) -> anyhow::Result<u64> {
        let state = SystemState {
            morphogenesis: world.clone(),
            rlm_state: rlm_state.cloned(),
            saved_at: HyperStigmergicMorphogenesis::current_timestamp(),
            version: "0.2.0".to_string(),
        };

        // Serialize full state as bincode blob
        let state_data = bincode::serialize(&state)?;

        let mut conn = self.pool.get_conn().await?;

        // Insert snapshot and get the generated id
        conn.exec_drop(
            "INSERT INTO system_snapshots (version, saved_at, tick_count, state_data) \
             VALUES (?, ?, ?, ?)",
            (
                &state.version,
                state.saved_at,
                world.tick_count,
                &state_data,
            ),
        )
        .await?;

        let snapshot_id: u64 = conn
            .query_first("SELECT LAST_INSERT_ID()")
            .await?
            .unwrap_or(0);

        // Insert agents
        for agent in &world.agents {
            let role_str = format!("{:?}", agent.role);
            conn.exec_drop(
                "INSERT INTO agents \
                 (snapshot_id, agent_id, role, description, curiosity, harmony, growth, transcendence, learning_rate, bid_bias, jw) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    snapshot_id, agent.id, &role_str, &agent.description,
                    agent.drives.curiosity, agent.drives.harmony,
                    agent.drives.growth, agent.drives.transcendence,
                    agent.learning_rate, agent.bid_bias, agent.jw,
                ),
            ).await?;
        }

        // Insert edges
        for (idx, edge) in world.edges.iter().enumerate() {
            let participants_json = serde_json::to_string(&edge.participants)?;
            let scope_str = edge.scope.as_ref().map(|s| format!("{:?}", s));
            let origin = edge.origin_system.as_deref();
            conn.exec_drop(
                "INSERT INTO hyper_edges \
                 (snapshot_id, edge_idx, participants, weight, emergent, age, created_at, scope, origin_system) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    snapshot_id, idx as u64, &participants_json, edge.weight,
                    edge.emergent as u8, edge.age, edge.created_at,
                    &scope_str, &origin,
                ),
            ).await?;
        }

        // Insert beliefs
        for belief in &world.beliefs {
            let source_str = format!("{:?}", belief.source);
            conn.exec_drop(
                "INSERT INTO beliefs \
                 (snapshot_id, belief_id, content, confidence, source, created_at, updated_at, update_count) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    snapshot_id, belief.id as u64, &belief.content,
                    belief.confidence, &source_str,
                    belief.created_at, belief.updated_at, belief.update_count,
                ),
            ).await?;
        }

        // Insert experiences
        for exp in &world.experiences {
            let (outcome_type, coherence_delta) = match &exp.outcome {
                crate::hyper_stigmergy::ExperienceOutcome::Positive { coherence_delta } => {
                    ("Positive", Some(*coherence_delta))
                }
                crate::hyper_stigmergy::ExperienceOutcome::Negative { coherence_delta } => {
                    ("Negative", Some(*coherence_delta))
                }
                crate::hyper_stigmergy::ExperienceOutcome::Neutral => ("Neutral", None),
            };
            conn.exec_drop(
                "INSERT INTO experiences \
                 (snapshot_id, experience_id, description, context, outcome_type, coherence_delta, timestamp, tick) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    snapshot_id, exp.id as u64, &exp.description, &exp.context,
                    outcome_type, coherence_delta, exp.timestamp, exp.tick,
                ),
            ).await?;
        }

        // Insert improvement events
        for (idx, event) in world.improvement_history.iter().enumerate() {
            let mutation_str = format!("{:?}", event.mutation_type);
            conn.exec_drop(
                "INSERT INTO improvement_events \
                 (snapshot_id, event_idx, timestamp, intent, mutation_type, coherence_before, coherence_after, novelty_score, applied) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    snapshot_id, idx as u64, event.timestamp, &event.intent,
                    &mutation_str, event.coherence_before, event.coherence_after,
                    event.novelty_score, event.applied as u8,
                ),
            ).await?;
        }

        // Insert ontology entries
        for (concept, entry) in &world.ontology {
            let instances_json = serde_json::to_string(&entry.instances)?;
            let parents_json = serde_json::to_string(&entry.parent_concepts)?;
            conn.exec_drop(
                "INSERT INTO ontology \
                 (snapshot_id, concept, instances, confidence, parent_concepts, created_epoch, last_modified) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                (
                    snapshot_id, concept, &instances_json, entry.confidence,
                    &parents_json, entry.created_epoch, entry.last_modified,
                ),
            ).await?;
        }

        Ok(snapshot_id)
    }

    /// Load the most recent system state from RooDB.
    pub async fn load_latest(
        &self,
    ) -> anyhow::Result<(HyperStigmergicMorphogenesis, Option<RLMState>)> {
        let mut conn = self.pool.get_conn().await?;

        let row: Option<(u64, Vec<u8>)> = conn
            .query_first("SELECT id, state_data FROM system_snapshots ORDER BY id DESC LIMIT 1")
            .await?;

        match row {
            Some((_id, state_data)) => {
                let state: SystemState = bincode::deserialize(&state_data)?;
                let mut morph = state.morphogenesis;
                morph.rebuild_adjacency();
                Ok((morph, state.rlm_state))
            }
            None => {
                anyhow::bail!("No snapshots found in database")
            }
        }
    }

    /// Load a specific snapshot by ID.
    pub async fn load_snapshot(
        &self,
        snapshot_id: u64,
    ) -> anyhow::Result<(HyperStigmergicMorphogenesis, Option<RLMState>)> {
        let mut conn = self.pool.get_conn().await?;

        let row: Option<Vec<u8>> = conn
            .exec_first(
                "SELECT state_data FROM system_snapshots WHERE id = ?",
                (snapshot_id,),
            )
            .await?;

        match row {
            Some(state_data) => {
                let state: SystemState = bincode::deserialize(&state_data)?;
                let mut morph = state.morphogenesis;
                morph.rebuild_adjacency();
                Ok((morph, state.rlm_state))
            }
            None => {
                anyhow::bail!("Snapshot {} not found", snapshot_id)
            }
        }
    }

    /// List available snapshots (id, version, tick_count, saved_at).
    pub async fn list_snapshots(&self) -> anyhow::Result<Vec<(u64, String, u64, u64)>> {
        let mut conn = self.pool.get_conn().await?;

        let rows: Vec<(u64, String, u64, u64)> = conn
            .query(
                "SELECT id, version, tick_count, saved_at \
                 FROM system_snapshots ORDER BY id DESC LIMIT 20",
            )
            .await?;

        Ok(rows)
    }

    /// Test the database connection.
    pub async fn ping(&self) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        conn.query_drop("SELECT 1").await?;
        Ok(())
    }

    /// Run an arbitrary SELECT query and return (headers, rows) as strings.
    /// Suitable for display in the TUI chat.
    pub async fn raw_query(&self, sql: &str) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
        let mut conn = self.pool.get_conn().await?;
        let mut result = conn.query_iter(sql).await?;
        let columns: Vec<String> = result
            .columns_ref()
            .iter()
            .map(|c| c.name_str().to_string())
            .collect();
        let mut rows: Vec<Vec<String>> = Vec::new();
        result
            .for_each(|row: mysql_async::Row| {
                let vals: Vec<String> = (0..row.len())
                    .map(|i| {
                        row.get_opt::<mysql_async::Value, _>(i)
                            .unwrap_or(Ok(mysql_async::Value::NULL))
                            .unwrap_or(mysql_async::Value::NULL)
                            .as_sql(true)
                    })
                    .collect();
                rows.push(vals);
            })
            .await?;
        Ok((columns, rows))
    }

    /// Execute a SQL statement (INSERT, UPDATE, DELETE, CREATE, etc.)
    pub async fn execute(&self, sql: &str) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        conn.query_drop(sql).await?;
        Ok(())
    }

    /// Disconnect the pool gracefully.
    pub async fn disconnect(self) -> anyhow::Result<()> {
        self.pool.disconnect().await?;
        Ok(())
    }

    // === CODE AGENT SESSION PERSISTENCE ===

    /// Start a new code agent session.
    pub async fn start_code_agent_session(
        &self,
        session_id: &str,
        query: &str,
        model: &str,
        working_dir: &str,
    ) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        let started_at = HyperStigmergicMorphogenesis::current_timestamp();

        conn.exec_drop(
            "INSERT INTO code_agent_sessions \
             (session_id, query, model, started_at, status, working_dir, turn_count) \
             VALUES (?, ?, ?, ?, ?, ?, 0)",
            (session_id, query, model, started_at, "running", working_dir),
        )
        .await?;

        Ok(())
    }

    /// Record a message in the session transcript.
    pub async fn record_code_agent_message(
        &self,
        session_id: &str,
        turn_number: i32,
        role: &str,
        content: &str,
        has_tool_calls: bool,
    ) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        let timestamp = HyperStigmergicMorphogenesis::current_timestamp();

        conn.exec_drop(
            "INSERT INTO code_agent_messages \
             (session_id, turn_number, role, content, timestamp, has_tool_calls) \
             VALUES (?, ?, ?, ?, ?, ?)",
            (
                session_id,
                turn_number,
                role,
                content,
                timestamp,
                has_tool_calls as u8,
            ),
        )
        .await?;

        Ok(())
    }

    /// Record a tool call execution.
    pub async fn record_code_agent_tool_call(
        &self,
        session_id: &str,
        turn_number: i32,
        tool_name: &str,
        arguments: &serde_json::Value,
        result: Option<&str>,
        error: Option<&str>,
        execution_time_ms: u64,
        file_path: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        let executed_at = HyperStigmergicMorphogenesis::current_timestamp();
        let args_json = arguments.to_string();

        conn.exec_drop(
            "INSERT INTO code_agent_tool_calls \
             (session_id, turn_number, tool_name, arguments, result, error, execution_time_ms, executed_at, file_path) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                session_id, turn_number, tool_name, &args_json,
                result, error, execution_time_ms as i64, executed_at, file_path,
            ),
        ).await?;

        Ok(())
    }

    /// Complete a code agent session.
    pub async fn complete_code_agent_session(
        &self,
        session_id: &str,
        final_response: Option<&str>,
        quality_score: Option<f64>,
        turn_count: i32,
        error_message: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        let completed_at = HyperStigmergicMorphogenesis::current_timestamp();
        let status = if error_message.is_some() {
            "error"
        } else {
            "completed"
        };

        conn.exec_drop(
            "UPDATE code_agent_sessions SET \
             completed_at = ?, final_response = ?, quality_score = ?, \
             turn_count = ?, status = ?, error_message = ? \
             WHERE session_id = ?",
            (
                completed_at,
                final_response,
                quality_score,
                turn_count,
                status,
                error_message,
                session_id,
            ),
        )
        .await?;

        Ok(())
    }

    /// Query code agent sessions with optional filters.
    pub async fn query_code_agent_sessions(
        &self,
        limit: usize,
        status_filter: Option<&str>,
    ) -> anyhow::Result<Vec<CodeAgentSessionRow>> {
        let mut conn = self.pool.get_conn().await?;

        let sql = if let Some(status) = status_filter {
            format!(
                "SELECT session_id, query, model, started_at, completed_at, \
                 quality_score, turn_count, status, working_dir \
                 FROM code_agent_sessions \
                 WHERE status = '{}' \
                 ORDER BY started_at DESC \
                 LIMIT {}",
                status, limit
            )
        } else {
            format!(
                "SELECT session_id, query, model, started_at, completed_at, \
                 quality_score, turn_count, status, working_dir \
                 FROM code_agent_sessions \
                 ORDER BY started_at DESC \
                 LIMIT {}",
                limit
            )
        };

        let rows: Vec<(
            String,
            String,
            String,
            u64,
            Option<u64>,
            Option<f64>,
            i32,
            String,
            String,
        )> = conn.query(&sql).await?;

        Ok(rows
            .into_iter()
            .map(|r| CodeAgentSessionRow {
                session_id: r.0,
                query: r.1,
                model: r.2,
                started_at: r.3,
                completed_at: r.4,
                quality_score: r.5,
                turn_count: r.6,
                status: r.7,
                working_dir: r.8,
            })
            .collect())
    }

    /// Get full session transcript including all messages.
    pub async fn get_code_agent_session_transcript(
        &self,
        session_id: &str,
    ) -> anyhow::Result<(
        CodeAgentSessionRow,
        Vec<CodeAgentMessageRow>,
        Vec<CodeAgentToolCallRow>,
    )> {
        let mut conn = self.pool.get_conn().await?;

        // Get session info
        let session: (
            String,
            String,
            String,
            u64,
            Option<u64>,
            Option<f64>,
            i32,
            String,
            String,
        ) = conn
            .exec_first(
                "SELECT session_id, query, model, started_at, completed_at, \
                 quality_score, turn_count, status, working_dir \
                 FROM code_agent_sessions WHERE session_id = ?",
                (session_id,),
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;

        let session_row = CodeAgentSessionRow {
            session_id: session.0,
            query: session.1,
            model: session.2,
            started_at: session.3,
            completed_at: session.4,
            quality_score: session.5,
            turn_count: session.6,
            status: session.7,
            working_dir: session.8,
        };

        // Get messages
        let messages: Vec<(i32, String, String, u64, u8)> = conn
            .exec(
                "SELECT turn_number, role, content, timestamp, has_tool_calls \
             FROM code_agent_messages WHERE session_id = ? ORDER BY turn_number, timestamp",
                (session_id,),
            )
            .await?;

        let message_rows = messages
            .into_iter()
            .map(|m| CodeAgentMessageRow {
                turn_number: m.0,
                role: m.1,
                content: m.2,
                timestamp: m.3,
                has_tool_calls: m.4 != 0,
            })
            .collect();

        // Get tool calls
        let tool_calls: Vec<(
            i32,
            String,
            String,
            Option<String>,
            Option<String>,
            i64,
            u64,
            Option<String>,
        )> = conn
            .exec(
                "SELECT turn_number, tool_name, arguments, result, error, \
                 execution_time_ms, executed_at, file_path \
                 FROM code_agent_tool_calls WHERE session_id = ? ORDER BY turn_number, executed_at",
                (session_id,),
            )
            .await?;

        let tool_call_rows = tool_calls
            .into_iter()
            .map(|t| CodeAgentToolCallRow {
                turn_number: t.0,
                tool_name: t.1,
                arguments: t.2,
                result: t.3,
                error: t.4,
                execution_time_ms: t.5 as u64,
                executed_at: t.6,
                file_path: t.7,
            })
            .collect();

        Ok((session_row, message_rows, tool_call_rows))
    }

    /// Get files touched by code agent sessions (for audit trail).
    pub async fn get_code_agent_touched_files(
        &self,
        session_id: Option<&str>,
    ) -> anyhow::Result<Vec<(String, String, i32, u64)>> {
        let mut conn = self.pool.get_conn().await?;

        let rows: Vec<(String, String, i32, u64)> = if let Some(sid) = session_id {
            conn.exec(
                "SELECT s.session_id, t.file_path, t.turn_number, t.executed_at \
                 FROM code_agent_tool_calls t \
                 JOIN code_agent_sessions s ON t.session_id = s.session_id \
                 WHERE t.session_id = ? AND t.file_path IS NOT NULL \
                 ORDER BY t.executed_at DESC",
                (sid,),
            )
            .await?
        } else {
            conn.query(
                "SELECT s.session_id, t.file_path, t.turn_number, t.executed_at \
                 FROM code_agent_tool_calls t \
                 JOIN code_agent_sessions s ON t.session_id = s.session_id \
                 WHERE t.file_path IS NOT NULL \
                 ORDER BY t.executed_at DESC LIMIT 100",
            )
            .await?
        };

        Ok(rows)
    }

    // === VAULT EMBEDDINGS ===

    pub async fn upsert_vault_embedding(&self, row: &VaultEmbeddingRow) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        let tags_json = serde_json::to_string(&row.tags)?;
        let embedding_json = serde_json::to_string(&row.embedding)?;
        let metadata_json = serde_json::to_string(&row.metadata)?;
        conn.exec_drop(
            "INSERT INTO vault_embeddings \
             (note_id, title, tags, path, preview, content_hash, embedding, metadata, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON DUPLICATE KEY UPDATE \
             title = VALUES(title), tags = VALUES(tags), path = VALUES(path), \
             preview = VALUES(preview), content_hash = VALUES(content_hash), \
             embedding = VALUES(embedding), metadata = VALUES(metadata), updated_at = VALUES(updated_at)",
            (
                &row.note_id,
                &row.title,
                &tags_json,
                &row.path,
                &row.preview,
                &row.content_hash,
                &embedding_json,
                &metadata_json,
                row.updated_at,
            ),
        ).await?;
        Ok(())
    }

    pub async fn fetch_vault_embeddings(&self) -> anyhow::Result<Vec<VaultEmbeddingRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>, String, String, Option<String>, u64)> = conn.exec(
            "SELECT note_id, title, tags, path, preview, content_hash, embedding, metadata, updated_at FROM vault_embeddings",
            (),
        ).await?;

        let mut out = Vec::new();
        for (
            note_id,
            title,
            tags_raw,
            path,
            preview,
            content_hash,
            embedding_raw,
            metadata_raw,
            updated_at,
        ) in rows
        {
            let tags: Vec<String> = tags_raw
                .as_deref()
                .and_then(|t| serde_json::from_str(t).ok())
                .unwrap_or_default();
            let embedding: Vec<f32> = serde_json::from_str(&embedding_raw).unwrap_or_default();
            let metadata = metadata_raw
                .as_deref()
                .and_then(|m| serde_json::from_str(m).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            out.push(VaultEmbeddingRow {
                note_id,
                title,
                tags,
                path: path.unwrap_or_default(),
                preview: preview.unwrap_or_default(),
                content_hash,
                embedding,
                metadata,
                updated_at,
            });
        }
        Ok(out)
    }

    pub async fn fetch_vault_note_by_id(
        &self,
        note_id: &str,
    ) -> anyhow::Result<Option<VaultEmbeddingRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>, String, String, Option<String>, u64)> = conn.exec(
            "SELECT note_id, title, tags, path, preview, content_hash, embedding, metadata, updated_at FROM vault_embeddings WHERE note_id = ? LIMIT 1",
            (note_id,),
        ).await?;

        if let Some((
            note_id,
            title,
            tags_raw,
            path,
            preview,
            content_hash,
            embedding_raw,
            metadata_raw,
            updated_at,
        )) = rows.into_iter().next()
        {
            let tags: Vec<String> = tags_raw
                .as_deref()
                .and_then(|t| serde_json::from_str(t).ok())
                .unwrap_or_default();
            let embedding: Vec<f32> = serde_json::from_str(&embedding_raw).unwrap_or_default();
            let metadata = metadata_raw
                .as_deref()
                .and_then(|m| serde_json::from_str(m).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            return Ok(Some(VaultEmbeddingRow {
                note_id,
                title,
                tags,
                path: path.unwrap_or_default(),
                preview: preview.unwrap_or_default(),
                content_hash,
                embedding,
                metadata,
                updated_at,
            }));
        }
        Ok(None)
    }

    // === SKILLS ===

    pub async fn upsert_skill(&self, row: &SkillRow) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        conn.exec_drop(
            "INSERT INTO skills \
             (skill_id, title, principle, level, role, task, confidence, usage_count, success_count, failure_count, status, created_at, updated_at) \
             VALUES (:skill_id, :title, :principle, :level, :role, :task, :confidence, :usage_count, :success_count, :failure_count, :status, :created_at, :updated_at) \
             ON DUPLICATE KEY UPDATE \
             title = VALUES(title), principle = VALUES(principle), level = VALUES(level), \
             role = VALUES(role), task = VALUES(task), confidence = VALUES(confidence), \
             usage_count = VALUES(usage_count), success_count = VALUES(success_count), \
             failure_count = VALUES(failure_count), status = VALUES(status), updated_at = VALUES(updated_at)",
            mysql_async::params! {
                "skill_id" => &row.skill_id,
                "title" => &row.title,
                "principle" => &row.principle,
                "level" => &row.level,
                "role" => &row.role,
                "task" => &row.task,
                "confidence" => row.confidence,
                "usage_count" => row.usage_count,
                "success_count" => row.success_count,
                "failure_count" => row.failure_count,
                "status" => &row.status,
                "created_at" => row.created_at,
                "updated_at" => row.updated_at,
            },
        ).await?;
        Ok(())
    }

    pub async fn insert_skill_evidence(&self, row: &SkillEvidenceRow) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        conn.exec_drop(
            "INSERT IGNORE INTO skill_evidence (skill_id, msg_id, edge_id, outcome, created_at) \
             VALUES (?, ?, ?, ?, ?)",
            (
                &row.skill_id,
                &row.msg_id,
                row.edge_id,
                &row.outcome,
                row.created_at,
            ),
        )
        .await?;
        Ok(())
    }

    pub async fn fetch_skills(&self, limit: usize) -> anyhow::Result<Vec<SkillRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT skill_id, title, principle, level, role, task, confidence, \
             usage_count, success_count, failure_count, status, created_at, updated_at \
             FROM skills ORDER BY updated_at DESC LIMIT ?",
                (limit as u64,),
            )
            .await?;

        let mut out = Vec::new();
        for mut row in rows {
            let skill_id: String = row.take("skill_id").unwrap_or_default();
            let title: String = row.take("title").unwrap_or_default();
            let principle: String = row.take("principle").unwrap_or_default();
            let level: String = row.take("level").unwrap_or_else(|| "General".to_string());
            let role: Option<String> = row.take("role");
            let task: Option<String> = row.take("task");
            let confidence: f64 = row.take("confidence").unwrap_or(0.5);
            let usage_count: u64 = row.take("usage_count").unwrap_or(0);
            let success_count: u64 = row.take("success_count").unwrap_or(0);
            let failure_count: u64 = row.take("failure_count").unwrap_or(0);
            let status: Option<String> = row.take("status");
            let created_at: u64 = row.take("created_at").unwrap_or(0);
            let updated_at: u64 = row.take("updated_at").unwrap_or(0);
            out.push(SkillRow {
                skill_id,
                title,
                principle,
                level,
                role,
                task,
                confidence,
                usage_count,
                success_count,
                failure_count,
                status: status.unwrap_or_else(|| "active".to_string()),
                created_at,
                updated_at,
            });
        }
        Ok(out)
    }

    // === MESSAGES ===

    pub async fn insert_message(&self, row: &MessageRow) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        conn.exec_drop(
            "INSERT IGNORE INTO messages (msg_id, sender, target, kind, content, created_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
            (
                &row.msg_id,
                row.sender,
                &row.target,
                &row.kind,
                &row.content,
                row.created_at,
            ),
        )
        .await?;
        Ok(())
    }

    pub async fn fetch_messages(&self, limit: usize) -> anyhow::Result<Vec<MessageRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT msg_id, sender, target, kind, content, created_at \
             FROM messages ORDER BY created_at DESC LIMIT ?",
                (limit as u64,),
            )
            .await?;
        let mut out = Vec::new();
        for mut row in rows {
            let msg_id: String = row.take("msg_id").unwrap_or_default();
            let sender: u64 = row.take("sender").unwrap_or(0);
            let target: String = row.take("target").unwrap_or_default();
            let kind: String = row.take("kind").unwrap_or_default();
            let content: String = row.take("content").unwrap_or_default();
            let created_at: u64 = row.take("created_at").unwrap_or(0);
            out.push(MessageRow {
                msg_id,
                sender,
                target,
                kind,
                content,
                created_at,
            });
        }
        Ok(out)
    }

    // === COUNCIL CLAIMS ===

    pub async fn insert_council_claim(&self, row: &CouncilClaimRow) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        let msgs_json = serde_json::to_string(&row.evidence_msgs)?;
        let edges_json = serde_json::to_string(&row.evidence_edges)?;
        conn.exec_drop(
            "INSERT INTO council_claims \
             (question, claim, evidence_msgs, evidence_edges, confidence, coverage, mode, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            (
                &row.question,
                &row.claim,
                &msgs_json,
                &edges_json,
                row.confidence,
                row.coverage,
                &row.mode,
                row.created_at,
            ),
        ).await?;
        Ok(())
    }

    // === REWARDS ===

    pub async fn insert_reward_log(&self, row: &RewardLogRow) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        conn.exec_drop(
            "INSERT INTO reward_logs (tick, agent_id, reward, source, created_at) \
             VALUES (?, ?, ?, ?, ?)",
            (
                row.tick,
                row.agent_id,
                row.reward,
                &row.source,
                row.created_at,
            ),
        )
        .await?;
        Ok(())
    }

    // === PLAN STEPS ===

    pub async fn insert_plan_step(&self, row: &PlanStepRow) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        let msg_ids_json = serde_json::to_string(&row.evidence_msg_ids).unwrap_or_default();
        let qmd_ids_json = serde_json::to_string(&row.qmd_ids).unwrap_or_default();
        let skill_refs_json = serde_json::to_string(&row.skill_ref_ids).unwrap_or_default();
        conn.exec_drop(
            "INSERT INTO plan_steps (step_index, claim, plan_text, evidence_msg_ids, qmd_ids, skill_refs, has_task_msg, workflow_msg_id, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                row.step_index as u32,
                &row.claim,
                &row.plan_text,
                &msg_ids_json,
                &qmd_ids_json,
                &skill_refs_json,
                row.has_task_msg,
                &row.workflow_msg_id,
                row.created_at,
            ),
        ).await?;
        Ok(())
    }

    pub async fn fetch_plan_steps(&self, limit: usize) -> anyhow::Result<Vec<PlanStepRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn.exec(
            "SELECT step_index, claim, plan_text, evidence_msg_ids, qmd_ids, skill_refs, has_task_msg, workflow_msg_id, created_at \
             FROM plan_steps ORDER BY created_at DESC, step_index ASC LIMIT ?",
            (limit as u32,),
        ).await?;
        let mut out = Vec::new();
        for r in rows {
            let msg_ids_raw: String = r.get("evidence_msg_ids").unwrap_or_default();
            let qmd_ids_raw: String = r.get("qmd_ids").unwrap_or_default();
            let skill_refs_raw: String = r.get("skill_refs").unwrap_or_default();
            out.push(PlanStepRow {
                step_index: r.get::<u32, _>("step_index").unwrap_or(0) as usize,
                claim: r.get("claim").unwrap_or_default(),
                plan_text: r.get("plan_text").unwrap_or_default(),
                evidence_msg_ids: serde_json::from_str(&msg_ids_raw).unwrap_or_default(),
                qmd_ids: serde_json::from_str(&qmd_ids_raw).unwrap_or_default(),
                skill_ref_ids: serde_json::from_str(&skill_refs_raw).unwrap_or_default(),
                has_task_msg: r.get("has_task_msg").unwrap_or(false),
                workflow_msg_id: r.get("workflow_msg_id"),
                created_at: r.get("created_at").unwrap_or(0),
            });
        }
        Ok(out)
    }

    // === SKILL HIRES (Delegation Tree) ===

    pub async fn insert_skill_hire(&self, row: &SkillHireRow) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        let domains_json = serde_json::to_string(&row.subproblem_domains).unwrap_or_default();
        let briefing_json = serde_json::to_string(&row.skill_briefing).unwrap_or_default();
        let evidence_json = serde_json::to_string(&row.signature_evidence).unwrap_or_default();
        conn.exec_drop(
            "INSERT INTO skill_hires \
             (hire_id, parent_skill_id, child_skill_id, plan_step_index, \
              subproblem, subproblem_domains, skill_briefing, \
              signature_id, signature_claim, signature_evidence, parent_sig_id, \
              depth, budget, status, outcome_score, created_at, completed_at) \
             VALUES (:hire_id, :parent_skill_id, :child_skill_id, :plan_step_index, \
              :subproblem, :subproblem_domains, :skill_briefing, \
              :signature_id, :signature_claim, :signature_evidence, :parent_sig_id, \
              :depth, :budget, :status, :outcome_score, :created_at, :completed_at)",
            mysql_async::params! {
                "hire_id" => &row.hire_id,
                "parent_skill_id" => &row.parent_skill_id,
                "child_skill_id" => &row.child_skill_id,
                "plan_step_index" => row.plan_step_index as u32,
                "subproblem" => &row.subproblem,
                "subproblem_domains" => &domains_json,
                "skill_briefing" => &briefing_json,
                "signature_id" => &row.signature_id,
                "signature_claim" => &row.signature_claim,
                "signature_evidence" => &evidence_json,
                "parent_sig_id" => &row.parent_sig_id,
                "depth" => row.depth,
                "budget" => row.budget,
                "status" => &row.status,
                "outcome_score" => row.outcome_score,
                "created_at" => row.created_at,
                "completed_at" => row.completed_at,
            },
        )
        .await?;
        Ok(())
    }

    pub async fn update_skill_hire_status(
        &self,
        hire_id: &str,
        status: &str,
        outcome_score: Option<f64>,
        completed_at: Option<u64>,
    ) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        conn.exec_drop(
            "UPDATE skill_hires SET status = ?, outcome_score = ?, completed_at = ? \
             WHERE hire_id = ?",
            (status, outcome_score, completed_at, hire_id),
        )
        .await?;
        Ok(())
    }

    pub async fn fetch_skill_hires(&self, limit: usize) -> anyhow::Result<Vec<SkillHireRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT hire_id, parent_skill_id, child_skill_id, plan_step_index, \
             subproblem, subproblem_domains, skill_briefing, \
             signature_id, signature_claim, signature_evidence, parent_sig_id, \
             depth, budget, status, outcome_score, created_at, completed_at \
             FROM skill_hires ORDER BY created_at DESC LIMIT ?",
                (limit as u32,),
            )
            .await?;
        let mut out = Vec::new();
        for r in rows {
            let domains_raw: String = r.get("subproblem_domains").unwrap_or_default();
            let briefing_raw: String = r.get("skill_briefing").unwrap_or_default();
            let evidence_raw: String = r.get("signature_evidence").unwrap_or_default();
            out.push(SkillHireRow {
                hire_id: r.get("hire_id").unwrap_or_default(),
                parent_skill_id: r.get("parent_skill_id").unwrap_or_default(),
                child_skill_id: r.get("child_skill_id").unwrap_or_default(),
                plan_step_index: r.get::<u32, _>("plan_step_index").unwrap_or(0) as usize,
                subproblem: r.get("subproblem").unwrap_or_default(),
                subproblem_domains: serde_json::from_str(&domains_raw).unwrap_or_default(),
                skill_briefing: serde_json::from_str(&briefing_raw).unwrap_or_default(),
                signature_id: r.get("signature_id").unwrap_or_default(),
                signature_claim: r.get("signature_claim").unwrap_or_default(),
                signature_evidence: serde_json::from_str(&evidence_raw).unwrap_or_default(),
                parent_sig_id: r.get("parent_sig_id"),
                depth: r.get::<u8, _>("depth").unwrap_or(0),
                budget: r.get("budget").unwrap_or(1.0),
                status: r.get("status").unwrap_or_else(|| "Active".to_string()),
                outcome_score: r.get("outcome_score"),
                created_at: r.get("created_at").unwrap_or(0),
                completed_at: r.get("completed_at"),
            });
        }
        Ok(out)
    }

    pub async fn fetch_skill_hires_by_plan_step(
        &self,
        plan_step_index: usize,
    ) -> anyhow::Result<Vec<SkillHireRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT hire_id, parent_skill_id, child_skill_id, plan_step_index, \
             subproblem, subproblem_domains, skill_briefing, \
             signature_id, signature_claim, signature_evidence, parent_sig_id, \
             depth, budget, status, outcome_score, created_at, completed_at \
             FROM skill_hires WHERE plan_step_index = ? ORDER BY depth ASC, created_at ASC",
                (plan_step_index as u32,),
            )
            .await?;
        let mut out = Vec::new();
        for r in rows {
            let domains_raw: String = r.get("subproblem_domains").unwrap_or_default();
            let briefing_raw: String = r.get("skill_briefing").unwrap_or_default();
            let evidence_raw: String = r.get("signature_evidence").unwrap_or_default();
            out.push(SkillHireRow {
                hire_id: r.get("hire_id").unwrap_or_default(),
                parent_skill_id: r.get("parent_skill_id").unwrap_or_default(),
                child_skill_id: r.get("child_skill_id").unwrap_or_default(),
                plan_step_index: r.get::<u32, _>("plan_step_index").unwrap_or(0) as usize,
                subproblem: r.get("subproblem").unwrap_or_default(),
                subproblem_domains: serde_json::from_str(&domains_raw).unwrap_or_default(),
                skill_briefing: serde_json::from_str(&briefing_raw).unwrap_or_default(),
                signature_id: r.get("signature_id").unwrap_or_default(),
                signature_claim: r.get("signature_claim").unwrap_or_default(),
                signature_evidence: serde_json::from_str(&evidence_raw).unwrap_or_default(),
                parent_sig_id: r.get("parent_sig_id"),
                depth: r.get::<u8, _>("depth").unwrap_or(0),
                budget: r.get("budget").unwrap_or(1.0),
                status: r.get("status").unwrap_or_else(|| "Active".to_string()),
                outcome_score: r.get("outcome_score"),
                created_at: r.get("created_at").unwrap_or(0),
                completed_at: r.get("completed_at"),
            });
        }
        Ok(out)
    }

    // ─── DSPy Optimizer Persistence ───

    /// Insert a trace record from a run_signature() call.
    pub async fn insert_dspy_trace(&self, row: &DspyTraceRow) -> anyhow::Result<i64> {
        let mut conn = self.pool.get_conn().await?;
        conn.exec_drop(
            "INSERT INTO dspy_traces \
             (signature_name, input_question, input_context_hash, output, score, \
              semantic_ok, repair_count, model, latency_ms, created_at, \
              failure_code, failure_detail, signals_json) \
             VALUES (:signature_name, :input_question, :input_context_hash, :output, :score, \
              :semantic_ok, :repair_count, :model, :latency_ms, :created_at, \
              :failure_code, :failure_detail, :signals_json)",
            mysql_async::params! {
                "signature_name" => &row.signature_name,
                "input_question" => &row.input_question,
                "input_context_hash" => &row.input_context_hash,
                "output" => &row.output,
                "score" => row.score,
                "semantic_ok" => row.semantic_ok,
                "repair_count" => row.repair_count,
                "model" => &row.model,
                "latency_ms" => row.latency_ms,
                "created_at" => row.created_at,
                "failure_code" => &row.failure_code,
                "failure_detail" => &row.failure_detail,
                "signals_json" => &row.signals_json,
            },
        )
        .await?;
        let id: i64 = conn
            .query_first("SELECT LAST_INSERT_ID()")
            .await?
            .unwrap_or(0);
        Ok(id)
    }

    /// Fetch recent traces for a signature, ordered by score descending.
    pub async fn fetch_dspy_traces(
        &self,
        signature_name: &str,
        min_score: f64,
        limit: usize,
    ) -> anyhow::Result<Vec<DspyTraceRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT id, signature_name, input_question, input_context_hash, output, \
             score, semantic_ok, repair_count, model, latency_ms, created_at, \
             IFNULL(failure_code, '') AS failure_code, \
             IFNULL(failure_detail, '') AS failure_detail, \
             IFNULL(signals_json, '') AS signals_json \
             FROM dspy_traces \
             WHERE signature_name = ? AND score >= ? \
             ORDER BY score DESC LIMIT ?",
                (signature_name, min_score, limit as u32),
            )
            .await?;
        let mut out = Vec::new();
        for r in rows {
            out.push(DspyTraceRow {
                id: r.get("id").unwrap_or(0),
                signature_name: r.get("signature_name").unwrap_or_default(),
                input_question: r.get("input_question").unwrap_or_default(),
                input_context_hash: r.get("input_context_hash").unwrap_or_default(),
                output: r.get("output").unwrap_or_default(),
                score: r.get("score").unwrap_or(0.0),
                semantic_ok: r.get("semantic_ok").unwrap_or(false),
                repair_count: r.get("repair_count").unwrap_or(0),
                model: r.get("model").unwrap_or_default(),
                latency_ms: r.get("latency_ms").unwrap_or(0),
                created_at: r.get("created_at").unwrap_or(0),
                failure_code: r.get("failure_code").unwrap_or_default(),
                failure_detail: r.get("failure_detail").unwrap_or_default(),
                signals_json: r.get("signals_json").unwrap_or_default(),
            });
        }
        Ok(out)
    }

    /// Traces at or below `max_score` (failure band) for GEPA collect / diagnosis.
    pub async fn fetch_dspy_traces_low_scoring(
        &self,
        signature_name: &str,
        max_score: f64,
        limit: usize,
    ) -> anyhow::Result<Vec<DspyTraceRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT id, signature_name, input_question, input_context_hash, output, \
             score, semantic_ok, repair_count, model, latency_ms, created_at, \
             IFNULL(failure_code, '') AS failure_code, \
             IFNULL(failure_detail, '') AS failure_detail, \
             IFNULL(signals_json, '') AS signals_json \
             FROM dspy_traces \
             WHERE signature_name = ? AND score <= ? \
             ORDER BY score ASC, created_at DESC LIMIT ?",
                (signature_name, max_score, limit as u32),
            )
            .await?;
        let mut out = Vec::new();
        for r in rows {
            out.push(DspyTraceRow {
                id: r.get("id").unwrap_or(0),
                signature_name: r.get("signature_name").unwrap_or_default(),
                input_question: r.get("input_question").unwrap_or_default(),
                input_context_hash: r.get("input_context_hash").unwrap_or_default(),
                output: r.get("output").unwrap_or_default(),
                score: r.get("score").unwrap_or(0.0),
                semantic_ok: r.get("semantic_ok").unwrap_or(false),
                repair_count: r.get("repair_count").unwrap_or(0),
                model: r.get("model").unwrap_or_default(),
                latency_ms: r.get("latency_ms").unwrap_or(0),
                created_at: r.get("created_at").unwrap_or(0),
                failure_code: r.get("failure_code").unwrap_or_default(),
                failure_detail: r.get("failure_detail").unwrap_or_default(),
                signals_json: r.get("signals_json").unwrap_or_default(),
            });
        }
        Ok(out)
    }

    /// Count total traces per signature for optimization trigger.
    #[allow(dead_code)]
    pub async fn count_dspy_traces(&self, signature_name: &str) -> anyhow::Result<u64> {
        let mut conn = self.pool.get_conn().await?;
        let count: u64 = conn
            .exec_first(
                "SELECT COUNT(*) FROM dspy_traces WHERE signature_name = ?",
                (signature_name,),
            )
            .await?
            .unwrap_or(0);
        Ok(count)
    }

    /// Insert a demonstration (few-shot example).
    pub async fn insert_dspy_demonstration(
        &self,
        row: &DspyDemonstrationRow,
    ) -> anyhow::Result<i64> {
        let mut conn = self.pool.get_conn().await?;
        conn.exec_drop(
            "INSERT INTO dspy_demonstrations \
             (signature_name, input_summary, output, score, source, source_trace_id, \
              active, promoted_by, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                &row.signature_name,
                &row.input_summary,
                &row.output,
                row.score,
                &row.source,
                row.source_trace_id,
                row.active,
                &row.promoted_by,
                row.created_at,
            ),
        )
        .await?;
        let id: i64 = conn
            .query_first("SELECT LAST_INSERT_ID()")
            .await?
            .unwrap_or(0);
        Ok(id)
    }

    /// Fetch active demonstrations for a signature, best-scoring first.
    pub async fn fetch_dspy_demonstrations(
        &self,
        signature_name: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<DspyDemonstrationRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT id, signature_name, input_summary, output, score, source, \
             source_trace_id, active, promoted_by, created_at \
             FROM dspy_demonstrations \
             WHERE signature_name = ? AND active = TRUE \
             ORDER BY score DESC LIMIT ?",
                (signature_name, limit as u32),
            )
            .await?;
        let mut out = Vec::new();
        for r in rows {
            out.push(DspyDemonstrationRow {
                id: r.get("id").unwrap_or(0),
                signature_name: r.get("signature_name").unwrap_or_default(),
                input_summary: r.get("input_summary").unwrap_or_default(),
                output: r.get("output").unwrap_or_default(),
                score: r.get("score").unwrap_or(0.0),
                source: r.get("source").unwrap_or_default(),
                source_trace_id: r.get("source_trace_id"),
                active: r.get("active").unwrap_or(false),
                promoted_by: r.get("promoted_by"),
                created_at: r.get("created_at").unwrap_or(0),
            });
        }
        Ok(out)
    }

    /// Fetch demonstrations by ID list (for optimizer config loading).
    pub async fn fetch_dspy_demonstrations_by_ids(
        &self,
        ids: &[i64],
    ) -> anyhow::Result<Vec<DspyDemonstrationRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.pool.get_conn().await?;
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, signature_name, input_summary, output, score, source, \
             source_trace_id, active, promoted_by, created_at \
             FROM dspy_demonstrations WHERE id IN ({})",
            placeholders
        );
        let params: Vec<mysql_async::Value> =
            ids.iter().map(|&id| mysql_async::Value::from(id)).collect();
        let rows: Vec<mysql_async::Row> = conn.exec(&sql, params).await?;
        let mut out = Vec::new();
        for r in rows {
            out.push(DspyDemonstrationRow {
                id: r.get("id").unwrap_or(0),
                signature_name: r.get("signature_name").unwrap_or_default(),
                input_summary: r.get("input_summary").unwrap_or_default(),
                output: r.get("output").unwrap_or_default(),
                score: r.get("score").unwrap_or(0.0),
                source: r.get("source").unwrap_or_default(),
                source_trace_id: r.get("source_trace_id"),
                active: r.get("active").unwrap_or(false),
                promoted_by: r.get("promoted_by"),
                created_at: r.get("created_at").unwrap_or(0),
            });
        }
        Ok(out)
    }

    /// Save an optimized config and deactivate previous active configs for this signature.
    pub async fn save_dspy_optimized_config(
        &self,
        row: &DspyOptimizedConfigRow,
    ) -> anyhow::Result<i64> {
        let mut conn = self.pool.get_conn().await?;
        // Deactivate previous active configs for this signature
        conn.exec_drop(
            "UPDATE dspy_optimized_configs SET active = FALSE WHERE signature_name = ? AND active = TRUE",
            (&row.signature_name,),
        ).await?;
        let demo_ids_json =
            serde_json::to_string(&row.demo_ids).unwrap_or_else(|_| "[]".to_string());
        conn.exec_drop(
            "INSERT INTO dspy_optimized_configs \
             (signature_name, system_text, prompt_template, demo_ids, demo_count, \
              eval_score, eval_set_size, trials_run, version, created_at, active) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, TRUE)",
            (
                &row.signature_name,
                &row.system_text,
                &row.prompt_template,
                &demo_ids_json,
                row.demo_count,
                row.eval_score,
                row.eval_set_size,
                row.trials_run,
                row.version,
                row.created_at,
            ),
        )
        .await?;
        let id: i64 = conn
            .query_first("SELECT LAST_INSERT_ID()")
            .await?
            .unwrap_or(0);
        Ok(id)
    }

    /// Load the active optimized config for a signature (if any).
    pub async fn load_dspy_optimized_config(
        &self,
        signature_name: &str,
    ) -> anyhow::Result<Option<DspyOptimizedConfigRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT id, signature_name, system_text, prompt_template, demo_ids, \
             demo_count, eval_score, eval_set_size, trials_run, version, created_at, active \
             FROM dspy_optimized_configs \
             WHERE signature_name = ? AND active = TRUE \
             LIMIT 1",
                (signature_name,),
            )
            .await?;
        if let Some(r) = rows.into_iter().next() {
            let demo_ids_raw: String = r.get("demo_ids").unwrap_or_else(|| "[]".to_string());
            Ok(Some(DspyOptimizedConfigRow {
                id: r.get("id").unwrap_or(0),
                signature_name: r.get("signature_name").unwrap_or_default(),
                system_text: r.get("system_text").unwrap_or_default(),
                prompt_template: r.get("prompt_template").unwrap_or_default(),
                demo_ids: serde_json::from_str(&demo_ids_raw).unwrap_or_default(),
                demo_count: r.get("demo_count").unwrap_or(0),
                eval_score: r.get("eval_score").unwrap_or(0.0),
                eval_set_size: r.get("eval_set_size").unwrap_or(0),
                trials_run: r.get("trials_run").unwrap_or(0),
                version: r.get("version").unwrap_or(1),
                created_at: r.get("created_at").unwrap_or(0),
                active: r.get("active").unwrap_or(false),
            }))
        } else {
            Ok(None)
        }
    }

    /// List all signature names that have traces (for optimization scheduling).
    pub async fn list_dspy_signature_names(&self) -> anyhow::Result<Vec<(String, u64)>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn.query(
            "SELECT signature_name, COUNT(*) as cnt FROM dspy_traces GROUP BY signature_name ORDER BY cnt DESC",
        ).await?;
        let mut out = Vec::new();
        for r in rows {
            let name: String = r.get("signature_name").unwrap_or_default();
            let cnt: u64 = r.get("cnt").unwrap_or(0);
            out.push((name, cnt));
        }
        Ok(out)
    }

    pub async fn fetch_skill_evidence(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<SkillEvidenceRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT skill_id, msg_id, edge_id, outcome, created_at \
             FROM skill_evidence ORDER BY created_at DESC LIMIT ?",
                (limit as u64,),
            )
            .await?;
        let mut out = Vec::new();
        for mut row in rows {
            let skill_id: String = row.take("skill_id").unwrap_or_default();
            let msg_id: String = row.take("msg_id").unwrap_or_default();
            let edge_id: i64 = row.take("edge_id").unwrap_or(-1);
            let outcome: Option<String> = row.take("outcome");
            let created_at: u64 = row.take("created_at").unwrap_or(0);
            out.push(SkillEvidenceRow {
                skill_id,
                msg_id,
                edge_id,
                outcome,
                created_at,
            });
        }
        Ok(out)
    }

    pub async fn fetch_reward_logs(&self, limit: usize) -> anyhow::Result<Vec<RewardLogRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT tick, agent_id, reward, source, created_at \
             FROM reward_logs ORDER BY created_at DESC LIMIT ?",
                (limit as u64,),
            )
            .await?;
        let mut out = Vec::new();
        for mut row in rows {
            let tick: u64 = row.take("tick").unwrap_or(0);
            let agent_id: u64 = row.take("agent_id").unwrap_or(0);
            let reward: f64 = row.take("reward").unwrap_or(0.0);
            let source: String = row.take("source").unwrap_or_else(|| "unknown".to_string());
            let created_at: u64 = row.take("created_at").unwrap_or(0);
            out.push(RewardLogRow {
                tick,
                agent_id,
                reward,
                source,
                created_at,
            });
        }
        Ok(out)
    }

    // === OUROBOROS COMPATIBILITY AUDITS ===

    pub async fn insert_ouroboros_gate_audit(
        &self,
        row: &OuroborosGateAuditRow,
    ) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        conn.exec_drop(
            "INSERT INTO ouroboros_gate_audits \
             (action_id, action_kind, risk_level, policy_decision, council_required, council_mode, approved, reason, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                &row.action_id,
                &row.action_kind,
                &row.risk_level,
                &row.policy_decision,
                row.council_required,
                &row.council_mode,
                row.approved,
                &row.reason,
                row.created_at,
            ),
        )
        .await?;
        Ok(())
    }

    pub async fn fetch_ouroboros_gate_audits(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<OuroborosGateAuditRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT action_id, action_kind, risk_level, policy_decision, council_required, \
                 council_mode, approved, reason, created_at \
                 FROM ouroboros_gate_audits ORDER BY created_at DESC LIMIT ?",
                (limit as u64,),
            )
            .await?;

        let mut out = Vec::with_capacity(rows.len());
        for mut r in rows {
            out.push(OuroborosGateAuditRow {
                action_id: r.take("action_id").unwrap_or_default(),
                action_kind: r.take("action_kind").unwrap_or_default(),
                risk_level: r.take("risk_level").unwrap_or_default(),
                policy_decision: r.take("policy_decision").unwrap_or_default(),
                council_required: r.take("council_required").unwrap_or(false),
                council_mode: r.take("council_mode"),
                approved: r.take("approved").unwrap_or(false),
                reason: r.take("reason"),
                created_at: r.take("created_at").unwrap_or(0),
            });
        }
        Ok(out)
    }

    pub async fn insert_ouroboros_memory_event(
        &self,
        row: &OuroborosMemoryEventRow,
    ) -> anyhow::Result<()> {
        let mut conn = self.pool.get_conn().await?;
        conn.exec_drop(
            "INSERT IGNORE INTO ouroboros_memory_events \
             (event_id, event_kind, payload, created_at) VALUES (?, ?, ?, ?)",
            (&row.event_id, &row.event_kind, &row.payload, row.created_at),
        )
        .await?;
        Ok(())
    }

    pub async fn fetch_ouroboros_memory_events(
        &self,
        limit: usize,
    ) -> anyhow::Result<Vec<OuroborosMemoryEventRow>> {
        let mut conn = self.pool.get_conn().await?;
        let rows: Vec<mysql_async::Row> = conn
            .exec(
                "SELECT event_id, event_kind, payload, created_at \
                 FROM ouroboros_memory_events ORDER BY created_at DESC LIMIT ?",
                (limit as u64,),
            )
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for mut r in rows {
            out.push(OuroborosMemoryEventRow {
                event_id: r.take("event_id").unwrap_or_default(),
                event_kind: r.take("event_kind").unwrap_or_default(),
                payload: r.take("payload").unwrap_or_default(),
                created_at: r.take("created_at").unwrap_or(0),
            });
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct VaultEmbeddingRow {
    pub note_id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub path: String,
    pub preview: String,
    pub content_hash: String,
    pub embedding: Vec<f32>,
    pub metadata: serde_json::Value,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRow {
    pub skill_id: String,
    pub title: String,
    pub principle: String,
    pub level: String,
    pub role: Option<String>,
    pub task: Option<String>,
    pub confidence: f64,
    pub usage_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEvidenceRow {
    pub skill_id: String,
    pub msg_id: String,
    pub edge_id: i64,
    pub outcome: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRow {
    pub msg_id: String,
    pub sender: u64,
    pub target: String,
    pub kind: String,
    pub content: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilClaimRow {
    pub question: String,
    pub claim: String,
    pub evidence_msgs: Vec<String>,
    pub evidence_edges: Vec<usize>,
    pub confidence: f64,
    pub coverage: f64,
    pub mode: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardLogRow {
    pub tick: u64,
    pub agent_id: u64,
    pub reward: f64,
    pub source: String,
    pub created_at: u64,
}

/// Plan step persisted for audit/replay of council evidence chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepRow {
    pub step_index: usize,
    pub claim: String,
    pub plan_text: String,
    pub evidence_msg_ids: Vec<String>,
    pub qmd_ids: Vec<String>,
    pub skill_ref_ids: Vec<String>,
    pub has_task_msg: bool,
    pub workflow_msg_id: Option<String>,
    pub created_at: u64,
}

/// Skill hire record — one edge in the recursive delegation tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillHireRow {
    pub hire_id: String,
    pub parent_skill_id: String,
    pub child_skill_id: String,
    pub plan_step_index: usize,
    pub subproblem: String,
    pub subproblem_domains: Vec<String>,
    pub skill_briefing: Vec<String>,
    pub signature_id: String,
    pub signature_claim: String,
    pub signature_evidence: Vec<String>,
    pub parent_sig_id: Option<String>,
    pub depth: u8,
    pub budget: f64,
    pub status: String,
    pub outcome_score: Option<f64>,
    pub created_at: u64,
    pub completed_at: Option<u64>,
}

// ─── DSPy Optimizer Row Types ───

/// A single execution trace of a DSPy signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DspyTraceRow {
    pub id: i64,
    pub signature_name: String,
    pub input_question: String,
    pub input_context_hash: String,
    pub output: String,
    pub score: f64,
    pub semantic_ok: bool,
    pub repair_count: i32,
    pub model: String,
    pub latency_ms: i32,
    pub created_at: u64,
    /// Short slug from heuristic or manual tagging (e.g. `format`, `empty_output`).
    pub failure_code: String,
    /// One-line “why this failed” for GEPA clustering (local-only; may be redacted in bundles).
    pub failure_detail: String,
    /// JSON blob with numeric signals (score, repairs, lens, etc.).
    pub signals_json: String,
}

/// A curated demonstration (few-shot example) for a signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DspyDemonstrationRow {
    pub id: i64,
    pub signature_name: String,
    pub input_summary: String,
    pub output: String,
    pub score: f64,
    pub source: String,
    pub source_trace_id: Option<i64>,
    pub active: bool,
    pub promoted_by: Option<String>,
    pub created_at: u64,
}

/// An optimized configuration for a signature (system text + prompt + demo selection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DspyOptimizedConfigRow {
    pub id: i64,
    pub signature_name: String,
    pub system_text: String,
    pub prompt_template: String,
    pub demo_ids: Vec<i64>,
    pub demo_count: i32,
    pub eval_score: f64,
    pub eval_set_size: i32,
    pub trials_run: i32,
    pub version: i32,
    pub created_at: u64,
    pub active: bool,
}

/// Row type for code agent session queries.
#[derive(Debug, Clone)]
pub struct CodeAgentSessionRow {
    pub session_id: String,
    pub query: String,
    pub model: String,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub quality_score: Option<f64>,
    pub turn_count: i32,
    pub status: String,
    pub working_dir: String,
}

/// Row type for code agent messages.
#[derive(Debug, Clone)]
pub struct CodeAgentMessageRow {
    pub turn_number: i32,
    pub role: String,
    pub content: String,
    pub timestamp: u64,
    pub has_tool_calls: bool,
}

/// Row type for code agent tool calls.
#[derive(Debug, Clone)]
pub struct CodeAgentToolCallRow {
    pub turn_number: i32,
    pub tool_name: String,
    pub arguments: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub execution_time_ms: u64,
    pub executed_at: u64,
    pub file_path: Option<String>,
}

/// Gate audit row for Ouroboros compatibility decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OuroborosGateAuditRow {
    pub action_id: String,
    pub action_kind: String,
    pub risk_level: String,
    pub policy_decision: String,
    pub council_required: bool,
    pub council_mode: Option<String>,
    pub approved: bool,
    pub reason: Option<String>,
    pub created_at: u64,
}

/// Persisted event for event-sourced memory projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OuroborosMemoryEventRow {
    pub event_id: String,
    pub event_kind: String,
    pub payload: String,
    pub created_at: u64,
}
