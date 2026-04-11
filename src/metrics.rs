//! Real-time metrics collection for HSM-II empirical evaluation.
//!
//! Collects time-series data for:
//! - Global coherence C(t)
//! - Skill accumulation (harvested vs promoted)
//! - Council decisions by mode
//! - Federation trust scores
//! - DKS population dynamics

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RewardSignal {
    pub coherence_delta: f64,
    pub exec_ok: bool,
    pub exec_bonus: f64,
    pub task_score: Option<f64>,
    pub tests_passed: Option<bool>,
    pub ground_truth_score: Option<f64>,
    pub latency_penalty: Option<f64>,
    pub total: f64,
}

/// Snapshot of system state at a given tick
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TickSnapshot {
    pub tick: usize,
    pub timestamp: DateTime<Utc>,

    // Coherence metrics
    pub global_coherence: f64,
    pub edge_density: f64,
    pub emergent_coverage: f64,
    pub ontological_consistency: f64,
    pub belief_convergence: f64,

    // Skill metrics
    pub skills_harvested: usize,
    pub skills_promoted: usize,
    pub skills_level_2_plus: usize,
    pub jury_pass_rate: f64,

    // Council metrics
    pub council_proposals_total: usize,
    pub council_approved: usize,
    pub council_rejected: usize,
    pub council_deferred: usize,
    pub council_mode_usage: HashMap<String, usize>,

    // Bidding metrics
    pub mean_agent_reward: f64,
    pub grpo_entropy: f64,

    // DKS metrics
    pub dks_population_size: usize,
    pub dks_mean_stability: f64,
    pub dks_multifractal_width: f64,
    pub dks_stigmergic_edges: usize,

    // Federation metrics (if applicable)
    pub federation_trust_scores: HashMap<String, f64>,
    pub knowledge_layer_counts: HashMap<String, usize>,
}

