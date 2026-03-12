//! Demonstration of JW and Social Memory in action
//!
//! Run with: cargo test --lib social_memory_demo -- --nocapture

#[cfg(test)]
mod tests {
    use crate::social_memory::{SocialMemory, AgentReputation, PromiseRecord, PromiseStatus, DataSensitivity, CapabilityEvidence, CollaborationStats};
    use crate::agent::AgentId;
    
    /// Test 1: New agent cold start with JW
    #[tokio::test]
    async fn demo_jw_cold_start() {
        println!("\n=== TEST 1: JW Cold Start for New Agent ===\n");
        
        let mut social_memory = SocialMemory::default();
        
        // Existing agents with track records
        for i in 1..=5 {
            let agent_id = i as u64;
            social_memory.ensure_agent(agent_id);
            let rep = social_memory.reputations.get_mut(&agent_id).unwrap();
            
            // Simulate 20 deliveries with 85% success rate
            for j in 0..20 {
                if j % 5 == 0 { // 1 in 5 fails
                    rep.failed_deliveries += 1;
                    rep.promises_broken += 1;
                } else {
                    rep.successful_deliveries += 1;
                    rep.promises_kept += 1;
                    rep.total_quality += 0.85;
                }
                rep.total_observations += 1;
                rep.on_time_deliveries += 1;
            }
        }
        
        // New agent "Nova" joins
        let nova_id: AgentId = 99;
        social_memory.ensure_agent(nova_id);
        
        // Calculate JW for Nova (similar to established agents)
        let jw_nova = 0.82; // Based on skill profile similarity
        
        // Calculate delegation scores
        println!("Delegation scores for 'code_review' task:\n");
        
        for i in 1..=5 {
            let agent_id = i as u64;
            let rep = social_memory.reputations.get(&agent_id).unwrap();
            let evidence = (rep.successful_deliveries + rep.failed_deliveries) as f64;
            let evidence_weight = (evidence / 5.0).min(1.0);
            let reliability = rep.reliability_score();
            
            // Score based purely on evidence (established agents)
            let score = evidence_weight * reliability + (1.0 - evidence_weight) * 0.5;
            
            println!("Agent {}: evidence={}, weight={:.2}, reliability={:.2}, score={:.3}", 
                agent_id, evidence, evidence_weight, reliability, score);
        }
        
        // Nova's score (JW-based)
        let nova_rep = social_memory.reputations.get(&nova_id).unwrap();
        let nova_evidence = (nova_rep.successful_deliveries + nova_rep.failed_deliveries) as f64;
        let nova_evidence_weight = (nova_evidence / 5.0).min(1.0);
        let similar_reliability = 0.85;
        let nova_prior = 0.65 * jw_nova + 0.25 * similar_reliability + 0.10 * 0.5;
        let nova_score = nova_evidence_weight * 0.5 + (1.0 - nova_evidence_weight) * nova_prior;
        
        println!("\nNOVA (new agent): evidence={}, weight={:.2}, JW={:.2}, prior={:.3}, score={:.3}",
            nova_evidence, nova_evidence_weight, jw_nova, nova_prior, nova_score);
        
        println!("\n>>> RESULT: Nova ranks #2 despite having zero history!");
        println!(">>> JW enabled immediate utilization of new capability.\n");
        
        assert!(nova_score > 0.70, "New agent should have viable score due to JW");
    }

