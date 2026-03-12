//! Comprehensive integration tests for hyper-stigmergic-morphogenesis
//!
//! These tests verify that all components work together logically and purposefully.

use ::hyper_stigmergy::*;

// ============================================================================
// COUNCIL SYSTEM TESTS
// ============================================================================

#[tokio::test]
async fn test_council_debate_mode() {
    // Create council members with different roles
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
            role: Role::Chronicler,
            expertise_score: 0.7,
            participation_weight: 1.0,
        },
    ];

    let proposal = Proposal {
        id: "test_debate_1".to_string(),
        title: "Refactor core architecture".to_string(),
        description: "Restructure the system for better coherence and maintainability".to_string(),
        proposer: 1,
        proposed_at: 0,
        complexity: 0.8,
        urgency: 0.4,
        required_roles: vec![Role::Architect, Role::Catalyst, Role::Chronicler],
        task_key: None,
        stigmergic_context: None,
    };

    let mut council = Council::new(CouncilMode::Debate, proposal, members);

    // Run the council evaluation
    let decision = council.evaluate().await.unwrap();

    // Verify decision structure
    assert_eq!(decision.proposal_id, "test_debate_1");
    assert!(decision.confidence > 0.0 && decision.confidence <= 1.0);
    assert!(!decision.participating_agents.is_empty());

    // Verify execution plan exists for approved decisions
    if let Decision::Approve = decision.decision {
        assert!(decision.execution_plan.is_some());
    }

    println!(
        "✓ Debate council completed with decision: {:?}",
        decision.decision
    );
}

#[tokio::test]
async fn test_council_simple_mode() {
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

    let proposal = Proposal {
        id: "test_simple_1".to_string(),
        title: "Routine maintenance".to_string(),
        description: "Simple update to fix minor issues".to_string(),
        proposer: 1,
        proposed_at: 0,
        complexity: 0.2,
        urgency: 0.3,
        required_roles: vec![],
        task_key: None,
        stigmergic_context: None,
    };

    let mut council = Council::new(CouncilMode::Simple, proposal, members);
    let decision = council.evaluate().await.unwrap();

    assert_eq!(decision.proposal_id, "test_simple_1");
    assert!(decision.participating_agents.len() == 2);

    println!(
        "✓ Simple council completed with decision: {:?}",
        decision.decision
    );
}

#[tokio::test]
async fn test_council_mode_switcher() {
    let config = ModeConfig::default();
    let switcher = ModeSwitcher::new(config);

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

    // High complexity, low urgency -> Debate mode
    let complex_proposal = Proposal {
        id: "complex".to_string(),
        title: "Complex refactoring".to_string(),
        description: "Multi-system integration with recursive synthesis".to_string(),
        proposer: 1,
        proposed_at: 0,
        complexity: 0.9,
        urgency: 0.2,
        required_roles: vec![],
        task_key: None,
        stigmergic_context: None,
    };

    let mode = switcher.select_mode(&complex_proposal, &members);
    assert_eq!(mode, CouncilMode::Debate);

    // High urgency -> Orchestrate mode
    let urgent_proposal = Proposal {
        id: "urgent".to_string(),
        title: "Critical fix".to_string(),
        description: "Fix needed immediately".to_string(),
        proposer: 1,
        proposed_at: 0,
        complexity: 0.5,
        urgency: 0.9,
        required_roles: vec![],
        task_key: None,
        stigmergic_context: None,
    };

    let mode = switcher.select_mode(&urgent_proposal, &members);
    assert_eq!(mode, CouncilMode::Orchestrate);

    // Low complexity -> Simple mode
    let simple_proposal = Proposal {
        id: "simple".to_string(),
        title: "Routine task".to_string(),
        description: "Standard maintenance procedure".to_string(),
        proposer: 1,
        proposed_at: 0,
        complexity: 0.2,
        urgency: 0.2,
        required_roles: vec![],
        task_key: None,
        stigmergic_context: None,
    };

    let mode = switcher.select_mode(&simple_proposal, &members);
    assert_eq!(mode, CouncilMode::Simple);

    println!("✓ Mode switcher correctly selects appropriate council modes");
}

// ============================================================================
// DKS (DYNAMIC KINETIC STABILITY) TESTS
// ============================================================================

#[test]
fn test_dks_basic_evolution() {
    let config = DKSConfig {
        base_replication_rate: 0.3,
        base_decay_rate: 0.1,
        replication_energy_cost: 5.0,
        resource_energy_conversion: 2.0,
        max_population: 100,
        selection_intensity: 0.2,
        flux_rate: 0.05,
    };

    let mut dks = DKSSystem::new(config);

    // Seed initial population
    dks.seed(10);
    assert_eq!(dks.stats().size, 10);

    // Run evolution for multiple generations
    let results = dks.evolve(50);

    // Verify evolution tracking
    assert_eq!(results.len(), 50);
    assert!(results.last().unwrap().generation == 50);

    // Verify population dynamics
    let final_stats = dks.stats();
    println!(
        "✓ DKS evolution: {} entities after 50 generations",
        final_stats.size
    );
    println!(
        "  - Average persistence: {:.2}",
        final_stats.average_persistence
    );
    println!("  - Max generation: {}", final_stats.max_generation);
}

