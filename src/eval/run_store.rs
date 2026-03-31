//! SQLite-backed index for `runs_index.jsonl` — query past runs, **deduped ingests**, **FTS** search.

use std::path::Path;

use anyhow::Context;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Stable hex digest of a JSONL line (dedupe key).
pub fn hash_index_line(line: &str) -> String {
    let mut h = Sha256::new();
    h.update(line.trim().as_bytes());
    format!("{:x}", h.finalize())
}

/// One row in the run store (denormalized from JSONL + optional `objective_score` etc.).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRow {
    pub id: i64,
    pub run_dir: Option<String>,
    pub created_unix: Option<u64>,
    pub harness: Option<String>,
    pub best_candidate: Option<String>,
    pub objective_score: Option<f64>,
    pub keyword_delta: Option<f64>,
    pub git_commit: Option<String>,
    pub raw_json: String,
}

pub fn open_run_store(db_path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path).with_context(|| format!("open {}", db_path.display()))?;
    ensure_schema(&conn)?;
    Ok(conn)
}

fn table_has_column(conn: &Connection, table: &str, col: &str) -> rusqlite::Result<bool> {
    let pragma = format!("PRAGMA table_info({})", table);
    let mut stmt = conn.prepare(&pragma)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == col {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_dir TEXT,
            created_unix INTEGER,
            harness TEXT,
            best_candidate TEXT,
            objective_score REAL,
            keyword_delta REAL,
            git_commit TEXT,
            raw_json TEXT NOT NULL,
            content_hash TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_runs_harness ON runs(harness);
        CREATE INDEX IF NOT EXISTS idx_runs_created ON runs(created_unix);
        "#,
    )?;
    if !table_has_column(conn, "runs", "content_hash")? {
        conn.execute("ALTER TABLE runs ADD COLUMN content_hash TEXT", [])?;
    }
    conn.execute_batch(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_runs_content_hash
            ON runs(content_hash) WHERE content_hash IS NOT NULL;
        "#,
    )?;

    conn.execute_batch(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS runs_fts USING fts5(
            run_id UNINDEXED,
            body,
            tokenize = 'porter unicode61'
        );
        "#,
    )?;

    conn.execute_batch(
        r#"
        DROP TRIGGER IF EXISTS runs_ai_fts;
        CREATE TRIGGER runs_ai_fts AFTER INSERT ON runs
        WHEN new.content_hash IS NOT NULL
        BEGIN
            INSERT INTO runs_fts(rowid, run_id, body)
            VALUES (
                new.id,
                CAST(new.id AS TEXT),
                coalesce(new.run_dir,'') || ' ' || coalesce(new.harness,'') || ' ' ||
                coalesce(new.best_candidate,'') || ' ' || coalesce(new.git_commit,'') || ' ' ||
                coalesce(new.raw_json,'')
            );
        END;
        "#,
    )?;

    Ok(())
}

fn fts_body(
    run_dir: &Option<String>,
    harness: &Option<String>,
    best_candidate: &Option<String>,
    git_commit: &Option<String>,
    raw_json: &str,
) -> String {
    format!(
        "{} {} {} {} {}",
        run_dir.as_deref().unwrap_or(""),
        harness.as_deref().unwrap_or(""),
        best_candidate.as_deref().unwrap_or(""),
        git_commit.as_deref().unwrap_or(""),
        raw_json
    )
}

/// Insert one JSONL line; returns `true` if a new row was added (false = duplicate hash).
pub fn insert_index_jsonl_line(conn: &Connection, line: &str) -> anyhow::Result<bool> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(false);
    }
    let hash = hash_index_line(line);
    let v: serde_json::Value = serde_json::from_str(line).context("JSONL line parse")?;
    let run_dir = v.get("run_dir").and_then(|x| x.as_str()).map(String::from);
    let created_unix = v
        .get("created_unix")
        .and_then(|x| x.as_u64())
        .or_else(|| v.get("created_unix").and_then(|x| x.as_i64().map(|i| i as u64)));
    let harness = v.get("harness").and_then(|x| x.as_str()).map(String::from);
    let best_candidate = v
        .get("best_candidate")
        .and_then(|x| x.as_str())
        .map(String::from);
    let objective_score = v.get("objective_score").and_then(|x| x.as_f64());
    let keyword_delta = v.get("keyword_delta").and_then(|x| x.as_f64());
    let git_commit = v.get("git_commit").and_then(|x| x.as_str()).map(String::from);
    let raw = line.to_string();

    conn.execute(
        "INSERT OR IGNORE INTO runs (run_dir, created_unix, harness, best_candidate, objective_score, keyword_delta, git_commit, raw_json, content_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            run_dir,
            created_unix.map(|u| u as i64),
            harness,
            best_candidate,
            objective_score,
            keyword_delta,
            git_commit,
            raw,
            hash,
        ],
    )?;
    Ok(conn.changes() > 0)
}