/// Full experiment run data
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExperimentRun {
    pub run_id: String,
    pub seed: u64,
    pub config: MetricsExperimentConfig,
    pub snapshots: Vec<TickSnapshot>,
    pub final_stats: FinalStats,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionCredit {
    pub decision_id: u64,
    pub tick: usize,
    pub decision_type: String,
    pub actual_score: f64,
    pub counterfactual_score: f64,
    pub delta: f64,
    pub metadata: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MetricsExperimentConfig {
    pub ticks: usize,
    pub agent_count: usize,
    pub dks_enabled: bool,
    pub federation_enabled: bool,
    pub llm_deliberation: bool,
    pub stigmergic_entities: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct FinalStats {
    pub final_coherence: f64,
    pub coherence_growth: f64,
    pub total_skills_promoted: usize,
    pub jury_pass_rate: f64,
    pub mean_reward_per_tick: f64,
    pub final_grpo_entropy: f64,
    pub council_proposals_resolved: usize,
    pub council_approve_rate: f64,
    pub optimize_anything_best_score: f64,
    pub dks_mean_stability: f64,
}

/// Metrics collector that accumulates data during a run
#[derive(Clone, Debug, Default)]
pub struct MetricsCollector {
    pub run_id: String,
    pub seed: u64,
    pub snapshots: Vec<TickSnapshot>,
    pub config: MetricsExperimentConfig,

    // Accumulators
    pub skills_harvested_total: usize,
    pub skills_promoted_total: usize,
    pub council_decisions: Vec<MetricsCouncilDecision>,
    pub federation_events: Vec<FederationEvent>,
    pub decision_credits: Vec<DecisionCredit>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricsCouncilDecision {
    pub tick: usize,
    pub mode: String,
    pub outcome: String,
    pub complexity: f64,
    pub urgency: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FederationEvent {
    pub tick: usize,
    pub peer_id: String,
    pub trust_score: f64,
    pub event_type: String,
}

impl MetricsCollector {
    pub fn new(run_id: String, seed: u64, config: MetricsExperimentConfig) -> Self {
        Self {
            run_id,
            seed,
            config,
            snapshots: Vec::new(),
            skills_harvested_total: 0,
            skills_promoted_total: 0,
            council_decisions: Vec::new(),
            federation_events: Vec::new(),
            decision_credits: Vec::new(),
        }
    }

    pub fn record_tick(&mut self, snapshot: TickSnapshot) {
        self.snapshots.push(snapshot);
    }

    pub fn record_council_decision(&mut self, decision: MetricsCouncilDecision) {
        self.council_decisions.push(decision);
    }

    pub fn record_federation_event(&mut self, event: FederationEvent) {
        self.federation_events.push(event);
    }

    pub fn record_decision_credit(&mut self, credit: DecisionCredit) {
        self.decision_credits.push(credit);
    }

    pub fn record_skill_harvested(&mut self) {
        self.skills_harvested_total += 1;
    }

    pub fn record_skill_promoted(&mut self) {
        self.skills_promoted_total += 1;
    }

    /// Export all data to CSV files
    pub fn export_csv(
        &self,
        output_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        std::fs::create_dir_all(output_dir)?;

        // Export tick snapshots
        self.export_snapshots_csv(&output_dir.join(format!("{}_snapshots.csv", self.run_id)))?;

        // Export council decisions
        self.export_council_csv(&output_dir.join(format!("{}_council.csv", self.run_id)))?;

        // Export federation events
        if !self.federation_events.is_empty() {
            self.export_federation_csv(
                &output_dir.join(format!("{}_federation.csv", self.run_id)),
            )?;
        }

        // Export decision credits
        if !self.decision_credits.is_empty() {
            self.export_credit_csv(&output_dir.join(format!("{}_credits.csv", self.run_id)))?;
        }

        // Export summary
        self.export_summary_json(&output_dir.join(format!("{}_summary.json", self.run_id)))?;

        Ok(())
    }

    fn export_snapshots_csv(
        &self,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut file = File::create(path)?;

        // Header
        writeln!(file, "tick,timestamp,global_coherence,edge_density,emergent_coverage,ontological_consistency,belief_convergence,skills_harvested,skills_promoted,skills_level_2_plus,jury_pass_rate,council_proposals_total,council_approved,council_rejected,council_deferred,mean_agent_reward,grpo_entropy,dks_population_size,dks_mean_stability,dks_multifractal_width,dks_stigmergic_edges")?;

        // Data
        for snap in &self.snapshots {
            writeln!(
                file,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                snap.tick,
                snap.timestamp.to_rfc3339(),
                snap.global_coherence,
                snap.edge_density,
                snap.emergent_coverage,
                snap.ontological_consistency,
                snap.belief_convergence,
                snap.skills_harvested,
                snap.skills_promoted,
                snap.skills_level_2_plus,
                snap.jury_pass_rate,
                snap.council_proposals_total,
                snap.council_approved,
                snap.council_rejected,
                snap.council_deferred,
                snap.mean_agent_reward,
                snap.grpo_entropy,
                snap.dks_population_size,
                snap.dks_mean_stability,
                snap.dks_multifractal_width,
                snap.dks_stigmergic_edges,
            )?;
        }

        Ok(())
    }

    fn export_council_csv(
        &self,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut file = File::create(path)?;
        writeln!(file, "tick,mode,outcome,complexity,urgency")?;

        for decision in &self.council_decisions {
            writeln!(
                file,
                "{},{},{},{},{}",
                decision.tick,
                decision.mode,
                decision.outcome,
                decision.complexity,
                decision.urgency
            )?;
        }

        Ok(())
    }

    fn export_federation_csv(
        &self,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut file = File::create(path)?;
        writeln!(file, "tick,peer_id,trust_score,event_type")?;

        for event in &self.federation_events {
            writeln!(
                file,
                "{},{},{},{}",
                event.tick, event.peer_id, event.trust_score, event.event_type
            )?;
        }

        Ok(())
    }

    fn export_credit_csv(
        &self,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut file = File::create(path)?;
        writeln!(
            file,
            "decision_id,tick,decision_type,actual_score,counterfactual_score,delta,metadata"
        )?;

        for credit in &self.decision_credits {
            writeln!(
                file,
                "{},{},{},{:.6},{:.6},{:.6},{}",
                credit.decision_id,
                credit.tick,
                credit.decision_type,
                credit.actual_score,
                credit.counterfactual_score,
                credit.delta,
                credit.metadata.replace(',', ";")
            )?;
        }

        Ok(())
    }

    fn export_summary_json(
        &self,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(last) = self.snapshots.last() {
            let first = &self.snapshots[0];
            let growth = last.global_coherence - first.global_coherence;

            let total_council = self.council_decisions.len();
            let approved = self
                .council_decisions
                .iter()
                .filter(|d| d.outcome == "Approve")
                .count();

            let summary = serde_json::json!({
                "run_id": self.run_id,
                "seed": self.seed,
                "config": self.config,
                "ticks": self.snapshots.len(),
                "final_coherence": last.global_coherence,
                "coherence_growth": growth,
                "skills_harvested": self.skills_harvested_total,
                "skills_promoted": self.skills_promoted_total,
                "skills_level_2_plus": last.skills_level_2_plus,
                "jury_pass_rate": last.jury_pass_rate,
                "mean_reward_per_tick": last.mean_agent_reward,
                "grpo_entropy": last.grpo_entropy,
                "council_proposals_resolved": total_council,
                "council_approve_rate": if total_council > 0 { approved as f64 / total_council as f64 } else { 0.0 },
                "dks_mean_stability": last.dks_mean_stability,
                "dks_final_population": last.dks_population_size,
            });

            let mut file = File::create(path)?;
            file.write_all(serde_json::to_string_pretty(&summary)?.as_bytes())?;
        }

        Ok(())
    }
}

/// Aggregate statistics across multiple runs
pub struct BatchAggregator;

impl BatchAggregator {
    pub fn aggregate_runs(
        run_dirs: &[&Path],
    ) -> Result<AggregatedStats, Box<dyn std::error::Error + Send + Sync>> {
        let mut all_coherence_trajectories: Vec<Vec<f64>> = Vec::new();
        let mut final_coherences: Vec<f64> = Vec::new();
        let mut coherence_growths: Vec<f64> = Vec::new();
        let mut skills_promoted: Vec<usize> = Vec::new();
        let mut council_approve_rates: Vec<f64> = Vec::new();
        let mut dks_stabilities: Vec<f64> = Vec::new();
        let mut credit_deltas: HashMap<String, Vec<f64>> = HashMap::new();

        for dir in run_dirs {
            // Summary files are named run_XX_seed_SEED_summary.json inside the run dir.
            // Try the exact name first; fall back to glob-style scan for any *_summary.json.
            let summary_content = std::fs::read_dir(dir)
                .ok()
                .and_then(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .find(|e| {
                            let name = e.file_name();
                            let s = name.to_string_lossy();
                            s.ends_with("_summary.json") && !s.contains("aggregate")
                        })
                        .and_then(|e| std::fs::read_to_string(e.path()).ok())
                })
                // Legacy fallback: flat summary.json
                .or_else(|| std::fs::read_to_string(dir.join("summary.json")).ok());

            if let Some(content) = summary_content {
                if let Ok(summary) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(coh) = summary["final_coherence"].as_f64() {
                        final_coherences.push(coh);
                    }
                    if let Some(growth) = summary["coherence_growth"].as_f64() {
                        coherence_growths.push(growth);
                    }
                    if let Some(skills) = summary["skills_promoted"].as_u64() {
                        skills_promoted.push(skills as usize);
                    }
                    if let Some(rate) = summary["council_approve_rate"].as_f64() {
                        council_approve_rates.push(rate);
                    }
                    if let Some(stab) = summary["dks_mean_stability"].as_f64() {
                        dks_stabilities.push(stab);
                    }
                }
            }

            // Load snapshots for trajectory
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path
                    .file_name()
                    .map(|n| n.to_string_lossy().contains("snapshots"))
                    .unwrap_or(false)
                {
                    // Parse CSV and extract coherence trajectory
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let trajectory: Vec<f64> = content
                            .lines()
                            .skip(1) // Skip header
                            .filter_map(|line| line.split(',').nth(2)?.parse::<f64>().ok())
                            .collect();
                        if !trajectory.is_empty() {
                            all_coherence_trajectories.push(trajectory);
                        }
                    }
                }
                if path
                    .file_name()
                    .map(|n| n.to_string_lossy().contains("credits"))
                    .unwrap_or(false)
                {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for line in content.lines().skip(1) {
                            let parts: Vec<&str> = line.split(',').collect();
                            if parts.len() < 6 {
                                continue;
                            }
                            let decision_type = parts[2].to_string();
                            if let Ok(delta) = parts[5].parse::<f64>() {
                                credit_deltas
                                    .entry(decision_type.clone())
                                    .or_default()
                                    .push(delta);
                                if decision_type == "council" && parts.len() >= 7 {
                                    let metadata = parts[6];
                                    let mut mode: Option<String> = None;
                                    for token in metadata.split_whitespace() {
                                        if let Some(value) = token.strip_prefix("mode=") {
                                            mode = Some(value.to_string());
                                            break;
                                        }
                                    }
                                    if let Some(mode) = mode {
                                        let key = format!("council:{}", mode);
                                        credit_deltas.entry(key).or_default().push(delta);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut credit_delta_mean: HashMap<String, f64> = HashMap::new();
        let mut credit_delta_std: HashMap<String, f64> = HashMap::new();
        for (decision_type, deltas) in &credit_deltas {
            credit_delta_mean.insert(decision_type.clone(), Self::mean(deltas));
            credit_delta_std.insert(decision_type.clone(), Self::std(deltas));
        }

        let coh_mean = Self::mean(&final_coherences);
        let coh_std = Self::std(&final_coherences);
        let final_coherence_cv = if coh_mean > 0.0 { coh_std / coh_mean } else { 0.0 };

        Ok(AggregatedStats {
            coherence_trajectories: all_coherence_trajectories,
            final_coherence_mean: coh_mean,
            final_coherence_std: coh_std,
            final_coherence_cv,
            coherence_growth_mean: Self::mean(&coherence_growths),
            coherence_growth_std: Self::std(&coherence_growths),
            skills_promoted_mean: Self::mean_usize(&skills_promoted),
            skills_promoted_std: Self::std_usize(&skills_promoted),
            council_approve_rate_mean: Self::mean(&council_approve_rates),
            council_approve_rate_std: Self::std(&council_approve_rates),
            dks_stability_mean: Self::mean(&dks_stabilities),
            dks_stability_std: Self::std(&dks_stabilities),
            credit_delta_mean,
            credit_delta_std,
        })
    }

    fn mean(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        values.iter().sum::<f64>() / values.len() as f64
    }

    fn std(values: &[f64]) -> f64 {
        if values.len() < 2 {
            return 0.0;
        }
        let mean = Self::mean(values);
        let variance =
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (values.len() - 1) as f64;
        variance.sqrt()
    }

    fn mean_usize(values: &[usize]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        values.iter().sum::<usize>() as f64 / values.len() as f64
    }

    fn std_usize(values: &[usize]) -> f64 {
        if values.len() < 2 {
            return 0.0;
        }
        let mean = Self::mean_usize(values);
        let variance: f64 = values
            .iter()
            .map(|v| (*v as f64 - mean).powi(2))
            .sum::<f64>()
            / (values.len() - 1) as f64;
        variance.sqrt()
    }
}

#[derive(Debug, Clone)]
pub struct AggregatedStats {
    pub coherence_trajectories: Vec<Vec<f64>>,
    pub final_coherence_mean: f64,
    pub final_coherence_std: f64,
    /// Coefficient of variation (std/mean) for final coherence.
    /// Lower CV = tighter, more predictable outcomes across seeds.
    pub final_coherence_cv: f64,
    pub coherence_growth_mean: f64,
    pub coherence_growth_std: f64,
    pub skills_promoted_mean: f64,
    pub skills_promoted_std: f64,
    pub council_approve_rate_mean: f64,
    pub council_approve_rate_std: f64,
    pub dks_stability_mean: f64,
    pub dks_stability_std: f64,
    pub credit_delta_mean: HashMap<String, f64>,
    pub credit_delta_std: HashMap<String, f64>,
}
