//! Opt-in diagnostic bundle (ZIP) for support — versions, redacted environment, optional rustc info.
//!
//! Does not include chat logs, graph dumps, or API keys (values redacted heuristically).

use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use zip::write::{FileOptions, ZipWriter};
use zip::CompressionMethod;

#[derive(Parser, Debug)]
#[command(name = "hsm_diagnostic")]
struct Args {
    /// Output `.zip` path (default: `./hsm-diagnostic-<unix_ts>.zip` in cwd)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

fn should_redact_key(key: &str) -> bool {
    let u = key.to_uppercase();
    u.contains("SECRET")
        || u.contains("PASSWORD")
        || u.contains("TOKEN")
        || u.ends_with("_KEY")
        || u.contains("API_KEY")
        || u.contains("BEARER")
        || u.contains("PRIVATE")
        || u.contains("CREDENTIAL")
}

fn redacted_env_dump() -> String {
    let mut lines: Vec<String> = std::env::vars()
        .map(|(k, v)| {
            let v = if should_redact_key(&k) && !v.is_empty() {
                "<redacted>".to_string()
            } else {
                v
            };
            format!("{k}={v}")
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

fn main() -> Result<()> {
    let args = Args::parse();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out = args
        .output
        .unwrap_or_else(|| PathBuf::from(format!("hsm-diagnostic-{ts}.zip")));

    let file = std::fs::File::create(&out)?;
    let mut zip = ZipWriter::new(file);
    fn entry_opts() -> FileOptions<'static, ()> {
        FileOptions::<()>::default().compression_method(CompressionMethod::Deflated)
    }

    zip.start_file("hsm_version.txt", entry_opts())?;
    writeln!(zip, "{}", env!("CARGO_PKG_VERSION"))?;
    writeln!(zip, "{}", env!("CARGO_PKG_NAME"))?;

    zip.start_file("environment_redacted.txt", entry_opts())?;
    zip.write_all(redacted_env_dump().as_bytes())?;

    zip.start_file("rustc.txt", entry_opts())?;
    let rustc = std::process::Command::new("rustc")
        .arg("-V")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "(rustc not in PATH or failed)".to_string());
    zip.write_all(rustc.as_bytes())?;

    zip.start_file("readme.txt", entry_opts())?;
    zip.write_all(
        b"HSM-II diagnostic bundle (opt-in).\n\
\n\
This archive was generated locally. Review before sharing.\n\
Sensitive env var *values* are heuristically redacted; keys are preserved.\n\
\n\
See docs/ROADMAP.md for observability / telemetry phases.\n",
    )?;

    zip.finish()?;

    println!("Wrote {}", out.display());
    Ok(())
}