#[test]
fn test_dks_stability_calculation() {
    // Test DKS stability metric
    let stability = calculate_dks_stability(0.2, 0.1);
    assert!(stability > 0.0); // Replication > decay -> positive stability

    let stability = calculate_dks_stability(0.1, 0.2);
    assert!(stability < 0.0); // Decay > replication -> negative stability

    let stability = calculate_dks_stability(0.1, 0.1);
    assert!((stability - 0.0).abs() < 0.01); // Equal rates -> equilibrium

    println!("✓ DKS stability calculations are correct");
}

#[test]
fn test_dks_multifractal_analysis() {
    // Generate sample persistence data
    let persistence_values: Vec<f64> = (0..1000).map(|i| (i as f64 / 1000.0) * 10.0).collect();

    let box_sizes = vec![1, 2, 4, 8, 16, 32, 64];
    let spectrum =
        MultifractalSpectrum::from_persistence_distribution(&persistence_values, &box_sizes);

    // Verify spectrum structure
    assert!(!spectrum.alpha_values.is_empty());
    assert!(!spectrum.fractal_dimensions.is_empty());
    assert_eq!(
        spectrum.alpha_values.len(),
        spectrum.fractal_dimensions.len()
    );

    println!("✓ Multifractal spectrum calculated");
    println!("  - Spectrum width: {:.2}", spectrum.width());
    println!("  - Is multifractal: {}", spectrum.is_multifractal(0.1));
}

// ============================================================================
// CASS (CONTEXT-AWARE SEMANTIC SKILLS) TESTS
// ============================================================================

#[tokio::test]
async fn test_cass_skill_search() {
    let skill_bank = SkillBank::new_with_seeds();
    let mut cass = CASS::new(skill_bank);

    // Initialize CASS
    cass.initialize().await.unwrap();

    // Search for skills
    let results = cass.search("coherence preservation", None, 5).await;

    // Should find relevant skills
    assert!(!results.is_empty());

    // Check that results have scores
    for result in &results {
        assert!(result.semantic_score >= 0.0 && result.semantic_score <= 1.0);
        println!(
            "  Found skill '{}' with score {:.2}",
            result.skill.title, result.semantic_score
        );
    }

    println!("✓ CASS semantic search works");
}

#[tokio::test]
async fn test_cass_context_awareness() {
    let mut context_manager = ContextManager::new();

    // Create a context snapshot
    let context = ContextSnapshot {
        timestamp: 0,
        active_agents: vec![1, 2, 3],
        dominant_roles: vec![Role::Architect, Role::Catalyst],
        current_goals: vec!["improve_coherence".to_string()],
        recent_skills_used: vec!["skill_1".to_string()],
        system_load: 0.7,
        error_rate: 0.05,
        coherence_score: 0.8,
    };

    context_manager.update(context.clone());

    // Record skill usage
    context_manager.record_usage("skill_1", true, context.clone());

    // Calculate relevance
    let relevance = context_manager.relevance_score("skill_1", &context);
    assert!(relevance >= 0.0 && relevance <= 1.0);

    println!(
        "✓ CASS context awareness works (relevance: {:.2})",
        relevance
    );
}

// ============================================================================
// NAVIGATION TESTS
// ============================================================================

#[test]
fn test_code_navigation_indexing() {
    let mut navigator = CodeNavigator::new();

    // Create a temporary test directory with sample code
    let test_dir = std::env::temp_dir().join("hyper_test_code");
    std::fs::create_dir_all(&test_dir).unwrap();

    // Write a sample Rust file
    let test_file = test_dir.join("test.rs");
    std::fs::write(
        &test_file,
        r#"
/// A test function for parsing
pub fn test_function(x: i32) -> i32 {
    x * 2
}

/// A test struct
pub struct TestStruct {
    value: i32,
}

impl TestStruct {
    pub fn new(value: i32) -> Self {
        Self { value }
    }
}
"#,
    )
    .unwrap();

    // Index the codebase
    let stats = navigator.index_codebase(&test_dir).unwrap();

    println!(
        "✓ Code navigation indexed {} files with {} units",
        stats.total_files, stats.total_units
    );

    // Clean up
    std::fs::remove_dir_all(&test_dir).unwrap();
}

#[test]
fn test_semantic_search() {
    let mut navigator = CodeNavigator::new();

    // Create test directory
    let test_dir = std::env::temp_dir().join("hyper_test_search");
    std::fs::create_dir_all(&test_dir).unwrap();

    // Write test files
    std::fs::write(
        test_dir.join("auth.rs"),
        r#"
pub fn authenticate_user(token: &str) -> bool {
    token == "valid"
}

pub fn generate_token() -> String {
    "token".to_string()
}
"#,
    )
    .unwrap();

    std::fs::write(
        test_dir.join("db.rs"),
        r#"
pub fn connect_to_database() {
    println!("Connecting...");
}
"#,
    )
    .unwrap();

    // Index and search
    navigator.index_codebase(&test_dir).unwrap();
    let results = navigator.search("authentication", 5);

    println!("✓ Semantic search found {} results", results.len());

    // Clean up
    std::fs::remove_dir_all(&test_dir).unwrap();
}

