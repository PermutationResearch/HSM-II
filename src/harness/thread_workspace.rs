//! Per-thread workspace roots for tool FS isolation (long-horizon phase 4).
//!
//! When enabled (`HSM_THREAD_WORKSPACE=1`), relative paths and `.` in file/shell tools resolve under
//! `<appliance_home>/workspaces/<sanitized_thread_id>/`. Absolute paths are only allowed if they
//! stay under that directory after lexical normalization.
//!
//! The active workspace is process-global (single-flight personal agent assumption). HTTP uploads
//! use the same layout via [`crate::harness::thread_workspace::workspace_dirs`].

use std::path::{Component, Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use sha2::{Digest, Sha256};

/// Root for workspaces + default skill install target (override with `HSM_APPLIANCE_HOME`).
pub fn appliance_home() -> PathBuf {
    std::env::var("HSM_APPLIANCE_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".hsmii")))
        .unwrap_or_else(|| PathBuf::from(".hsmii"))
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let s = v.trim();
            s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// When true, [`activate_thread_workspace`] and path resolution apply.
pub fn thread_workspace_enabled() -> bool {
    env_truthy("HSM_THREAD_WORKSPACE")
}

static ACTIVE_ROOT: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn active_slot() -> &'static RwLock<Option<PathBuf>> {
    ACTIVE_ROOT.get_or_init(|| RwLock::new(None))
}

/// `(workspace_root, uploads_dir, artifacts_dir)` for a thread id (no I/O).
pub fn workspace_dirs(appliance_home: &Path, thread_id: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = appliance_home
        .join("workspaces")
        .join(sanitize_thread_id(thread_id));
    (root.clone(), root.join("uploads"), root.join("artifacts"))
}

pub fn sanitize_thread_id(raw: &str) -> String {
    let mut s = String::new();
    for ch in raw.chars().take(128) {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => s.push(ch),
            _ => s.push('_'),
        }
    }
    if s.is_empty() {
        s = "default".into();
    }
    // Preserve readability while avoiding collisions from normalization.
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{s}-{}", &digest[..10])
}

/// Ensure workspace + `uploads/` + `artifacts/` exist. Returns the workspace root.
pub async fn ensure_thread_workspace_on_disk(
    appliance_home: &Path,
    thread_id: &str,
) -> std::io::Result<PathBuf> {
    let (root, uploads, artifacts) = workspace_dirs(appliance_home, thread_id);
    tokio::fs::create_dir_all(&uploads).await?;
    tokio::fs::create_dir_all(&artifacts).await?;
    Ok(root)
}

/// Set the process-global active workspace root (used by tools). No-op if disabled by env.
pub fn activate_thread_workspace(thread_id: &str) -> std::io::Result<()> {
    if !thread_workspace_enabled() {
        return Ok(());
    }
    let home = appliance_home();
    let (root, uploads, artifacts) = workspace_dirs(&home, thread_id);
    std::fs::create_dir_all(&uploads)?;
    std::fs::create_dir_all(&artifacts)?;
    *active_slot().write().expect("workspace lock poisoned") = Some(root);
    Ok(())
}

pub fn deactivate_thread_workspace() {
    if let Ok(mut g) = active_slot().write() {
        *g = None;
    }
}

pub fn current_root() -> Option<PathBuf> {
    active_slot().read().ok().and_then(|g| g.clone())
}

fn lexical_normalize(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(p) => out.push(Component::Prefix(p)),
            Component::RootDir => {
                out.push(Component::RootDir);
            }
            Component::Normal(n) => out.push(n),
        }
    }
    out
}

/// Returns true if `candidate` is `base` or a path inside it (lexical, no symlink chase).
fn is_subpath(base: &Path, candidate: &Path) -> bool {
    let mut bc = base.components();
    let mut cc = candidate.components();
    loop {
        match (bc.next(), cc.next()) {
            (None, None) => return true,
            (None, Some(_)) => return true,
            (Some(_), None) => return false,
            (Some(a), Some(b)) if a == b => continue,
            _ => return false,
        }
    }
}

/// Resolve a user-supplied path for sandboxed tools. When no workspace is active, returns the path as-is.
pub fn resolve_tool_fs_path(user_path: &str) -> Result<PathBuf, String> {
    let t = user_path.trim();
    if t.is_empty() {
        return Err("path is empty".into());
    }

    let Some(base) = current_root() else {
        if env_truthy("HSM_THREAD_WORKSPACE_STRICT") {
            return Err(
                "thread workspace strict mode is enabled but no active workspace is set".into(),
            );
        }
        return Ok(PathBuf::from(t));
    };

    let candidate = if t == "." {
        base.clone()
    } else if Path::new(t).is_absolute() {
        lexical_normalize(PathBuf::from(t))
    } else {
        lexical_normalize(base.join(t))
    };

    let base_norm = lexical_normalize(base);
    if !is_subpath(&base_norm, &candidate) {
        return Err(format!(
            "path escapes thread workspace (allowed under {})",
            base_norm.display()
        ));
    }

    Ok(candidate)
}

/// Clears harness tool context + workspace on drop (personal agent single turn).
///
/// Stores the registry address as `usize` so the enclosing async `handle_message` future stays
/// [`Send`] (required by `tokio::spawn` callers). The address is only dereferenced on [`Drop`]
/// before `handle_message` returns, while `registry` remains live.
pub struct HarnessTurnCleanup {
    tr: usize,
}

impl HarnessTurnCleanup {
    pub fn new(registry: &mut crate::tools::ToolRegistry) -> Self {
        Self {
            tr: registry as *mut crate::tools::ToolRegistry as usize,
        }
    }
}

impl Drop for HarnessTurnCleanup {
    fn drop(&mut self) {
        let tr = self.tr as *mut crate::tools::ToolRegistry;
        // SAFETY: see struct note — same as `new`, valid until end of `handle_message`.
        unsafe {
            (*tr).set_harness_context(None);
        }
        deactivate_thread_workspace();
    }
}
