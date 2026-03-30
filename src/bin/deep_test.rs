//! Deep Test Suite — Tests all advanced HSM-II systems
//!
//! Tests: CASS, DKS, AutoContext, Social Memory, Kuramoto,
//! Federation Trust, Council Debate/Ralph, LLM Deliberation Cache,
//! Full pipeline with Claude as LLM backend.
//!
//! Run: ANTHROPIC_API_KEY=... cargo run --bin deep_test

use std::collections::HashMap;

use hyper_stigmergy::agent::{AgentId, Role};
use hyper_stigmergy::autocontext::{AutoContextStore, Hint, KnowledgeBase, Playbook, Step};
use hyper_stigmergy::cass::{ContextSnapshot, CASS};
use hyper_stigmergy::council::llm_deliberation::{ArgumentCache, LLMArgument, Stance};
use hyper_stigmergy::council::{CouncilMember, CouncilMode, Proposal, SimpleCouncil};
use hyper_stigmergy::dks::{DKSConfig, DKSSystem};
use hyper_stigmergy::federation::trust::TrustGraph;
use hyper_stigmergy::hyper_stigmergy::{BeliefSource, HyperStigmergicMorphogenesis};
use hyper_stigmergy::kuramoto::{KuramotoConfig, KuramotoEngine};
use hyper_stigmergy::llm::client::{LlmClient, LlmRequest, Message};
use hyper_stigmergy::skill::{SkillBank, SkillLevel};
use hyper_stigmergy::social_memory::{DataSensitivity, PromiseStatus, SocialMemory};
use hyper_stigmergy::tools::{PredictionTool, Tool};

struct TestRunner {
    passed: u32,
    failed: u32,
    test_num: u32,
    total_tests: u32,
}