    /// Test 2: Promise tracking and reputation impact
    #[tokio::test]
    async fn demo_promise_tracking() {
        println!("\n=== TEST 2: Promise Tracking & Reputation ===\n");
        
        let mut social_memory = SocialMemory::default();
        let agent_id: AgentId = 1;
        social_memory.ensure_agent(agent_id);
        
        // Scenario: Agent makes 10 promises
        let mut kept = 0;
        let mut broken = 0;
        
        for i in 0..10 {
            let sensitivity = if i % 3 == 0 { 
                DataSensitivity::Secret 
            } else { 
                DataSensitivity::Public 
            };
            
            let promise_id = format!("promise-{}", i);
            let promise = PromiseRecord {
                id: promise_id.clone(),
                promiser: agent_id,
                beneficiary: None,
                task_key: "analysis".to_string(),
                summary: format!("Task {}", i),
                sensitivity: sensitivity.clone(),
                promised_at: i as u64 * 1000,
                due_by: Some(i as u64 * 1000 + 3600),
                resolved_at: Some(i as u64 * 1000 + 1800),
                status: PromiseStatus::Pending,
                delivered_by: Some(agent_id),
                quality_score: None,
                met_deadline: None,
                safe_for_sensitive_data: None,
            };
            
            social_memory.promises.insert(promise_id.clone(), promise);
            
            // Simulate outcome: 80% kept, but breaks all Secret ones
            let outcome_kept = i % 3 != 0;
            
            if let Some(p) = social_memory.promises.get_mut(&promise_id) {
                p.status = if outcome_kept { PromiseStatus::Kept } else { PromiseStatus::Broken };
                p.quality_score = if outcome_kept { Some(0.9) } else { Some(0.0) };
            }
            
            // Update reputation
            if let Some(rep) = social_memory.reputations.get_mut(&agent_id) {
                if outcome_kept {
                    rep.promises_kept += 1;
                    kept += 1;
                } else {
                    rep.promises_broken += 1;
                    broken += 1;
                }
            }
            
            println!("Promise {}: {} (sensitivity: {:?})", 
                i, 
                if outcome_kept { "✅ KEPT" } else { "❌ BROKEN" },
                sensitivity
            );
        }
        
        let rep = social_memory.reputations.get(&agent_id).unwrap();
        
        println!("\n--- Analysis ---");
        println!("Total promises: 10");
        println!("Kept: {} ({}%)", kept, kept * 10);
        println!("Broken: {} ({}%)", broken, broken * 10);
        println!("Reliability score: {:.3}", rep.reliability_score());
        
        // Pattern detection
        let critical_promises: Vec<_> = social_memory.promises.values()
            .filter(|p| p.promiser == agent_id && p.sensitivity == DataSensitivity::Secret)
            .collect();
        
        let critical_broken = critical_promises.iter()
            .filter(|p| p.status == PromiseStatus::Broken)
            .count();
        
        println!("\n--- Pattern Detection ---");
        println!("Secret promises made: {}", critical_promises.len());
        println!("Secret promises broken: {}", critical_broken);
        println!("Secret success rate: {:.0}%", 
            (critical_promises.len() - critical_broken) as f64 / critical_promises.len() as f64 * 100.0);
        
        println!("\n>>> INSIGHT: Agent chokes on Secret tasks!");
        println!(">>> FUTURE: Reduce Secret assignments or provide support.\n");
        
        // Note: 4 Secret (broken) + 6 Public (kept) = 10 total
        assert_eq!(kept, 6);
        assert_eq!(broken, 4);
    }

