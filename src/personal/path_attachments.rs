//! Expand `@/path/to/file` tokens (Cursor-style) into inlined markdown for prompts.

use anyhow::Result;
use regex::Regex;
use std::path::{Path, PathBuf};

const MAX_ATTACH_BYTES: usize = 64 * 1024;

fn looks_like_path_token(token: &str) -> bool {
    let t = token.trim();
    if t.is_empty() || t.len() > 2048 {
        return false;
    }
    if t.contains('@') && !t.starts_with('/') && !t.starts_with("./") && !t.starts_with("~/") {
        return false;
    }
    t.starts_with('/')
        || t.starts_with("./")
        || t.starts_with("../")
        || t.starts_with("~/")
        || t.contains('/')
        || t.ends_with(".md")
        || t.ends_with(".txt")
        || t.ends_with(".yaml")
        || t.ends_with(".yml")
        || t.ends_with(".eml")
}

fn resolve_against(token: &str, base: &Path) -> PathBuf {
    let t = token.trim();
    if t.starts_with("~/") {
        if let Some(h) = dirs::home_dir() {
            return h.join(&t[2..]);
        }
    }
    let p = Path::new(t);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// Replace each `@token` with an inlined `## Attached: …` block when the path exists and is a file.
pub async fn expand_at_paths(text: &str, base_dir: &Path) -> Result<String> {
    let re = Regex::new(r"@([^\s\n]+)").expect("regex");
    let mut out = text.to_string();
    let mut replacements: Vec<(String, String)> = Vec::new();

    for cap in re.captures_iter(text) {
        let full = cap.get(0).map(|m| m.as_str()).unwrap_or("");
        let token = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if !looks_like_path_token(token) {
            continue;
        }
        let path = resolve_against(token, base_dir);
        if !path.is_file() {
            continue;
        }
        let bytes = tokio::fs::read(&path).await?;
        let slice = if bytes.len() > MAX_ATTACH_BYTES {
            let mut n = MAX_ATTACH_BYTES;
            while n > 0 && std::str::from_utf8(&bytes[..n]).is_err() {
                n -= 1;
            }
            &bytes[..n]
        } else {
            &bytes[..]
        };
        let body = String::from_utf8_lossy(slice);
        let note = if bytes.len() > MAX_ATTACH_BYTES {
            "\n\n_(truncated to 64 KiB)_"
        } else {
            ""
        };
        let block = format!(
            "\n\n## Attached file: `{}`\n\n{}{}\n",
            path.display(),
            body,
            note
        );
        replacements.push((full.to_string(), block));
    }

    for (needle, block) in replacements {
        out = out.replace(&needle, &block);
    }

    Ok(out)
}
