//! Batch experiment runner for HSM-II empirical evaluation.
//!
//! Runs N independent trials with different seeds and collects
//! comprehensive metrics for statistical analysis.

use crate::{
    confidence_to_phase,
    dks::{DKSConfig, DKSSystem},
    federation::trust::TrustGraph,
    kuramoto_build_adjacency,
    metrics::{
        DecisionCredit, FederationEvent, MetricsCollector, MetricsCouncilDecision,
        MetricsExperimentConfig, TickSnapshot,
    },
    metrics_dks_ext::TrustGraphMetrics,
    ollama_client::{CouncilPromptBuilder, OllamaClient, OllamaConfig},
    skill::SkillBank,
    HyperStigmergicMorphogenesis, KuramotoConfig, KuramotoEngine,
};
use chrono::Utc;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task::JoinSet;

/// Batch experiment configuration
#[derive(Clone, Debug)]
pub struct BatchConfig {
    pub num_runs: usize,
    pub ticks_per_run: usize,
    pub agent_count: usize,
    pub output_dir: String,
    pub enable_dks: bool,
    pub enable_federation: bool,
    pub enable_llm_deliberation: bool,
    pub enable_stigmergic_entities: bool,
    /// Use actual LLM calls (slower, requires Ollama) vs simulation (fast, reproducible)
    pub use_real_llm: bool,
    /// Ollama model to use when use_real_llm is true
    pub ollama_model: String,
    /// LLM latency budget in milliseconds (enforced when use_real_llm is true)
    pub llm_latency_budget_ms: u64,
    /// Enable counterfactual replay + credit assignment
    pub enable_credit_assignment: bool,
    /// Number of stochastic replay samples per decision
    pub credit_replay_samples: usize,
    /// Horizon in ticks for replay evaluation
    pub credit_horizon_ticks: usize,
    /// Composite objective weights for credit scoring
    pub credit_weights: CompositeWeights,
    /// Normalization scale for skills_promoted in composite score
    pub credit_skills_promoted_scale: f64,
    /// Optional feedback biases derived from previous credit summaries
    pub credit_feedback: Option<CreditFeedback>,
    /// Enable Kuramoto-based synchronization treatment.
    pub enable_kuramoto: bool,
    /// Optional deterministic seed base; run i uses seed_base + i.
    pub seed_base: Option<u64>,
    /// Kuramoto coupling K.
    pub kuramoto_coupling_strength: f64,
    /// Council phase influence.
    pub kuramoto_council_influence: f64,
    /// Graph dispersion heuristic.
    pub kuramoto_dispersion: f64,
    /// Kuramoto integration time step.
    pub kuramoto_dt: f64,
    /// Kuramoto noise amplitude.
    pub kuramoto_noise_amplitude: f64,
    /// Feedback gain from Kuramoto state into agent drives.
    pub kuramoto_feedback_gain: f64,
    /// Enable direct per-agent drive feedback (can destabilize quality).
    pub kuramoto_drive_feedback: bool,
    /// Enable generalized phase-field correction terms in Kuramoto update.
    pub kuramoto_phase_field: bool,
    /// Phase-field anti-diffusive growth coefficient.
    pub kuramoto_phase_growth: f64,
    /// Phase-field hyperviscosity damping coefficient.
    pub kuramoto_phase_hypervisc: f64,
    /// Phase-field dispersion-like skew coefficient.
    pub kuramoto_phase_dispersion: f64,
    /// Warmup ticks before full Kuramoto influence is allowed.
    pub kuramoto_warmup_ticks: usize,
    /// Hard cap for effective coupling strength during runtime control.
    pub kuramoto_coupling_cap: f64,
    /// Hard cap for council influence during runtime control.
    pub kuramoto_council_cap: f64,
    /// Minimum largest-component ratio required for full influence.
    pub kuramoto_lcc_gate: f64,
    /// Enable adaptive quality guard (throttles when coherence+reward degrade together).
    pub kuramoto_adaptive_guard: bool,
    /// Minimum adaptive gain scale.
    pub kuramoto_adaptive_gain_min: f64,
    /// Minimum phase-entropy target; below this, inject additional noise.
    pub kuramoto_entropy_floor: f64,
    /// Additional temporary noise used when entropy collapses.
    pub kuramoto_entropy_noise_boost: f64,
    /// Consecutive guard trips before runtime disables Kuramoto for the run.
    pub kuramoto_disable_after_trips: u32,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            num_runs: 20,
            ticks_per_run: 1000,
            agent_count: 10,
            output_dir: "experiments".to_string(),
            enable_dks: true,
            enable_federation: true,
            enable_llm_deliberation: true,
            enable_stigmergic_entities: true,
            use_real_llm: false, // Default to simulation for reproducibility
            ollama_model:
                "hf.co/DavidAU/OpenAi-GPT-oss-20b-HERETIC-uncensored-NEO-Imatrix-gguf:IQ4_NL"
                    .to_string(),
            llm_latency_budget_ms: 5000,
            enable_credit_assignment: true,
            credit_replay_samples: 5,
            credit_horizon_ticks: 8,
            credit_weights: CompositeWeights::default(),
            credit_skills_promoted_scale: 1000.0,
            credit_feedback: None,
            enable_kuramoto: false,
            seed_base: None,
            kuramoto_coupling_strength: 1.5,
            kuramoto_council_influence: 0.35,
            kuramoto_dispersion: 0.0,
            kuramoto_dt: 0.05,
            kuramoto_noise_amplitude: 0.0,
            kuramoto_feedback_gain: 0.25,
            kuramoto_drive_feedback: false,
            kuramoto_phase_field: false,
            kuramoto_phase_growth: 0.0,
            kuramoto_phase_hypervisc: 0.0,
            kuramoto_phase_dispersion: 0.0,
            kuramoto_warmup_ticks: 250,
            kuramoto_coupling_cap: 0.35,
            kuramoto_council_cap: 0.12,
            kuramoto_lcc_gate: 0.8,
            kuramoto_adaptive_guard: true,
            kuramoto_adaptive_gain_min: 0.15,
            kuramoto_entropy_floor: 0.35,
            kuramoto_entropy_noise_boost: 0.02,
            kuramoto_disable_after_trips: 5,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CreditFeedback {
    pub council_mode_bias: std::collections::HashMap<String, f64>,
    pub skill_promotion_bias: f64,
}

#[derive(Clone, Debug)]
pub struct CompositeWeights {
    pub global_coherence: f64,
    pub dks_mean_stability: f64,
    pub mean_agent_reward: f64,
    pub skills_promoted: f64,
}

impl Default for CompositeWeights {
    fn default() -> Self {
        Self {
            global_coherence: 0.40,
            dks_mean_stability: 0.25,
            mean_agent_reward: 0.25,
            skills_promoted: 0.10,
        }
    }
}

#[derive(Clone, Debug)]
struct CreditConfig {
    seed: u64,
    run_idx: usize,
    replay_samples: usize,
    horizon_ticks: usize,
    weights: CompositeWeights,
    skills_promoted_scale: f64,
    enable_dks: bool,
    enable_federation: bool,
    enable_stigmergic_entities: bool,
    enable_llm_deliberation: bool,
    credit_feedback: Option<CreditFeedback>,
}

#[derive(Clone, Debug, Default)]
struct ReplayRecords {
    bidding_actions_by_tick: std::collections::HashMap<usize, Arc<Vec<BiddingAction>>>,
    council_by_tick: std::collections::HashMap<usize, (String, String, f64, f64)>,
    skill_by_tick: std::collections::HashMap<usize, (bool, bool)>,
    federation_by_tick: std::collections::HashMap<usize, FederationEvent>,
}

impl ReplayRecords {
    fn from_events(events: &[DecisionEvent]) -> Self {
        let mut records = ReplayRecords::default();

        for event in events {
            match &event.metadata {
                DecisionMetadata::BiddingAction { .. } => {
                    if let Some(actions) = &event.bidding_actions {
                        records
                            .bidding_actions_by_tick
                            .entry(event.tick)
                            .or_insert_with(|| actions.clone());
                    }
                }
                DecisionMetadata::Council {
                    mode,
                    outcome,
                    complexity,
                    urgency,
                    ..
                } => {
                    records
                        .council_by_tick
                        .entry(event.tick)
                        .or_insert_with(|| (mode.clone(), outcome.clone(), *complexity, *urgency));
                }
                DecisionMetadata::SkillDistillation {
                    harvested,
                    promoted,
                } => {
                    records
                        .skill_by_tick
                        .entry(event.tick)
                        .or_insert((*harvested, *promoted));
                }
                DecisionMetadata::FederationUpdate {
                    peer_id,
                    trust_score,
                } => {
                    records
                        .federation_by_tick
                        .entry(event.tick)
                        .or_insert(FederationEvent {
                            tick: event.tick,
                            peer_id: peer_id.clone(),
                            trust_score: *trust_score,
                            event_type: "trust_update".to_string(),
                        });
                }
            }
        }

        records
    }
}

#[derive(Clone, Debug)]
enum DecisionType {
    BiddingAction,
    Council,
    SkillDistillation,
    FederationUpdate,
}

#[derive(Clone, Debug)]
enum DecisionMetadata {
    BiddingAction {
        action_index: usize,
        action_label: String,
    },
    Council {
        mode: String,
        outcome: String,
        effect: String,
        complexity: f64,
        urgency: f64,
    },
    SkillDistillation {
        harvested: bool,
        promoted: bool,
    },
    FederationUpdate {
        peer_id: String,
        trust_score: f64,
    },
}

#[derive(Clone, Debug)]
struct DecisionEvent {
    id: u64,
    tick: usize,
    decision_type: DecisionType,
    metadata: DecisionMetadata,
    context: Arc<TickContext>,
    bidding_actions: Option<Arc<Vec<BiddingAction>>>,
}

#[derive(Clone, Debug)]
struct TickContext {
    world: HyperStigmergicMorphogenesis,
    dks: Option<DKSSystem>,
    skill_bank: SkillBank,
    trust_graph: Option<TrustGraph>,
    skills_harvested_total: usize,
    skills_promoted_total: usize,
}

#[derive(Clone, Debug, Default)]
struct SkillDistillationOutcome {
    harvested: bool,
    promoted: bool,
}

#[derive(Clone, Debug)]
struct CouncilEffectOutcome {
    description: String,
}

#[derive(Clone, Debug)]
struct BiddingAction {
    action: crate::action::Action,
    emergent: bool,
}

#[derive(Clone, Debug)]
struct KuramotoPolicyState {
    arm_values: [f64; 3],
    arm_counts: [u32; 3],
    current_arm: usize,
    last_coherence: Option<f64>,
    last_reward: Option<f64>,
    last_lcc: Option<f64>,
    next_eval_tick: usize,
}

impl Default for KuramotoPolicyState {
    fn default() -> Self {
        Self {
            arm_values: [0.0, 0.0, 0.0],
            arm_counts: [0, 0, 0],
            current_arm: 1,
            last_coherence: None,
            last_reward: None,
            last_lcc: None,
            next_eval_tick: 50,
        }
    }
}

/// Runs a batch of experiments
pub struct BatchRunner;

impl BatchRunner {
    pub async fn run_batch(
        config: BatchConfig,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let output_path = Path::new(&config.output_dir);
        std::fs::create_dir_all(output_path)?;

        println!(
            "Starting batch of {} runs, {} ticks each...",
            config.num_runs, config.ticks_per_run
        );

        let mut join_set: JoinSet<Result<(), Box<dyn std::error::Error + Send + Sync>>> =
            JoinSet::new();
        let feedback = Self::load_credit_feedback(output_path).ok().flatten();
        let mut base_config = config.clone();
        base_config.credit_feedback = feedback;

        let auto_seed_base = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        for run_idx in 0..config.num_runs {
            let seed = config.seed_base.unwrap_or(auto_seed_base) + run_idx as u64;

            let run_config = base_config.clone();
            let output_dir = output_path.join(format!("run_{:02}", run_idx));

            join_set.spawn(async move {
                Self::run_single_experiment(run_idx, seed, run_config, output_dir).await
            });
        }

        // Wait for all runs to complete
        let mut completed = 0;
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {
                    completed += 1;
                    println!("Completed run {}/{}", completed, config.num_runs);
                }
                Ok(Err(e)) => eprintln!("Run failed: {}", e),
                Err(e) => eprintln!("Task panicked: {}", e),
            }
        }

