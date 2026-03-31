//! Local Hermes-style GEPA workflow for HSM-II DSPy:
//! - **collect** — Read low-scoring traces from RooDB, redact, cluster by failure code, write JSON (no remote API).
//! - **optimize** — Run [`hyper_stigmergy::dspy::optimize_signature`] with mutation order driven by a bundle (when provided).

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use ollama_rs::Ollama;

use hyper_stigmergy::database::{RooDb, RooDbConfig};
use hyper_stigmergy::dspy::{get_template_by_name, optimize_signature};
use hyper_stigmergy::gepa::{
    collect_bundle, load_bundle, mutation_style_names_from_bundle, save_bundle, GepaConfig,
};

#[derive(Parser)]
#[command(name = "hsm_gepa")]
struct Cli {
    /// RooDB URL: `mysql://user:pass@host:port/db` (also reads `HSM_ROO_DB_URL` if unset).
    #[arg(long)]
    roodb_url: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Aggregate failure traces into a redacted bundle JSON (`gepa-collect`-style).
    Collect {
        #[arg(long)]
        signature: String,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, short)]
        config: Option<PathBuf>,
    },
    /// Optimize a signature; pass `--bundle` to prioritize mutations from collected clusters.
    Optimize {
        #[arg(long)]
        signature: String,
        #[arg(long)]
        bundle: Option<PathBuf>,
        #[arg(long, default_value_t = 8)]
        trials: usize,
        #[arg(long, default_value = "http://localhost")]
        ollama_host: String,
        #[arg(long, default_value_t = 11434)]
        ollama_port: u16,
        #[arg(long, default_value = "llama3.2")]
        model: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let db_cfg = cli
        .roodb_url
        .or_else(|| std::env::var("HSM_ROO_DB_URL").ok())
        .filter(|s| !s.trim().is_empty())
        .map(|url| RooDbConfig::from_url(&url))
        .unwrap_or_default();
    let db = RooDb::new(&db_cfg);
    db.init_schema()
        .await
        .context("RooDB init_schema (check HSM_ROO_DB_URL / RooDB running)")?;

    match cli.command {
        Command::Collect {
            signature,
            out,
            config,
        } => {
            let gcfg = match config {
                Some(p) => GepaConfig::load_path(&p)?,
                None => GepaConfig::default(),
            };
            let bundle = collect_bundle(&db, &signature, &gcfg).await?;
            let n_fail = bundle.failure_traces.len();
            let n_cl = bundle.clusters.len();
            save_bundle(&out, &bundle)?;
            println!(
                "GEPA collect: {} failure traces → {} clusters → {}",
                n_fail,
                n_cl,
                out.display()
            );
        }
        Command::Optimize {
            signature,
            bundle,
            trials,
            ollama_host,
            ollama_port,
            model,
        } => {
            let tmpl =
                get_template_by_name(&signature).context("unknown signature name for template")?;
            let gepa_names = if let Some(p) = bundle {
                let b = load_bundle(&p)?;
                if b.signature_name != signature {
                    eprintln!(
                        "warning: bundle signature '{}' != --signature {}",
                        b.signature_name, signature
                    );
                }
                Some(mutation_style_names_from_bundle(&b))
            } else {
                None
            };
            let ollama = Ollama::new(ollama_host, ollama_port);
            let r = optimize_signature(&ollama, &model, &db, &tmpl, trials, gepa_names).await?;
            println!(
                "GEPA optimize: {} improved={} score {:.3} → {:.3} trials={} demos={}",
                r.signature_name,
                r.improved,
                r.previous_score,
                r.new_score,
                r.trials_run,
                r.demo_count
            );
        }
    }

    Ok(())
}
