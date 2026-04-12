use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct RawKbManifest {
    #[serde(default)]
    file: Vec<RawKbFileEntry>,
}

#[derive(Debug, Deserialize)]
struct RawKbFileEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Clone)]
pub struct KbFileEntry {
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct KbManifestReport {
    pub manifest_path: PathBuf,
    pub files: Vec<KbFileEntry>,
    pub existing_files: usize,
    pub missing_files: Vec<String>,
}

impl KbManifestReport {
    pub fn status_line(&self) -> String {
        if self.files.is_empty() {
            return "KB assets loaded: 0".to_string();
        }
        if self.missing_files.is_empty() {
            return format!(
                "KB assets loaded: {} ({})",
                self.files.len(),
                self.files
                    .iter()
                    .map(|f| f.kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        format!(
            "KB assets loaded: {} total, {} present, {} missing",
            self.files.len(),
            self.existing_files,
            self.missing_files.len()
        )
    }
}

pub fn load_kb_manifest_report(base_path: &Path) -> Result<Option<KbManifestReport>> {
    let manifest_path = base_path.join("company-files").join("manifest.toml");
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("read kb manifest {}", manifest_path.display()))?;
    let parsed: RawKbManifest = toml::from_str(&raw)
        .with_context(|| format!("parse kb manifest {}", manifest_path.display()))?;

    let mut files = Vec::new();
    let mut missing = Vec::new();
    let mut present = 0usize;

    for entry in parsed.file {
        let rel = entry.path.trim().to_string();
        if rel.is_empty() {
            continue;
        }
        let abs = base_path.join("company-files").join(&rel);
        if abs.is_file() {
            present += 1;
        } else {
            missing.push(rel.clone());
        }
        files.push(KbFileEntry {
            path: rel,
            kind: entry.kind.trim().to_string(),
        });
    }

    Ok(Some(KbManifestReport {
        manifest_path,
        files,
        existing_files: present,
        missing_files: missing,
    }))
}
