//! Atomic file writes (write-temp + rename) to reduce torn config on crash.
//!
//! Used for user-facing config and metrics under `~/.hsmii/`.

use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Write `contents` to `path` atomically using a same-directory temp file and rename.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp.{}.{}",
        name,
        std::process::id(),
        std::time::UNIX_EPOCH
            .elapsed()
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    {
        let mut f = File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
    }

    replace_file(&tmp, path)?;
    Ok(())
}

#[cfg(unix)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

#[cfg(not(unix))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_file(to);
    std::fs::rename(from, to)
}
