//! Skill conversion between CASS (HSM-II) and Hermes formats

use crate::types::{CASSSkill, HermesSkill, SkillLevel};
use anyhow::Result;

/// Converts between CASS skills and Hermes skills
pub struct SkillConverter;

impl SkillConverter {
    /// Create a new skill converter
    pub fn new() -> Self {
        Self
    }

    /// Convert a CASS skill to Hermes format
    pub fn cass_to_hermes(&self, skill: &CASSSkill) -> HermesSkill {
        let level_tag = match &skill.level {
            SkillLevel::General => "general",
            SkillLevel::RoleSpecific(role) => &format!("role:{}", role.to_lowercase()),
            SkillLevel::TaskSpecific(task) => &format!("task:{}", task.to_lowercase()),
        };

        let tags = vec![
            "hsmii-import".to_string(),
            level_tag.to_string(),
            format!("confidence:{:.0}", skill.confidence * 100.0),
        ];

        // Generate skill content in Hermes format
        let content = format!(
            r#"# {}

## Principle
{}

## When to Apply
This skill is applicable when:
- Context matches the learned pattern
- Confidence level: {:.0}%
- Skill ID: {}

## Usage
```
Apply principle: {}
```

## Metadata
- Source: HSM-II CASS
- Level: {:?}
- Confidence: {}
- Last Updated: {}
"#,
            skill.title,
            skill.principle,
            skill.confidence * 100.0,
            skill.id,
            skill.principle,
            skill.level,
            skill.confidence,
            chrono::Utc::now().to_rfc3339(),
        );

        HermesSkill {
            name: skill.id.clone(),
            description: format!("{}: {}", skill.title, skill.principle),
            tags,
            content,
            source: Some("hsmii-cass".to_string()),
            metadata: Some(serde_json::json!({
                "confidence": skill.confidence,
                "embedding": skill.embedding,
            })),
        }
    }

    /// Convert multiple CASS skills to Hermes format
    pub fn cass_batch_to_hermes(&self, skills: &[CASSSkill]) -> Vec<HermesSkill> {
        skills.iter().map(|s| self.cass_to_hermes(s)).collect()
    }

    /// Convert a Hermes skill to CASS format (best effort)
    pub fn hermes_to_cass(&self, skill: &HermesSkill) -> Result<CASSSkill> {
        // Extract level from tags
        let level = skill
            .tags
            .iter()
            .find_map(|tag| {
                if tag.starts_with("role:") {
                    Some(SkillLevel::RoleSpecific(tag[5..].to_string()))
                } else if tag.starts_with("task:") {
                    Some(SkillLevel::TaskSpecific(tag[5..].to_string()))
                } else if tag == "general" {
                    Some(SkillLevel::General)
                } else {
                    None
                }
            })
            .unwrap_or(SkillLevel::General);

        // Extract confidence from metadata or tags
        let confidence = skill
            .metadata
            .as_ref()
            .and_then(|m| m.get("confidence"))
            .and_then(|c| c.as_f64())
            .or_else(|| {
                skill.tags.iter().find_map(|tag| {
                    if tag.starts_with("confidence:") {
                        tag[11..].parse::<f64>().ok().map(|c| c / 100.0)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(0.5);

        // Parse content to extract principle (simplified)
        let principle = self.extract_principle(&skill.content)
            .unwrap_or_else(|| skill.description.clone());

        let embedding = skill
            .metadata
            .as_ref()
            .and_then(|m| m.get("embedding"))
            .and_then(|e| e.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect()
            });

        Ok(CASSSkill {
            id: skill.name.clone(),
            title: skill.name.clone(),
            principle,
            level,
            confidence,
            embedding,
        })
    }

    /// Extract principle from Hermes skill content
    fn extract_principle(&self, content: &str) -> Option<String> {
        // Look for "## Principle" section
        let lines: Vec<&str> = content.lines().collect();
        let mut in_principle = false;
        let mut principle_lines = Vec::new();

        for line in lines {
            if line.trim() == "## Principle" {
                in_principle = true;
                continue;
            }
            if in_principle {
                if line.starts_with("##") {
                    break;
                }
                if !line.trim().is_empty() {
                    principle_lines.push(line.trim());
                }
            }
        }

        if principle_lines.is_empty() {
            None
        } else {
            Some(principle_lines.join(" "))
        }
    }

    /// Generate Hermes-compatible system prompt from HSM-II context
    pub fn generate_system_prompt(
        &self,
        role: &str,
        skills: &[CASSSkill],
        coherence: f64,
    ) -> String {
        let skill_section = if skills.is_empty() {
            "No specific skills loaded.".to_string()
        } else {
            skills
                .iter()
                .map(|s| format!("- [{}] {}", s.id, s.principle))
                .collect::<Vec<_>>()
                .join("\n")
        };

        format!(
            r#"You are an AI agent operating as part of HSM-II (Hyper-Stigmergic Morphogenesis II).

## Your Role
{}

## Current System State
- Coherence: {:.4}
- Active Skills: {}

## Available Skills
{}

## Guidelines
1. Use available tools to accomplish tasks
2. Reference relevant skills when making decisions
3. Maintain coherence with the broader HSM-II system
4. Report outcomes clearly for credit attribution
5. Spawn subagents for parallel work when beneficial

## Output Format
When completing a task, summarize:
- What was accomplished
- Which skills were applied
- Any coherence implications
- Recommended next steps"#,
            role,
            coherence,
            skills.len(),
            skill_section
        )
    }
}

impl Default for SkillConverter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cass_to_hermes_conversion() {
        let converter = SkillConverter::new();
        
        let cass_skill = CASSSkill {
            id: "skill_001".to_string(),
            title: "Coherence Preservation".to_string(),
            principle: "Before any mutation, verify coherence delta is positive".to_string(),
            level: SkillLevel::General,
            confidence: 0.85,
            embedding: Some(vec![0.1, 0.2, 0.3]),
        };

        let hermes_skill = converter.cass_to_hermes(&cass_skill);
        
        assert_eq!(hermes_skill.name, "skill_001");
        assert!(hermes_skill.tags.contains(&"hsmii-import".to_string()));
        assert!(hermes_skill.tags.contains(&"general".to_string()));
        assert!(hermes_skill.content.contains("Coherence Preservation"));
    }

    #[test]
    fn test_principle_extraction() {
        let converter = SkillConverter::new();
        
        let content = r#"# Test Skill

## Principle
This is the principle line.
Another principle line.

## Usage
Some usage info
"#;

        let principle = converter.extract_principle(content);
        assert_eq!(principle, Some("This is the principle line. Another principle line.".to_string()));
    }
}
