//! Communication system comprehensive tests

use ::hyper_stigmergy::*;

#[test]
fn test_message_creation() {
    let message = Message::new(MessageType::Proposal, "Test proposal content")
        .with_priority(MessagePriority::High)
        .with_metadata("topic", "architecture");

    assert_eq!(message.message_type, MessageType::Proposal);
    assert_eq!(message.priority, MessagePriority::High);
    assert_eq!(
        message.metadata.get("topic"),
        Some(&"architecture".to_string())
    );
}

#[test]
fn test_message_envelope() {
    let message = Message::new(MessageType::Info, "Test message");
    let envelope = MessageEnvelope::new(
        1, // sender
        message,
        Target::Broadcast,
    );

    assert_eq!(envelope.sender, 1);
    assert!(matches!(envelope.recipient, Target::Broadcast));
    assert!(!envelope.id.is_empty());
}

#[test]
fn test_gossip_protocol_basic() {
    let config = GossipConfig {
        fanout: 3,
        default_ttl: 10,
        hot_process_interval: 5,
        max_hot_rumors: 100,
        cold_forward_probability: 0.1,
    };

    let mut gossip = GossipProtocol::new(config);

    // Submit a message
    let message = Message::new(MessageType::Info, "Gossip test message");
    let envelope = MessageEnvelope::new(1, message, Target::Broadcast);
    gossip.submit_message(envelope);

    // Check stats
    let stats = gossip.stats();
    assert_eq!(stats.rumors_active, 1);

    // Tick the protocol
    gossip.tick();

    // Get outgoing messages
    let outgoing = gossip.get_outgoing(10);
    println!("✓ Gossip protocol: {} outgoing messages", outgoing.len());
}

#[test]
fn test_gossip_message_handlers() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let mut gossip = GossipProtocol::new(GossipConfig::default());

    static HANDLER_CALLED: AtomicBool = AtomicBool::new(false);
    HANDLER_CALLED.store(false, Ordering::SeqCst);

    gossip.register_handler("info", |_msg| {
        HANDLER_CALLED.store(true, Ordering::SeqCst);
    });

    // Submit and process a message
    let message = Message::new(MessageType::Info, "Test");
    let envelope = MessageEnvelope::new(1, message, Target::Broadcast);
    gossip.receive_message(envelope);

    assert!(HANDLER_CALLED.load(Ordering::SeqCst));
}

#[test]
fn test_swarm_position_updates() {
    let mut swarm = SwarmCommunication::new();

    // Update positions
    swarm.update_position(1, Position::new(0.0, 0.0, 0.0));
    swarm.update_position(2, Position::new(10.0, 0.0, 0.0));
    swarm.update_position(3, Position::new(5.0, 5.0, 0.0));

    // Check field value at location
    swarm.deposit_pheromone(FieldType::Resource, Position::new(5.0, 0.0, 0.0), 1.0);

    let value = swarm.get_field_value(FieldType::Resource, Position::new(5.0, 0.0, 0.0));
    assert!(value > 0.0);

    println!("✓ Swarm position and field test");
}

#[test]
fn test_waggle_dance() {
    let mut swarm = SwarmCommunication::new();

    // Perform waggle dances
    swarm.perform_waggle_dance(1, Position::new(100.0, 100.0, 0.0), 0.9);
    swarm.perform_waggle_dance(2, Position::new(50.0, 50.0, 0.0), 0.7);
    swarm.perform_waggle_dance(3, Position::new(110.0, 90.0, 0.0), 0.8);

    // Get relevant dances near a position
    let dances = swarm.get_relevant_dances(Position::new(105.0, 95.0, 0.0), 20.0);

    println!(
        "✓ Waggle dance test: {} relevant dances found",
        dances.len()
    );

    // Should find dances near (105, 95)
    assert!(!dances.is_empty());
}

#[test]
fn test_flocking_forces() {
    let mut swarm = SwarmCommunication::new();

    // Set up agent positions
    swarm.update_position(1, Position::new(0.0, 0.0, 0.0));
    swarm.update_position(2, Position::new(5.0, 0.0, 0.0));
    swarm.update_position(3, Position::new(-5.0, 0.0, 0.0));
    swarm.update_position(4, Position::new(0.0, 5.0, 0.0));

    // Calculate flocking forces for agent 1
    let forces = swarm.calculate_flocking_forces(1);

    println!("✓ Flocking forces for agent 1:");
    println!(
        "  Separation: ({:.2}, {:.2}, {:.2})",
        forces.separation.x, forces.separation.y, forces.separation.z
    );
    println!(
        "  Alignment: ({:.2}, {:.2}, {:.2})",
        forces.alignment.x, forces.alignment.y, forces.alignment.z
    );
    println!(
        "  Cohesion: ({:.2}, {:.2}, {:.2})",
        forces.cohesion.x, forces.cohesion.y, forces.cohesion.z
    );

    // Combined force
    let combined = forces.combined(1.5, 1.0, 1.0);
    println!(
        "  Combined force: ({:.2}, {:.2}, {:.2})",
        combined.x, combined.y, combined.z
    );
}

