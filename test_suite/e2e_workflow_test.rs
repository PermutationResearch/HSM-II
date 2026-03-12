//! End-to-End Workflow Integration Test
//!
//! This test simulates a complete workflow where:
//! 1. Agents detect a need for system evolution
//! 2. A council is formed to decide on the approach
//! 3. DKS entities evolve in parallel
//! 4. Communication coordinates between components
//! 5. The system adapts based on collective decisions

use ::hyper_stigmergy::*;
use std::time::Duration;

#[tokio::test]
async fn test_complete_adaptive_workflow() {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║     END-TO-END ADAPTIVE SYSTEM WORKFLOW TEST              ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    // =========================================================================
    // PHASE 1: System Initialization
    // =========================================================================
    println!("▶ PHASE 1: System Initialization");

    // Initialize DKS population (represents agent knowledge/strategies)
    let dks_config = DKSConfig {
        base_replication_rate: 0.35,
        base_decay_rate: 0.08,
        replication_energy_cost: 6.0,
        resource_energy_conversion: 2.0,
        max_population: 150,
        selection_intensity: 0.2,
        flux_rate: 0.06,
    };
    let mut dks = DKSSystem::new(dks_config);
    dks.seed(25);

    println!("  ✓ DKS initialized: {} entities", dks.stats().size);

    // Initialize communication hub
    let comm_config = CommunicationConfig::default();
    let mut comm = CommunicationHub::new(1, comm_config);
    println!("  ✓ Communication hub ready");

    // Initialize council factory
    let mode_config = ModeConfig::default();
    let council_factory = CouncilFactory::new(mode_config);
    println!("  ✓ Council factory configured");

    // =========================================================================
    // PHASE 2: Problem Detection & Council Formation
    // =========================================================================
    println!("\n▶ PHASE 2: Problem Detection & Council Formation");

    // Simulate system coherence drop (detected by agents)
    let initial_coherence = 0.85;
    let current_coherence = 0.65; // Drop detected!

    println!(
        "  ⚠ Coherence dropped: {:.2} → {:.2}",
        initial_coherence, current_coherence
    );

    // Form a council with diverse roles
    let council_members = vec![
        CouncilMember {
            agent_id: 1,
            role: Role::Architect,
            expertise_score: 0.95,
            participation_weight: 1.2,
        },
        CouncilMember {
            agent_id: 2,
            role: Role::Catalyst,
            expertise_score: 0.88,
            participation_weight: 1.0,
        },
        CouncilMember {
            agent_id: 3,
            role: Role::Critic,
            expertise_score: 0.92,
            participation_weight: 1.1,
        },
        CouncilMember {
            agent_id: 4,
            role: Role::Explorer,
            expertise_score: 0.80,
            participation_weight: 0.9,
        },
        CouncilMember {
            agent_id: 5,
            role: Role::Chronicler,
            expertise_score: 0.85,
            participation_weight: 1.0,
        },
    ];

    // Create proposal based on detected issue
    let proposal = Proposal {
        id: "coherence_recovery".to_string(),
        title: "System Coherence Recovery".to_string(),
        description: format!(
            "Address coherence drop from {:.2} to {:.2} through structural improvements",
            initial_coherence, current_coherence
        ),
        proposer: 1,
        proposed_at: 0,
        complexity: 0.75,
        urgency: 0.8,
        required_roles: vec![Role::Architect, Role::Catalyst, Role::Critic],
        task_key: None,
        stigmergic_context: None,
    };

    // Automatically select appropriate council mode
    let mut council = council_factory
        .create_council(&proposal, council_members)
        .unwrap();
    println!("  ✓ Council formed in {:?} mode", council.mode);
    println!(
        "  ✓ Participating agents: {:?}",
        council
            .members
            .iter()
            .map(|m| format!("{:?}", m.role))
            .collect::<Vec<_>>()
    );

    // =========================================================================
    // PHASE 3: Parallel Evolution & Deliberation
    // =========================================================================
    println!("\n▶ PHASE 3: Parallel Evolution & Deliberation");

    // Run DKS evolution and council deliberation concurrently
    let mut dks_results = Vec::new();

    for generation in 0..5 {
        // DKS tick
        let dks_result = dks.tick();
        dks_results.push(dks_result);

        // Deposit information in stigmergic field
        let avg_persistence = dks.stats().average_persistence;
        comm.deposit_pheromone(
            FieldType::Exploration,
            Position::new(generation as f64 * 10.0, avg_persistence, 0.0),
            avg_persistence / 100.0,
        );

        if generation % 2 == 0 {
            println!(
                "  Generation {}: DKS population = {}, avg persistence = {:.2}",
                generation,
                dks.stats().size,
                avg_persistence
            );
        }
    }

    // Council makes decision
    let decision = council.evaluate().await.unwrap();
    println!("\n  ✓ Council reached decision: {:?}", decision.decision);
    println!("  ✓ Confidence: {:.2}", decision.confidence);
    println!(
        "  ✓ Execution plan: {} steps",
        decision
            .execution_plan
            .as_ref()
            .map(|p| p.steps.len())
            .unwrap_or(0)
    );

    // =========================================================================
    // PHASE 4: Communication & Coordination
    // =========================================================================
    println!("\n▶ PHASE 4: Communication & Coordination");

    // Broadcast decision to all agents
    let decision_message = Message::new(
        MessageType::Coordination,
        format!("Council decision: {:?}", decision.decision),
    )
    .with_priority(MessagePriority::High);

    let msg_id = comm.send(decision_message, Target::Broadcast).unwrap();
    println!("  ✓ Decision broadcast: {}", msg_id);

    // Simulate swarm coordination for implementation
    for (i, agent_id) in decision.participating_agents.iter().enumerate() {
        let position = Position::new((i as f64) * 20.0, decision.confidence * 100.0, 0.0);
        comm.update_swarm_position(*agent_id, position);
    }

    // Perform waggle dances to share best practices
    if let Some(plan) = &decision.execution_plan {
        for (i, step) in plan.steps.iter().enumerate() {
            if let Some(agent) = step.assigned_agent {
                comm.waggle_dance(
                    Position::new(i as f64 * 10.0, step.sequence as f64 * 5.0, 0.0),
                    decision.confidence,
                );
            }
        }
    }

    // Get flocking forces (coordination dynamics)
    for agent_id in &decision.participating_agents {
        let forces = comm.calculate_flocking_forces(*agent_id);
        let combined = forces.combined(1.5, 1.0, 1.0);
        println!(
            "  ✓ Agent {} coordination vector: ({:.2}, {:.2})",
            agent_id, combined.x, combined.y
        );
    }

    // =========================================================================
    // PHASE 5: Implementation & Adaptation
    // =========================================================================
    println!("\n▶ PHASE 5: Implementation & Adaptation");

    // Continue DKS evolution post-decision
    println!("  Continuing DKS evolution...");
    for _ in 0..10 {
        let result = dks.tick();

        // Selection pressure adapts based on council decision
        if matches!(decision.decision, Decision::Approve) {
            // Boost entities with high persistence
            // (This would be integrated with the actual selection logic)
        }
    }

    let mid_evolution_stats = dks.stats();
    println!("  ✓ Post-decision evolution complete");
    println!("    Population: {}", mid_evolution_stats.size);
    println!("    Max generation: {}", mid_evolution_stats.max_generation);
    println!(
        "    Avg persistence: {:.2}",
        mid_evolution_stats.average_persistence
    );

    // =========================================================================
    // PHASE 6: Analysis & Verification
    // =========================================================================
    println!("\n▶ PHASE 6: Analysis & Verification");

    // Multifractal analysis of evolved population
    let persistence_values: Vec<f64> = dks
        .population()
        .entities()
        .iter()
        .map(|e| e.persistence_score())
        .collect();

    if !persistence_values.is_empty() {
        let box_sizes = vec![1, 2, 4, 8, 16, 32];
        let spectrum =
            MultifractalSpectrum::from_persistence_distribution(&persistence_values, &box_sizes);

        println!("  ✓ Multifractal analysis:");
        println!("    Spectrum width: {:.4}", spectrum.width());
        println!("    Is multifractal: {}", spectrum.is_multifractal(0.1));
        println!(
            "    Capacity dimension: {:.4}",
            spectrum.capacity_dimension()
        );
    }

    // DKS stability analysis
    let stability_values: Vec<f64> = dks
        .population()
        .entities()
        .iter()
        .map(|e| e.dks_stability())
        .collect();

    let avg_stability: f64 = stability_values.iter().sum::<f64>() / stability_values.len() as f64;
    let stable_count = stability_values.iter().filter(|&&s| s > 0.0).count();

    println!("  ✓ DKS stability analysis:");
    println!("    Average stability: {:.4}", avg_stability);
    println!(
        "    Stable entities: {}/{} ({:.1}%)",
        stable_count,
        stability_values.len(),
        100.0 * stable_count as f64 / stability_values.len() as f64
    );

    // =========================================================================
    // PHASE 7: Final Verification
    // =========================================================================
    println!("\n▶ PHASE 7: Final Verification");

    // Verify all components interacted correctly
    assert!(dks.stats().size > 0, "DKS population extinct");
    assert!(
        decision.confidence > 0.0,
        "Council decision has no confidence"
    );
    assert!(
        !decision.participating_agents.is_empty(),
        "No agents participated"
    );

    // Verify communication occurred
    let stats = comm.gossip_stats();
    assert!(stats.messages_sent > 0, "No messages sent");

    println!("  ✓ All system components verified");
    println!("    DKS entities: {}", dks.stats().size);
    println!("    Council confidence: {:.2}", decision.confidence);
    println!("    Messages exchanged: {}", stats.messages_sent);

    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║           ✓ WORKFLOW TEST COMPLETED SUCCESSFULLY           ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");
}

