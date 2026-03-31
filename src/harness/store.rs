//! Append-only JSONL event log and optional checkpoint directory.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::events::HarnessEvent;

pub struct HarnessStore {
    log_path: PathBuf,
    checkpoint_dir: Option<PathBuf>,
}

impl HarnessStore {
    pub fn new(log_path: PathBuf, checkpoint_dir: Option<PathBuf>) -> io::Result<Self> {
        if let Some(ref d) = checkpoint_dir {
            fs::create_dir_all(d)?;
        }
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self {
            log_path,
            checkpoint_dir,
        })
    }

    pub fn append_event(&self, event: &HarnessEvent) -> io::Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;
        let line = serde_json::to_string(event)?;
        writeln!(f, "{}", line)?;
        Ok(())
    }

    pub fn write_checkpoint(&self, name: &str, payload: &[u8]) -> io::Result<()> {
        let Some(dir) = &self.checkpoint_dir else {
            return Ok(());
        };
        fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.json", sanitize_filename(name)));
        fs::write(path, payload)
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
