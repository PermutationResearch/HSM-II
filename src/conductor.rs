use std::io::{BufRead, Seek};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::autocontext::{AutoContextLoop, LoopResult};
use crate::consensus::{ConsensusEngine, CorrelationMonitor, GuardianCritic, JuryContext};
use crate::dream::{DreamCycleResult, StigmergicDreamEngine};
use crate::federation::client::FederationClient;
use crate::federation::types::*;
use crate::hyper_stigmergy::HyperStigmergicMorphogenesis;
use crate::meta_graph::MetaGraph;
use crate::metrics::RewardSignal;
use crate::reasoning_braid::{BraidOrchestrator, BraidSynthesis};
use crate::reward::{DefaultTaskEvaluator, TaskEvalContext, TaskEvaluator};
use crate::rlm::{ReflectionResult, RLM};
use crate::skill::{SkillBank, SkillCreditRecord};

pub struct Conductor {
    pub model: String,
    pub world: Arc<RwLock<HyperStigmergicMorphogenesis>>,
    pub braid_orchestrator: BraidOrchestrator,
    pub correlation_monitor: CorrelationMonitor,
    pub meta_graph: Option<Arc<RwLock<MetaGraph>>>,
    pub federation_client: Option<FederationClient>,
    pub task_evaluator: Arc<dyn TaskEvaluator>,
    pub pending_eval_report: Option<TaskEvalContext>,
    pub reward_report_path: Option<std::path::PathBuf>,
    pub reward_report_offset: u64,
    /// AutoContext closed-loop learning (optional; runs every N ticks)
    pub autocontext: Option<AutoContextLoop>,
    /// Stigmergic Dream Consolidation engine (optional; runs every N ticks)
    pub dream_engine: Option<StigmergicDreamEngine>,
}

impl Conductor {
    pub fn new(model: &str, world: HyperStigmergicMorphogenesis) -> Self {
        Self {
            model: model.to_string(),
            world: Arc::new(RwLock::new(world)),
            braid_orchestrator: BraidOrchestrator::default(),
            correlation_monitor: CorrelationMonitor::default(),
            meta_graph: None,
            federation_client: None,
            task_evaluator: Arc::new(DefaultTaskEvaluator::default()),
            pending_eval_report: None,
            reward_report_path: None,
            reward_report_offset: 0,
            autocontext: None,
            dream_engine: None,
        }
    }

    /// Enable federation on this conductor.
    pub fn with_federation(
        mut self,
        meta_graph: Arc<RwLock<MetaGraph>>,
        client: FederationClient,
    ) -> Self {
        self.meta_graph = Some(meta_graph);
        self.federation_client = Some(client);
        self
    }

    pub fn with_task_evaluator(mut self, evaluator: Arc<dyn TaskEvaluator>) -> Self {
        self.task_evaluator = evaluator;
        self
    }

    pub fn set_task_eval_report(&mut self, report: TaskEvalContext) {
        self.pending_eval_report = Some(report);
    }

    pub fn with_reward_report_path(mut self, path: std::path::PathBuf) -> Self {
        self.reward_report_path = Some(path);
        self.reward_report_offset = 0;
        self
    }

    pub fn with_autocontext(mut self, ac: AutoContextLoop) -> Self {
        self.autocontext = Some(ac);
        self
    }

    pub fn with_dream_engine(mut self, engine: StigmergicDreamEngine) -> Self {
        self.dream_engine = Some(engine);
        self
    }