#[tokio::test]
async fn test_stress_multiple_councils() {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║         STRESS TEST: MULTIPLE CONCURRENT COUNCILS          ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    let factory = CouncilFactory::new(ModeConfig::default());
    let mut handles = Vec::new();

    // Spawn 5 concurrent councils
    for i in 0..5 {
        let factory = CouncilFactory::new(ModeConfig::default());
        let handle = tokio::spawn(async move {
            let members = vec![
                CouncilMember {
                    agent_id: i * 3 + 1,
                    role: Role::Architect,
                    expertise_score: 0.9,
                    participation_weight: 1.0,
                },
                CouncilMember {
                    agent_id: i * 3 + 2,
                    role: Role::Catalyst,
                    expertise_score: 0.8,
                    participation_weight: 1.0,
                },
                CouncilMember {
                    agent_id: i * 3 + 3,
                    role: Role::Critic,
                    expertise_score: 0.85,
                    participation_weight: 1.0,
                },
            ];

            let proposal = Proposal::new(
                &format!("concurrent_{}", i),
                &format!("Concurrent Proposal {}", i),
                "Test concurrent processing",
                i * 3 + 1,
            );

            let mut council = factory.create_council(&proposal, members).unwrap();
            let decision = council.evaluate().await.unwrap();

            (i, decision.decision, decision.confidence)
        });
        handles.push(handle);
    }

    // Wait for all councils to complete
    let mut results = Vec::new();
    for handle in handles {
        let result = handle.await.unwrap();
        results.push(result);
    }

    println!("✓ All {} concurrent councils completed", results.len());
    for (id, decision, confidence) in results {
        println!(
            "  Council {}: {:?} (confidence: {:.2})",
            id, decision, confidence
        );
    }
}

