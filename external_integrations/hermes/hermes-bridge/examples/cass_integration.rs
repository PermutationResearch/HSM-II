//! Example: CASS Skill Integration with Hermes
//!
//! This example shows how CASS skills from HSM-II can be executed via Hermes.

use hermes_bridge::{
    CASSSkill, HermesClientBuilder, SkillConverter, SkillLevel,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== CASS + Hermes Integration Demo ===\n");

    // Create client
    let client = HermesClientBuilder::new()
        .endpoint("http://localhost:8000")
        .build()?;

    client.initialize().await?;

    // Create sample CASS skills
    let cass_skills = vec![
        CASSSkill {
            id: "skill_001".to_string(),
            title: "Coherence Preservation".to_string(),
            principle: "Before any mutation, verify coherence delta is positive".to_string(),
            level: SkillLevel::General,
            confidence: 0.9,
            embedding: Some(vec![0.1, 0.2, 0.3, 0.4]),
        },
        CASSSkill {
            id: "skill_002".to_string(),
            title: "Web Research".to_string(),
            principle: "Use web search to gather current information before decision".to_string(),
            level: SkillLevel::TaskSpecific("research".to_string()),
            confidence: 0.8,
            embedding: Some(vec![0.5, 0.6, 0.7, 0.8]),
        },
        CASSSkill {
            id: "skill_003".to_string(),
            title: "Controlled Disruption".to_string(),
            principle: "Introduce novelty by rewiring weakest edges rather than random mutations".to_string(),
            level: SkillLevel::RoleSpecific("Catalyst".to_string()),
            confidence: 0.75,
            embedding: Some(vec![0.2, 0.4, 0.6, 0.8]),
        },
    ];

    println!("--- Converting CASS Skills to Hermes Format ---");
    let converter = SkillConverter::new();
    
    for skill in &cass_skills {
        let hermes_skill = converter.cass_to_hermes(skill);
        println!("CASS: [{}] {} (confidence: {:.0}%)", 
            skill.id, skill.title, skill.confidence * 100.0);
        println!("Hermes: [{}] {}", 
            hermes_skill.name, hermes_skill.description);
        println!("Tags: {:?}\n", hermes_skill.tags);
    }

    println!("--- Generating System Prompt ---");
    let system_prompt = converter.generate_system_prompt(
        "Catalyst Agent (HSM-II)",
        &cass_skills,
        0.85, // coherence
    );
    println!("{}\n", system_prompt);

    println!("--- Syncing Skills with Hermes ---");
    match client.sync_skills(cass_skills).await {
        Ok(result) => {
            println!("Imported: {} skills", result.imported.len());
            println!("Exported: {} skills", result.exported.len());
            println!("Conflicts: {}\n", result.conflicts.len());
            
            for skill in &result.imported {
                println!("  ✓ {}", skill.name);
            }
        }
        Err(e) => println!("Sync failed: {}\n", e),
    }

    println!("=== Demo Complete ===");
    Ok(())
}
