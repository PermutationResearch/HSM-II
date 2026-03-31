use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Parser, Subcommand};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

use hyper_stigmergy::eval::{
    append_runs_index, compare, default_runs_index, eval_tasks_for_suite, filter_tasks,
    parse_weighted_suites, sync_index_line_to_sqlite, write_jsonl, write_manifest,
    write_turn_metrics_jsonl, ArtifactPaths, BaselineRunner, HsmRunner, HsmRunnerConfig,
    RunManifest, RunnerMetrics, WeightedEvalSuite,
};
use hyper_stigmergy::llm::client::LlmClient;

const DEFAULT_PROMOTE_TO: &str = "config/hsm_harness.default.json";

#[derive(Parser)]
#[command(name = "hsm_meta_harness")]
#[command(about = "Phase-1 Meta-Harness search over HSM eval harness knobs")]
struct TopCli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[command(flatten)]
    search: SearchArgs,
}

#[derive(Subcommand)]
enum Commands {
    /// Copy a winning `best_config.json` (or any harness JSON) to the default path for reuse
    Promote {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        to: Option<PathBuf>,
    },
}

#[derive(Parser)]
struct SearchArgs {
    #[arg(long, default_value_t = 12)]
    candidates: usize,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Output root directory (default: `runs/run_<unix_ts>/`).
    #[arg(long)]
    out_dir: Option<PathBuf>,
    #[arg(long)]
    suite: Option<String>,
    /// Multi-suite transfer objective: `memory:1,tool:0.5,council:1` (weights optional, default 1). Overrides `--suite` when set.
    #[arg(long)]
    suites: Option<String>,
    #[arg(long)]
    tasks: Option<String>,
    /// Record HSM retrieval/skill traces per suite under each candidate directory.
    #[arg(long, default_value_t = false)]
    trace: bool,
    /// Append one line to `runs/runs_index.jsonl` (relative to run parent).
    #[arg(long, default_value_t = true)]
    write_runs_index: bool,
    /// JSON file: array of [`HsmRunnerConfig`] or `{\"candidates\":[...]}`. Also `HSM_META_HARNESS_CONFIG`.
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, default_value_t = true)]
    include_default: bool,
    #[arg(long, default_value_t = 1.0)]
    objective_keyword_weight: f64,
    #[arg(long, default_value_t = 0.6)]
    objective_recall_weight: f64,
    #[arg(long, default_value_t = 0.25)]
    objective_latency_penalty: f64,
    #[arg(long, default_value_t = 0.5)]
    objective_rubric_weight: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PerSuiteEvalSummary {
    suite_name: String,
    weight: f64,
    task_count: usize,
    hsm_avg_keyword_score: f64,
    hsm_avg_recall_score: f64,
    hsm_avg_rubric_composite: f64,
    hsm_rubric_pass_rate: f64,
    hsm_avg_latency_ms: f64,
    keyword_delta_vs_baseline: f64,
    recall_delta_vs_baseline: f64,
    rubric_composite_delta: f64,
    rubric_pass_rate_delta: f64,
    latency_ratio_vs_baseline: f64,
    objective_score: f64,
    verdict: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CandidateResult {
    candidate_id: String,
    config: HsmRunnerConfig,
    avg_keyword_score: f64,
    avg_recall_score: f64,
    avg_rubric_composite: f64,
    rubric_pass_rate: f64,
    avg_latency_ms: f64,
    keyword_delta_vs_baseline: f64,
    recall_delta_vs_baseline: f64,
    rubric_composite_delta: f64,
    rubric_pass_rate_delta: f64,
    latency_ratio_vs_baseline: f64,
    objective_score: f64,
    verdict: String,
    #[serde(default)]
    per_suite: Vec<PerSuiteEvalSummary>,
}

fn objective_score(
    keyword_delta: f64,
    recall_delta: f64,
    rubric_delta: f64,
    latency_ratio_vs_baseline: f64,
    kw_w: f64,
    recall_w: f64,
    rubric_w: f64,
    latency_penalty_w: f64,
) -> f64 {
    let latency_penalty = (latency_ratio_vs_baseline - 1.0).max(0.0);
    (kw_w * keyword_delta)
        + (recall_w * recall_delta)
        + (rubric_w * rubric_delta)
        - (latency_penalty_w * latency_penalty)
}

fn pareto_dominates(a: &CandidateResult, b: &CandidateResult) -> bool {
    let kw_ge = a.keyword_delta_vs_baseline >= b.keyword_delta_vs_baseline;
    let rec_ge = a.recall_delta_vs_baseline >= b.recall_delta_vs_baseline;
    let rub_ge = a.rubric_composite_delta >= b.rubric_composite_delta;
    let lat_le = a.latency_ratio_vs_baseline <= b.latency_ratio_vs_baseline;
    let strict_better = a.keyword_delta_vs_baseline > b.keyword_delta_vs_baseline
        || a.recall_delta_vs_baseline > b.recall_delta_vs_baseline
        || a.rubric_composite_delta > b.rubric_composite_delta
        || a.latency_ratio_vs_baseline < b.latency_ratio_vs_baseline;
    kw_ge && rec_ge && rub_ge && lat_le && strict_better
}

fn pareto_frontier(results: &[CandidateResult]) -> Vec<CandidateResult> {
    results
        .iter()
        .filter(|c| {
            !results
                .iter()
                .any(|o| o.candidate_id != c.candidate_id && pareto_dominates(o, c))
        })
        .cloned()
        .collect()
}

#[derive(Serialize)]
struct ParetoFrontierExport {
    maximize: [&'static str; 3],
    minimize: [&'static str; 1],
    count: usize,
    points: Vec<ParetoPoint>,
}

#[derive(Serialize)]
struct ParetoPoint {
    candidate_id: String,
    keyword_delta_vs_baseline: f64,
    recall_delta_vs_baseline: f64,
    rubric_composite_delta: f64,
    latency_ratio_vs_baseline: f64,
    avg_latency_ms: f64,
    objective_score: f64,
}

fn sample_config(rng: &mut StdRng) -> HsmRunnerConfig {
    HsmRunnerConfig {
        context_top_k: rng.gen_range(2..=8),
        context_score_threshold: rng.gen_range(0.05..=0.35),
        skill_success_threshold: rng.gen_range(0.45..=0.75),
        skill_reputation_alpha: rng.gen_range(0.1..=0.6),
        store_belief_min_score: rng.gen_range(0.2..=0.6),
        context_char_budget: rng.gen_range(800..=6000),
        include_session_summaries: rng.gen_bool(0.8),
        query_overlap_weight: rng.gen_range(0.05..=0.3),
        domain_match_bonus: rng.gen_range(0.1..=0.7),
        same_task_bonus: rng.gen_range(0.1..=0.9),
        belief_keyword_overlap_weight: rng.gen_range(0.05..=0.4),
        llm_temperature: rng.gen_range(0.1..=0.6),
        llm_max_tokens: rng.gen_range(700..=2000),
    }
}

#[derive(Deserialize)]
struct CandidateFile {
    candidates: Vec<HsmRunnerConfig>,
}

fn load_candidate_configs(path: &Path) -> anyhow::Result<Vec<HsmRunnerConfig>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read harness candidate file {}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).context("parse candidate JSON")?;
    if let Ok(list) = serde_json::from_value::<Vec<HsmRunnerConfig>>(v.clone()) {
        return Ok(list);
    }
    let w: CandidateFile = serde_json::from_value(v).context(
        "expected [HsmRunnerConfig, ...] or {\"candidates\":[...]}",
    )?;
    Ok(w.candidates)
}

fn promote(from: &Path, to: &Path) -> anyhow::Result<()> {
    fs::copy(from, to).with_context(|| {
        format!(
            "copy {} -> {}",
            from.display(),
            to.display()
        )
    })?;
    println!(
        "Promoted harness config:\n  from {}\n  to   {}",
        from.display(),
        to.display()
    );
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,hsm_meta_harness=info")),
        )
        .compact()
        .init();

    let top = TopCli::parse();

    match top.command {
        Some(Commands::Promote { from, to }) => {
            let dest = to.unwrap_or_else(|| PathBuf::from(DEFAULT_PROMOTE_TO));
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            return promote(&from, &dest);
        }
        None => run_search(top.search).await?,
    }

    Ok(())
}

