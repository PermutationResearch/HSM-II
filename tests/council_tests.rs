//! Council system comprehensive tests

use ::hyper_stigmergy::council::ExecutionStep;
use ::hyper_stigmergy::hyper_stigmergy::RecursiveRelationKind;
use ::hyper_stigmergy::*;

#[tokio::test]
async fn test_all_council_modes() {
    let members = vec![
        CouncilMember {
            agent_id: 1,
            role: Role::Architect,
            expertise_score: 0.9,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 2,
            role: Role::Catalyst,
            expertise_score: 0.8,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 3,
            role: Role::Critic,
            expertise_score: 0.85,
            participation_weight: 1.0,
        },
    ];

    let proposal = Proposal::new("test", "Test Proposal", "A test proposal", 1);

    // Test Debate mode
    let mut debate = Council::new(CouncilMode::Debate, proposal.clone(), members.clone());
    let debate_decision = debate.evaluate().await.unwrap();
    assert!(!debate_decision.participating_agents.is_empty());
    println!("Debate mode: {:?}", debate_decision.decision);

    // Test Orchestrate mode
    let mut orchestrate = Council::new(CouncilMode::Orchestrate, proposal.clone(), members.clone());
    let orch_decision = orchestrate.evaluate().await.unwrap();
    if let Decision::Approve = orch_decision.decision {
        assert!(orch_decision.execution_plan.is_some());
        let plan = orch_decision.execution_plan.unwrap();
        assert!(!plan.steps.is_empty());
    }
    println!("Orchestrate mode: {:?}", orch_decision.decision);

    // Test Simple mode
    let mut simple = Council::new(CouncilMode::Simple, proposal, members);
    let simple_decision = simple.evaluate().await.unwrap();
    println!("Simple mode: {:?}", simple_decision.decision);
}

#[test]
fn test_execution_plan() {
    let steps = vec![
        ExecutionStep {
            sequence: 1,
            description: "Initialize".to_string(),
            assigned_agent: Some(1),
            dependencies: vec![],
        },
        ExecutionStep {
            sequence: 2,
            description: "Validate".to_string(),
            assigned_agent: Some(2),
            dependencies: vec![1],
        },
    ];

    let plan = ExecutionPlan {
        steps,
        estimated_duration_ms: 60000,
        rollback_strategy: Some("Revert".to_string()),
    };

    assert_eq!(plan.steps.len(), 2);
    assert_eq!(plan.steps[1].dependencies, vec![1]);
    assert_eq!(plan.estimated_duration_ms, 60000);
}

#[test]
fn test_council_factory() {
    let config = ModeConfig {
        debate_complexity_threshold: 0.6,
        orchestrate_urgency_threshold: 0.7,
        min_agents_for_debate: 3,
        max_agents_for_simple: 2,
        history_window_size: 100,
        diversity_weight: 0.3,
        llm_deliberation_complexity_threshold: 0.7,
        llm_deliberation_enabled: true,
        llm_latency_budget_ms: 10000,
    };

    let factory = CouncilFactory::new(config);

    let members = vec![CouncilMember {
        agent_id: 1,
        role: Role::Architect,
        expertise_score: 0.9,
        participation_weight: 1.0,
    }];

    // Single member with simple proposal -> Simple mode
    let simple_proposal = Proposal::new("simple", "Simple", "A simple task", 1);
    let council = factory
        .create_council(&simple_proposal, members.clone())
        .unwrap();
    assert_eq!(council.mode, CouncilMode::Simple);
}

#[tokio::test]
async fn test_debate_structure() {
    let members = vec![
        CouncilMember {
            agent_id: 1,
            role: Role::Architect,
            expertise_score: 0.9,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 2,
            role: Role::Catalyst,
            expertise_score: 0.8,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 3,
            role: Role::Critic,
            expertise_score: 0.85,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 4,
            role: Role::Explorer,
            expertise_score: 0.7,
            participation_weight: 1.0,
        },
    ];

    let mut debate = DebateCouncil::new(uuid::Uuid::new_v4(), members);

    let proposal = Proposal::new(
        "debate_test",
        "Complex Issue",
        "A complex architectural decision",
        1,
    );

    // Check initial status
    assert!(matches!(debate.status(), CouncilStatus::NotStarted));

    // Evaluate
    let decision = debate
        .evaluate(&proposal, CouncilMode::Debate)
        .await
        .unwrap();

    // Verify debate completed
    assert!(matches!(debate.status(), CouncilStatus::Completed { .. }));

    // Verify participating agents
    assert_eq!(decision.participating_agents.len(), 4);
}

#[tokio::test]
async fn test_orchestrate_subtasks() {
    let members = vec![
        CouncilMember {
            agent_id: 1,
            role: Role::Architect,
            expertise_score: 0.9,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 2,
            role: Role::Catalyst,
            expertise_score: 0.8,
            participation_weight: 1.0,
        },
    ];

    let mut orchestrate = OrchestratorCouncil::new(uuid::Uuid::new_v4(), members);

    let proposal = Proposal::new("orch_test", "Urgent Task", "An urgent implementation", 1);

    let decision = orchestrate
        .evaluate(&proposal, CouncilMode::Orchestrate)
        .await
        .unwrap();

    // Should have execution plan with subtasks
    assert!(decision.execution_plan.is_some());
}