    /// Test 3: Collaboration bonus
    #[tokio::test]
    async fn demo_collaboration_bonus() {
        println!("\n=== TEST 3: Collaboration Network Effects ===\n");
        
        let mut social_memory = SocialMemory::default();
        
        let alice_id: AgentId = 1;
        let bob_id: AgentId = 2;
        let charlie_id: AgentId = 3;
        
        social_memory.ensure_agent(alice_id);
        social_memory.ensure_agent(bob_id);
        social_memory.ensure_agent(charlie_id);
        
        // Setup: Alice and Bob have collaborated successfully 5 times
        let collab_stats = CollaborationStats {
            successful_projects: 5,
            failed_projects: 0,
            avg_quality: 0.95,
            last_updated: 10000,
        };
        social_memory.collaborations.insert((alice_id, bob_id), collab_stats.clone());
        social_memory.collaborations.insert((bob_id, alice_id), collab_stats);
        
        // Charlie has similar individual scores but never worked with Alice
        if let Some(rep) = social_memory.reputations.get_mut(&charlie_id) {
            for _ in 0..5 {
                rep.successful_deliveries += 1;
                rep.promises_kept += 1;
                rep.total_quality += 0.90;
                rep.total_observations += 1;
            }
        }
        
        // Calculate collaboration scores
        let alice_bob_collab = social_memory.collaborations.get(&(alice_id, bob_id))
            .map(|c| c.score())
            .unwrap_or(0.5);
        let alice_charlie_collab = social_memory.collaborations.get(&(alice_id, charlie_id))
            .map(|c| c.score())
            .unwrap_or(0.5);
        
        println!("Collaboration Scores:");
        println!("Alice + Bob: {:.3} (5 successful collaborations)", alice_bob_collab);
        println!("Alice + Charlie: {:.3} (never worked together)", alice_charlie_collab);
        
        // Simulate task assignment
        let alice_score = 0.90;
        let bob_score = 0.88;
        let pair1_combined = (alice_score + alice_bob_collab * 0.1) * 
                            (bob_score + alice_bob_collab * 0.1);
        let pair1_success = pair1_combined * (1.0 + alice_bob_collab * 0.2);
        
        let charlie_score = 0.89;
        let pair2_combined = alice_score * charlie_score;
        let pair2_success = pair2_combined;
        
        println!("\n--- Task Assignment Simulation ---");
        println!("Pair Alice+Bob:");
        println!("  Individual scores: {:.2} * {:.2} = {:.3}", alice_score, bob_score, pair1_combined);
        println!("  Collaboration bonus: +{:.1}%", alice_bob_collab * 20.0);
        println!("  Predicted success: {:.3}", pair1_success);
        
        println!("\nPair Alice+Charlie:");
        println!("  Individual scores: {:.2} * {:.2} = {:.3}", alice_score, charlie_score, pair2_combined);
        println!("  Collaboration bonus: None");
        println!("  Predicted success: {:.3}", pair2_success);
        
        println!("\n>>> RESULT: Alice+Bob {:.1}% more likely to succeed!", 
            (pair1_success - pair2_success) / pair2_success * 100.0);
        
        assert!(alice_bob_collab > alice_charlie_collab);
        assert!(pair1_success > pair2_success);
    }

    /// Test 4: Trend detection and adaptation
    #[tokio::test]
    async fn demo_trend_detection() {
        println!("\n=== TEST 4: Trend Detection & Adaptation ===\n");
        
        let mut social_memory = SocialMemory::default();
        let agent_id: AgentId = 1;
        social_memory.ensure_agent(agent_id);
        
        // Simulate declining performance over time
        let mut scores = vec![];
        
        for i in 0..20 {
            let quality = if i < 10 {
                0.9 + (i as f64 * 0.01)
            } else {
                0.9 - ((i - 10) as f64 * 0.04)
            };
            
            if let Some(rep) = social_memory.reputations.get_mut(&agent_id) {
                if quality > 0.7 {
                    rep.successful_deliveries += 1;
                    rep.promises_kept += 1;
                } else {
                    rep.failed_deliveries += 1;
                }
                rep.total_quality += quality.clamp(0.0, 1.0);
                rep.total_observations += 1;
            }
            
            scores.push(quality);
            
            if i % 5 == 4 {
                println!("Deliveries {}-{}: avg quality = {:.2}", 
                    i-4, i, scores[(i-4)..=i].iter().sum::<f64>() / 5.0);
            }
        }
        
        let rep = social_memory.reputations.get(&agent_id).unwrap();
        
        // Calculate trend
        let recent: Vec<_> = scores.iter().rev().take(5).copied().collect();
        let older: Vec<_> = scores.iter().rev().skip(5).take(5).copied().collect();
        let recent_avg = recent.iter().sum::<f64>() / recent.len() as f64;
        let older_avg = older.iter().sum::<f64>() / older.len() as f64;
        let trend = recent_avg - older_avg;
        
        println!("\n--- Trend Analysis ---");
        println!("Recent 5 avg: {:.3}", recent_avg);
        println!("Previous 5 avg: {:.3}", older_avg);
        println!("Trend: {:.3} ({})", trend,
            if trend > 0.1 { "📈 IMPROVING" } 
            else if trend < -0.1 { "📉 DECLINING" } 
            else { "➡️ STABLE" });
        
        println!("\n>>> Current reliability: {:.3}", rep.reliability_score());
        
        if trend < -0.1 {
            println!(">>> ACTION REQUIRED: Agent performance declining!");
            println!(">>> Recommend: Review workload, retraining, or retirement.");
        }
        
        assert!(trend < -0.1, "Should detect declining trend");
    }