#[test]
fn test_communication_hub() {
    let config = CommunicationConfig::default();
    let mut hub = CommunicationHub::new(1, config);

    // Send different types of messages
    let msg1 = Message::new(MessageType::Proposal, "System proposal");
    let id1 = hub.send(msg1, Target::Broadcast).unwrap();

    let msg2 = Message::new(MessageType::Task, "Execute task");
    let id2 = hub.send(msg2, Target::Agent(2)).unwrap();

    let msg3 = Message::new(MessageType::Coordination, "Swarm signal");
    let id3 = hub.send(msg3, Target::Swarm).unwrap();

    println!("✓ Communication hub sent 3 messages");
    println!("  Broadcast: {}", id1);
    println!("  Direct: {}", id2);
    println!("  Swarm: {}", id3);

    // Tick and check stats
    hub.tick();
    let stats = hub.gossip_stats();
    println!(
        "  Gossip stats: {} active, {} sent",
        stats.rumors_active, stats.messages_sent
    );
}

#[test]
fn test_message_priority() {
    use MessagePriority::*;

    // Test priority ordering
    assert!(Critical < High);
    assert!(High < Normal);
    assert!(Normal < Low);

    // Create messages with different priorities
    let critical = Message::new(MessageType::Alert, "Critical!").with_priority(Critical);
    let normal = Message::new(MessageType::Info, "Info").with_priority(Normal);

    assert!(critical.priority < normal.priority);
}

#[test]
fn test_stigmergic_field_decay() {
    let mut field = StigmergicField::new(FieldType::Resource);

    // Deposit pheromones
    field.deposit(Position::new(0.0, 0.0, 0.0), 1.0);
    field.deposit(Position::new(1.0, 0.0, 0.0), 0.8);
    field.deposit(Position::new(2.0, 0.0, 0.0), 0.6);

    let initial_value = field.value_at(Position::new(0.0, 0.0, 0.0));

    // Apply decay multiple times
    for _ in 0..10 {
        field.decay();
    }

    let final_value = field.value_at(Position::new(0.0, 0.0, 0.0));

    println!("✓ Stigmergic field decay:");
    println!("  Initial: {:.4}, Final: {:.4}", initial_value, final_value);

    assert!(final_value < initial_value);
}

#[test]
fn test_position_distance() {
    let pos1 = Position::new(0.0, 0.0, 0.0);
    let pos2 = Position::new(3.0, 4.0, 0.0);
    let pos3 = Position::new(0.0, 0.0, 5.0);

    // 3-4-5 triangle in 2D
    let dist2d = pos1.distance(&pos2);
    assert!((dist2d - 5.0).abs() < 0.001);

    // 3D distance
    let dist3d = pos1.distance(&pos3);
    assert!((dist3d - 5.0).abs() < 0.001);

    // Same position
    let dist_same = pos1.distance(&pos1);
    assert!(dist_same < 0.001);

    println!("✓ Position distance calculations correct");
}

#[test]
fn test_rumor_lifecycle() {
    let mut gossip = GossipProtocol::new(GossipConfig::default());

    // Submit message
    let message = Message::new(MessageType::Discovery, "New discovery");
    let envelope = MessageEnvelope::new(1, message, Target::Broadcast);
    let id = envelope.id.clone();

    gossip.submit_message(envelope);

    // Check initial state
    let stats1 = gossip.stats();
    assert_eq!(stats1.rumors_active, 1);

    // Receive acknowledgment (simulating another node resolving it)
    let ack = MessageEnvelope::new(2, Message::new(MessageType::Info, ""), Target::Agent(1));
    gossip.receive_message(ack);

    // Check final stats
    let stats2 = gossip.stats();
    println!(
        "✓ Rumor lifecycle test: {} active -> {} resolved",
        stats1.rumors_resolved, stats2.rumors_resolved
    );
}