#[test]
fn test_mode_switcher_learning() {
    let mut switcher = ModeSwitcher::new(ModeConfig::default());

    // Record some outcomes
    switcher.record_outcome("proposal_1", 0.8);
    switcher.record_outcome("proposal_2", 0.9);
    switcher.record_outcome("proposal_3", 0.7);

    // Check effectiveness scores updated
    let scores = switcher.effectiveness_scores();
    println!("Effectiveness scores: {:?}", scores);
}

#[test]
fn test_enrich_council_proposal_includes_trace_graph_context() {
    let mut world = HyperStigmergicMorphogenesis::new(3);
    let promise_id = world.record_agent_promise(
        1,
        Some(2),
        "compile code",
        "deliver compiler fix",
        DataSensitivity::Internal,
        Some(10),
    );
    world.resolve_agent_promise(
        &promise_id,
        PromiseStatus::Kept,
        Some(1),
        Some(0.91),
        Some(true),
        Some(true),
        &[2],
    );
    world.record_agent_delivery(1, "compile code", true, 0.93, true, true, &[2]);
    world.apply_stigmergic_cycle();

    let mut proposal = Proposal::new("stig", "Compile code", "Compile code safely", 1);
    proposal.task_key = Some("compile code".into());
    world.enrich_council_proposal(&mut proposal);

    let context = proposal
        .stigmergic_context
        .expect("expected stigmergic council context");
    assert!(context
        .evidence
        .iter()
        .any(|item| item.id.starts_with("trace-")));
    assert!(context
        .evidence
        .iter()
        .any(|item| item.id.starts_with("directive:")));
    assert!(!context.graph_snapshot_bullets.is_empty());
    assert!(!context.graph_queries.is_empty());
}

#[tokio::test]
async fn test_debate_decision_metadata_tracks_trace_ids() {
    let members = vec![
        CouncilMember {
            agent_id: 1,
            role: Role::Architect,
            expertise_score: 0.9,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 2,
            role: Role::Critic,
            expertise_score: 0.85,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 3,
            role: Role::Chronicler,
            expertise_score: 0.8,
            participation_weight: 1.0,
        },
    ];

    let proposal =
        Proposal::new(
            "debate_meta",
            "Compile code",
            "Implement a compile code pathway",
            1,
        )
        .with_stigmergic_context(StigmergicCouncilContext {
            preferred_agent: Some(1),
            preferred_tool: Some(GraphToolKind::CypherLikeQuery),
            confidence: 0.92,
            require_council_review: false,
            rationale: "trace graph strongly favors agent 1".into(),
            evidence: vec![
                CouncilEvidence {
                    id: "trace-42".into(),
                    kind: CouncilEvidenceKind::Trace,
                    summary: "trace-42 says agent 1 kept the last compile promise".into(),
                },
                CouncilEvidence {
                    id: "directive:compile_code".into(),
                    kind: CouncilEvidenceKind::Directive,
                    summary: "directive routes compile code to agent 1".into(),
                },
            ],
            graph_snapshot_bullets: vec![
                "agent 1 is trusted and recent promise quality is 0.91".into()
            ],
            graph_queries: vec![CouncilGraphQuery {
                purpose: "recent task traces".into(),
                query:
                    "MATCH (t:StigmergicTrace) WHERE t.task_key = 'compile code' RETURN t LIMIT 3"
                        .into(),
                evidence: vec![],
            }],
        });

    let mut debate = DebateCouncil::new(uuid::Uuid::new_v4(), members);
    let decision = debate
        .evaluate(&proposal, CouncilMode::Debate)
        .await
        .expect("debate should succeed");

    assert!(decision
        .metadata
        .trace_ids
        .iter()
        .any(|id| id == "trace-42"));
    assert!(decision
        .metadata
        .directive_ids
        .iter()
        .any(|id| id == "directive:compile_code"));
    assert!(!decision.metadata.graph_snapshot_bullets.is_empty());
}

#[test]
fn test_property_graph_roundtrip_preserves_recursive_memory_evidence() {
    let mut world = HyperStigmergicMorphogenesis::new(3);
    let promise_id = world.record_agent_promise(
        1,
        Some(2),
        "compile code",
        "deliver compiler fix",
        DataSensitivity::Internal,
        Some(10),
    );
    world.resolve_agent_promise(
        &promise_id,
        PromiseStatus::Kept,
        Some(2),
        Some(0.92),
        Some(true),
        Some(true),
        &[1],
    );
    world.record_tool_execution_evidence(
        1,
        "bash",
        "council",
        Some("compile code"),
        true,
        "compiled the patch and validated the promise outcome",
        Some(&promise_id),
        Some(2),
        Some("cargo test --no-run"),
    );

    let snapshot = world.to_property_graph_snapshot();
    let restored = HyperStigmergicMorphogenesis::from_property_graph_snapshot(&snapshot);

    assert!(restored
        .fact_templates
        .iter()
        .any(|template| template.label == "tool_execution"));
    assert!(restored
        .composite_facts
        .iter()
        .any(|fact| fact.label.contains("Tool execution")));
    assert!(restored
        .composite_facts
        .iter()
        .flat_map(|fact| fact.slots.iter())
        .any(|slot| slot.role == "tool" && slot.value == "bash"));
    assert!(restored
        .recursive_fact_relations
        .iter()
        .any(|relation| matches!(relation.kind, RecursiveRelationKind::Supports)));
    assert!(restored
        .delegation_frames
        .iter()
        .any(|frame| frame.task_key == "compile code" && frame.delegated_to == 2));
}
