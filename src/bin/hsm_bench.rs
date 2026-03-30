//! HSM-II Benchmark runner binary.
//!
//! Runs the full benchmark suite and prints results.
//!
//! Usage:
//!   cargo run --bin hsm-bench
//!   cargo run --bin hsm-bench -- --json          # Output as JSON
//!   cargo run --bin hsm-bench -- --filter memory  # Run only matching benchmarks

use clap::Parser;

use hyper_stigmergy::bench;

#[derive(Parser, Debug)]
#[command(name = "hsm-bench", about = "HSM-II benchmark suite")]
struct Args {
    /// Output results as JSON instead of table
    #[arg(long)]
    json: bool,

    /// Filter benchmarks by name substring
    #[arg(long)]
    filter: Option<String>,
}

fn main() {
    let args = Args::parse();

    println!("Running HSM-II benchmarks...\n");

    let mut suite = bench::run_all();

    // Filter if requested
    if let Some(ref filter) = args.filter {
        suite.results.retain(|r| r.name.contains(filter.as_str()));
    }

    if args.json {
        println!("{}", suite.to_json());
    } else {
        suite.print_report();
    }
}
