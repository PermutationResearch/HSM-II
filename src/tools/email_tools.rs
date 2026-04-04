//! Email file tools: `.eml` RFC822 (including attachment / “paperclip” inventory) and Maildir.

use async_trait::async_trait;
use mailparse::{parse_mail, DispositionType};
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::{object_schema, Tool, ToolOutput};

const MAX_INLINE_ATTACHMENT_BYTES: usize = 48 * 1024;
const MAX_MAILDIR_LIST: usize = 50;

/// Shared RFC822 → prompt-friendly summary (headers, text parts, paperclip list).
pub(crate) fn summarize_eml_raw(raw: &[u8]) -> Result<String, String> {
    summarize_eml(raw)
}

fn summarize_eml(raw: &[u8]) -> Result<String, String> {
    let parsed = parse_mail(raw).map_err(|e| e.to_string())?;

    let mut out = String::from("## Headers (summary)\n");
    for h in parsed.headers.iter() {
        let key = h.get_key();
        if key.eq_ignore_ascii_case("content-type")
            || key.eq_ignore_ascii_case("mime-version")
            || key.eq_ignore_ascii_case("received")
        {
            continue;
        }
        let v = h.get_value();
        let line = format!("{}: {}\n", key, v);
        if out.len() + line.len() > 6000 {
            out.push_str("… (headers truncated)\n");
            break;
        }
        out.push_str(&line);
    }

    out.push_str("\n## Body (text parts)\n\n");
    collect_text_and_attachments(&parsed, &mut out, 0)?;

    Ok(out)
}

fn collect_text_and_attachments(
    part: &mailparse::ParsedMail<'_>,
    out: &mut String,
    depth: usize,
) -> Result<(), String> {
    if depth > 32 {
        return Err("MIME nesting too deep".into());
    }

    let ctype = part.ctype.mimetype.to_lowercase();

    if ctype.starts_with("multipart/") {
        for sub in &part.subparts {
            collect_text_and_attachments(sub, out, depth + 1)?;
        }
        return Ok(());
    }

    let disp = part.get_content_disposition();
    let is_attachment = matches!(disp.disposition, DispositionType::Attachment)
        || disp.params.contains_key("filename");

    if is_attachment
        || (ctype.starts_with("application/")
            && !ctype.contains("json")
            && !ctype.contains("x-www-form-urlencoded"))
    {
        let fname = disp
            .params
            .get("filename")
            .map(|s: &String| s.as_str())
            .unwrap_or("(no filename)");
        let raw_len = part.get_body_raw().unwrap_or_default().len();
        out.push_str(&format!(
            "\n📎 **Attachment (paperclip):** `{}` | `{}` | {} bytes\n",
            fname, ctype, raw_len
        ));

        if ctype.starts_with("text/") {
            if raw_len <= MAX_INLINE_ATTACHMENT_BYTES {
                match part.get_body() {
                    Ok(b) => {
                        let snippet: String = b.chars().take(8000).collect();
                        out.push_str("```\n");
                        out.push_str(&snippet);
                        if b.len() > 8000 {
                            out.push_str("\n… (truncated)");
                        }
                        out.push_str("\n```\n");
                    }
                    Err(e) => out.push_str(&format!("(could not decode body: {})\n", e)),
                }
            } else {
                out.push_str("(text attachment too large to inline)\n");
            }
        }
        return Ok(());
    }

    if ctype.starts_with("text/plain") || ctype.starts_with("text/html") {
        match part.get_body() {
            Ok(b) => {
                let cap = 24_000usize;
                let take: String = b.chars().take(cap).collect();
                out.push_str(&take);
                if b.len() > cap {
                    out.push_str("\n… (body truncated)\n");
                }
                out.push('\n');
            }
            Err(e) => out.push_str(&format!("(decode error: {})\n", e)),
        }
    } else if ctype == "message/rfc822" && !part.subparts.is_empty() {
        for sub in &part.subparts {
            collect_text_and_attachments(sub, out, depth + 1)?;
        }
    }

    Ok(())
}

// --- read_eml ---

pub struct ReadEmlTool;