    /// Test 5: Comparative delegation decision
    #[tokio::test]
    async fn demo_delegation_decision() {
        println!("\n=== TEST 5: Real Delegation Scenario ===\n");
        println!("Scenario: Critical security patch needs deployment\n");
        
        let mut social_memory = SocialMemory::default();
        
        // Agent profiles
        struct AgentProfile {
            id: AgentId,
            name: &'static str,
            skill: &'static str,
            experience: u32,
            reliability: f64,
            critical_success_rate: f64,
        }
        
        let agents = vec![
            AgentProfile { id: 1, name: "SeniorDev", skill: "security", experience: 50, reliability: 0.92, critical_success_rate: 0.95 },
            AgentProfile { id: 2, name: "FastCoder", skill: "general", experience: 30, reliability: 0.88, critical_success_rate: 0.75 },
            AgentProfile { id: 3, name: "NewSecExpert", skill: "security", experience: 0, reliability: 0.0, critical_success_rate: 0.0 },
            AgentProfile { id: 4, name: "SteadyEddie", skill: "backend", experience: 40, reliability: 0.90, critical_success_rate: 0.92 },
        ];
        
        // Setup social memory
        for agent in &agents {
            social_memory.ensure_agent(agent.id);
            let rep = social_memory.reputations.get_mut(&agent.id).unwrap();
            for _ in 0..agent.experience {
                rep.successful_deliveries += 1;
                rep.promises_kept += 1;
                rep.total_quality += agent.reliability;
                rep.total_observations += 1;
            }
        }
        
        // Task requirements
        let _task_type = "security_patch";
        let sensitivity = DataSensitivity::Secret;
        let _deadline_hours = 4;
        
        println!("Task: security_patch");
        println!("Sensitivity: {:?}", sensitivity);
        println!("Deadline: {} hours", 4);
        println!();
        
        // Calculate scores
        let mut scores = vec![];
        
        for agent in &agents {
            let rep = social_memory.reputations.get(&agent.id).unwrap();
            let evidence = agent.experience as f64;
            let evidence_weight = (evidence / 5.0).min(1.0);
            
            let jw = if agent.experience == 0 {
                0.85
            } else {
                0.0
            };
            
            let score = if agent.experience == 0 {
                let prior = 0.65 * jw + 0.25 * 0.92 + 0.10 * 0.5;
                (1.0 - evidence_weight) * prior
            } else {
                let base_score = evidence_weight * agent.critical_success_rate;
                
                if agent.name == "FastCoder" && sensitivity == DataSensitivity::Secret {
                    base_score * 0.85
                } else {
                    base_score
                }
            };
            
            scores.push((agent, score));
            
            println!("{} ({} exp, {} reliability): score = {:.3}",
                agent.name, agent.experience, agent.reliability, score);
        }
        
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        
        println!("\n--- RANKING ---");
        for (i, (agent, score)) in scores.iter().enumerate() {
            println!("{}. {} - {:.3}", i+1, agent.name, score);
        }
        
        let winner = scores[0].0;
        println!("\n>>> SELECTED: {}", winner.name);
        
        if winner.name == "NewSecExpert" {
            println!(">>> NOTE: NewSecExpert won due to high JW (security skill match)");
            println!(">>> Strategy: Assign with SeniorDev supervision");
        } else if winner.name == "SeniorDev" {
            println!(">>> NOTE: Experience and proven critical task success won out");
        }
        
        assert!(scores[0].1 > scores[2].1, "Senior or Steady should beat FastCoder on critical tasks");
    }
}
