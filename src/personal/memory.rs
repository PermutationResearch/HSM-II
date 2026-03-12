//! Personal Memory System - Hermes-inspired MEMORY.md + USER.md
//!
//! Persistent, structured memory that survives restarts and evolves with use.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::gateway::Message;

/// Personal memory manager
pub struct PersonalMemory {
    /// Core memory file
    pub memory_md: MemoryMd,
    /// User profile
    pub user_md: UserMd,
    /// Storage path
    path: PathBuf,
}

impl PersonalMemory {
    /// Load from disk
    pub async fn load(base_path: &Path) -> Result<Self> {
        let path = base_path.to_path_buf();

        let memory_md =
            if let Ok(content) = tokio::fs::read_to_string(base_path.join("MEMORY.md")).await {
                MemoryMd::parse(&content)?
            } else {
                MemoryMd::default()
            };

        let user_md =
            if let Ok(content) = tokio::fs::read_to_string(base_path.join("USER.md")).await {
                UserMd::parse(&content)?
            } else {
                UserMd::default()
            };

        Ok(Self {
            memory_md,
            user_md,
            path,
        })
    }

    /// Bootstrap new memory for new user
    pub async fn bootstrap(base_path: &Path) -> Result<Self> {
        println!("\n📝 Setting up your memory profile...");

        // Interactive user profile creation
        let user_md = UserMd::interactive_create().await?;

        let memory = Self {
            memory_md: MemoryMd::default(),
            user_md,
            path: base_path.to_path_buf(),
        };

        memory.save(base_path).await?;

        println!("✓ Memory profile created\n");

        Ok(memory)
    }

    /// Get relevant context for a query
    pub async fn get_context(&self, query: &str) -> Result<String> {
        let mut context = String::new();

        // Add user preferences
        context.push_str("## User Preferences\n");
        for (key, value) in &self.user_md.preferences {
            context.push_str(&format!("- {}: {}\n", key, value));
        }

        // Search for relevant facts
        let relevant_facts = self.search_facts(query, 5).await?;
        if !relevant_facts.is_empty() {
            context.push_str("\n## Relevant Facts\n");
            for fact in relevant_facts {
                context.push_str(&format!("- {}\n", fact.content));
            }
        }

        // Add active projects
        let active_projects: Vec<_> = self
            .memory_md
            .projects
            .iter()
            .filter(|p| p.status == ProjectStatus::Active)
            .collect();

        if !active_projects.is_empty() {
            context.push_str("\n## Active Projects\n");
            for project in active_projects {
                context.push_str(&format!("- {}: {}\n", project.name, project.description));
            }
        }

        Ok(context)
    }

    /// Record an interaction
    pub async fn record_interaction(&mut self, msg: &Message, response: &str) -> Result<()> {
        // Save to today's memory file
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let memory_file = self.path.join("memory").join(format!("{}.md", today));

        let entry = format!(
            "\n## {}\n**User**: {}\n**Assistant**: {}\n\n",
            Utc::now().format("%H:%M:%S"),
            msg.content,
            response
        );

        // Append to file
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&memory_file)
            .await?;

        file.write_all(entry.as_bytes()).await?;

        // TODO: Use LLM to extract facts and update MEMORY.md
        // For now, just log
        tracing::debug!("Recorded interaction to {}", memory_file.display());

        Ok(())
    }

    /// Add a new fact to memory
    pub fn add_fact(&mut self, content: impl Into<String>, category: impl Into<String>) {
        self.memory_md.facts.push(MemoryFact {
            content: content.into(),
            category: category.into(),
            created_at: Utc::now(),
            confidence: 1.0,
        });
    }

    /// Search facts (simple keyword search for now)
    async fn search_facts(&self, query: &str, limit: usize) -> Result<Vec<&MemoryFact>> {
        let query_lower = query.to_lowercase();
        let words: Vec<_> = query_lower.split_whitespace().collect();

        let mut scored: Vec<_> = self
            .memory_md
            .facts
            .iter()
            .map(|fact| {
                let fact_lower = fact.content.to_lowercase();
                let score = words.iter().filter(|w| fact_lower.contains(*w)).count();
                (fact, score)
            })
            .filter(|(_, score)| *score > 0)
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));

        Ok(scored.into_iter().take(limit).map(|(f, _)| f).collect())
    }

    /// Save to disk
    pub async fn save(&self, base_path: &Path) -> Result<()> {
        let memory_content = self.memory_md.to_markdown();
        tokio::fs::write(base_path.join("MEMORY.md"), memory_content).await?;

        let user_content = self.user_md.to_markdown();
        tokio::fs::write(base_path.join("USER.md"), user_content).await?;

        Ok(())
    }
}