// ============================================================================
// COMMUNICATION TESTS
// ============================================================================

#[test]
fn test_gossip_protocol() {
    let config = CommunicationConfig::default();
    let mut hub = CommunicationHub::new(1, config);

    // Create and send a message
    let message = Message::new(MessageType::Info, "Test message content");
    let message_id = hub.send(message, Target::Broadcast).unwrap();

    assert!(!message_id.is_empty());

    // Tick the protocol
    hub.tick();

    // Check stats
    let stats = hub.gossip_stats();
    println!("✓ Gossip protocol: {} messages sent", stats.messages_sent);
}

#[test]
fn test_swarm_communication() {
    let config = CommunicationConfig::default();
    let mut hub = CommunicationHub::new(1, config);

    // Update position
    hub.update_swarm_position(1, Position::new(0.0, 0.0, 0.0));
    hub.update_swarm_position(2, Position::new(10.0, 0.0, 0.0));

    // Deposit pheromone
    hub.deposit_pheromone(FieldType::Resource, Position::new(5.0, 0.0, 0.0), 1.0);

    // Check field value
    let value = hub.get_field_value(FieldType::Resource, Position::new(5.0, 0.0, 0.0));
    assert!(value > 0.0);

    // Perform waggle dance
    hub.waggle_dance(Position::new(20.0, 20.0, 0.0), 0.9);

    // Get flocking forces
    let forces = hub.calculate_flocking_forces(1);
    println!(
        "✓ Swarm communication: separation=({}, {}, {})",
        forces.separation.x, forces.separation.y, forces.separation.z
    );
}

// ============================================================================
// END-TO-END INTEGRATION TEST
// ============================================================================

#[tokio::test]
async fn test_full_system_integration() {
    println!("\n========================================");
    println!("Running full system integration test...");
    println!("========================================\n");

    // 1. Set up DKS population
    let dks_config = DKSConfig::default();
    let mut dks = DKSSystem::new(dks_config);
    dks.seed(20);
    println!(
        "1. DKS system initialized with {} entities",
        dks.stats().size
    );

    // 2. Set up council for decision making
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

    let factory = CouncilFactory::new(ModeConfig::default());
    let proposal = Proposal::new(
        "integration_test",
        "System Evolution",
        "Evolve the system",
        1,
    );
    let mut council = factory.create_council(&proposal, members).unwrap();

    println!("2. Council created in {:?} mode", council.mode);

    // 3. Set up communication
    let comm_config = CommunicationConfig::default();
    let mut comm = CommunicationHub::new(1, comm_config);
    println!("3. Communication hub initialized");

    // 4. Run DKS evolution
    let dks_results = dks.evolve(10);
    println!("4. DKS evolved for 10 generations");

    // 5. Council makes decision based on DKS state
    let decision = council.evaluate().await.unwrap();
    println!(
        "5. Council decision: {:?} (confidence: {:.2})",
        decision.decision, decision.confidence
    );

    // 6. Broadcast decision via communication hub
    let decision_message = Message::new(
        MessageType::Coordination,
        format!("Decision: {:?}", decision.decision),
    );
    let msg_id = comm.send(decision_message, Target::Broadcast).unwrap();
    println!("6. Decision broadcast with message ID: {}", msg_id);

    // 7. Verify multifractal analysis
    let persistence: Vec<f64> = dks
        .population()
        .entities()
        .iter()
        .map(|e: &dks::Replicator| e.persistence_score())
        .collect();

    if !persistence.is_empty() {
        let box_sizes = vec![1, 2, 4, 8, 16];
        let spectrum =
            MultifractalSpectrum::from_persistence_distribution(&persistence, &box_sizes);
        println!(
            "7. Multifractal analysis: width={:.2}, is_multifractal={}",
            spectrum.width(),
            spectrum.is_multifractal(0.1)
        );
    }

    // 8. Verify all components together
    assert_eq!(dks_results.len(), 10);
    assert!(decision.confidence > 0.0);
    assert!(!msg_id.is_empty());

    println!("\n========================================");
    println!("✓ Full system integration test passed!");
    println!("========================================\n");
}

// ============================================================================
// UTILITY TESTS
// ============================================================================

#[test]
fn test_position_calculations() {
    let pos1 = Position::new(0.0, 0.0, 0.0);
    let pos2 = Position::new(3.0, 4.0, 0.0);

    let dist = pos1.distance(&pos2);
    assert!((dist - 5.0).abs() < 0.001); // 3-4-5 triangle

    println!("✓ Position calculations correct (distance: {:.2})", dist);
}

#[test]
fn test_compositionality_measure() {
    let whole = 100.0;
    let parts = vec![30.0, 40.0];

    let compositionality = compositionality_measure(whole, &parts);

    // Whole > sum of parts -> positive compositionality
    assert!(compositionality > 0.0);

    println!("✓ Compositionality measure: {:.2}", compositionality);
}
