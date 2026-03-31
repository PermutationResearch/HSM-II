//! Lightweight state migration runner for runtime artifacts.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

pub type MigrationFn = fn(&Path) -> Result<()>;

pub struct Migration {
    pub id: &'static str,
    pub run: MigrationFn,
}

pub struct MigrationRunner {
    state_dir: PathBuf,
    marker_path: PathBuf,
    migrations: Vec<Migration>,
}

impl MigrationRunner {
    pub fn new(state_dir: PathBuf) -> Self {
        let marker_path = state_dir.join("applied_migrations.json");
        Self {
            state_dir,
            marker_path,
            migrations: Vec::new(),
        }
    }

    pub fn register(mut self, migration: Migration) -> Self {
        self.migrations.push(migration);
        self
    }

    pub fn run_pending(&self) -> Result<Vec<String>> {
        fs::create_dir_all(&self.state_dir)?;
        let mut applied = self.load_applied()?;
        let mut ran = Vec::new();
        for m in &self.migrations {
            if applied.contains(m.id) {
                continue;
            }
            (m.run)(&self.state_dir)?;
            applied.insert(m.id.to_string());
            ran.push(m.id.to_string());
        }
        self.save_applied(&applied)?;
        Ok(ran)
    }

    fn load_applied(&self) -> Result<BTreeSet<String>> {
        if !self.marker_path.exists() {
            return Ok(BTreeSet::new());
        }
        let raw = fs::read_to_string(&self.marker_path)?;
        let v: Vec<String> = serde_json::from_str(&raw)?;
        Ok(v.into_iter().collect())
    }

    fn save_applied(&self, applied: &BTreeSet<String>) -> Result<()> {
        let v: Vec<String> = applied.iter().cloned().collect();
        fs::write(&self.marker_path, serde_json::to_vec_pretty(&v)?)?;
        Ok(())
    }
}
