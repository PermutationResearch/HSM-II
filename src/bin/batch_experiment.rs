//! Batch experiment runner binary for HSM-II empirical evaluation.
//!
//! Usage: cargo run --release --bin batch_experiment -- [options]

use hyper_stigmergy::batch_runner::{BatchConfig, BatchRunner};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Parse command line arguments first
    let args: Vec<String> = std::env::args().collect();

    // Show help if requested
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("HSM-II Batch Experiment Runner\n");
        println!("Usage: cargo run --release --bin batch_experiment [options] [RUNS] [TICKS] [OUTPUT_DIR]\n");
        println!("Arguments:");
        println!("  RUNS        Number of experiment runs (default: 20)");
        println!("  TICKS       Ticks per run (default: 1000)");
        println!("  OUTPUT_DIR  Output directory (default: experiments)\n");
        println!("Options:");
        println!("  --real-llm              Use actual Ollama LLM calls (slower but real)");
        println!("  --credit                Enable credit assignment (default)");
        println!("  --no-credit             Disable credit assignment");
        println!("  --credit-samples N      Stochastic replay samples per decision");
        println!("  --credit-horizon N      Replay horizon in ticks");
        println!(
            "  --credit-weights W      Weights as a,b,c,d for coherence,stability,reward,skills"
        );
        println!("  --credit-skills-scale N Normalization scale for skills promoted");
        println!("  --seed-base N           Deterministic seed base (run i uses N+i)");
        println!("  --kuramoto              Enable Kuramoto treatment in workload dynamics");
        println!("  --kuramoto-k N          Kuramoto coupling strength");
        println!("  --kuramoto-council N    Kuramoto council influence");
        println!("  --kuramoto-disp N       Kuramoto dispersion");
        println!("  --kuramoto-dt N         Kuramoto integration dt");
        println!("  --kuramoto-noise N      Kuramoto noise amplitude");
        println!("  --kuramoto-gain N       Drive feedback gain");
        println!("  --kuramoto-drive-feedback Enable direct per-agent drive feedback");
        println!("  --kuramoto-phase-field  Enable phase-field correction terms");
        println!("  --kuramoto-pf-growth N  Phase-field growth coefficient");
        println!("  --kuramoto-pf-hyper N   Phase-field hyperviscosity coefficient");
        println!("  --kuramoto-pf-disp N    Phase-field dispersion-like coefficient");
        println!("  --kuramoto-warmup N     Warmup ticks before full influence");
        println!("  --kuramoto-cap-k N      Runtime cap for effective coupling");
        println!("  --kuramoto-cap-c N      Runtime cap for council influence");
        println!("  --kuramoto-lcc-gate N   Largest-component ratio gate (0..1)");
        println!("  --kuramoto-no-adaptive  Disable adaptive quality guard");
        println!("  --kuramoto-gain-min N   Adaptive guard minimum gain scale");
        println!("  --kuramoto-entropy-floor N  Entropy floor for anti-collapse");
        println!("  --kuramoto-entropy-boost N  Extra noise when below entropy floor");
        println!("  --kuramoto-disable-trips N  Consecutive guard trips before disable");
        println!("  --help                  Show this help message\n");
        return Ok(());
    }

    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║  HSM-II Batch Experiment Runner - Empirical Evaluation        ║");
    println!("║  Generates real data for paper figures                        ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    let mut use_real_llm = false;
    let mut enable_credit = true;
    let mut credit_samples: Option<usize> = None;
    let mut credit_horizon: Option<usize> = None;
    let mut credit_weights: Option<[f64; 4]> = None;
    let mut credit_skills_scale: Option<f64> = None;
    let mut enable_kuramoto = false;
    let mut kuramoto_k: Option<f64> = None;
    let mut kuramoto_council: Option<f64> = None;
    let mut kuramoto_disp: Option<f64> = None;
    let mut kuramoto_dt: Option<f64> = None;
    let mut kuramoto_noise: Option<f64> = None;
    let mut kuramoto_gain: Option<f64> = None;
    let mut kuramoto_drive_feedback = false;
    let mut kuramoto_phase_field = false;
    let mut kuramoto_pf_growth: Option<f64> = None;
    let mut kuramoto_pf_hyper: Option<f64> = None;
    let mut kuramoto_pf_disp: Option<f64> = None;
    let mut kuramoto_warmup: Option<usize> = None;
    let mut kuramoto_cap_k: Option<f64> = None;
    let mut kuramoto_cap_c: Option<f64> = None;
    let mut kuramoto_lcc_gate: Option<f64> = None;
    let mut kuramoto_no_adaptive = false;
    let mut kuramoto_gain_min: Option<f64> = None;
    let mut kuramoto_entropy_floor: Option<f64> = None;
    let mut kuramoto_entropy_boost: Option<f64> = None;
    let mut kuramoto_disable_trips: Option<u32> = None;
    let mut seed_base: Option<u64> = None;
    let mut positional_args: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--real-llm" | "--llm" => {
                use_real_llm = true;
                i += 1;
            }
            "--credit" => {
                enable_credit = true;
                i += 1;
            }
            "--no-credit" => {
                enable_credit = false;
                i += 1;
            }
            "--credit-samples" => {
                if i + 1 < args.len() {
                    credit_samples = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--credit-horizon" => {
                if i + 1 < args.len() {
                    credit_horizon = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--credit-weights" => {
                if i + 1 < args.len() {
                    let parts: Vec<f64> = args[i + 1]
                        .split(',')
                        .filter_map(|v| v.trim().parse().ok())
                        .collect();
                    if parts.len() == 4 {
                        credit_weights = Some([parts[0], parts[1], parts[2], parts[3]]);
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--credit-skills-scale" => {
                if i + 1 < args.len() {
                    credit_skills_scale = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--seed-base" => {
                if i + 1 < args.len() {
                    seed_base = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto" => {
                enable_kuramoto = true;
                i += 1;
            }
            "--kuramoto-k" => {
                if i + 1 < args.len() {
                    kuramoto_k = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-council" => {
                if i + 1 < args.len() {
                    kuramoto_council = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-disp" => {
                if i + 1 < args.len() {
                    kuramoto_disp = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-dt" => {
                if i + 1 < args.len() {
                    kuramoto_dt = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-noise" => {
                if i + 1 < args.len() {
                    kuramoto_noise = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-gain" => {
                if i + 1 < args.len() {
                    kuramoto_gain = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-drive-feedback" => {
                kuramoto_drive_feedback = true;
                i += 1;
            }
            "--kuramoto-phase-field" => {
                kuramoto_phase_field = true;
                i += 1;
            }
            "--kuramoto-pf-growth" => {
                if i + 1 < args.len() {
                    kuramoto_pf_growth = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-pf-hyper" => {
                if i + 1 < args.len() {
                    kuramoto_pf_hyper = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-pf-disp" => {
                if i + 1 < args.len() {
                    kuramoto_pf_disp = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-warmup" => {
                if i + 1 < args.len() {
                    kuramoto_warmup = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-cap-k" => {
                if i + 1 < args.len() {
                    kuramoto_cap_k = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-cap-c" => {
                if i + 1 < args.len() {
                    kuramoto_cap_c = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-lcc-gate" => {
                if i + 1 < args.len() {
                    kuramoto_lcc_gate = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-no-adaptive" => {
                kuramoto_no_adaptive = true;
                i += 1;
            }
            "--kuramoto-gain-min" => {
                if i + 1 < args.len() {
                    kuramoto_gain_min = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-entropy-floor" => {
                if i + 1 < args.len() {
                    kuramoto_entropy_floor = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-entropy-boost" => {
                if i + 1 < args.len() {
                    kuramoto_entropy_boost = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--kuramoto-disable-trips" => {
                if i + 1 < args.len() {
                    kuramoto_disable_trips = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            arg => {
                if arg.starts_with("--") {
                    i += 1;
                } else {
                    positional_args.push(arg.to_string());
                    i += 1;
                }
            }
        }
    }

    let mut config = BatchConfig::default();
    config.num_runs = positional_args
        .get(0)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    config.ticks_per_run = positional_args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    config.agent_count = 10;
    config.output_dir = positional_args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "experiments".to_string());
    config.enable_dks = true;
    config.enable_federation = true;
    config.enable_llm_deliberation = true;
    config.enable_stigmergic_entities = true;
    config.llm_latency_budget_ms = 10000; // 10 second timeout per LLM call
    config.ollama_model =
        "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL".to_string();
    config.use_real_llm = use_real_llm; // Enable via --real-llm flag
    config.enable_credit_assignment = enable_credit;
    config.seed_base = seed_base;
    if let Some(samples) = credit_samples {
        config.credit_replay_samples = samples;
    }
    if let Some(horizon) = credit_horizon {
        config.credit_horizon_ticks = horizon;
    }
    if let Some(weights) = credit_weights {
        config.credit_weights.global_coherence = weights[0];
        config.credit_weights.dks_mean_stability = weights[1];
        config.credit_weights.mean_agent_reward = weights[2];
        config.credit_weights.skills_promoted = weights[3];
    }
    if let Some(scale) = credit_skills_scale {
        config.credit_skills_promoted_scale = scale;
    }
    config.enable_kuramoto = enable_kuramoto;
    if let Some(v) = kuramoto_k {
        config.kuramoto_coupling_strength = v;
    }
    if let Some(v) = kuramoto_council {
        config.kuramoto_council_influence = v;
    }
    if let Some(v) = kuramoto_disp {
        config.kuramoto_dispersion = v;
    }
    if let Some(v) = kuramoto_dt {
        config.kuramoto_dt = v;
    }
    if let Some(v) = kuramoto_noise {
        config.kuramoto_noise_amplitude = v;
    }
    if let Some(v) = kuramoto_gain {
        config.kuramoto_feedback_gain = v;
    }
    config.kuramoto_drive_feedback = kuramoto_drive_feedback;
    config.kuramoto_phase_field = kuramoto_phase_field;
    if let Some(v) = kuramoto_pf_growth {
        config.kuramoto_phase_growth = v;
    }
    if let Some(v) = kuramoto_pf_hyper {
        config.kuramoto_phase_hypervisc = v;
    }
    if let Some(v) = kuramoto_pf_disp {
        config.kuramoto_phase_dispersion = v;
    }
    if let Some(v) = kuramoto_warmup {
        config.kuramoto_warmup_ticks = v;
    }
    if let Some(v) = kuramoto_cap_k {
        config.kuramoto_coupling_cap = v;
    }
    if let Some(v) = kuramoto_cap_c {
        config.kuramoto_council_cap = v;
    }
    if let Some(v) = kuramoto_lcc_gate {
        config.kuramoto_lcc_gate = v;
    }
    if kuramoto_no_adaptive {
        config.kuramoto_adaptive_guard = false;
    }
    if let Some(v) = kuramoto_gain_min {
        config.kuramoto_adaptive_gain_min = v;
    }
    if let Some(v) = kuramoto_entropy_floor {
        config.kuramoto_entropy_floor = v;
    }
    if let Some(v) = kuramoto_entropy_boost {
        config.kuramoto_entropy_noise_boost = v;
    }
    if let Some(v) = kuramoto_disable_trips {
        config.kuramoto_disable_after_trips = v;
    }

    println!("Configuration:");
    println!("  Runs:          {}", config.num_runs);
    println!("  Ticks/run:     {}", config.ticks_per_run);
    println!("  Agents:        {}", config.agent_count);
    println!("  Output dir:    {}", config.output_dir);
    println!(
        "  Seed base:     {}",
        config
            .seed_base
            .map(|v| v.to_string())
            .unwrap_or_else(|| "auto(time-based)".to_string())
    );
    println!(
        "  DKS:           {}",
        if config.enable_dks {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Federation:    {}",
        if config.enable_federation {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  LLM Council:   {}",
        if config.enable_llm_deliberation {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Stigmergic:    {}",
        if config.enable_stigmergic_entities {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Kuramoto:      {}",
        if config.enable_kuramoto {
            "enabled"
        } else {
            "disabled"
        }
    );
    if config.enable_kuramoto {
        println!(
            "  Kuramoto cfg:  K={:.3} council={:.3} disp={:.3} dt={:.3} noise={:.3} gain={:.3}",
            config.kuramoto_coupling_strength,
            config.kuramoto_council_influence,
            config.kuramoto_dispersion,
            config.kuramoto_dt,
            config.kuramoto_noise_amplitude,
            config.kuramoto_feedback_gain
        );
        println!(
            "  Feedback mode: {}",
            if config.kuramoto_drive_feedback {
                "drive+structural"
            } else {
                "structural-only"
            }
        );
        println!(
            "  Phase-field:   {} growth={:.3} hyper={:.3} disp={:.3}",
            if config.kuramoto_phase_field {
                "enabled"
            } else {
                "disabled"
            },
            config.kuramoto_phase_growth,
            config.kuramoto_phase_hypervisc,
            config.kuramoto_phase_dispersion
        );
        println!(
            "  Runtime guard: {} warmup={} cap_k={:.3} cap_c={:.3} lcc_gate={:.2} gain_min={:.2} entropy_floor={:.2} entropy_boost={:.3} disable_trips={}",
            if config.kuramoto_adaptive_guard { "enabled" } else { "disabled" },
            config.kuramoto_warmup_ticks,
            config.kuramoto_coupling_cap,
            config.kuramoto_council_cap,
            config.kuramoto_lcc_gate,
            config.kuramoto_adaptive_gain_min,
            config.kuramoto_entropy_floor,
            config.kuramoto_entropy_noise_boost,
            config.kuramoto_disable_after_trips
        );
    }
    println!(
        "  Real LLM:      {}",
        if config.use_real_llm {
            "enabled"
        } else {
            "simulated"
        }
    );
    println!("  LLM Timeout:   {}ms", config.llm_latency_budget_ms);
    println!(
        "  Credit:        {}",
        if config.enable_credit_assignment {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Credit cfg:    samples={} horizon={}",
        config.credit_replay_samples, config.credit_horizon_ticks
    );
    println!(
        "  Credit wts:    coherence={:.2} stability={:.2} reward={:.2} skills={:.2}",
        config.credit_weights.global_coherence,
        config.credit_weights.dks_mean_stability,
        config.credit_weights.mean_agent_reward,
        config.credit_weights.skills_promoted
    );
    println!();

    let start_time = std::time::Instant::now();

    BatchRunner::run_batch(config).await?;

    let elapsed = start_time.elapsed();
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("Total time: {:.2?}", elapsed);
    println!("═══════════════════════════════════════════════════════════════");

    println!("\nNext steps:");
    println!(
        "  1. Generate plots: python3 scripts/plot_results.py {}",
        std::env::args()
            .nth(3)
            .unwrap_or_else(|| "experiments".to_string())
    );
    println!("  2. Copy figures to paper: cp experiments/figures/*.pdf paper/figures/");

    Ok(())
}