fn resolve_eval_suites(cli: &SearchArgs) -> anyhow::Result<Vec<WeightedEvalSuite>> {
    let mut out = if let Some(ref spec) = cli.suites {
        parse_weighted_suites(spec).map_err(anyhow::Error::msg)?
    } else {
        let name = cli.suite.as_deref().unwrap_or("full").to_string();
        let tasks = eval_tasks_for_suite(&name).map_err(anyhow::Error::msg)?;
        vec![WeightedEvalSuite {
            name,
            weight: 1.0,
            tasks,
        }]
    };
    for s in &mut out {
        filter_tasks(&mut s.tasks, cli.tasks.as_deref(), cli.limit);
        if s.tasks.is_empty() {
            anyhow::bail!("suite {:?} has no tasks after filters", s.name);
        }
    }
    Ok(out)
}

async fn run_search(cli: SearchArgs) -> anyhow::Result<()> {
    let suites = resolve_eval_suites(&cli)?;

    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let run_dir = cli
        .out_dir
        .unwrap_or_else(|| PathBuf::from(format!("runs/run_{}", stamp)));
    fs::create_dir_all(&run_dir)?;

    let suite_names: Vec<String> = suites.iter().map(|s| s.name.clone()).collect();
    let suite_weights: Vec<f64> = suites.iter().map(|s| s.weight).collect();
    let total_tasks: usize = suites.iter().map(|s| s.tasks.len()).sum();
    let total_turns: usize = suites
        .iter()
        .map(|s| s.tasks.iter().map(|t| t.turns.len()).sum::<usize>())
        .sum();

    println!(
        "Baselines for {} suite slice(s) — {} tasks, {} turns…",
        suites.len(),
        total_tasks,
        total_turns
    );

    let mut baseline_by_name: BTreeMap<String, RunnerMetrics> = BTreeMap::new();
    for ws in &suites {
        println!("  baseline | {} | {} tasks", ws.name, ws.tasks.len());
        let b = BaselineRunner::new(LlmClient::new()?).run(&ws.tasks).await;
        baseline_by_name.insert(ws.name.clone(), b);
    }

    fs::write(
        run_dir.join("baseline_by_suite.json"),
        serde_json::to_string_pretty(&baseline_by_name)?,
    )?;

    if suites.len() == 1 {
        let only = suites.first().unwrap();
        fs::write(
            run_dir.join("baseline_metrics.json"),
            serde_json::to_string_pretty(baseline_by_name.get(&only.name).unwrap())?,
        )?;
    }

    let config_path = cli
        .config
        .or_else(|| std::env::var("HSM_META_HARNESS_CONFIG").ok().map(PathBuf::from));

    let mut candidate_configs: Vec<(String, HsmRunnerConfig)> = Vec::new();
    if cli.include_default {
        candidate_configs.push(("cand_000_default".to_string(), HsmRunnerConfig::default()));
    }

    if let Some(ref p) = config_path {
        let loaded = load_candidate_configs(p)?;
        for (idx, cfg) in loaded.into_iter().enumerate() {
            candidate_configs.push((format!("cand_{:03}_file", idx + 1), cfg));
        }
    } else {
        let mut rng = StdRng::seed_from_u64(cli.seed);
        for idx in 0..cli.candidates {
            candidate_configs.push((format!("cand_{:03}", idx + 1), sample_config(&mut rng)));
        }
    }

    let mut results = Vec::new();

    for (candidate_id, cfg) in candidate_configs {
        println!("Evaluating {} …", candidate_id);
        let cand_dir = run_dir.join(&candidate_id);
        fs::create_dir_all(&cand_dir)?;

        let mut per_suite: Vec<PerSuiteEvalSummary> = Vec::new();
        let mut w_sum = 0.0;
        let mut obj_acc = 0.0;
        let mut agg_kw = 0.0;
        let mut agg_rec = 0.0;
        let mut agg_rub = 0.0;
        let mut agg_pr = 0.0;
        let mut agg_lat = 0.0;
        let mut agg_kd = 0.0;
        let mut agg_rd = 0.0;
        let mut agg_rcd = 0.0;
        let mut agg_prd = 0.0;
        let mut agg_lr = 0.0;

        for ws in &suites {
            let baseline = baseline_by_name.get(&ws.name).expect("baseline for suite");
            let mut runner = HsmRunner::with_config(LlmClient::new()?, cfg.clone());
            if cli.trace {
                runner.set_collect_traces(true);
            }
            let hsm = runner.run(&ws.tasks).await;
            let traces = if cli.trace {
                runner.take_traces()
            } else {
                Vec::new()
            };

            let report = compare(baseline, &hsm, &ws.tasks);
            let baseline_latency = baseline.avg_latency_ms().max(1.0);
            let latency_ratio_vs_baseline = hsm.avg_latency_ms() / baseline_latency;
            let obj = objective_score(
                report.improvement.keyword_score_delta,
                report.improvement.recall_score_delta,
                report.improvement.rubric_composite_delta,
                latency_ratio_vs_baseline,
                cli.objective_keyword_weight,
                cli.objective_recall_weight,
                cli.objective_rubric_weight,
                cli.objective_latency_penalty,
            );

            let w = ws.weight;
            w_sum += w;
            obj_acc += w * obj;
            agg_kw += w * hsm.avg_keyword_score();
            agg_rec += w * hsm.avg_recall_score();
            agg_rub += w * hsm.avg_rubric_composite();
            agg_pr += w * hsm.rubric_pass_rate();
            agg_lat += w * hsm.avg_latency_ms();
            agg_kd += w * report.improvement.keyword_score_delta;
            agg_rd += w * report.improvement.recall_score_delta;
            agg_rcd += w * report.improvement.rubric_composite_delta;
            agg_prd += w * report.improvement.rubric_pass_rate_delta;
            agg_lr += w * latency_ratio_vs_baseline;

            per_suite.push(PerSuiteEvalSummary {
                suite_name: ws.name.clone(),
                weight: w,
                task_count: ws.tasks.len(),
                hsm_avg_keyword_score: hsm.avg_keyword_score(),
                hsm_avg_recall_score: hsm.avg_recall_score(),
                hsm_avg_rubric_composite: hsm.avg_rubric_composite(),
                hsm_rubric_pass_rate: hsm.rubric_pass_rate(),
                hsm_avg_latency_ms: hsm.avg_latency_ms(),
                keyword_delta_vs_baseline: report.improvement.keyword_score_delta,
                recall_delta_vs_baseline: report.improvement.recall_score_delta,
                rubric_composite_delta: report.improvement.rubric_composite_delta,
                rubric_pass_rate_delta: report.improvement.rubric_pass_rate_delta,
                latency_ratio_vs_baseline,
                objective_score: obj,
                verdict: report.verdict.clone(),
            });

            let suite_dir = cand_dir.join(&ws.name);
            fs::create_dir_all(&suite_dir)?;
            fs::write(
                suite_dir.join("hsm_metrics.json"),
                serde_json::to_string_pretty(&hsm)?,
            )?;
            fs::write(
                suite_dir.join("comparison_report.json"),
                serde_json::to_string_pretty(&report)?,
            )?;
            write_turn_metrics_jsonl(&suite_dir.join("turns_hsm.jsonl"), &hsm.turns)?;
            write_turn_metrics_jsonl(&suite_dir.join("turns_baseline.jsonl"), &baseline.turns)?;
            if cli.trace && !traces.is_empty() {
                write_jsonl(&suite_dir.join("hsm_trace.jsonl"), &traces)?;
            }
        }

        let inv = if w_sum > 0.0 { 1.0 / w_sum } else { 0.0 };
        let result = CandidateResult {
            candidate_id: candidate_id.clone(),
            config: cfg,
            avg_keyword_score: agg_kw * inv,
            avg_recall_score: agg_rec * inv,
            avg_rubric_composite: agg_rub * inv,
            rubric_pass_rate: agg_pr * inv,
            avg_latency_ms: agg_lat * inv,
            keyword_delta_vs_baseline: agg_kd * inv,
            recall_delta_vs_baseline: agg_rd * inv,
            rubric_composite_delta: agg_rcd * inv,
            rubric_pass_rate_delta: agg_prd * inv,
            latency_ratio_vs_baseline: agg_lr * inv,
            objective_score: obj_acc * inv,
            verdict: if per_suite.len() == 1 {
                per_suite[0].verdict.clone()
            } else {
                format!(
                    "Multi-suite aggregate [{}] — see per_suite.json",
                    suite_names.join(", ")
                )
            },
            per_suite: per_suite.clone(),
        };

        fs::write(
            cand_dir.join("candidate_result.json"),
            serde_json::to_string_pretty(&result)?,
        )?;
        fs::write(
            cand_dir.join("per_suite.json"),
            serde_json::to_string_pretty(&per_suite)?,
        )?;

        // Single-suite layouts: mirror suite dir at candidate root (backward compatible).
        if suites.len() == 1 {
            let src = cand_dir.join(&suites[0].name);
            for f in [
                "hsm_metrics.json",
                "comparison_report.json",
                "turns_hsm.jsonl",
                "turns_baseline.jsonl",
            ] {
                let p = src.join(f);
                if p.exists() {
                    fs::copy(&p, cand_dir.join(f))?;
                }
            }
            if cli.trace {
                let t = src.join("hsm_trace.jsonl");
                if t.exists() {
                    fs::copy(&t, cand_dir.join("hsm_trace.jsonl"))?;
                }
            }
        }
        results.push(result);
    }

    results.sort_by(|a, b| {
        b.objective_score
            .partial_cmp(&a.objective_score)
            .unwrap_or(Ordering::Equal)
    });

    fs::write(
        run_dir.join("leaderboard.json"),
        serde_json::to_string_pretty(&results)?,
    )?;

    let mut frontier = pareto_frontier(&results);
    frontier.sort_by(|a, b| {
        b.keyword_delta_vs_baseline
            .partial_cmp(&a.keyword_delta_vs_baseline)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                b.recall_delta_vs_baseline
                    .partial_cmp(&a.recall_delta_vs_baseline)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| {
                b.rubric_composite_delta
                    .partial_cmp(&a.rubric_composite_delta)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| {
                a.latency_ratio_vs_baseline
                    .partial_cmp(&b.latency_ratio_vs_baseline)
                    .unwrap_or(Ordering::Equal)
            })
    });

    let pareto_export = ParetoFrontierExport {
        maximize: [
            "keyword_delta_vs_baseline",
            "recall_delta_vs_baseline",
            "rubric_composite_delta",
        ],
        minimize: ["latency_ratio_vs_baseline"],
        count: frontier.len(),
        points: frontier
            .iter()
            .map(|c| ParetoPoint {
                candidate_id: c.candidate_id.clone(),
                keyword_delta_vs_baseline: c.keyword_delta_vs_baseline,
                recall_delta_vs_baseline: c.recall_delta_vs_baseline,
                rubric_composite_delta: c.rubric_composite_delta,
                latency_ratio_vs_baseline: c.latency_ratio_vs_baseline,
                avg_latency_ms: c.avg_latency_ms,
                objective_score: c.objective_score,
            })
            .collect(),
    };
    fs::write(
        run_dir.join("pareto_frontier.json"),
        serde_json::to_string_pretty(&pareto_export)?,
    )?;

    println!("\nTop candidates by objective score:");
    for c in results.iter().take(5) {
        println!(
            "{} | objective {:+.3} | rubric Δ {:+.3} | keyword Δ {:+.3} | recall Δ {:+.3} | latency x{:.2}",
            c.candidate_id,
            c.objective_score,
            c.rubric_composite_delta,
            c.keyword_delta_vs_baseline,
            c.recall_delta_vs_baseline,
            c.latency_ratio_vs_baseline,
        );
    }
    if let Some(best) = results.first() {
        println!(
            "\nBest candidate: {} (objective {:+.3}, rubric Δ {:+.3}, keyword Δ {:+.3}, latency x{:.2})",
            best.candidate_id,
            best.objective_score,
            best.rubric_composite_delta,
            best.keyword_delta_vs_baseline,
            best.latency_ratio_vs_baseline
        );
        fs::write(
            run_dir.join("best_config.json"),
            serde_json::to_string_pretty(&best.config)?,
        )?;
    }

    let manifest = RunManifest::new(
        "hsm_meta_harness",
        &run_dir,
        suite_names.clone(),
        Some(suite_weights.clone()),
        cli.tasks.clone(),
        total_tasks,
        total_turns,
        std::env::var("HSM_PARENT_RUN_ID").ok(),
        ArtifactPaths {
            manifest: "manifest.json".into(),
            turns_baseline_jsonl: Some("<candidate>/<suite>/turns_baseline.jsonl".into()),
            turns_hsm_jsonl: Some("<candidate>/<suite>/turns_hsm.jsonl".into()),
            hsm_trace_jsonl: if cli.trace {
                Some("<candidate>/<suite>/hsm_trace.jsonl".into())
            } else {
                None
            },
            comparison_json: Some("<candidate>/<suite>/comparison_report.json".into()),
            per_suite_json: Some("<candidate>/per_suite.json".into()),
        },
    );
    write_manifest(&run_dir, &manifest)?;

    if cli.write_runs_index {
        if let Some(best) = results.first() {
            let idx_path = default_runs_index(&run_dir);
            let line = serde_json::json!({
                "harness": "hsm_meta_harness",
                "run_dir": run_dir.display().to_string(),
                "created_unix": manifest.created_unix,
                "git_commit": manifest.git_commit,
                "best_candidate": best.candidate_id,
                "objective_score": best.objective_score,
                "suites": suite_names,
            });
            append_runs_index(&idx_path, &line)?;
            if let Ok(db) = std::env::var("HSM_RUNS_SQLITE") {
                if let Err(e) = sync_index_line_to_sqlite(std::path::Path::new(&db), &line) {
                    eprintln!("warning: HSM_RUNS_SQLITE sync failed: {}", e);
                }
            }
        }
    }

    println!(
        "\nPareto frontier ({} points; kw/recall/rubric Δ ↑, latency ratio ↓):",
        frontier.len()
    );
    for c in &frontier {
        println!(
            "  {} | kw Δ {:+.3} | recall Δ {:+.3} | rubric Δ {:+.3} | latency x{:.2}",
            c.candidate_id,
            c.keyword_delta_vs_baseline,
            c.recall_delta_vs_baseline,
            c.rubric_composite_delta,
            c.latency_ratio_vs_baseline
        );
    }

    println!("Artifacts written to {}", run_dir.display());

    Ok(())
}
