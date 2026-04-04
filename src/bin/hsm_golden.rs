//! Golden-path smoke test: Ladybug primary store + optional ad-hoc Cypher.
//!
//! ```text
//! export HSMII_LADYBUG_PATH=./data/hsm_lbug_primary
//! export HSMII_LADYBUG_PRIMARY=1
//! cargo run --bin hsm_golden --features lbug
//! cargo run --bin hsm_golden --features lbug -- --cypher "MATCH (s:HsmSkill) RETURN s.sid LIMIT 5;"
//! ```

use clap::Parser;

use hyper_stigmergy::embedded_graph_store::EmbeddedGraphStore;
use hyper_stigmergy::persistence::lbug_world_store;
use hyper_stigmergy::HyperStigmergicMorphogenesis;

#[derive(Parser)]
#[command(name = "hsm_golden")]
struct Cli {
    /// Run a Cypher query against `HSMII_LADYBUG_PATH` and print the result table.
    #[arg(long)]
    cypher: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(q) = cli.cypher {
        let out = lbug_world_store::run_cypher_debug(&q)?;
        println!("{}", out);
        return Ok(());
    }

    if !lbug_world_store::primary_enabled() {
        anyhow::bail!(
            "Set HSMII_LADYBUG_PATH and HSMII_LADYBUG_PRIMARY=1 for the golden Ladybug save path, or pass --cypher with HSMII_LADYBUG_PATH set."
        );
    }

    let world = HyperStigmergicMorphogenesis::new(4);
    let n = EmbeddedGraphStore::save_world(&world, None)?;
    println!(
        "hsm_golden: saved primary Ladybug world ({} bytes checkpoint payload).",
        n
    );
    Ok(())
}
