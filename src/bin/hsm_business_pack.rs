//! Validate or install Business Pack templates (`business/pack.yaml`).
//!
//! ```bash
//! cargo run -p hyper-stigmergy --bin hsm-business-pack -- validate --home ~/.hsmii
//! cargo run -p hyper-stigmergy --bin hsm-business-pack -- validate --pack ./business/pack.yaml
//! cargo run -p hyper-stigmergy --bin hsm-business-pack -- init marketing_solo --to ./my_business
//! cargo run -p hyper-stigmergy --bin hsm-business-pack -- list-starters
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use hyper_stigmergy::company_os::onboarding_contracts::{
    evaluate_gate_results, find_contract, load_contracts_hot,
};
use hyper_stigmergy::personal::{validate_pack_yaml_file, BusinessPack};

fn starters_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/business/starters")
}

const STARTER_NAMES: &[&str] = &[
    "generic_smb",
    "property_management",
    "gestion_velora",
    "velora_enticy",
    "online_commerce_squad",
    "construction_trades",
    "online_services",
    "marketing_solo",
];

#[derive(Parser)]
#[command(name = "hsm-business-pack")]
#[command(about = "Validate and scaffold HSM-II business/pack.yaml")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run validation (errors exit non-zero).
    Validate {
        /// Agent home (looks for business/pack.yaml or business_pack.yaml).
        #[arg(long)]
        home: Option<PathBuf>,
        /// Explicit pack file path.
        #[arg(long)]
        pack: Option<PathBuf>,
    },
    /// Copy a starter tree to `--to` (e.g. your profile's `business/` directory).
    Init {
        /// One of: generic_smb, property_management, gestion_velora, velora_enticy, online_commerce_squad, construction_trades, online_services, marketing_solo
        starter: String,
        /// Destination directory (created). Use e.g. ~/.hsmii/business or ~/.hsmii/profiles/acme/business
        #[arg(long)]
        to: PathBuf,
    },
    ListStarters,
    /// Validate onboarding KPI/risk gates for a pack contract.
    ValidateOnboardingContract {
        /// Contract id (e.g. generic_smb_core_v1, ecommerce_ops_v1, property_management_ops_v1)
        #[arg(long)]
        contract: String,
        /// Transcript text to evaluate.
        #[arg(long)]
        transcript: String,
    },
}

fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&s, &d)?;
        } else {
            fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Cmd::ListStarters => {
            println!("Available starters (under templates/business/starters/):");
            for n in STARTER_NAMES {
                println!("  {n}");
            }
            return Ok(());
        }
        Cmd::Init { starter, to } => {
            if !STARTER_NAMES.contains(&starter.as_str()) {
                anyhow::bail!(
                    "unknown starter {:?}. Run `list-starters` for options.",
                    starter
                );
            }
            let src = starters_root().join(&starter);
            if !src.is_dir() {
                anyhow::bail!("starter directory missing: {}", src.display());
            }
            if to.exists() {
                anyhow::bail!(
                    "destination {} already exists; remove it or pick another --to",
                    to.display()
                );
            }
            copy_dir_all(&src, &to)?;
            println!(
                "Copied starter {:?} → {}\nNext: point HSMII_HOME at the parent if needed, or move this tree to <home>/business/",
                starter,
                to.display()
            );
            let pack = to.join("pack.yaml");
            if pack.is_file() {
                let r = validate_pack_yaml_file(&pack)?;
                print!("{}", r.format_cli());
                if !r.ok() {
                    anyhow::bail!("pack validation failed after copy");
                }
            }
            return Ok(());
        }
        Cmd::Validate { home, pack } => {
            let report = match (home, pack) {
                (_, Some(p)) => validate_pack_yaml_file(&p)?,
                (Some(h), None) => {
                    let Some((_, yaml)) = BusinessPack::resolve_yaml_path(&h) else {
                        anyhow::bail!(
                            "no business/pack.yaml or business_pack.yaml under {}",
                            h.display()
                        );
                    };
                    validate_pack_yaml_file(&yaml)?
                }
                (None, None) => {
                    anyhow::bail!("provide --home <dir> or --pack <pack.yaml>");
                }
            };
            print!("{}", report.format_cli());
            if !report.ok() {
                std::process::exit(1);
            }
            return Ok(());
        }
        Cmd::ValidateOnboardingContract {
            contract,
            transcript,
        } => {
            let contracts = load_contracts_hot()?;
            let selected = find_contract(&contracts, &contract, "");
            let kpi = evaluate_gate_results(&transcript, &selected.kpi_gates);
            let risk = evaluate_gate_results(&transcript, &selected.risk_gates);
            let mut missing = Vec::new();
            println!("Contract: {} ({})", selected.id, selected.display_name);
            for g in kpi.iter().chain(risk.iter()) {
                let ok = g.satisfied;
                println!("  - {}: {}", g.id, if ok { "OK" } else { "MISSING" });
                if g.required && !ok {
                    missing.push(g.id.clone());
                }
            }
            if !missing.is_empty() {
                anyhow::bail!("required onboarding gates missing: {}", missing.join(", "));
            }
            println!("OK: all required onboarding gates satisfied.");
            return Ok(());
        }
    }
}