impl ReadEmlTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReadEmlTool {
    fn name(&self) -> &str {
        "read_eml"
    }

    fn description(&self) -> &str {
        "Parse a .eml or raw RFC822 file: headers, text body parts, and attachment inventory (paperclip) with optional inline text for small attachments."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![(
            "path",
            "Absolute or relative path to .eml file",
            true,
        )])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() {
            return ToolOutput::error("path is required");
        }
        let p = Path::new(path);
        match tokio::fs::read(p).await {
            Ok(raw) => {
                if raw.len() > 20 * 1024 * 1024 {
                    return ToolOutput::error("file larger than 20 MiB; refuse");
                }
                match summarize_eml(&raw) {
                    Ok(s) => ToolOutput::success(s),
                    Err(e) => ToolOutput::error(e),
                }
            }
            Err(e) => ToolOutput::error(format!("read failed: {e}")),
        }
    }
}

// --- maildir_list ---

pub struct MaildirListTool;

impl MaildirListTool {
    pub fn new() -> Self {
        Self
    }
}

fn maildir_candidates(root: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for sub in ["cur", "new"] {
        let d = root.join(sub);
        if d.is_dir() {
            v.push(d);
        }
    }
    v
}

#[async_trait]
impl Tool for MaildirListTool {
    fn name(&self) -> &str {
        "maildir_list"
    }

    fn description(&self) -> &str {
        "List recent messages in a Maildir folder (scans `cur/` and `new/` under the given path)."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![
            (
                "maildir_root",
                "Path to Maildir (directory containing cur/ and/or new/)",
                true,
            ),
            ("limit", "Max entries (default 30, max 50)", false),
        ])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let root = params
            .get("maildir_root")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if root.is_empty() {
            return ToolOutput::error("maildir_root is required");
        }
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(30)
            .min(MAX_MAILDIR_LIST as u64) as usize;

        let root_path = Path::new(root);
        let dirs = maildir_candidates(root_path);
        if dirs.is_empty() {
            return ToolOutput::error(
                "no cur/ or new/ subdirectories found — path may not be a Maildir",
            );
        }

        let mut entries: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        for d in dirs {
            let mut rd = match tokio::fs::read_dir(&d).await {
                Ok(x) => x,
                Err(e) => return ToolOutput::error(format!("read_dir {}: {}", d.display(), e)),
            };
            while let Ok(Some(ent)) = rd.next_entry().await {
                let p = ent.path();
                if p.is_file() {
                    let mt = ent
                        .metadata()
                        .await
                        .map(|m| m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH))
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    entries.push((mt, p));
                }
            }
        }

        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries.truncate(limit);

        let mut out = String::from("## Maildir messages (newest first)\n\n");
        for (i, (_, p)) in entries.iter().enumerate() {
            out.push_str(&format!("{}. `{}`\n", i + 1, p.display()));
        }
        if entries.is_empty() {
            out.push_str("(no messages)\n");
        } else {
            out.push_str(
                "\nUse `maildir_read` with one of these paths to parse headers/body/attachments.\n",
            );
        }
        ToolOutput::success(out)
    }
}

// --- maildir_read ---

pub struct MaildirReadTool;

impl MaildirReadTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for MaildirReadTool {
    fn name(&self) -> &str {
        "maildir_read"
    }

    fn description(&self) -> &str {
        "Read one Maildir message file (same output shape as read_eml: paperclip / attachments listed)."
    }

    fn parameters_schema(&self) -> Value {
        object_schema(vec![(
            "path",
            "Full path to one file inside Maildir cur/ or new/",
            true,
        )])
    }

    async fn execute(&self, params: Value) -> ToolOutput {
        let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
        if path.is_empty() {
            return ToolOutput::error("path is required");
        }
        match tokio::fs::read(path).await {
            Ok(raw) => {
                if raw.len() > 20 * 1024 * 1024 {
                    return ToolOutput::error("message larger than 20 MiB");
                }
                match summarize_eml(&raw) {
                    Ok(s) => ToolOutput::success(s),
                    Err(e) => ToolOutput::error(e),
                }
            }
            Err(e) => ToolOutput::error(format!("read failed: {e}")),
        }
    }
}