/// MEMORY.md structure
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MemoryMd {
    /// Learned facts about the world
    pub facts: Vec<MemoryFact>,
    /// Active and past projects
    pub projects: Vec<Project>,
    /// User preferences
    pub preferences: Vec<Preference>,
}

impl MemoryMd {
    /// Parse from markdown content
    pub fn parse(_content: &str) -> Result<Self> {
        // Simple parser - in production, use a proper MD parser
        let facts = Vec::new();
        let projects = Vec::new();
        let preferences = Vec::new();

        // TODO: Implement proper markdown parsing
        // For now, return empty

        Ok(Self {
            facts,
            projects,
            preferences,
        })
    }

    /// Convert to markdown
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# Memory\n\n");
        md.push_str("*What I've learned about the world and our work together*\n\n");

        // Facts
        md.push_str("## Facts\n\n");
        for fact in &self.facts {
            md.push_str(&format!("- [{}] {}\n", fact.category, fact.content));
        }
        md.push('\n');

        // Projects
        md.push_str("## Projects\n\n");
        for project in &self.projects {
            md.push_str(&format!(
                "### {} ({:?})\n{}\n\n",
                project.name, project.status, project.description
            ));
        }

        // Preferences
        md.push_str("## Preferences\n\n");
        for pref in &self.preferences {
            md.push_str(&format!("- **{}**: {}\n", pref.key, pref.value));
        }

        md
    }
}

/// A learned fact
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryFact {
    pub content: String,
    pub category: String,
    pub created_at: DateTime<Utc>,
    pub confidence: f64,
}

/// A project
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub description: String,
    pub status: ProjectStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ProjectStatus {
    Active,
    Paused,
    Completed,
    Archived,
}

/// A preference
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preference {
    pub key: String,
    pub value: String,
}

/// USER.md structure
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserMd {
    pub name: String,
    pub expertise: Vec<String>,
    pub communication_style: String,
    pub goals: Vec<String>,
    pub preferences: HashMap<String, String>,
}

impl UserMd {
    /// Parse from markdown
    pub fn parse(_content: &str) -> Result<Self> {
        // TODO: Proper parsing
        Ok(Self::default())
    }

    /// Interactive creation
    pub async fn interactive_create() -> Result<Self> {
        use tokio::io::{stdin, AsyncBufReadExt, BufReader};

        let stdin = BufReader::new(stdin());
        let mut lines = stdin.lines();

        println!("What's your name?");
        let name = lines.next_line().await?.unwrap_or_default();

        println!("What are your main areas of expertise? (comma-separated)");
        let expertise_input = lines.next_line().await?.unwrap_or_default();
        let expertise: Vec<_> = expertise_input
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        println!("How would you describe your communication style?");
        let style = lines.next_line().await?.unwrap_or_default();

        println!("What are your main goals for using this AI assistant?");
        let goals_input = lines.next_line().await?.unwrap_or_default();
        let goals: Vec<_> = goals_input
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        let mut preferences = HashMap::new();
        preferences.insert("response_length".to_string(), "concise".to_string());
        preferences.insert("technical_level".to_string(), "expert".to_string());

        Ok(Self {
            name,
            expertise,
            communication_style: style,
            goals,
            preferences,
        })
    }

    /// Convert to markdown
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str("# User Profile\n\n");
        md.push_str("*What I know about you*\n\n");

        md.push_str(&format!("## Name\n{}\n\n", self.name));

        md.push_str("## Expertise\n");
        for exp in &self.expertise {
            md.push_str(&format!("- {}\n", exp));
        }
        md.push('\n');

        md.push_str(&format!(
            "## Communication Style\n{}\n\n",
            self.communication_style
        ));

        md.push_str("## Goals\n");
        for goal in &self.goals {
            md.push_str(&format!("- {}\n", goal));
        }
        md.push('\n');

        md.push_str("## Preferences\n");
        for (key, value) in &self.preferences {
            md.push_str(&format!("- {}: {}\n", key, value));
        }

        md
    }
}

/// Template for new MEMORY.md
pub const MEMORY_TEMPLATE: &str = r#"# Memory

*What I've learned about the world and our work together*

## Facts

## Projects

## Preferences
"#;

/// Template for new USER.md
pub const USER_TEMPLATE: &str = r#"# User Profile

*What I know about you*

## Name

## Expertise

## Communication Style

## Goals

## Preferences
"#;