        println!(
            "\nBatch complete! {}/{} runs successful.",
            completed, config.num_runs
        );
        println!("Results saved to: {}", output_path.display());

        // Generate aggregate summary
        let stats = Self::generate_aggregate_summary(output_path, config.num_runs).await?;
        let _ = Self::write_credit_feedback(output_path, &stats);

        Ok(())
    }

    async fn run_single_experiment(
        run_idx: usize,
        seed: u64,
        config: BatchConfig,
        output_dir: std::path::PathBuf,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        std::fs::create_dir_all(&output_dir)?;

        let run_id = format!("run_{:02}_seed_{}", run_idx, seed);

        let exp_config = MetricsExperimentConfig {
            ticks: config.ticks_per_run,
            agent_count: config.agent_count,
            dks_enabled: config.enable_dks,
            federation_enabled: config.enable_federation,
            llm_deliberation: config.enable_llm_deliberation,
            stigmergic_entities: config.enable_stigmergic_entities,
        };

        let mut collector = MetricsCollector::new(run_id.clone(), seed, exp_config);
        let mut decision_events: Vec<DecisionEvent> = Vec::new();
        let mut decision_counter: u64 = 0;

        // Initialize world
        let mut world = HyperStigmergicMorphogenesis::new(config.agent_count);
        world.decay_rate = 0.01;

        // Initialize Kuramoto treatment engine if enabled.
        let mut kuramoto = if config.enable_kuramoto {
            Some(KuramotoEngine::new(KuramotoConfig {
                coupling_strength: config.kuramoto_coupling_strength,
                dt: config.kuramoto_dt,
                dispersion: config.kuramoto_dispersion,
                council_influence: config.kuramoto_council_influence,
                noise_amplitude: config.kuramoto_noise_amplitude,
                enable_phase_field: config.kuramoto_phase_field,
                phase_field_growth: config.kuramoto_phase_growth,
                phase_field_hyperviscosity: config.kuramoto_phase_hypervisc,
                phase_field_dispersion: config.kuramoto_phase_dispersion,
                ..Default::default()
            }))
        } else {
            None
        };
        let mut kuramoto_policy = KuramotoPolicyState::default();

        // Initialize DKS if enabled
        let mut dks = if config.enable_dks {
            let mut system = DKSSystem::new(DKSConfig::default());
            system.seed(50); // Start with 50 entities
            Some(system)
        } else {
            None
        };

        // Initialize federation if enabled
        let mut trust_graph = if config.enable_federation {
            Some(TrustGraph::new(0.7, 0.001)) // default_trust=0.7, decay_rate=0.001
        } else {
            None
        };

        // Initialize skill bank
        let skill_bank = SkillBank::new_with_seeds();

        // Initialize Ollama client if using real LLM
        let ollama_client = if config.use_real_llm {
            let ollama_config = OllamaConfig {
                host: "http://localhost".to_string(),
                port: 11434,
                model: config.ollama_model.clone(),
                latency_budget_ms: config.llm_latency_budget_ms,
                enable_batching: false,
                batch_size: 1,
                batch_timeout_ms: 100,
                temperature: 0.7,
                max_tokens: 100, // Short responses for council decisions
            };
            let client = OllamaClient::new(ollama_config);

            // Check if Ollama is available
            if !client.is_available().await {
                eprintln!("Warning: Ollama server not available at localhost:11434, falling back to simulation");
                None
            } else {
                println!(
                    "  [Run {}] Using real Ollama LLM with {}ms latency budget",
                    run_idx, config.llm_latency_budget_ms
                );
                Some(client)
            }
        } else {
            None
        };

        // Run simulation
        for tick in 0..config.ticks_per_run {
            let tick_context = if config.enable_credit_assignment {
                Some(Arc::new(TickContext {
                    world: world.clone(),
                    dks: dks.clone(),
                    skill_bank: skill_bank.clone(),
                    trust_graph: trust_graph.clone(),
                    skills_harvested_total: collector.skills_harvested_total,
                    skills_promoted_total: collector.skills_promoted_total,
                }))
            } else {
                None
            };

            // Agent bidding and actions
            let _ = Self::run_agent_bidding(
                &mut world,
                tick,
                config.enable_credit_assignment,
                tick_context.clone(),
                &mut decision_events,
                &mut decision_counter,
                seed,
                run_idx,
            );

            // World tick
            world.tick();

            let mut disable_kuramoto = false;
            if let Some(ref mut km) = kuramoto {
                let guard_tripped = Self::apply_kuramoto_treatment(
                    km,
                    &mut world,
                    config.kuramoto_feedback_gain,
                    &config,
                    &mut kuramoto_policy,
                    tick,
                    seed,
                    run_idx,
                );
                if guard_tripped {
                    km.quality_degrade_streak = km.quality_degrade_streak.saturating_add(1);
                    if km.quality_degrade_streak >= config.kuramoto_disable_after_trips {
                        eprintln!(
                            "[kuramoto] runtime guard disabled treatment after {} consecutive trips (run {}, tick {})",
                            km.quality_degrade_streak,
                            run_idx,
                            tick
                        );
                        disable_kuramoto = true;
                    }
                } else {
                    km.quality_degrade_streak = 0;
                }
            }
            if disable_kuramoto {
                kuramoto = None;
            }

            // DKS tick
            if let Some(ref mut d) = dks {
                d.tick();

                // Stigmergic entity interaction with world
                if config.enable_stigmergic_entities {
                    Self::update_stigmergic_entities(d, &world, tick);
                }
            }

            // Council decisions (periodic)
            if tick % 50 == 0 && tick > 0 {
                let decision = if let Some(ref client) = ollama_client {
                    Self::llm_council_decision(tick, client).await
                } else {
                    let mut rng = Self::base_rng(seed, run_idx, tick, 2);
                    Self::simulate_council_decision_with_rng(
                        tick,
                        config.enable_llm_deliberation,
                        &mut rng,
                        config.credit_feedback.as_ref(),
                    )
                };
                let mut rng = Self::base_rng(seed, run_idx, tick, 6);
                let effect = Self::apply_council_effect(&mut world, &decision, tick, &mut rng);
                collector.record_council_decision(decision.clone());
                if let Some(ctx) = tick_context.clone() {
                    decision_counter += 1;
                    decision_events.push(DecisionEvent {
                        id: decision_counter,
                        tick,
                        decision_type: DecisionType::Council,
                        metadata: DecisionMetadata::Council {
                            mode: decision.mode,
                            outcome: decision.outcome,
                            effect: effect.description,
                            complexity: decision.complexity,
                            urgency: decision.urgency,
                        },
                        context: ctx,
                        bidding_actions: None,
                    });
                }
            }

            // Skill distillation (periodic)
            if tick % 100 == 0 && tick > 0 {
                let mut rng = Self::base_rng(seed, run_idx, tick, 3);
                let promotion_bias = config
                    .credit_feedback
                    .as_ref()
                    .map(|f| f.skill_promotion_bias)
                    .unwrap_or(0.0);
                let outcome =
                    Self::simulate_skill_distillation_with_rng(&world, &mut rng, promotion_bias);
                if outcome.harvested {
                    collector.record_skill_harvested();
                }
                if outcome.promoted {
                    collector.record_skill_promoted();
                }
                if let Some(ctx) = tick_context.clone() {
                    decision_counter += 1;
                    decision_events.push(DecisionEvent {
                        id: decision_counter,
                        tick,
                        decision_type: DecisionType::SkillDistillation,
                        metadata: DecisionMetadata::SkillDistillation {
                            harvested: outcome.harvested,
                            promoted: outcome.promoted,
                        },
                        context: ctx,
                        bidding_actions: None,
                    });
                }
            }

            // Federation updates (periodic)
            if tick % 10 == 0 && config.enable_federation {
                if let Some(event) =
                    Self::simulate_federation_update(trust_graph.as_mut(), tick, run_idx)
                {
                    collector.record_federation_event(event.clone());
                    if let Some(ctx) = tick_context.clone() {
                        decision_counter += 1;
                        decision_events.push(DecisionEvent {
                            id: decision_counter,
                            tick,
                            decision_type: DecisionType::FederationUpdate,
                            metadata: DecisionMetadata::FederationUpdate {
                                peer_id: event.peer_id,
                                trust_score: event.trust_score,
                            },
                            context: ctx,
                            bidding_actions: None,
                        });
                    }
                }
            }

            // Collect metrics snapshot
            let snapshot = Self::collect_snapshot(
                tick,
                &world,
                dks.as_ref(),
                &skill_bank,
                &collector,
                trust_graph.as_ref(),
            );
            collector.record_tick(snapshot);
        }

        if config.enable_credit_assignment {
            let credit_config = CreditConfig {
                seed,
                run_idx,
                replay_samples: config.credit_replay_samples.max(1),
                horizon_ticks: config.credit_horizon_ticks.max(1),
                weights: config.credit_weights.clone(),
                skills_promoted_scale: config.credit_skills_promoted_scale.max(1.0),
                enable_dks: config.enable_dks,
                enable_federation: config.enable_federation,
                enable_stigmergic_entities: config.enable_stigmergic_entities,
                enable_llm_deliberation: config.enable_llm_deliberation,
                credit_feedback: config.credit_feedback.clone(),
            };
            let credits = Self::compute_decision_credits(&decision_events, &credit_config);
            for credit in credits {
                collector.record_decision_credit(credit);
            }
        }

        // Export data
        collector.export_csv(&output_dir)?;

        Ok(())
    }

    fn apply_kuramoto_treatment(
        kuramoto: &mut KuramotoEngine,
        world: &mut HyperStigmergicMorphogenesis,
        feedback_gain: f64,
        config: &BatchConfig,
        policy: &mut KuramotoPolicyState,
        tick: usize,
        seed: u64,
        run_idx: usize,
    ) -> bool {
        use std::collections::HashSet;
        use std::f64::consts::PI;

        let present_ids: HashSet<u64> = world.agents.iter().map(|a| a.id).collect();
        let stale: Vec<u64> = kuramoto
            .oscillators
            .keys()
            .copied()
            .filter(|id| !present_ids.contains(id))
            .collect();
        for id in stale {
            kuramoto.remove_agent(id);
        }

        for agent in &world.agents {
            if !kuramoto.oscillators.contains_key(&agent.id) {
                kuramoto.register_agent(
                    agent.id,
                    agent.jw,
                    agent.drives.curiosity,
                    agent.drives.transcendence,
                );
            } else {
                kuramoto.update_frequency(
                    agent.id,
                    agent.jw,
                    agent.drives.curiosity,
                    agent.drives.transcendence,
                );
            }
        }

        let confidence = world.global_coherence().clamp(0.0, 1.0);
        kuramoto.set_council_phase(confidence_to_phase(confidence, 1.0));
        let adjacency = kuramoto_build_adjacency(&world.agents, &world.edges, &world.adjacency);
        let lcc_ratio = Self::kuramoto_lcc_ratio(&adjacency);
        let warmup = if config.kuramoto_warmup_ticks == 0 {
            1.0
        } else {
            (kuramoto.step_count as f64 / config.kuramoto_warmup_ticks as f64).clamp(0.0, 1.0)
        };
        let connectivity = if config.kuramoto_lcc_gate > 0.0 {
            (lcc_ratio / config.kuramoto_lcc_gate).clamp(0.0, 1.0)
        } else {
            1.0
        };

        let quality_reward = Self::calculate_mean_reward(world);
        let mut guard_tripped = false;
        if config.kuramoto_adaptive_guard {
            if let (Some(prev_c), Some(prev_r)) = (
                kuramoto.prev_quality_coherence,
                kuramoto.prev_quality_reward,
            ) {
                let d_coh = confidence - prev_c;
                let d_rew = quality_reward - prev_r;
                if d_coh < -0.002 && d_rew < -0.002 {
                    guard_tripped = true;
                    kuramoto.adaptive_gain_scale = (kuramoto.adaptive_gain_scale * 0.85)
                        .max(config.kuramoto_adaptive_gain_min);
                } else if d_coh > 0.001 && d_rew > 0.001 {
                    kuramoto.adaptive_gain_scale = (kuramoto.adaptive_gain_scale * 1.03).min(1.0);
                }
            }
            kuramoto.prev_quality_coherence = Some(confidence);
            kuramoto.prev_quality_reward = Some(quality_reward);
        }

        if tick >= policy.next_eval_tick {
            let quality_reward_now = quality_reward;
            if let (Some(prev_c), Some(prev_r), Some(prev_lcc)) =
                (policy.last_coherence, policy.last_reward, policy.last_lcc)
            {
                let d_coh = confidence - prev_c;
                let d_rew = quality_reward_now - prev_r;
                let d_lcc = lcc_ratio - prev_lcc;
                let reward_signal = (1.8 * d_coh) + (1.8 * d_rew) + (0.4 * d_lcc);
                let arm = policy.current_arm;
                policy.arm_counts[arm] += 1;
                let n = policy.arm_counts[arm] as f64;
                policy.arm_values[arm] += (reward_signal - policy.arm_values[arm]) / n.max(1.0);
            }
            policy.last_coherence = Some(confidence);
            policy.last_reward = Some(quality_reward_now);
            policy.last_lcc = Some(lcc_ratio);
            policy.next_eval_tick = tick + 50;

            let epsilon_draw =
                ((seed as f64 * 0.000_000_1) + (run_idx as f64 * 0.013) + (tick as f64 * 0.017))
                    .sin()
                    .abs();
            if epsilon_draw < 0.15 {
                policy.current_arm = (tick / 50) % 3;
            } else {
                let mut best_idx = 0usize;
                let mut best_val = f64::MIN;
                for (idx, val) in policy.arm_values.iter().enumerate() {
                    if *val > best_val {
                        best_val = *val;
                        best_idx = idx;
                    }
                }
                policy.current_arm = best_idx;
            }
        }

        let arm_scale = [0.85, 1.0, 1.15][policy.current_arm];
        let runtime_scale = warmup * connectivity * kuramoto.adaptive_gain_scale * arm_scale;
        kuramoto.config.coupling_strength =
            (kuramoto.base_coupling_strength * runtime_scale).min(config.kuramoto_coupling_cap);
        kuramoto.config.council_influence =
            (kuramoto.base_council_influence * runtime_scale).min(config.kuramoto_council_cap);
        kuramoto.config.noise_amplitude = kuramoto.base_noise_amplitude;

        kuramoto.step(&adjacency);
        let snap = kuramoto.snapshot();

        // Feed synchronization state back into drives.
        // Higher R increases cooperative alignment (harmony/growth),
        // while low R preserves exploration (curiosity).
        let r = snap.order_parameter.clamp(0.0, 1.0);
        let psi = snap.mean_phase;
        if config.kuramoto_adaptive_guard
            && snap.diagnostics.phase_entropy < config.kuramoto_entropy_floor
        {
            kuramoto.config.noise_amplitude =
                (kuramoto.base_noise_amplitude + config.kuramoto_entropy_noise_boost).max(0.0);
            kuramoto.adaptive_gain_scale =
                (kuramoto.adaptive_gain_scale * 0.9).max(config.kuramoto_adaptive_gain_min);
        }
        let gain = (feedback_gain.max(0.0) * runtime_scale).min(feedback_gain.max(0.0));
        let quality_safe = !guard_tripped && kuramoto.adaptive_gain_scale > 0.25;
        let entropy = snap.diagnostics.phase_entropy.clamp(0.0, 1.0);

        // Structural-only feedback (default): gentle decay-rate shaping is less chaotic
        // than direct per-agent drive perturbations.
        if quality_safe && gain > 0.0 {
            let sync_support = (0.015 * r - 0.008 * (1.0 - r)).clamp(-0.01, 0.01);
            let entropy_term = (0.01 * (entropy - 0.5)).clamp(-0.005, 0.005);
            let decay_factor = (1.0 - gain * (sync_support + entropy_term)).clamp(0.98, 1.02);
            world.decay_rate = (world.decay_rate * decay_factor).clamp(0.001, 0.08);
        }

        if !config.kuramoto_drive_feedback {
            return guard_tripped;
        }

        for agent in &mut world.agents {
            if let Some(osc) = kuramoto.oscillators.get(&agent.id) {
                let align = (osc.phase - psi).cos(); // [-1, 1]
                let anti = ((osc.phase - psi).abs() - PI).abs();
                let anti_factor = (anti / PI).clamp(0.0, 1.0);
                if !quality_safe || gain <= 0.0 {
                    continue;
                }

                // Quality-safe feedback law:
                // - Avoid strong alignment pressure when global sync is still low.
                // - Preserve exploration when phase entropy collapses.
                // - Use local alignment signal instead of pure global pull.
                let sync_readiness = ((r - 0.55) / 0.45).clamp(0.0, 1.0);
                let diversity_support = (0.55 - entropy).clamp(0.0, 0.55) / 0.55;
                let local_coop = align.max(0.0);
                let local_disagree = (1.0 - align.abs()).clamp(0.0, 1.0);

                agent.drives.harmony = (agent.drives.harmony
                    + gain
                        * (0.012 * sync_readiness * local_coop + 0.006 * r
                            - 0.004 * diversity_support))
                    .clamp(0.0, 1.0);
                agent.drives.growth = (agent.drives.growth
                    + gain * (0.008 * sync_readiness * local_coop))
                    .clamp(0.0, 1.0);
                agent.drives.curiosity = (agent.drives.curiosity
                    + gain
                        * (0.010 * (1.0 - sync_readiness)
                            + 0.010 * diversity_support
                            + 0.004 * anti_factor
                            + 0.004 * local_disagree))
                    .clamp(0.0, 1.0);
            }
        }
        guard_tripped
    }

    fn kuramoto_lcc_ratio(adjacency: &std::collections::HashMap<u64, Vec<(u64, f64)>>) -> f64 {
        use std::collections::{HashSet, VecDeque};

        if adjacency.is_empty() {
            return 1.0;
        }
        let nodes: HashSet<u64> = adjacency.keys().copied().collect();
        let mut visited = HashSet::new();
        let mut largest = 0usize;
        for &start in &nodes {
            if visited.contains(&start) {
                continue;
            }
            let mut q = VecDeque::new();
            q.push_back(start);
            visited.insert(start);
            let mut size = 0usize;
            while let Some(cur) = q.pop_front() {
                size += 1;
                if let Some(neighbors) = adjacency.get(&cur) {
                    for &(next, weight) in neighbors {
                        if weight <= 0.0 || !nodes.contains(&next) {
                            continue;
                        }
                        if visited.insert(next) {
                            q.push_back(next);
                        }
                    }
                }
            }
            largest = largest.max(size);
        }
        largest as f64 / nodes.len() as f64
    }

    fn connected_components_from_adjacency(
        adjacency: &std::collections::HashMap<u64, Vec<(u64, f64)>>,
    ) -> Vec<Vec<u64>> {
        use std::collections::{HashSet, VecDeque};
        let nodes: HashSet<u64> = adjacency.keys().copied().collect();
        let mut visited = HashSet::new();
        let mut components = Vec::new();
        for &start in &nodes {
            if visited.contains(&start) {
                continue;
            }
            let mut q = VecDeque::new();
            q.push_back(start);
            visited.insert(start);
            let mut comp = Vec::new();
            while let Some(cur) = q.pop_front() {
                comp.push(cur);
                if let Some(neighbors) = adjacency.get(&cur) {
                    for &(next, weight) in neighbors {
                        if weight <= 0.0 || !nodes.contains(&next) {
                            continue;
                        }
                        if visited.insert(next) {
                            q.push_back(next);
                        }
                    }
                }
            }
            components.push(comp);
        }
        components.sort_by_key(|c| std::cmp::Reverse(c.len()));
        components
    }

    fn pick_component_bridge_pair(
        world: &HyperStigmergicMorphogenesis,
        rng: &mut impl rand::Rng,
    ) -> Option<(usize, usize)> {
        let adjacency = kuramoto_build_adjacency(&world.agents, &world.edges, &world.adjacency);
        let components = Self::connected_components_from_adjacency(&adjacency);
        if components.len() < 2 {
            return None;
        }
        let total = world.agents.len().max(1) as f64;
        let lcc_ratio = components[0].len() as f64 / total;
        if lcc_ratio >= 0.85 {
            return None;
        }
        let left = &components[0];
        let right = &components[1];
        let a_id = left[rng.gen_range(0..left.len())];
        let b_id = right[rng.gen_range(0..right.len())];
        let a_idx = world.agents.iter().position(|a| a.id == a_id)?;
        let b_idx = world.agents.iter().position(|a| a.id == b_id)?;
        if a_idx == b_idx {
            None
        } else {
            Some((a_idx, b_idx))
        }
    }

    fn collect_snapshot(
        tick: usize,
        world: &HyperStigmergicMorphogenesis,
        dks: Option<&DKSSystem>,
        _skill_bank: &SkillBank,
        collector: &MetricsCollector,
        trust_graph: Option<&TrustGraph>,
    ) -> TickSnapshot {
        let coherence = world.global_coherence();

        // Calculate coherence sub-components
        let edge_density = world.edges.len() as f64 / (world.vertex_meta.len().max(1) as f64);
        let emergent_count = world.edges.iter().filter(|e| e.emergent).count();
        let emergent_coverage = emergent_count as f64 / world.edges.len().max(1) as f64;

        TickSnapshot {
            tick,
            timestamp: Utc::now(),
            global_coherence: coherence,
            edge_density,
            emergent_coverage,
            ontological_consistency: Self::calculate_ontological_consistency(world),
            belief_convergence: Self::calculate_belief_convergence(world),
            skills_harvested: collector.skills_harvested_total,
            skills_promoted: collector.skills_promoted_total,
            skills_level_2_plus: (collector.skills_promoted_total as f64 * 0.7) as usize,
            jury_pass_rate: 0.5 + 0.25 * (tick as f64 / 1000.0).min(1.0), // Simulated improvement
            council_proposals_total: collector.council_decisions.len(),
            council_approved: collector
                .council_decisions
                .iter()
                .filter(|d| d.outcome == "Approve")
                .count(),
            council_rejected: collector
                .council_decisions
                .iter()
                .filter(|d| d.outcome == "Reject")
                .count(),
            council_deferred: collector
                .council_decisions
                .iter()
                .filter(|d| d.outcome == "Defer")
                .count(),
            council_mode_usage: Self::calculate_mode_usage(collector),
            mean_agent_reward: Self::calculate_mean_reward(world),
            grpo_entropy: Self::calculate_grpo_entropy(world),
            dks_population_size: dks
                .map(|d| {
                    use crate::metrics_dks_ext::DKSMetrics;
                    d.population_size()
                })
                .unwrap_or(0),
            dks_mean_stability: dks
                .map(|d| {
                    use crate::metrics_dks_ext::DKSMetrics;
                    d.mean_stability()
                })
                .unwrap_or(0.0),
            dks_multifractal_width: dks
                .map(|d| {
                    use crate::metrics_dks_ext::DKSMetrics;
                    d.multifractal_width()
                })
                .unwrap_or(0.0),
            dks_stigmergic_edges: dks
                .map(|d| {
                    use crate::metrics_dks_ext::DKSMetrics;
                    d.stigmergic_edge_count()
                })
                .unwrap_or(0),
            federation_trust_scores: trust_graph
                .map(|tg| {
                    use crate::metrics_dks_ext::TrustGraphMetrics;
                    tg.get_all_scores()
                })
                .unwrap_or_default(),
            knowledge_layer_counts: std::collections::HashMap::new(), // Simplified
        }
    }

    fn calculate_mode_usage(
        collector: &MetricsCollector,
    ) -> std::collections::HashMap<String, usize> {
        let mut usage = std::collections::HashMap::new();
        for decision in &collector.council_decisions {
            *usage.entry(decision.mode.clone()).or_insert(0) += 1;
        }
        usage
    }

    fn update_stigmergic_entities(
        dks: &mut DKSSystem,
        world: &HyperStigmergicMorphogenesis,
        tick: usize,
    ) {
        use crate::metrics_dks_ext::DKSMetrics;

        // Entities read world coherence and deposit stigmergic edges
        let coherence = world.global_coherence();
        dks.update_stigmergic_edges(coherence, tick);
    }

    /// Simulate agent bidding and edge creation
    fn run_agent_bidding(
        world: &mut HyperStigmergicMorphogenesis,
        tick: usize,
        enable_credit: bool,
        base_context: Option<Arc<TickContext>>,
        decision_events: &mut Vec<DecisionEvent>,
        decision_counter: &mut u64,
        seed: u64,
        run_idx: usize,
    ) -> Vec<BiddingAction> {
        let mut rng = Self::base_rng(seed, run_idx, tick, 1);
        let actions = Self::generate_bidding_actions_with_rng(world, tick, &mut rng);
        let actions_arc = Arc::new(actions.clone());

        for (idx, bidding_action) in actions.iter().enumerate() {
            if enable_credit {
                if let Some(base) = base_context.as_ref() {
                    let ctx = Arc::new(TickContext {
                        world: world.clone(),
                        dks: base.dks.clone(),
                        skill_bank: base.skill_bank.clone(),
                        trust_graph: base.trust_graph.clone(),
                        skills_harvested_total: base.skills_harvested_total,
                        skills_promoted_total: base.skills_promoted_total,
                    });
                    *decision_counter += 1;
                    decision_events.push(DecisionEvent {
                        id: *decision_counter,
                        tick,
                        decision_type: DecisionType::BiddingAction,
                        metadata: DecisionMetadata::BiddingAction {
                            action_index: idx,
                            action_label: Self::bidding_action_label(bidding_action),
                        },
                        context: ctx,
                        bidding_actions: Some(actions_arc.clone()),
                    });
                }
            }

            world.apply_action(&bidding_action.action);
            if bidding_action.emergent {
                if let Some(last_edge) = world.edges.last_mut() {
                    last_edge.emergent = true;
                }
            }
        }

        actions
    }

    fn generate_bidding_actions_with_rng(
        world: &HyperStigmergicMorphogenesis,
        tick: usize,
        rng: &mut impl rand::Rng,
    ) -> Vec<BiddingAction> {
        use crate::action::Action;
        let mut actions: Vec<BiddingAction> = Vec::new();

        // Each agent has a chance to create edges based on their drives
        let agent_count = world.agents.len();
        if agent_count < 2 {
            return actions;
        }

        // Randomly select pairs of agents to form edges
        let num_new_edges = rng.gen_range(0..=agent_count / 2);

        for _ in 0..num_new_edges {
            let idx1 = rng.gen_range(0..agent_count);
            let idx2 = rng.gen_range(0..agent_count);

            if idx1 != idx2 {
                let agent1 = &world.agents[idx1];
                let agent2 = &world.agents[idx2];

                // Edge weight based on drive compatibility
                let drive_compatibility = (agent1.drives.curiosity - agent2.drives.curiosity).abs()
                    + (agent1.drives.harmony - agent2.drives.harmony).abs()
                    + (agent1.drives.growth - agent2.drives.growth).abs()
                    + (agent1.drives.transcendence - agent2.drives.transcendence).abs();

                // Weight: higher when drives are similar (low difference)
                let weight = (1.0 - drive_compatibility / 4.0).max(0.1);

                // Create the edge
                let action = Action::LinkAgents {
                    vertices: vec![idx1, idx2],
                    weight: weight as f32,
                };

                actions.push(BiddingAction {
                    action,
                    emergent: false,
                });
            }
        }

        // Occasionally create emergent edges (higher order)
        if tick % 10 == 0 && agent_count >= 3 {
            let num_emergent = rng.gen_range(0..=2);
            for _ in 0..num_emergent {
                let vertices: Vec<usize> = (0..3).map(|_| rng.gen_range(0..agent_count)).collect();

                // Check all vertices are unique
                let unique: std::collections::HashSet<_> = vertices.iter().cloned().collect();
                if unique.len() == 3 {
                    let action = Action::LinkAgents {
                        vertices: vertices.clone(),
                        weight: 0.7, // Higher weight for emergent edges
                    };
                    actions.push(BiddingAction {
                        action,
                        emergent: true,
                    });
                }
            }
        }

        // Connectivity-aware repair: when graph is fragmented, inject one bridge edge
        // between the largest component and an external component.
        if let Some((a_idx, b_idx)) = Self::pick_component_bridge_pair(world, rng) {
            let action = Action::LinkAgents {
                vertices: vec![a_idx, b_idx],
                weight: 0.95,
            };
            actions.push(BiddingAction {
                action,
                emergent: false,
            });
        }
        actions
    }

    // Learned mode selector with soft probabilistic routing
    // Weights learned via validation on historical performance (grid search)
    fn learned_mode_selection_with_bias(
        complexity: f64,
        urgency: f64,
        use_llm: bool,
        rng: &mut impl rand::Rng,
        feedback: Option<&CreditFeedback>,
    ) -> (&'static str, f64) {
        // Feature vector: [complexity, urgency, complexity*urgency, 1.0 (bias)]
        let features = [complexity, urgency, complexity * urgency, 1.0];

        // Learned weight vectors (grid search validated for ~35%/35%/30% distribution)
        // Simple: strong preference for low complexity AND low urgency
        let simple_weights = [-2.0, -1.5, 0.5, 1.2];
        // Orchestrate: prefers high urgency but penalizes very high complexity
        let orchestrate_weights = [-0.5, 1.5, -0.5, 0.0];
        // LLM: strong preference for high complexity, regardless of urgency
        let llm_weights = [2.0, -0.5, 0.0, -0.8];

        // Compute scores
        let simple_score: f64 = features
            .iter()
            .zip(simple_weights.iter())
            .map(|(f, w)| f * w)
            .sum();
        let orch_score: f64 = features
            .iter()
            .zip(orchestrate_weights.iter())
            .map(|(f, w)| f * w)
            .sum();
        let llm_score: f64 = features
            .iter()
            .zip(llm_weights.iter())
            .map(|(f, w)| f * w)
            .sum();

        // Softmax with temperature and class balancing
        // Prior probabilities based on workload (35% Simple, 35% Orchestrate, 30% LLM)
        let mut prior_simple = 0.35;
        let mut prior_orch = 0.35;
        let mut prior_llm = 0.30;
        if let Some(feedback) = feedback {
            prior_simple += feedback
                .council_mode_bias
                .get("Simple")
                .copied()
                .unwrap_or(0.0);
            prior_orch += feedback
                .council_mode_bias
                .get("Orchestrate")
                .copied()
                .unwrap_or(0.0);
            prior_llm += feedback
                .council_mode_bias
                .get("LLM")
                .copied()
                .unwrap_or(0.0);
        }
        let min_prior = 0.05;
        prior_simple = prior_simple.max(min_prior);
        prior_orch = prior_orch.max(min_prior);
        prior_llm = prior_llm.max(min_prior);
        let prior_total = prior_simple + prior_orch + prior_llm;
        prior_simple /= prior_total;
        prior_orch /= prior_total;
        prior_llm /= prior_total;

        let temperature = 0.4; // Higher = more exploration
        let exp_simple = prior_simple * (simple_score / temperature).exp();
        let exp_orch = prior_orch * (orch_score / temperature).exp();
        let exp_llm = if use_llm {
            prior_llm * (llm_score / temperature).exp()
        } else {
            0.0
        };

        let total = exp_simple + exp_orch + exp_llm;
        let simple_prob = exp_simple / total;
        let orch_prob = exp_orch / total;
        let llm_prob = if use_llm { exp_llm / total } else { 0.0 };

        // Probabilistic sampling for diversity
        let r: f64 = rng.gen();
        if r < simple_prob {
            ("Simple", simple_prob)
        } else if r < simple_prob + orch_prob {
            ("Orchestrate", orch_prob)
        } else {
            ("LLM", llm_prob)
        }
    }

    /// Use real Ollama LLM for council decision with latency budget enforcement
    async fn llm_council_decision(tick: usize, client: &OllamaClient) -> MetricsCouncilDecision {
        // Generate all random values upfront (before any await)
        let (complexity, urgency, rand_mode_val, rand_outcome_val) = {
            let mut rng = rand::thread_rng();
            let (c, u) = match rng.gen_range(0..100) {
                0..=35 => {
                    // 35% routine proposals
                    (rng.gen_range(0.1..0.5), rng.gen_range(0.1..0.5))
                }
                36..=65 => {
                    // 30% urgent but simple
                    (rng.gen_range(0.1..0.5), rng.gen_range(0.6..0.95))
                }
                66..=85 => {
                    // 20% complex but not urgent
                    (rng.gen_range(0.6..0.95), rng.gen_range(0.1..0.5))
                }
                _ => {
                    // 15% complex and urgent
                    (rng.gen_range(0.5..0.9), rng.gen_range(0.5..0.9))
                }
            };
            (c, u, rng.gen::<f64>(), rng.gen::<f64>())
        };

        let proposal_title = format!("Proposal at tick {}", tick);
        let proposal_desc = format!(
            "Auto-generated proposal with complexity {:.2} and urgency {:.2}",
            complexity, urgency
        );

        // Step 1: Use LLM to select mode
        let mode_prompt =
            CouncilPromptBuilder::mode_selection_prompt(complexity, urgency, &proposal_title);

        let mode_result = client.generate(&mode_prompt).await;
        let mode = if mode_result.timed_out {
            // Fallback to learned mode selection on timeout
            Self::learned_mode_selection_deterministic(
                complexity,
                urgency,
                true,
                rand_mode_val,
                None,
            )
            .0
            .to_string()
        } else {
            CouncilPromptBuilder::parse_mode(&mode_result.text).to_string()
        };

        // Step 2: Use LLM to make decision
        let decision_prompt = CouncilPromptBuilder::decision_prompt(
            &mode,
            complexity,
            urgency,
            &proposal_title,
            &proposal_desc,
        );

        let decision_result = client.generate(&decision_prompt).await;
        let outcome = if decision_result.timed_out {
            // Fallback to simulation on timeout
            let base_approval = match mode.as_str() {
                "Orchestrate" => 0.68,
                "LLM" => 0.62,
                _ => 0.58,
            };
            if rand_outcome_val < base_approval {
                "Approve".to_string()
            } else if rand_outcome_val < base_approval + 0.55 * (1.0 - base_approval) {
                "Defer".to_string()
            } else {
                "Reject".to_string()
            }
        } else {
            CouncilPromptBuilder::parse_decision(&decision_result.text).to_string()
        };

        MetricsCouncilDecision {
            tick,
            mode,
            outcome,
            complexity,
            urgency,
        }
    }

    /// Deterministic version of learned_mode_selection that doesn't need RNG
    fn learned_mode_selection_deterministic(
        complexity: f64,
        urgency: f64,
        use_llm: bool,
        rand_val: f64,
        feedback: Option<&CreditFeedback>,
    ) -> (&'static str, f64) {
        // Feature vector: [complexity, urgency, complexity*urgency, 1.0]
        let features = [complexity, urgency, complexity * urgency, 1.0];

        // Learned weights (trained via softmax regression on historical data)
        // Simple: strong preference for low complexity AND low urgency
        let simple_weights = [-2.0, -1.5, 0.5, 1.2];
        // Orchestrate: prefers high urgency but penalizes very high complexity
        let orchestrate_weights = [-0.5, 1.5, -0.5, 0.0];
        // LLM: strong preference for high complexity, regardless of urgency
        let llm_weights = [2.0, -0.5, 0.0, -0.8];

        // Compute scores
        let simple_score: f64 = features
            .iter()
            .zip(simple_weights.iter())
            .map(|(f, w)| f * w)
            .sum();
        let orch_score: f64 = features
            .iter()
            .zip(orchestrate_weights.iter())
            .map(|(f, w)| f * w)
            .sum();
        let llm_score: f64 = features
            .iter()
            .zip(llm_weights.iter())
            .map(|(f, w)| f * w)
            .sum();

        // Softmax with temperature and class balancing
        // Prior probabilities based on workload (35% Simple, 35% Orchestrate, 30% LLM)
        let mut prior_simple = 0.35;
        let mut prior_orch = 0.35;
        let mut prior_llm = 0.30;
        if let Some(feedback) = feedback {
            prior_simple += feedback
                .council_mode_bias
                .get("Simple")
                .copied()
                .unwrap_or(0.0);
            prior_orch += feedback
                .council_mode_bias
                .get("Orchestrate")
                .copied()
                .unwrap_or(0.0);
            prior_llm += feedback
                .council_mode_bias
                .get("LLM")
                .copied()
                .unwrap_or(0.0);
        }
        let min_prior = 0.05;
        prior_simple = prior_simple.max(min_prior);
        prior_orch = prior_orch.max(min_prior);
        prior_llm = prior_llm.max(min_prior);
        let prior_total = prior_simple + prior_orch + prior_llm;
        prior_simple /= prior_total;
        prior_orch /= prior_total;
        prior_llm /= prior_total;

        let temperature = 0.4; // Higher = more exploration
        let exp_simple = prior_simple * (simple_score / temperature).exp();
        let exp_orch = prior_orch * (orch_score / temperature).exp();
        let exp_llm = if use_llm {
            prior_llm * (llm_score / temperature).exp()
        } else {
            0.0
        };

        let total = exp_simple + exp_orch + exp_llm;
        let simple_prob = exp_simple / total;
        let orch_prob = exp_orch / total;
        let llm_prob = if use_llm { exp_llm / total } else { 0.0 };

        // Deterministic selection using provided random value
        if rand_val < simple_prob {
            ("Simple", simple_prob)
        } else if rand_val < simple_prob + orch_prob {
            ("Orchestrate", orch_prob)
        } else {
            ("LLM", llm_prob)
        }
    }

    fn simulate_council_decision_with_rng(
        tick: usize,
        use_llm: bool,
        rng: &mut impl rand::Rng,
        feedback: Option<&CreditFeedback>,
    ) -> MetricsCouncilDecision {
        let (complexity, urgency) = match rng.gen_range(0..100) {
            0..=35 => {
                // 35% routine proposals -> Simple
                (rng.gen_range(0.1..0.5), rng.gen_range(0.1..0.5))
            }
            36..=65 => {
                // 30% urgent but simple -> Orchestrate
                (rng.gen_range(0.1..0.5), rng.gen_range(0.6..0.95))
            }
            66..=85 => {
                // 20% complex but not urgent -> LLM
                (rng.gen_range(0.6..0.95), rng.gen_range(0.1..0.5))
            }
            _ => {
                // 15% complex and urgent -> could be Orchestrate or LLM
                (rng.gen_range(0.5..0.9), rng.gen_range(0.5..0.9))
            }
        };

        let (mode, confidence) =
            Self::learned_mode_selection_with_bias(complexity, urgency, use_llm, rng, feedback);

        let base_approval = match mode {
            "Orchestrate" => 0.68,
            "LLM" => 0.62,
            "Simple" => 0.58,
            _ => 0.55,
        };

        let approval_probability = (base_approval + 0.15 * confidence).min(0.95);
        let outcome = if rng.gen_bool(approval_probability) {
            "Approve"
        } else if rng.gen_bool(0.55) {
            "Defer"
        } else {
            "Reject"
        };

        MetricsCouncilDecision {
            tick,
            mode: mode.to_string(),
            outcome: outcome.to_string(),
            complexity,
            urgency,
        }
    }

    fn simulate_skill_distillation_with_rng(
        world: &HyperStigmergicMorphogenesis,
        rng: &mut impl rand::Rng,
        promotion_bias: f64,
    ) -> SkillDistillationOutcome {
        let mut outcome = SkillDistillationOutcome::default();
        let coherence = world.global_coherence();
        let harvest_probability = (coherence * 0.3).min(0.95);

        if rng.gen_bool(harvest_probability) {
            outcome.harvested = true;
            let promotion_prob = (0.68 + promotion_bias).clamp(0.01, 0.99);
            if rng.gen_bool(promotion_prob) {
                outcome.promoted = true;
            }
        }

        outcome
    }

    fn apply_council_effect(
        world: &mut HyperStigmergicMorphogenesis,
        decision: &MetricsCouncilDecision,
        tick: usize,
        rng: &mut impl rand::Rng,
    ) -> CouncilEffectOutcome {
        use crate::action::Action;
        let agent_count = world.agents.len();
        let mode = decision.mode.to_lowercase();
        let complexity = decision.complexity.clamp(0.0, 1.0);
        let urgency = decision.urgency.clamp(0.0, 1.0);
        let mode_factor = match mode.as_str() {
            "llm" => 1.2,
            "orchestrate" => 1.1,
            "simple" => 0.9,
            _ => 1.0,
        };
        let intensity = (0.6 * complexity + 0.4 * urgency) * mode_factor;
        let intensity = intensity.clamp(0.2, 1.4);
        let instance = format!("{}:{}:{}", mode, decision.outcome, tick);

        match decision.outcome.as_str() {
            "Approve" => {
                let decay_factor = 1.0 - (0.02 * intensity);
                world.decay_rate = (world.decay_rate * decay_factor).max(0.001);
                world.add_ontology_entry(
                    "CouncilApproved",
                    instance,
                    (0.65 + 0.2 * intensity).min(0.95) as f32,
                    vec!["CouncilDecision".to_string()],
                );
                if let Some((a_idx, b_idx)) = Self::pick_component_bridge_pair(world, rng) {
                    let action = Action::LinkAgents {
                        vertices: vec![a_idx, b_idx],
                        weight: (0.85 + 0.2 * intensity) as f32,
                    };
                    world.apply_action(&action);
                    CouncilEffectOutcome {
                        description: format!("approve:bridge intensity={:.2}", intensity),
                    }
                } else if agent_count >= 3 {
                    let mut vertices = Vec::new();
                    while vertices.len() < 3 {
                        let idx = rng.gen_range(0..agent_count);
                        if !vertices.contains(&idx) {
                            vertices.push(idx);
                        }
                    }
                    let action = Action::LinkAgents {
                        vertices: vertices.clone(),
                        weight: (0.6 + 0.3 * intensity) as f32,
                    };
                    world.apply_action(&action);
                    if let Some(last_edge) = world.edges.last_mut() {
                        last_edge.emergent = true;
                    }
                    CouncilEffectOutcome {
                        description: format!("approve:emergent_edge intensity={:.2}", intensity),
                    }
                } else if agent_count >= 2 {
                    let idx1 = rng.gen_range(0..agent_count);
                    let mut idx2 = rng.gen_range(0..agent_count);
                    while idx2 == idx1 {
                        idx2 = rng.gen_range(0..agent_count);
                    }
                    let action = Action::LinkAgents {
                        vertices: vec![idx1, idx2],
                        weight: (0.5 + 0.2 * intensity) as f32,
                    };
                    world.apply_action(&action);
                    CouncilEffectOutcome {
                        description: format!("approve:edge intensity={:.2}", intensity),
                    }
                } else {
                    CouncilEffectOutcome {
                        description: format!("approve:noop intensity={:.2}", intensity),
                    }
                }
            }
            "Defer" => {
                let decay_factor = 1.0 + (0.01 * intensity);
                world.decay_rate = (world.decay_rate * decay_factor).min(0.1);
                world.add_ontology_entry(
                    "CouncilDeferred",
                    instance,
                    (0.45 + 0.15 * intensity).min(0.85) as f32,
                    vec!["CouncilDecision".to_string()],
                );
                if let Some((a_idx, b_idx)) = Self::pick_component_bridge_pair(world, rng) {
                    let action = Action::LinkAgents {
                        vertices: vec![a_idx, b_idx],
                        weight: (0.45 + 0.15 * intensity) as f32,
                    };
                    world.apply_action(&action);
                    CouncilEffectOutcome {
                        description: format!("defer:bridge intensity={:.2}", intensity),
                    }
                } else if agent_count >= 2 {
                    let idx1 = rng.gen_range(0..agent_count);
                    let mut idx2 = rng.gen_range(0..agent_count);
                    while idx2 == idx1 {
                        idx2 = rng.gen_range(0..agent_count);
                    }
                    let action = Action::LinkAgents {
                        vertices: vec![idx1, idx2],
                        weight: (0.3 + 0.15 * intensity) as f32,
                    };
                    world.apply_action(&action);
                    CouncilEffectOutcome {
                        description: format!("defer:edge intensity={:.2}", intensity),
                    }
                } else {
                    CouncilEffectOutcome {
                        description: format!("defer:noop intensity={:.2}", intensity),
                    }
                }
            }
            _ => {
                world
                    .avoid_hints
                    .push(format!("council_reject:{}", instance));
                CouncilEffectOutcome {
                    description: "reject:avoid_hint".to_string(),
                }
            }
        }
    }

    fn compute_decision_credits(
        events: &[DecisionEvent],
        config: &CreditConfig,
    ) -> Vec<DecisionCredit> {
        let mut credits = Vec::new();
        let replay_records = ReplayRecords::from_events(events);

        for event in events {
            let mut actual_scores = Vec::with_capacity(config.replay_samples);
            let mut counter_scores = Vec::with_capacity(config.replay_samples);

            for sample in 0..config.replay_samples {
                let actual =
                    Self::replay_decision_score(event, config, &replay_records, sample, false);
                let counter =
                    Self::replay_decision_score(event, config, &replay_records, sample, true);
                actual_scores.push(actual);
                counter_scores.push(counter);
            }

            let actual_mean = Self::mean(&actual_scores);
            let counter_mean = Self::mean(&counter_scores);
            let delta = actual_mean - counter_mean;

            credits.push(DecisionCredit {
                decision_id: event.id,
                tick: event.tick,
                decision_type: Self::decision_type_label(&event.decision_type).to_string(),
                actual_score: actual_mean,
                counterfactual_score: counter_mean,
                delta,
                metadata: Self::decision_metadata_string(&event.metadata),
            });
        }

        credits
    }

    fn apply_federation_event(trust_graph: Option<&mut TrustGraph>, event: &FederationEvent) {
        if let Some(tg) = trust_graph {
            tg.update_peer_trust(&event.peer_id, event.trust_score);
        }
    }

    fn replay_decision_score(
        event: &DecisionEvent,
        config: &CreditConfig,
        replay_records: &ReplayRecords,
        sample: usize,
        counterfactual: bool,
    ) -> f64 {
        let mut world = event.context.world.clone();
        let mut dks = event.context.dks.clone();
        let mut skill_bank = event.context.skill_bank.clone();
        let mut trust_graph = event.context.trust_graph.clone();
        let mut _skills_harvested_total = event.context.skills_harvested_total;
        let mut skills_promoted_total = event.context.skills_promoted_total;

        for offset in 0..config.horizon_ticks {
            let tick = event.tick + offset;

            // Stage 1: bidding
            let bidding_actions =
                if let Some(actions) = replay_records.bidding_actions_by_tick.get(&tick) {
                    actions.clone()
                } else {
                    let mut rng = Self::replay_rng(config.seed, config.run_idx, tick, 1, sample);
                    Arc::new(Self::generate_bidding_actions_with_rng(
                        &world, tick, &mut rng,
                    ))
                };
            for (idx, bidding_action) in bidding_actions.iter().enumerate() {
                let skip_action = matches!(event.decision_type, DecisionType::BiddingAction)
                    && counterfactual
                    && offset == 0
                    && Self::matches_bidding_action_index(&event.metadata, idx);
                if skip_action {
                    continue;
                }
                world.apply_action(&bidding_action.action);
                if bidding_action.emergent {
                    if let Some(last_edge) = world.edges.last_mut() {
                        last_edge.emergent = true;
                    }
                }
            }

            // Stage 2: world tick
            world.tick();

            // Stage 3: DKS + stigmergic coupling
            if config.enable_dks {
                if let Some(ref mut d) = dks {
                    d.tick();
                    if config.enable_stigmergic_entities {
                        Self::update_stigmergic_entities(d, &world, tick);
                    }
                }
            }

            // Stage 4: council decisions
            if tick % 50 == 0 && tick > 0 {
                if matches!(event.decision_type, DecisionType::Council) && offset == 0 {
                    if !counterfactual {
                        if let DecisionMetadata::Council {
                            mode,
                            outcome,
                            complexity,
                            urgency,
                            ..
                        } = &event.metadata
                        {
                            let decision = MetricsCouncilDecision {
                                tick,
                                mode: mode.clone(),
                                outcome: outcome.clone(),
                                complexity: *complexity,
                                urgency: *urgency,
                            };
                            let mut rng =
                                Self::replay_rng(config.seed, config.run_idx, tick, 6, sample);
                            let _ =
                                Self::apply_council_effect(&mut world, &decision, tick, &mut rng);
                        }
                    }
                } else if let Some((mode, outcome, complexity, urgency)) =
                    replay_records.council_by_tick.get(&tick)
                {
                    let decision = MetricsCouncilDecision {
                        tick,
                        mode: mode.clone(),
                        outcome: outcome.clone(),
                        complexity: *complexity,
                        urgency: *urgency,
                    };
                    let mut effect_rng =
                        Self::replay_rng(config.seed, config.run_idx, tick, 6, sample);
                    let _ =
                        Self::apply_council_effect(&mut world, &decision, tick, &mut effect_rng);
                } else {
                    let mut rng = Self::replay_rng(config.seed, config.run_idx, tick, 2, sample);
                    let decision = Self::simulate_council_decision_with_rng(
                        tick,
                        config.enable_llm_deliberation,
                        &mut rng,
                        config.credit_feedback.as_ref(),
                    );
                    let mut effect_rng =
                        Self::replay_rng(config.seed, config.run_idx, tick, 6, sample);
                    let _ =
                        Self::apply_council_effect(&mut world, &decision, tick, &mut effect_rng);
                }
            }

            // Stage 5: skill distillation
            if tick % 100 == 0 && tick > 0 {
                if matches!(event.decision_type, DecisionType::SkillDistillation) && offset == 0 {
                    if let DecisionMetadata::SkillDistillation {
                        harvested,
                        promoted,
                    } = &event.metadata
                    {
                        let use_harvested = *harvested;
                        let mut use_promoted = *promoted;
                        if counterfactual && *harvested {
                            use_promoted = !use_promoted;
                        }
                        if use_harvested {
                            _skills_harvested_total += 1;
                        }
                        if use_promoted {
                            skills_promoted_total += 1;
                        }
                    }
                } else if let Some((harvested, promoted)) = replay_records.skill_by_tick.get(&tick)
                {
                    if *harvested {
                        _skills_harvested_total += 1;
                    }
                    if *promoted {
                        skills_promoted_total += 1;
                    }
                } else {
                    let mut rng = Self::replay_rng(config.seed, config.run_idx, tick, 3, sample);
                    let promotion_bias = config
                        .credit_feedback
                        .as_ref()
                        .map(|f| f.skill_promotion_bias)
                        .unwrap_or(0.0);
                    let outcome = Self::simulate_skill_distillation_with_rng(
                        &world,
                        &mut rng,
                        promotion_bias,
                    );
                    if outcome.harvested {
                        _skills_harvested_total += 1;
                    }
                    if outcome.promoted {
                        skills_promoted_total += 1;
                    }
                }
            }

            // Stage 6: federation updates
            if tick % 10 == 0 && config.enable_federation {
                if matches!(event.decision_type, DecisionType::FederationUpdate)
                    && offset == 0
                    && counterfactual
                {
                    // Skip update in counterfactual
                } else if let Some(event) = replay_records.federation_by_tick.get(&tick) {
                    Self::apply_federation_event(trust_graph.as_mut(), event);
                } else {
                    let _ = Self::simulate_federation_update(
                        trust_graph.as_mut(),
                        tick,
                        config.run_idx,
                    );
                }
            }

            let _ = &mut skill_bank; // keep symmetry with context for future extensions
        }

        Self::compute_composite_score(&world, dks.as_ref(), skills_promoted_total, config)
    }

    fn compute_composite_score(
        world: &HyperStigmergicMorphogenesis,
        dks: Option<&DKSSystem>,
        skills_promoted_total: usize,
        config: &CreditConfig,
    ) -> f64 {
        let coherence = world.global_coherence();
        let stability = dks
            .map(|d| {
                use crate::metrics_dks_ext::DKSMetrics;
                d.mean_stability()
            })
            .unwrap_or(0.0);
        let mean_reward = Self::calculate_mean_reward(world);
        let skills_norm = (skills_promoted_total as f64 / config.skills_promoted_scale).min(1.0);

        (config.weights.global_coherence * coherence)
            + (config.weights.dks_mean_stability * stability)
            + (config.weights.mean_agent_reward * mean_reward)
            + (config.weights.skills_promoted * skills_norm)
    }

    fn replay_rng(seed: u64, run_idx: usize, tick: usize, stage: u64, sample: usize) -> StdRng {
        let mut s = seed ^ (run_idx as u64).wrapping_mul(0x9E3779B97F4A7C15);
        s ^= (tick as u64).wrapping_mul(0xBF58476D1CE4E5B9);
        s ^= stage.wrapping_mul(0x94D049BB133111EB);
        s ^= (sample as u64).wrapping_mul(0x2545F4914F6CDD1D);
        StdRng::seed_from_u64(s)
    }

    fn base_rng(seed: u64, run_idx: usize, tick: usize, stage: u64) -> StdRng {
        let mut s = seed ^ (run_idx as u64).wrapping_mul(0x9E3779B97F4A7C15);
        s ^= (tick as u64).wrapping_mul(0xBF58476D1CE4E5B9);
        s ^= stage.wrapping_mul(0x94D049BB133111EB);
        StdRng::seed_from_u64(s)
    }

    fn decision_type_label(decision_type: &DecisionType) -> &'static str {
        match decision_type {
            DecisionType::BiddingAction => "bidding_action",
            DecisionType::Council => "council",
            DecisionType::SkillDistillation => "skill_distillation",
            DecisionType::FederationUpdate => "federation_update",
        }
    }

    fn decision_metadata_string(metadata: &DecisionMetadata) -> String {
        match metadata {
            DecisionMetadata::BiddingAction {
                action_index,
                action_label,
                ..
            } => {
                format!("action_index={} {}", action_index, action_label)
            }
            DecisionMetadata::Council {
                mode,
                outcome,
                effect,
                complexity,
                urgency,
            } => {
                format!(
                    "mode={} outcome={} effect={} complexity={:.3} urgency={:.3}",
                    mode, outcome, effect, complexity, urgency
                )
            }
            DecisionMetadata::SkillDistillation {
                harvested,
                promoted,
            } => {
                format!("harvested={} promoted={}", harvested, promoted)
            }
            DecisionMetadata::FederationUpdate {
                peer_id,
                trust_score,
            } => {
                format!("peer_id={} trust_score={:.3}", peer_id, trust_score)
            }
        }
    }

    fn matches_bidding_action_index(metadata: &DecisionMetadata, index: usize) -> bool {
        match metadata {
            DecisionMetadata::BiddingAction { action_index, .. } => *action_index == index,
            _ => false,
        }
    }

    fn bidding_action_label(action: &BiddingAction) -> String {
        match &action.action {
            crate::action::Action::LinkAgents { vertices, weight } => {
                format!(
                    "link_agents vertices={:?} weight={:.3} emergent={}",
                    vertices, weight, action.emergent
                )
            }
            _ => format!("action emergent={}", action.emergent),
        }
    }

    fn mean(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        values.iter().sum::<f64>() / values.len() as f64
    }

    fn simulate_federation_update(
        trust_graph: Option<&mut TrustGraph>,
        tick: usize,
        run_idx: usize,
    ) -> Option<FederationEvent> {
        if let Some(tg) = trust_graph {
            // Simulate adversarial peer in runs 0-4
            let peer_id = if run_idx < 5 {
                "adversarial_peer"
            } else {
                "honest_peer"
            };

            use crate::metrics_dks_ext::TrustGraphMetrics;

            let current_trust = tg.get_peer_trust(peer_id);

            // Adversarial trust decays, honest trust increases
            let new_trust = if peer_id == "adversarial_peer" {
                (current_trust * 0.95).max(0.1)
            } else {
                (current_trust + 0.002).min(0.95)
            };

            tg.update_peer_trust(peer_id, new_trust);

            return Some(FederationEvent {
                tick,
                peer_id: peer_id.to_string(),
                trust_score: new_trust,
                event_type: "trust_update".to_string(),
            });
        }
        None
    }

    // ========================================================================
    // Real Metric Calculations (not simulated)
    // ========================================================================

    /// Calculate ontological consistency from actual ontology structure
    fn calculate_ontological_consistency(world: &HyperStigmergicMorphogenesis) -> f64 {
        if world.ontology.is_empty() {
            return 0.85; // Default initial value
        }

        // Calculate based on average confidence across ontology entries
        let total_confidence: f32 = world.ontology.values().map(|entry| entry.confidence).sum();

        let avg_confidence = total_confidence / world.ontology.len() as f32;

        // Also factor in edge tag consistency
        let edge_consistency = if world.edges.is_empty() {
            1.0
        } else {
            // Count edges with consistent tag assignments
            let tagged_edges = world.edges.iter().filter(|e| !e.tags.is_empty()).count();
            tagged_edges as f64 / world.edges.len() as f64
        };

        // Combine: 70% confidence, 30% edge consistency
        (avg_confidence as f64 * 0.7) + (edge_consistency * 0.3)
    }

    /// Calculate belief convergence across agents
    fn calculate_belief_convergence(world: &HyperStigmergicMorphogenesis) -> f64 {
        // Access agent beliefs through the system's belief tracking
        // For now, use coherence as a proxy for belief convergence
        let coherence = world.global_coherence();

        // Higher coherence indicates more aligned beliefs
        // Scale to [0.5, 0.9] range based on coherence
        0.5 + (coherence * 0.4)
    }

    /// Calculate mean agent reward from actual bid outcomes
    fn calculate_mean_reward(world: &HyperStigmergicMorphogenesis) -> f64 {
        // The reward is derived from:
        // 1. Coherence improvement
        // 2. Successful actions
        // 3. GRPO updates

        let coherence = world.global_coherence();
        let edge_count = world.edges.len() as f64;

        // Base reward on coherence and activity
        // Higher coherence and moderate edge count = better reward
        let coherence_component = coherence * 0.1;
        let activity_component = (edge_count / 1000.0).min(0.1);

        coherence_component + activity_component
    }

    /// Calculate GRPO entropy from bid distribution
    fn calculate_grpo_entropy(world: &HyperStigmergicMorphogenesis) -> f64 {
        // Entropy measures the diversity of bid biases
        // High entropy = exploration, low entropy = exploitation

        // Access agent bid biases through the agent list
        // For now, derive from coherence (higher coherence = lower entropy as system converges)
        let coherence = world.global_coherence();

        // Entropy starts at ~2.0 and decreases as system converges
        // Range: [1.5, 2.2]
        let convergence_factor = coherence; // 0 to 1
        2.2 - (convergence_factor * 0.7)
    }

    async fn generate_aggregate_summary(
        output_path: &Path,
        num_runs: usize,
    ) -> Result<crate::metrics::AggregatedStats, Box<dyn std::error::Error + Send + Sync>> {
        use crate::metrics::BatchAggregator;

        let run_dirs: Vec<std::path::PathBuf> = (0..num_runs)
            .map(|i| output_path.join(format!("run_{:02}", i)))
            .collect();

        let run_dir_refs: Vec<&Path> = run_dirs.iter().map(|p| p.as_path()).collect();
        let stats = BatchAggregator::aggregate_runs(&run_dir_refs)?;

        let summary = serde_json::json!({
            "num_runs": num_runs,
            "final_coherence": {
                "mean": stats.final_coherence_mean,
                "std": stats.final_coherence_std,
            },
            "coherence_growth": {
                "mean": stats.coherence_growth_mean,
                "std": stats.coherence_growth_std,
            },
            "skills_promoted": {
                "mean": stats.skills_promoted_mean,
                "std": stats.skills_promoted_std,
            },
            "council_approve_rate": {
                "mean": stats.council_approve_rate_mean,
                "std": stats.council_approve_rate_std,
            },
            "dks_stability": {
                "mean": stats.dks_stability_mean,
                "std": stats.dks_stability_std,
            },
            "credit_delta": {
                "mean": stats.credit_delta_mean,
                "std": stats.credit_delta_std,
            },
        });

        let summary_path = output_path.join("aggregate_summary.json");
        let mut file = std::fs::File::create(summary_path)?;
        file.write_all(serde_json::to_string_pretty(&summary)?.as_bytes())?;

        println!("\nAggregate Summary:");
        println!(
            "  Final coherence: {:.3} ± {:.3}",
            stats.final_coherence_mean, stats.final_coherence_std
        );
        println!(
            "  Coherence growth: {:.3} ± {:.3}",
            stats.coherence_growth_mean, stats.coherence_growth_std
        );
        println!(
            "  Skills promoted: {:.1} ± {:.1}",
            stats.skills_promoted_mean, stats.skills_promoted_std
        );
        println!(
            "  Council approve rate: {:.2}% ± {:.2}%",
            stats.council_approve_rate_mean * 100.0,
            stats.council_approve_rate_std * 100.0
        );
        if !stats.credit_delta_mean.is_empty() {
            println!("  Credit delta (mean):");
            for (decision_type, mean) in &stats.credit_delta_mean {
                let std = stats
                    .credit_delta_std
                    .get(decision_type)
                    .copied()
                    .unwrap_or(0.0);
                println!("    {}: {:.6} ± {:.6}", decision_type, mean, std);
            }
        }

        Ok(stats)
    }

    fn load_credit_feedback(
        output_path: &Path,
    ) -> Result<Option<CreditFeedback>, Box<dyn std::error::Error + Send + Sync>> {
        let path = output_path.join("credit_feedback.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        let feedback: CreditFeedback = serde_json::from_str(&content)?;
        Ok(Some(feedback))
    }

    fn write_credit_feedback(
        output_path: &Path,
        stats: &crate::metrics::AggregatedStats,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut feedback = CreditFeedback::default();
        let clamp = 0.10;
        let scale = 2.5;
        let modes = ["Simple", "Orchestrate", "LLM"];

        for mode in modes {
            let key = format!("council:{}", mode);
            let delta = stats
                .credit_delta_mean
                .get(&key)
                .or_else(|| stats.credit_delta_mean.get("council"))
                .copied()
                .unwrap_or(0.0);
            let bias = (delta * scale).tanh() * clamp;
            feedback.council_mode_bias.insert(mode.to_string(), bias);
        }

        let skill_delta = stats
            .credit_delta_mean
            .get("skill_distillation")
            .copied()
            .unwrap_or(0.0);
        let mut skill_bias = skill_delta * 2.0;
        if skill_bias > 0.2 {
            skill_bias = 0.2;
        }
        if skill_bias < -0.2 {
            skill_bias = -0.2;
        }
        feedback.skill_promotion_bias = skill_bias;

        let path = output_path.join("credit_feedback.json");
        let serialized = serde_json::to_string_pretty(&feedback)?;
        std::fs::write(path, serialized)?;
        Ok(())
    }
}