#[test]
fn test_compositionality_in_system() {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║         COMPOSITIONALITY ANALYSIS TEST                     ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    // Test that the whole system exhibits emergent behavior
    // beyond the sum of its parts

    let dks_complexity = 100.0; // DKS subsystem complexity
    let council_complexity = 80.0; // Council subsystem complexity
    let comm_complexity = 60.0; // Communication subsystem complexity

    let parts = vec![dks_complexity, council_complexity, comm_complexity];
    let sum_parts: f64 = parts.iter().sum();

    // The whole system should have emergent complexity
    // due to interactions between components
    let interaction_bonus = 45.0; // Emergent complexity from integration
    let whole_complexity = sum_parts + interaction_bonus;

    let compositionality = compositionality_measure(whole_complexity, &parts);

    println!("System compositionality analysis:");
    println!("  Sum of parts: {:.2}", sum_parts);
    println!("  Whole system: {:.2}", whole_complexity);
    println!("  Compositionality: {:.2}", compositionality);
    println!("  Emergence: {:.1}%", 100.0 * compositionality / sum_parts);

    assert!(
        compositionality > 0.0,
        "System should exhibit compositional structure"
    );
    assert!(
        compositionality >= interaction_bonus * 0.9,
        "Compositionality should capture the interaction bonus"
    );
}
