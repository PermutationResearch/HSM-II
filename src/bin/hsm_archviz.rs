//! Print the HSM-II architecture blueprint as Markdown or JSON.
//!
//! Source of truth: `architecture/hsm-ii-blueprint.ron` (also embedded in the library).
//!
//! Examples (run from repo root — directory with `Cargo.toml`):
//! - `cargo run -q --bin hsm_archviz` — Markdown to stdout
//! - `cargo run -q --bin hsm_archviz -- --live` — Markdown + stats from embedded graph store (if present)
//! - `cargo run -q --bin hsm_archviz -- json | jq .`
//! - `cargo run -q --bin hsm_archviz -- json --live | jq .` — same shape as `GET /api/architecture`
//! - `./target/debug/hsm_archviz json` — after `cargo build --bin hsm_archviz`
//! - `cargo run -q --bin hsm_archviz -- -f ./architecture/hsm-ii-blueprint.ron markdown`

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use hyper_stigmergy::architecture_blueprint::{
    blueprint_markdown, blueprint_markdown_with_runtime, embedded_blueprint,
    load_blueprint_from_path,
};
use hyper_stigmergy::HyperStigmergicMorphogenesis;

#[derive(Parser)]
#[command(
    name = "hsm_archviz",
    about = "HSM-II architecture blueprint → Markdown / JSON"
)]
struct Cli {
    /// Use this `.ron` file instead of the embedded blueprint
    #[arg(short = 'f', long = "file")]
    blueprint: Option<PathBuf>,

    /// Load world from embedded graph store and append runtime stats (Markdown) or `{ blueprint, runtime }` (JSON)
    #[arg(short, long, global = true)]
    live: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Full Markdown report with tables and Mermaid overview (default)
    Markdown,
    /// Pretty-printed JSON (`--live`: same envelope as GET /api/architecture)
    Json,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let bp = match &cli.blueprint {
        Some(p) => load_blueprint_from_path(p)?,
        None => embedded_blueprint(),
    };

    match cli.command.unwrap_or(Commands::Markdown) {
        Commands::Markdown => {
            if cli.live {
                match HyperStigmergicMorphogenesis::load_from_disk() {
                    Ok((world, _)) => {
                        let rt = world.architecture_stats();
                        print!("{}", blueprint_markdown_with_runtime(&bp, &rt));
                    }
                    Err(e) => {
                        eprintln!(
                            "hsm_archviz: --live: no world loaded ({e}); printing blueprint only."
                        );
                        print!("{}", blueprint_markdown(&bp));
                    }
                }
            } else {
                print!("{}", blueprint_markdown(&bp));
            }
        }
        Commands::Json => {
            if cli.live {
                match HyperStigmergicMorphogenesis::load_from_disk() {
                    Ok((world, _)) => {
                        let envelope = serde_json::json!({
                            "blueprint": bp,
                            "runtime": world.architecture_stats(),
                        });
                        println!("{}", serde_json::to_string_pretty(&envelope)?);
                    }
                    Err(e) => {
                        eprintln!("hsm_archviz: --live: no world loaded ({e}); runtime is null.");
                        let envelope = serde_json::json!({
                            "blueprint": bp,
                            "runtime": serde_json::Value::Null,
                        });
                        println!("{}", serde_json::to_string_pretty(&envelope)?);
                    }
                }
            } else {
                println!("{}", serde_json::to_string_pretty(&bp)?);
            }
        }
    }
    Ok(())
}
