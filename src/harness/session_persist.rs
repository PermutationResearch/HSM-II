//! Persist session-scoped JSONL under `<appliance_home>/sessions/<thread_id>/` (gap 10).

use std::path::{Path, PathBuf};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn sessions_root(home: &Path, thread_id: &str) -> PathBuf {
    home.join("sessions")
        .join(crate::harness::sanitize_thread_id(thread_id))
}

pub fn session_events_path(home: &Path, thread_id: &str) -> PathBuf {
    sessions_root(home, thread_id).join("events.jsonl")
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err("odd hex length".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

fn xor_obfuscate(json: &str, secret: &str) -> String {
    let k = secret.as_bytes();
    if k.is_empty() {
        return json.to_string();
    }
    let out: Vec<u8> = json
        .bytes()
        .enumerate()
        .map(|(i, b)| b ^ k[i % k.len()])
        .collect();
    hex_encode(&out)
}

fn xor_deobfuscate(hex_str: &str, secret: &str) -> Result<String, std::string::String> {
    let k = secret.as_bytes();
    if k.is_empty() {
        return Err("empty secret".into());
    }
    let raw = hex_decode(hex_str.trim()).map_err(|e| e.to_string())?;
    let s: Vec<u8> = raw
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ k[i % k.len()])
        .collect();
    String::from_utf8(s).map_err(|e| e.to_string())
}

/// Append one event line. If `HSM_SESSION_SECRET` is set, stores `{"obf":"<hex>"}` instead of raw JSON.
pub async fn append_session_event(
    home: &Path,
    thread_id: &str,
    mut value: Value,
) -> std::io::Result<()> {
    let dir = sessions_root(home, thread_id);
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join("events.jsonl");
    let secret = std::env::var("HSM_SESSION_SECRET").unwrap_or_default();
    let line = if !secret.trim().is_empty() {
        let raw = serde_json::to_string(&value)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        let obf = xor_obfuscate(&raw, secret.trim());
        value = serde_json::json!({ "obf": obf });
        serde_json::to_string(&value)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?
    } else {
        serde_json::to_string(&value)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?
    };
    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    f.write_all(line.as_bytes()).await?;
    f.write_all(b"\n").await?;
    Ok(())
}

/// Read up to `max_lines` from the tail of `events.jsonl` (best-effort parse per line).
pub async fn load_recent_session_events(
    home: &Path,
    thread_id: &str,
    max_lines: usize,
) -> Vec<Value> {
    let path = session_events_path(home, thread_id);
    let Ok(file) = tokio::fs::File::open(&path).await else {
        return Vec::new();
    };
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut buf: Vec<String> = Vec::new();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        buf.push(line);
        if buf.len() > max_lines {
            buf.remove(0);
        }
    }
    let secret = std::env::var("HSM_SESSION_SECRET").unwrap_or_default();
    let mut out = Vec::new();
    for line in buf {
        match serde_json::from_str::<Value>(&line) {
            Ok(v) => {
                if let Some(obf) = v.get("obf").and_then(|x| x.as_str()) {
                    if !secret.trim().is_empty() {
                        if let Ok(s) = xor_deobfuscate(obf, secret.trim()) {
                            if let Ok(parsed) = serde_json::from_str::<Value>(&s) {
                                out.push(parsed);
                                continue;
                            }
                        }
                    }
                    out.push(v);
                } else {
                    out.push(v);
                }
            }
            Err(_) => out.push(serde_json::Value::String(line)),
        }
    }
    out
}
