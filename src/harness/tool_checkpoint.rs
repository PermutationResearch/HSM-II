//! Optional JSONL checkpoints for each tool step (`HSM_HARNESS_TOOL_CHECKPOINT_DIR`).

use std::path::Path;

use serde_json::Value;
use tokio::io::AsyncWriteExt;

/// Append one JSON object as a line (async, creates parent dirs).
pub async fn append_tool_checkpoint(dir: impl AsRef<Path>, row: &Value) -> std::io::Result<()> {
    let dir = dir.as_ref();
    tokio::fs::create_dir_all(dir).await?;
    let path = dir.join("tool_calls.jsonl");
    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    let line = serde_json::to_string(row)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    f.write_all(line.as_bytes()).await?;
    f.write_all(b"\n").await?;
    Ok(())
}