/// Open DB and insert one index line (for `HSM_RUNS_SQLITE` after append).
pub fn sync_index_line_to_sqlite(db_path: &Path, line: &serde_json::Value) -> anyhow::Result<bool> {
    let s = serde_json::to_string(line)?;
    let conn = open_run_store(db_path)?;
    insert_index_jsonl_line(&conn, &s)
}

/// Append non-empty JSONL lines; **skips duplicates** (same line bytes as before).
pub fn ingest_jsonl(conn: &Connection, jsonl_path: &Path) -> anyhow::Result<(usize, usize)> {
    let text = std::fs::read_to_string(jsonl_path)
        .with_context(|| format!("read {}", jsonl_path.display()))?;
    let mut seen = 0usize;
    let mut inserted = 0usize;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        seen += 1;
        if insert_index_jsonl_line(conn, line)? {
            inserted += 1;
        }
    }
    Ok((seen, inserted))
}

/// Wipe `runs` and `runs_fts` (triggers do not maintain FTS on bulk delete).
pub fn clear_runs(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM runs_fts", [])?;
    conn.execute("DELETE FROM runs", [])
}

/// Rebuild `runs_fts` from `runs` (e.g. after adding FTS to an old DB).
pub fn rebuild_fts(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM runs_fts", [])?;
    let mut stmt = conn.prepare(
        "SELECT id, run_dir, harness, best_candidate, git_commit, raw_json FROM runs WHERE content_hash IS NOT NULL",
    )?;
    let mut rows = stmt.query([])?;
    let mut n = 0usize;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let run_dir: Option<String> = row.get(1)?;
        let harness: Option<String> = row.get(2)?;
        let best_candidate: Option<String> = row.get(3)?;
        let git_commit: Option<String> = row.get(4)?;
        let raw_json: String = row.get(5)?;
        let body = fts_body(&run_dir, &harness, &best_candidate, &git_commit, &raw_json);
        conn.execute(
            "INSERT INTO runs_fts(rowid, run_id, body) VALUES (?1, ?2, ?3)",
            params![id, id.to_string(), body],
        )?;
        n += 1;
    }
    Ok(n)
}

fn map_run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RunRow> {
    Ok(RunRow {
        id: r.get(0)?,
        run_dir: r.get(1)?,
        created_unix: r.get::<_, Option<i64>>(2)?.map(|i| i as u64),
        harness: r.get(3)?,
        best_candidate: r.get(4)?,
        objective_score: r.get(5)?,
        keyword_delta: r.get(6)?,
        git_commit: r.get(7)?,
        raw_json: r.get(8)?,
    })
}

pub fn query_recent(conn: &Connection, limit: usize) -> anyhow::Result<Vec<RunRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_dir, created_unix, harness, best_candidate, objective_score, keyword_delta, git_commit, raw_json
         FROM runs ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit as i64], map_run_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn query_by_harness(
    conn: &Connection,
    harness: &str,
    limit: usize,
) -> anyhow::Result<Vec<RunRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_dir, created_unix, harness, best_candidate, objective_score, keyword_delta, git_commit, raw_json
         FROM runs WHERE harness = ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![harness, limit as i64], map_run_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn query_best_objective(
    conn: &Connection,
    min_objective: f64,
    limit: usize,
) -> anyhow::Result<Vec<RunRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_dir, created_unix, harness, best_candidate, objective_score, keyword_delta, git_commit, raw_json
         FROM runs WHERE objective_score IS NOT NULL AND objective_score >= ?1
         ORDER BY objective_score DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![min_objective, limit as i64], map_run_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn query_by_run_dir_contains(
    conn: &Connection,
    needle: &str,
    limit: usize,
) -> anyhow::Result<Vec<RunRow>> {
    let pat = format!("%{}%", needle);
    let mut stmt = conn.prepare(
        "SELECT id, run_dir, created_unix, harness, best_candidate, objective_score, keyword_delta, git_commit, raw_json
         FROM runs WHERE run_dir LIKE ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![pat, limit as i64], map_run_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// [FTS5](https://www.sqlite.org/fts5.html) over indexed `body` (denormalized fields + raw JSON).
/// `query` uses FTS syntax (e.g. `meta AND harness`, `"phrase"`).
pub fn search_fts(conn: &Connection, query: &str, limit: usize) -> anyhow::Result<Vec<RunRow>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.run_dir, r.created_unix, r.harness, r.best_candidate, r.objective_score, r.keyword_delta, r.git_commit, r.raw_json
         FROM runs_fts f
         INNER JOIN runs r ON r.id = f.rowid
         WHERE f.body MATCH ?1
         ORDER BY r.id DESC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![query, limit as i64], map_run_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn row_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))
}
