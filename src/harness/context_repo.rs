//! Context repository layout: structured agent-editable memory on disk, publishable into Company OS memory.
//!
//! **Company workspace** (`hsmii_home`): root is `context-repos/<session_key>/` with:
//! - `manifest.json`, `INDEX.md`, `notes/`, `snapshots/`
//!
//! **Thread workspace** (`HSM_THREAD_WORKSPACE`): root is `workspaces/<id>/context-repo/` with the same files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Contract version stored in `manifest.json` (`format_version` field).
pub const CONTEXT_REPO_FORMAT_VERSION: &str = "1";

pub const MANIFEST_FILE: &str = "manifest.json";
pub const INDEX_FILE: &str = "INDEX.md";
pub const NOTES_DIR: &str = "notes";
pub const SNAPSHOTS_DIR: &str = "snapshots";
pub const THREAD_REPO_DIR: &str = "context-repo";

/// Root for a company session: `<hsmii_home>/context-repos/<sanitized_session_key>/`.
pub fn repo_root_for_company_home(hsmii_home: &Path, session_key: &str) -> PathBuf {
    hsmii_home
        .join("context-repos")
        .join(sanitize_session_key(session_key))
}

/// Thread workspace: `<appliance_home>/workspaces/<sanitized_thread_id>/context-repo/`.
pub fn repo_root_for_thread(appliance_home: &Path, sanitized_thread_id: &str) -> PathBuf {
    super::thread_workspace::workspace_dirs(appliance_home, sanitized_thread_id)
        .0
        .join(THREAD_REPO_DIR)
}

/// Safe directory name for path segments.
pub fn sanitize_session_key(raw: &str) -> String {
    super::thread_workspace::sanitize_thread_id(raw)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRepoManifest {
    pub format_version: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub session_key: Option<String>,
    #[serde(default)]
    pub notes_globs: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl Default for ContextRepoManifest {
    fn default() -> Self {
        Self {
            format_version: CONTEXT_REPO_FORMAT_VERSION.to_string(),
            title: Some("Context repository".to_string()),
            session_key: None,
            notes_globs: vec!["notes/**/*.md".to_string(), "notes/*.md".to_string()],
            description: Some(
                "Structured long-horizon context: edit markdown under notes/, maintain INDEX.md, publish to Company OS memory when ready.".to_string(),
            ),
        }
    }
}

pub fn default_manifest_for_session(session_key: &str) -> ContextRepoManifest {
    let mut m = ContextRepoManifest::default();
    m.session_key = Some(session_key.to_string());
    m
}

pub fn expected_relative_paths() -> Vec<&'static str> {
    vec![MANIFEST_FILE, INDEX_FILE, NOTES_DIR, SNAPSHOTS_DIR]
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}