    fn load_next_reward_report(
        path: Option<&std::path::Path>,
        offset: &mut u64,
    ) -> Option<TaskEvalContext> {
        let path = path?;
        let file = std::fs::File::open(path).ok()?;
        let mut reader = std::io::BufReader::new(file);
        if reader.seek(std::io::SeekFrom::Start(*offset)).is_err() {
            return None;
        }
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).ok()?;
        if bytes == 0 {
            return None;
        }
        *offset += bytes as u64;
        TaskEvalContext::from_json_str(line.trim())
    }

    /// Execute one full tick cycle:
    /// 1. Run reasoning braids (Prolog System 2)
    /// 2. Feed braid synthesis into LivingPrompt
    /// 3. RLM execute (LLM System 1) with skill-augmented context
    /// 4. Reflect and distill skills from outcomes
    /// 5. Evolve skill bank if needed
    pub async fn tick(&mut self, rlm: &mut RLM) -> TickResult {
        let mut credit_records: Vec<SkillCreditRecord> = Vec::new();
        let credit_log_dir = std::env::var("HSM_CREDIT_LOG_DIR").ok();
        let credit_log_format = std::env::var("HSM_CREDIT_LOG_FORMAT").ok();
        if self.pending_eval_report.is_none() {
            if let Some(report) = Self::load_next_reward_report(
                self.reward_report_path.as_deref(),
                &mut self.reward_report_offset,
            ) {
                self.pending_eval_report = Some(report);
            }
        }
        let world = self.world.read().await;
        let skill_bank = &world.skill_bank;

        // Phase 1: Parallel reasoning braids
        let synthesis = self
            .braid_orchestrator
            .execute_braids(&world, skill_bank)
            .await;

        // Phase 2: Retrieve applicable skills
        // Use the first agent's role as context (bid winner will be selected later)
        let default_role = world
            .agents
            .first()
            .map(|a| a.role)
            .unwrap_or(crate::agent::Role::Architect);
        let applicable_skills = skill_bank.retrieve(
            &default_role,
            None, // no embedding yet
            &synthesis.applicable_skill_ids,
        );

        // Build skill prompt section
        let skill_prompt = SkillBank::format_for_prompt(&applicable_skills);
        let skill_count = applicable_skills.general.len() + applicable_skills.specific.len();

        drop(world); // Release read lock before RLM needs mutable world

        // Phase 3: Inject braid synthesis + skills into living prompt
        rlm.living_prompt.add_insight(format!(
            "[Braid] {} braids ({} succeeded): {}",
            synthesis.braids_run,
            synthesis.braids_succeeded,
            synthesis
                .prompt_section
                .lines()
                .take(5)
                .collect::<Vec<_>>()
                .join(" | ")
        ));

        if !skill_prompt.is_empty() {
            rlm.living_prompt.add_insight(format!(
                "[Skills] {} applicable: {}",
                skill_count,
                skill_prompt.lines().take(3).collect::<Vec<_>>().join(" | ")
            ));
        }

        // Phase 4: RLM execute with enriched context
        let intent = self.derive_intent(&synthesis);
        let exec_result = rlm.execute(&intent).await;

        // Phase 5: Reflect (generates insights, beliefs, evolves prompt)
        let reflection = rlm.reflect().await;

        // Phase 6: Distill skills from experiences, update outcomes, run consensus
        let mut tick_result = {
            let mut world = self.world.write().await;
            world.tick_count += 1;

            // Distill new skills from recent experiences + improvement events
            let experiences = world.experiences.clone();
            let improvement_history = world.improvement_history.clone();
            let distill_result = world
                .skill_bank
                .distill_from_experiences(&experiences, &improvement_history);
            let distilled_count = distill_result.new_skills;

            // Update skill outcomes based on whether coherence improved
            let coherence = world.global_coherence();
            let succeeded = coherence > 0.5;
            world
                .skill_bank
                .update_skill_outcomes(&synthesis.applicable_skill_ids, succeeded);

            // Apply causal credit every tick (credit-to-control loop)
            let prev_coherence = world
                .improvement_history
                .last()
                .map(|e| e.coherence_before)
                .unwrap_or(0.0);
            let coherence_delta = coherence - prev_coherence;
            let mut eval_context = self.pending_eval_report.take().unwrap_or_default();
            eval_context.coherence_delta = coherence_delta;
            eval_context.exec_ok = exec_result.is_ok();
            let reward = self.task_evaluator.evaluate(&eval_context);
            let tick_now = world.tick_count;
            let _credit_report = world.skill_bank.apply_skill_credit(
                &synthesis.applicable_skill_ids,
                reward.total,
                tick_now,
            );
            if credit_log_dir.is_some() {
                credit_records = world.skill_bank.drain_credit_history();
            }

            // Periodic skill evolution + consensus evaluation (every 10 ticks)
            let failed_experiences: Vec<_> = world
                .experiences
                .iter()
                .filter(|e| {
                    matches!(
                        e.outcome,
                        crate::hyper_stigmergy::ExperienceOutcome::Negative { .. }
                    )
                })
                .cloned()
                .collect();
            let evolved = if world.tick_count % 10 == 0 {
                // Phase 6a: Evolve skills (Bayesian + hard negative mining)
                let evolve_result = world.skill_bank.evolve(&failed_experiences);

                // Phase 6b: Detect emergent associations via braids
                let (associations, _assoc_synthesis) = self
                    .braid_orchestrator
                    .execute_association_braids(&world, &world.skill_bank);

                // Phase 6c: Build JuryContext from braid synthesis
                let _jury_context = JuryContext::from_synthesis(
                    &synthesis.prompt_section,
                    &world.skill_bank,
                    &associations,
                );

                // Phase 6d: Run jury pipeline with topological layers
                let consensus_engine = ConsensusEngine::default();
                // Execute jury pipeline: Layer 0 dyads → Chronicler synthesis → ACPO
                let consensus_results = consensus_engine.evaluate_all_skills(
                    &world.skill_bank,
                    &associations,
                    &world.agents,
                    coherence_delta,
                );

                // Phase 6e: Feed bid correlations into CorrelationMonitor
                let avg_correlation = if !consensus_results.is_empty() {
                    consensus_results
                        .iter()
                        .map(|r| r.bid_correlation)
                        .sum::<f64>()
                        / consensus_results.len() as f64
                } else {
                    0.0
                };

                if let Some(respec_action) = self.correlation_monitor.update(avg_correlation) {
                    match respec_action {
                        crate::consensus::RespecAction::Trigger => {
                            // Re-specialize: inject noise into bid_bias to break groupthink
                            CorrelationMonitor::apply_respec(&mut world.agents);
                        }
                        crate::consensus::RespecAction::Resume => {
                            // Correlation recovered — normal operation resumes
                        }
                    }
                }

                // Phase 6f: Apply consensus verdicts
                let tick_for_verdict = world.tick_count;
                let consensus_apply = ConsensusEngine::apply_verdicts(
                    &mut world.skill_bank,
                    &consensus_results,
                    tick_for_verdict,
                );

                // Phase 6g: Guardian critic check with real avg skill confidence
                let avg_skill_confidence = if !consensus_results.is_empty() {
                    consensus_results
                        .iter()
                        .map(|r| r.utility_score)
                        .sum::<f64>()
                        / consensus_results.len() as f64
                } else {
                    0.5
                };

                let guardian = GuardianCritic::default();
                let unresolved = world
                    .beliefs
                    .iter()
                    .filter(|b| !b.contradicting_evidence.is_empty())
                    .count();
                let _veto =
                    guardian.check(coherence, coherence_delta, unresolved, avg_skill_confidence);

                evolve_result.skills_refined > 0
                    || evolve_result.skills_deprecated > 0
                    || consensus_apply.promoted > 0
                    || consensus_apply.suspended > 0
            } else {
                false
            };

            TickResult {
                tick: world.tick_count,
                synthesis,
                reflection,
                skills_applicable: skill_count,
                skills_distilled: distilled_count,
                skills_evolved: evolved,
                intent,
                exec_ok: exec_result.is_ok(),
                reward,
                federation: None,  // populated below after world lock is released
                autocontext: None, // populated below
                dream: None,       // populated below in Phase 7
            }
        };

        if let Some(dir) = credit_log_dir {
            if !credit_records.is_empty() {
                let log_path = std::path::Path::new(&dir);
                let _ = std::fs::create_dir_all(log_path);
                let format = credit_log_format.unwrap_or_else(|| "json".to_string());
                if format == "csv" {
                    let _ = SkillBank::export_credit_history_csv(
                        &log_path.join("skill_credit_history.csv"),
                        &credit_records,
                    );
                } else {
                    let _ = SkillBank::export_credit_history_json(
                        &log_path.join("skill_credit_history.jsonl"),
                        &credit_records,
                    );
                }
            }
        }

        // Run federation sub-tick now that the world write lock is released
        let fed_result = self.federation_tick().await;
        if self.meta_graph.is_some() {
            tick_result.federation = Some(fed_result);
        }

        // Phase 7: Stigmergic Dream Consolidation
        // Runs every dream_interval ticks — replays traces, detects temporal
        // motifs, crystallizes patterns, deposits DreamTrail hyperedges,
        // applies DKS survival pressure, and generates proto-skills.
        if let Some(ref mut dream) = self.dream_engine {
            let current_tick = tick_result.tick;
            if dream.should_dream(current_tick) {
                let mut world = self.world.write().await;
                let dream_result = dream.dream(&mut world, &mut rlm.living_prompt);
                tick_result.dream = Some(dream_result);
            }
        }

        tick_result
    }

    /// Derive intent from braid synthesis — what should the system focus on?
    fn derive_intent(&self, synthesis: &BraidSynthesis) -> String {
        // Priority: risks > trust violations > belief conflicts > federation > topology > skills
        if !synthesis.risk_findings.is_empty() {
            return format!("Address risk: {}", synthesis.risk_findings[0]);
        }
        if !synthesis.trust_violations.is_empty() {
            return format!("Trust violation: {}", synthesis.trust_violations[0]);
        }
        if !synthesis.belief_findings.is_empty() {
            return format!("Resolve belief conflict: {}", synthesis.belief_findings[0]);
        }
        if !synthesis.cross_system_findings.is_empty() {
            return format!("Federation: {}", synthesis.cross_system_findings[0]);
        }
        if !synthesis.topology_findings.is_empty() {
            return format!("Fix topology: {}", synthesis.topology_findings[0]);
        }
        if !synthesis.applicable_skill_ids.is_empty() {
            return format!(
                "Apply skills: {}",
                synthesis
                    .applicable_skill_ids
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        "Explore and strengthen connections".to_string()
    }

    /// Execute the federation sub-tick after the main tick.
    /// 1. Project local shared-scope edges into MetaGraph
    /// 2. Process pending imports (trust-gated)
    /// 3. Run federation braids
    /// 4. Detect meta-hyperedges from patterns
    /// 5. Broadcast new shared edges to subscribers
    /// 6. Decay trust scores
    /// 7. Auto-promote qualifying edges
    pub async fn federation_tick(&mut self) -> FederationTickResult {
        let (meta_graph, client) = match (&self.meta_graph, &self.federation_client) {
            (Some(mg), Some(c)) => (mg, c),
            _ => return FederationTickResult::default(),
        };

        let world = self.world.read().await;
        let mut mg = meta_graph.write().await;
        let tick = world.tick_count;

        // 1. Project local shared-scope edges into H*
        let edges_before = mg.shared_edges.len();
        let local_id = mg.local_system_id.clone();
        mg.project_to_shared(&world, &local_id);
        let exported = mg.shared_edges.len() - edges_before;

        // 2. Process pending imports
        let import_result = mg.process_pending_imports(tick);

        // 3. Run federation braids
        let fed_synthesis =
            self.braid_orchestrator
                .execute_federation_braids(&world, &world.skill_bank, &mg);

        // 4. Detect meta-hyperedges
        let meta_hyperedges = mg.detect_meta_hyperedges();

        // 5. Collect newly projected edges for outbound push
        let new_edges: Vec<SharedEdge> = if exported > 0 {
            mg.shared_edges[mg.shared_edges.len() - exported..].to_vec()
        } else {
            Vec::new()
        };

        // 5a. Convert SharedEdges → HyperedgeInjectionRequests, stamping this
        //     node onto the hop_chain so receivers can detect forwarding cycles.
        let outbound: Vec<HyperedgeInjectionRequest> = new_edges
            .iter()
            .map(|e| {
                let mut prov = e.provenance.clone();
                prov.hop_chain.push((local_id.clone(), tick));
                HyperedgeInjectionRequest {
                    vertices: e.vertices.clone(),
                    edge_type: e.edge_type.clone(),
                    scope: EdgeScope::Shared,
                    trust_tags: e.trust_tags.clone(),
                    provenance: prov,
                    weight: e.weight,
                    embedding: None,
                    metadata: std::collections::HashMap::new(),
                }
            })
            .collect();

        // 5b. Push to all known peers (fan-out) — drop lock first so the
        //     async HTTP calls don't hold the MetaGraph write lock.
        drop(mg);
        drop(world);

        let peer_push_count = if !outbound.is_empty() && !client.known_peers.is_empty() {
            let results = client.broadcast_edges_to_peers(outbound.clone()).await;
            results.iter().filter(|(_, r)| r.is_ok()).count()
        } else {
            0
        };

        // 5c. Re-acquire locks for broadcast-to-subscribers + remaining steps
        let world = self.world.read().await;
        let mut mg = meta_graph.write().await;

        // 5d. Push to push-subscription callbacks
        let broadcast_count = if !new_edges.is_empty() {
            client.broadcast_to_subscribers(&mg, &new_edges).await
        } else {
            0
        };

        // 6. Decay trust scores
        mg.trust_graph.decay_all(tick);

        // 7. Auto-promote qualifying edges
        let config = world.federation_config.as_ref();
        let promote_after = config.map(|c| c.auto_promote_after).unwrap_or(50);
        mg.auto_promote(tick, promote_after);

        FederationTickResult {
            imported: import_result.imported,
            exported,
            conflicts: import_result.conflicts,
            meta_hyperedges_detected: meta_hyperedges.len(),
            broadcasts_sent: broadcast_count + peer_push_count,
            federation_braids: fed_synthesis.braids_run,
            trust_violations: fed_synthesis.trust_violations.len(),
        }
    }

    /// Build and run a tick cycle as a Mastra-style workflow
    pub async fn tick_as_workflow(&mut self, rlm: &mut RLM) -> TickResult {
        // For now delegate to the direct tick — workflow composition can be
        // used when we need suspend/resume or branching logic
        self.tick(rlm).await
    }

    pub async fn run(
        &mut self,
        mut intent_rx: tokio::sync::mpsc::Receiver<String>,
        event_tx: tokio::sync::mpsc::Sender<UiEvent>,
    ) {
        let world_snapshot = self.world.read().await.clone();
        let mut rlm = crate::rlm::rlm_from_world(world_snapshot, &self.model).await;

        loop {
            tokio::select! {
                Some(user_intent) = intent_rx.recv() => {
                    let _ = event_tx.send(UiEvent::Token(format!("Processing: {}\n", user_intent))).await;
                    let result = self.tick(&mut rlm).await;
                    let summary = format!(
                        "Tick {} | {} braids | {} skills | reward {:.4} (coh Δ {:.4}, exec {}) | intent: {}\n",
                        result.tick,
                        result.synthesis.braids_succeeded,
                        result.skills_applicable,
                        result.reward.total,
                        result.reward.coherence_delta,
                        result.reward.exec_ok,
                        result.intent
                    );
                    let _ = event_tx.send(UiEvent::StreamFinished(summary)).await;
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
            }
        }
    }

    pub async fn chat_and_act<F>(
        &self,
        _world: &HyperStigmergicMorphogenesis,
        user_msg: &str,
        on_chunk: F,
    ) -> anyhow::Result<String>
    where
        F: Fn(&str),
    {
        on_chunk(&format!("Processing: {}", user_msg));
        Ok(format!("Acknowledged: {}", user_msg))
    }
}

#[derive(Debug, Clone)]
pub struct TickResult {
    pub tick: u64,
    pub synthesis: BraidSynthesis,
    pub reflection: ReflectionResult,
    pub skills_applicable: usize,
    pub skills_distilled: usize,
    pub skills_evolved: bool,
    pub intent: String,
    pub exec_ok: bool,
    pub reward: RewardSignal,
    pub federation: Option<FederationTickResult>,
    pub autocontext: Option<LoopResult>,
    pub dream: Option<DreamCycleResult>,
}

#[derive(Debug, Clone, Default)]
pub struct FederationTickResult {
    pub imported: usize,
    pub exported: usize,
    pub conflicts: usize,
    pub meta_hyperedges_detected: usize,
    pub broadcasts_sent: usize,
    pub federation_braids: usize,
    pub trust_violations: usize,
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    Token(String),
    StreamFinished(String),
    Error(String),
}