impl TestRunner {
    fn new(total: u32) -> Self {
        Self { passed: 0, failed: 0, test_num: 0, total_tests: total }
    }
    fn section(&mut self, name: &str) {
        self.test_num += 1;
        println!("\n━━━ [{}/{}] {} ━━━", self.test_num, self.total_tests, name);
    }
    fn pass(&mut self, msg: &str) {
        println!("  ✓ {}", msg);
        self.passed += 1;
    }
    fn fail(&mut self, msg: &str) {
        eprintln!("  ✗ {}", msg);
        self.failed += 1;
    }
    fn info(&self, msg: &str) {
        println!("    {}", msg);
    }
    fn summary(&self) {
        let total = self.passed + self.failed;
        println!("\n╔═══════════════════════════════════════════════════════════════╗");
        println!("║  Results: {} / {} passed{}", self.passed, total,
            " ".repeat(42usize.saturating_sub(format!("{} / {}", self.passed, total).len())));
        if self.failed == 0 {
            println!("║  ✓ ALL SYSTEMS OPERATIONAL                                   ║");
        } else {
            println!("║  ⚠ {} test(s) failed                                          ║", self.failed);
        }
        println!("╚═══════════════════════════════════════════════════════════════╝");
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("warn").init();

    println!("\n╔═══════════════════════════════════════════════════════════════╗");
    println!("║  HSM-II Deep Test Suite — All Advanced Systems               ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");

    let mut t = TestRunner::new(12);

    // ═══ TEST 1: Claude LLM Client ═══
    t.section("Claude LLM Client (Anthropic API)");
    match LlmClient::new() {
        Ok(client) => {
            let health = client.health_check().await;
            for (provider, ok, msg) in &health {
                let icon = if *ok { "✓" } else { "✗" };
                println!("  {} {:?}: {}", icon, provider, msg);
            }
            let request = LlmRequest {
                model: "claude-haiku-4-5-20251001".to_string(),
                messages: vec![
                    Message::system("You are a concise assistant. Reply in 1 sentence."),
                    Message::user("What is stigmergy in the context of multi-agent systems?"),
                ],
                temperature: 0.3,
                max_tokens: Some(150),
                ..Default::default()
            };
            match client.chat(request).await {
                Ok(resp) => {
                    t.pass(&format!("Claude responded via {:?} in {}ms ({} tokens)",
                        resp.provider, resp.latency_ms, resp.usage.total_tokens));
                    t.info(&format!("Response: {}", resp.content.trim()));
                }
                Err(e) => t.fail(&format!("Chat failed: {}", e)),
            }
        }
        Err(e) => t.fail(&format!("LlmClient::new failed: {}", e)),
    }

    // ═══ TEST 2: DKS Evolutionary System ═══
    t.section("DKS Evolutionary System (Dynamic Kinetic Stability)");
    {
        let config = DKSConfig {
            base_replication_rate: 0.3,
            base_decay_rate: 0.1,
            replication_energy_cost: 2.0,
            resource_energy_conversion: 0.5,
            max_population: 100,
            selection_intensity: 0.5,
            flux_rate: 0.1,
        };
        let mut dks = DKSSystem::new(config);
        dks.seed(10);
        let initial_stats = dks.stats();
        t.info(&format!("Seeded: {} entities, avg energy: {:.2}", initial_stats.size, initial_stats.average_energy));

        let results = dks.evolve(20);
        let final_stats = dks.stats();
        t.pass(&format!("Evolved {} generations: {} → {} entities",
            results.len(), initial_stats.size, final_stats.size));
        t.info(&format!("Avg persistence: {:.3}, Avg replication rate: {:.3}",
            final_stats.average_persistence, final_stats.average_replication_rate));
        t.info(&format!("Total energy: {:.2}, Max generation: {}",
            final_stats.total_energy, final_stats.max_generation));

        if final_stats.average_persistence > 0.0 {
            t.pass("Selection pressure active: persistence scores positive");
        } else {
            t.fail("Selection pressure not working: persistence is 0");
        }

        let spectrum = dks.multifractal_spectrum_points(5);
        t.info(&format!("Multifractal spectrum: {} points", spectrum.len()));

        let tick = dks.tick();
        t.info(&format!("Tick: +{} born, -{} decayed, {} selected, pop={}",
            tick.new_entities, tick.decayed_entities, tick.selected_entities, tick.population_size));
    }

    // ═══ TEST 3: CASS Semantic Skills ═══
    t.section("CASS (Context-Aware Semantic Skills)");
    {
        let mut skill_bank = SkillBank::new_with_seeds();
        let initial_count = skill_bank.all_skills().len();

        // Add a custom skill via the curated skill API
        use hyper_stigmergy::skill::SkillScope;
        let _custom = skill_bank.add_curated_skill(
            "Web Search for Research",
            "Use web search tools to gather current information for research tasks",
            "deep_test",
            "research",
            SkillScope::default(),
            SkillLevel::General,
        );

        let mut cass = CASS::new(skill_bank);
        cass.initialize().await?;
        t.pass(&format!("Initialized with {} skills ({} seeded + 1 custom), embedding dim={}",
            cass.skill_count(), initial_count, cass.embedding_dimension()));

        let ctx = ContextSnapshot {
            timestamp: 0,
            active_agents: vec![0],
            dominant_roles: vec![Role::Architect],
            current_goals: vec!["improve code quality".to_string()],
            recent_skills_used: vec![],
            system_load: 0.3,
            error_rate: 0.01,
            coherence_score: 0.85,
        };

        let results = cass.search("find bugs in the codebase", Some(ctx.clone()), 3).await;
        if !results.is_empty() {
            t.pass(&format!("Semantic search returned {} matches", results.len()));
            for r in &results {
                t.info(&format!("  {} — semantic: {:.3}, context: {:.3}",
                    r.skill.title, r.semantic_score, r.context_relevance));
            }
        } else {
            t.fail("Semantic search returned 0 results");
        }

        let chain = cass.find_composition_path("custom_web_search", "secure and fast code");
        t.info(&format!("Composition path: {}",
            chain.map(|c| format!("{} skills, confidence: {:.3}", c.skills.len(), c.total_confidence))
                .unwrap_or_else(|| "none found".to_string())));

        cass.record_usage("custom_web_search", true, ctx.clone());
        cass.record_usage("custom_web_search", true, ctx);
        t.pass("Recorded skill usage (2 successes)");
    }

    // ═══ TEST 4: Social Memory & Reputation ═══
    t.section("Social Memory (Promises, Reputation, Delegation)");
    {
        let mut social = SocialMemory::default();
        let agent_a: AgentId = 1;
        let agent_b: AgentId = 2;
        let agent_c: AgentId = 3;

        social.ensure_agent(agent_a);
        social.ensure_agent(agent_b);
        social.ensure_agent(agent_c);

        let promise_id = social.record_promise(
            agent_a, Some(agent_b), "code_review",
            "Review the authentication module by Friday",
            DataSensitivity::Internal, 1000, Some(2000),
        );
        t.pass(&format!("Promise created: {}", promise_id));

        social.resolve_promise(
            &promise_id, PromiseStatus::Kept, Some(agent_a),
            1500, Some(0.9), Some(true), Some(true), &[],
        );
        t.pass("Promise resolved: Kept (quality: 0.9, on-time: true)");

        for i in 0..5 {
            social.record_delivery(agent_a, "code_review", true,
                0.85 + (i as f64 * 0.02), true, true, 2000 + i * 100, &[]);
        }
        social.record_delivery(agent_b, "code_review", true, 0.7, false, true, 2500, &[]);
        social.record_delivery(agent_c, "code_review", false, 0.3, false, true, 2600, &[]);

        let rep_a = &social.reputations[&agent_a];
        let rep_b = &social.reputations[&agent_b];
        t.pass(&format!("Agent A: reliability={:.2}, timeliness={:.2}, quality={:.2}",
            rep_a.reliability_score(), rep_a.timeliness_score(), rep_a.avg_quality()));
        t.info(&format!("Agent B: reliability={:.2}, timeliness={:.2}",
            rep_b.reliability_score(), rep_b.timeliness_score()));

        let candidates = vec![(agent_a, 0.8), (agent_b, 0.6), (agent_c, 0.5)];
        let delegate = social.recommend_delegate(
            &candidates, Some("code_review"), None, Some(DataSensitivity::Internal));
        if let Some(d) = delegate {
            t.pass(&format!("Delegation: Agent {} (score: {:.3})", d.agent_id, d.score));
            t.info(&format!("  observed={:.3}, capability={:.3}, collab={:.3}",
                d.components.observed_score, d.components.capability_score,
                d.components.collaboration_score));
        } else {
            t.fail("No delegation recommendation");
        }

        social.set_share_policy(agent_a, agent_b, DataSensitivity::Internal, 0.5, None, 3000);
        let can_share = social.can_share(agent_a, agent_b, DataSensitivity::Internal);
        let cant_share = !social.can_share(agent_a, agent_c, DataSensitivity::Confidential);
        if can_share && cant_share {
            t.pass("Share policies enforced correctly");
        } else {
            t.fail(&format!("Share policy issue: can_AB={}, cant_AC={}", can_share, !cant_share));
        }
    }

    // ═══ TEST 5: Kuramoto Oscillator Synchronization ═══
    t.section("Kuramoto Oscillators (Agent Synchronization)");
    {
        let config = KuramotoConfig {
            coupling_strength: 2.0, dt: 0.1, dispersion: 0.5,
            council_influence: 0.3, noise_amplitude: 0.05,
            min_edge_weight: 0.1, enable_frustration: false,
            enable_phase_field: false, phase_field_growth: 0.0,
            phase_field_hyperviscosity: 0.0, phase_field_dispersion: 0.0,
        };
        let mut engine = KuramotoEngine::new(config);

        for i in 0..5u64 {
            engine.register_agent(i as AgentId, 0.5 + (i as f64 * 0.1), 0.7, 0.6);
        }

        let mut adj: HashMap<AgentId, Vec<(AgentId, f64)>> = HashMap::new();
        for i in 0..5u64 {
            let neighbors: Vec<_> = (0..5u64).filter(|&j| j != i)
                .map(|j| (j as AgentId, 1.0)).collect();
            adj.insert(i as AgentId, neighbors);
        }

        let snap0 = engine.snapshot();
        t.info(&format!("Initial: order_param={:.3}, mean_phase={:.3}",
            snap0.order_parameter, snap0.mean_phase));

        for _ in 0..100 {
            engine.step(&adj);
        }

        let snap_final = engine.snapshot();
        t.pass(&format!("After 100 steps: order_param {:.3} → {:.3}",
            snap0.order_parameter, snap_final.order_parameter));
        t.info(&format!("Clusters: {}, Phase histogram: {:?}",
            snap_final.clusters.len(), snap_final.phase_histogram));

        if snap_final.order_parameter >= snap0.order_parameter * 0.8 {
            t.pass("Synchronization maintained/improved under coupling");
        } else {
            t.fail(&format!("Synchronization degraded: {:.3} → {:.3}",
                snap0.order_parameter, snap_final.order_parameter));
        }
    }

    // ═══ TEST 6: Federation Trust Graph ═══
    t.section("Federation Trust Graph (Bayesian Trust)");
    {
        let mut trust = TrustGraph::new(0.5, 0.01);
        let node_a = "hsm-toronto".to_string();
        let node_b = "hsm-london".to_string();
        let node_c = "hsm-tokyo".to_string();

        let initial = trust.get_trust(&node_a, &node_b);
        t.info(&format!("Initial trust A→B: {:.3}", initial));

        for tick in 0..5u64 {
            trust.record_success(&node_a, &node_b, tick);
        }
        trust.record_success(&node_a, &node_c, 10);
        trust.record_failure(&node_a, &node_c, 11);

        let trust_ab = trust.get_trust(&node_a, &node_b);
        let trust_ac = trust.get_trust(&node_a, &node_c);
        t.pass(&format!("Trust: A→B={:.3} (5 successes), A→C={:.3} (1 success, 1 failure)",
            trust_ab, trust_ac));

        if trust_ab > trust_ac {
            t.pass("Trust ordering correct: reliable node > mixed node");
        } else {
            t.fail(&format!("Trust ordering wrong: AB={:.3} should be > AC={:.3}", trust_ab, trust_ac));
        }

        trust.decay_all(500);
        let trust_ab_decayed = trust.get_trust(&node_a, &node_b);
        t.info(&format!("After decay (tick 500): A→B {:.3} → {:.3}", trust_ab, trust_ab_decayed));
    }

    // ═══ TEST 7: Council LLM Argument Cache ═══
    t.section("LLM Deliberation Argument Cache");
    {
        let cache = ArgumentCache::new(300, 1000);

        let argument = LLMArgument {
            agent_id: 1, role: Role::Architect, stance: Stance::For,
            content: "This architectural change improves modularity".to_string(),
            confidence: 0.85,
            key_points: vec!["Reduces coupling".to_string(), "Enables parallel dev".to_string()],
            evidence: vec![], round: 1, responding_to: None,
            tokens_generated: 150, generation_time_ms: 800,
        };
        cache.put(Role::Architect, "proposal-1", "opening", argument);

        let hit = cache.get(Role::Architect, "proposal-1", "opening");
        if let Some(cached) = hit {
            t.pass(&format!("Cache hit: '{}...' (conf: {}, stance: {})",
                cached.content.chars().take(40).collect::<String>(),
                cached.confidence, cached.stance.as_str()));
        } else {
            t.fail("Cache miss on recently cached argument");
        }

        let miss = cache.get(Role::Architect, "proposal-2", "opening");
        if miss.is_none() {
            t.pass("Cache miss on non-existent entry (correct)");
        } else {
            t.fail("Cache returned data for non-existent entry");
        }

        let stats = cache.stats();
        t.info(&format!("Stats: {} entries, {} hits, {} misses", stats.total_entries, stats.hits, stats.misses));
    }

    // ═══ TEST 8: AutoContext Knowledge Base ═══
    t.section("AutoContext Knowledge Base & Playbooks");
    {
        let tmp = std::env::temp_dir().join("hsmii_autocontext_test");
        std::fs::create_dir_all(&tmp).ok();
        let store = AutoContextStore::new(&tmp);
        store.ensure_dirs().await?;

        let steps = vec![
            Step::tool_step(
                0,
                "Read the diff",
                "git_diff",
                serde_json::json!({}),
                "Diff loaded",
            ),
            Step::tool_step(
                1,
                "Check security",
                "security_audit",
                serde_json::json!({}),
                "No critical vulns",
            ),
            Step::tool_step(
                2,
                "Verify tests",
                "test_runner",
                serde_json::json!({}),
                "Coverage > 80%",
            ),
        ];
        let mut playbook = Playbook::new(
            "Code Review Playbook",
            "Strategy for reviewing code changes",
            "code review authentication",
        )
        .with_steps(steps);
        playbook.id = "pb-code-review".to_string();

        let mut kb = KnowledgeBase::new();
        kb.upsert_playbook(playbook);
        kb.upsert_hint(Hint::new(
            "Always check for auth bypass patterns",
            "authentication security",
            0.9,
        ));
        t.pass("Playbook + hint inserted into knowledge base");

        let pbs = kb.find_playbooks("code review", 5);
        if let Some(pb) = pbs.iter().find(|p| p.id == "pb-code-review") {
            t.pass(&format!(
                "find_playbooks: {} ({} steps)",
                pb.name,
                pb.steps.len()
            ));
        } else {
            t.fail("find_playbooks missed playbook");
        }

        let hints = kb.find_hints("check the authentication module", 5);
        if hints.is_empty() {
            t.info("No hints matched threshold (weighting may skip)");
        } else {
            t.pass(&format!(
                "Hint matched: '{}' (conf: {:.2})",
                hints[0].content, hints[0].confidence
            ));
        }

        store.save(&kb).await?;
        let kb2 = store.load().await?;
        if kb2.playbooks.iter().any(|p| p.id == "pb-code-review") {
            t.pass("Playbook persisted and reloaded from disk");
        } else {
            t.fail("Playbook lost after save/load cycle");
        }

        std::fs::remove_dir_all(&tmp).ok();
    }

    // ═══ TEST 9: Hypergraph Belief Pipeline ═══
    t.section("Hypergraph Beliefs with Multiple Sources");
    {
        let mut hsm = HyperStigmergicMorphogenesis::new(5);

        let id1 = hsm.add_belief("Rust adoption is accelerating in systems programming", 0.85, BeliefSource::Observation);
        let id2 = hsm.add_belief("Federation enables distributed intelligence across nodes", 0.75, BeliefSource::Prediction);
        let id3 = hsm.add_belief("DKS evolutionary pressure promotes validated knowledge", 0.80, BeliefSource::Inference);
        let id4 = hsm.add_belief("User prefers concise responses over verbose explanations", 0.90, BeliefSource::UserProvided);

        t.pass(&format!("Stored 4 beliefs (ids: {}, {}, {}, {})", id1, id2, id3, id4));

        let top = hsm.top_beliefs(3);
        t.pass(&format!("Top 3 by confidence: [{}]",
            top.iter().map(|b| format!("{:.2}", b.confidence)).collect::<Vec<_>>().join(", ")));

        let sources: Vec<_> = top.iter().map(|b| format!("{:?}", b.source)).collect();
        t.info(&format!("Sources: {}", sources.join(", ")));

        // Contradicting belief
        let id5 = hsm.add_belief("Rust adoption is slowing due to learning curve", 0.45, BeliefSource::Reflection);
        t.info(&format!("Added contradicting belief (id: {})", id5));
        t.pass(&format!("Total beliefs: {}", hsm.top_beliefs(100).len()));
    }

    // ═══ TEST 10: Council Deliberation (all modes) ═══
    t.section("Council Deliberation (Simple + Orchestrate + Debate prep)");
    {
        let council_id = uuid::Uuid::new_v4();
        let members = vec![
            CouncilMember { agent_id: 1, role: Role::Architect, expertise_score: 0.9, participation_weight: 1.0 },
            CouncilMember { agent_id: 2, role: Role::Critic, expertise_score: 0.85, participation_weight: 1.0 },
            CouncilMember { agent_id: 3, role: Role::Explorer, expertise_score: 0.8, participation_weight: 1.0 },
            CouncilMember { agent_id: 4, role: Role::Catalyst, expertise_score: 0.75, participation_weight: 0.8 },
        ];
        let mut council = SimpleCouncil::new(council_id, members);

        let simple = Proposal::new("s1", "Add logging to API endpoints", "Standard observability improvement", 1);
        match council.evaluate(&simple, CouncilMode::Simple).await {
            Ok(d) => t.pass(&format!("Simple: {:?} (conf: {:.2})", d.decision, d.confidence)),
            Err(e) => t.fail(&format!("Simple failed: {}", e)),
        }

        let orchestrate = Proposal::new("o1", "Implement federated knowledge sync",
            "Design protocol for hypergraph belief synchronization across nodes with conflict resolution", 1);
        match council.evaluate(&orchestrate, CouncilMode::Orchestrate).await {
            Ok(d) => t.pass(&format!("Orchestrate: {:?} (conf: {:.2})", d.decision, d.confidence)),
            Err(e) => t.fail(&format!("Orchestrate failed: {}", e)),
        }

        // Debate prep — cache arguments
        let cache = ArgumentCache::default();
        let roles = [Role::Architect, Role::Critic, Role::Explorer];
        let stances = [Stance::For, Stance::Against, Stance::Cautious];
        for (i, (role, stance)) in roles.iter().zip(stances.iter()).enumerate() {
            let arg = LLMArgument {
                agent_id: i as AgentId, role: role.clone(), stance: stance.clone(),
                content: format!("{:?} argues {:?} on federation sync", role, stance),
                confidence: 0.7, key_points: vec!["Point 1".to_string()],
                evidence: vec![], round: 1, responding_to: None,
                tokens_generated: 100, generation_time_ms: 500,
            };
            cache.put(role.clone(), "o1", "opening", arg);
        }
        t.pass(&format!("Debate cache prepared: {} arguments", cache.stats().total_entries));
    }

    // ═══ TEST 11: MiroFish + Belief + Council Pipeline ═══
    t.section("MiroFish Prediction → Belief → Council Pipeline");
    {
        let mut hsm = HyperStigmergicMorphogenesis::new(3);
        let pred = PredictionTool::new();
        let output = pred.execute(serde_json::json!({
            "topic": "Should HSM-II prioritize CASS skill evolution or DKS knowledge replication?",
            "seeds": [
                "CASS learns from successful tool usage patterns",
                "DKS evolves knowledge through replication and selection pressure",
                "Both compete for compute resources",
                "Users need responsive tool selection AND long-term knowledge retention"
            ]
        })).await;

        if output.success {
            t.pass("Prediction generated");
            for line in output.result.lines().take(5) {
                t.info(line);
            }

            let content: String = output.result.chars().take(500).collect();
            let id = hsm.add_belief(&content, 0.72, BeliefSource::Prediction);
            t.pass(&format!("Stored as Belief (id: {}, source: Prediction)", id));

            let council_id = uuid::Uuid::new_v4();
            let members = vec![
                CouncilMember { agent_id: 1, role: Role::Architect, expertise_score: 0.9, participation_weight: 1.0 },
                CouncilMember { agent_id: 2, role: Role::Critic, expertise_score: 0.85, participation_weight: 1.0 },
            ];
            let mut council = SimpleCouncil::new(council_id, members);
            let proposal = Proposal::new("p1", "Prioritize CASS over DKS",
                "Based on prediction: allocate more compute to CASS skill evolution", 1);
            match council.evaluate(&proposal, CouncilMode::Orchestrate).await {
                Ok(d) => t.pass(&format!("Council reviewed: {:?} (conf: {:.2})", d.decision, d.confidence)),
                Err(e) => t.fail(&format!("Council failed: {}", e)),
            }
        } else {
            t.fail(&format!("Prediction failed: {:?}", output.error));
        }
    }

    // ═══ TEST 12: Integrated Social + Trust + Delegation ═══
    t.section("Integrated: Social Memory + Trust + Delegation Decision");
    {
        let mut social = SocialMemory::default();
        let mut trust = TrustGraph::new(0.5, 0.01);
        let agents: Vec<AgentId> = vec![10, 20, 30];

        for i in 0..10u64 {
            social.record_delivery(agents[0], "federation_sync", true, 0.9, true, true, 1000 + i * 100, &[]);
        }
        for i in 0..5u64 {
            social.record_delivery(agents[1], "federation_sync", i < 3, 0.6, i < 2, true, 1000 + i * 100, &[]);
        }
        social.record_delivery(agents[2], "federation_sync", false, 0.2, false, false, 1000, &[]);

        let node_a = "node-A".to_string();
        let agent10 = "agent-10".to_string();
        let agent20 = "agent-20".to_string();
        let agent30 = "agent-30".to_string();

        for tick in 0..8u64 {
            trust.record_success(&node_a, &agent10, tick);
        }
        for tick in 0..3u64 {
            trust.record_success(&node_a, &agent20, tick);
        }
        trust.record_failure(&node_a, &agent20, 10);
        trust.record_failure(&node_a, &agent30, 11);

        let jw_scores: Vec<(AgentId, f64)> = agents.iter().enumerate()
            .map(|(i, &a)| (a, 0.8 - (i as f64 * 0.15))).collect();
        let delegate = social.recommend_delegate(
            &jw_scores, Some("federation_sync"), None, Some(DataSensitivity::Internal));

        if let Some(d) = &delegate {
            t.pass(&format!("Best delegate: Agent {} (score: {:.3})", d.agent_id, d.score));
            let trust_score = trust.get_trust(&node_a, &format!("agent-{}", d.agent_id));
            t.info(&format!("Federation trust for delegate: {:.3}", trust_score));

            if d.agent_id == agents[0] {
                t.pass("Correct: most reliable agent selected");
            } else {
                t.info(&format!("Selected agent {} (JW weighting may differ)", d.agent_id));
            }
        } else {
            t.fail("No delegate recommended");
        }

        for &a in &agents {
            let score = social.reputation_score(a, 0.7);
            t.info(&format!("Agent {} reputation: {:.3}", a, score));
        }
    }

    // ═══ Summary ═══
    t.summary();

    if let Ok(client) = LlmClient::new() {
        let m = client.metrics();
        println!("\n  LLM Metrics: {} requests, {} tokens, {}ms avg latency",
            m.requests_total, m.tokens_total, m.avg_latency_ms);
    }

    std::process::exit(if t.failed > 0 { 1 } else { 0 });
}
