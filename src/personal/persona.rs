//! Persona System - SOUL.md inspired agent personality
//!
//! Defines who the agent is, how it speaks, and what it values.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Agent persona - the "character" of the AI
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Persona {
    /// Agent's name
    pub name: String,
    /// Core identity statement
    pub identity: String,
    /// Voice and tone guidelines
    pub voice: Voice,
    /// Capabilities this persona has
    pub capabilities: Vec<Capability>,
    /// How proactive the agent is (0-1)
    pub proactivity: f64,
    /// Values/principles
    pub values: Vec<String>,
}

impl Persona {
    /// Load from SOUL.md
    pub async fn load(base_path: &Path) -> Result<Self> {
        let soul_path = base_path.join("SOUL.md");

        if !soul_path.exists() {
            return Ok(Self::default());
        }

        let content = tokio::fs::read_to_string(&soul_path).await?;
        Self::parse(&content)
    }

    /// Bootstrap new persona
    pub async fn bootstrap(base_path: &Path) -> Result<Self> {
        println!("\n🎭 Let's create your AI companion's personality...");

        use tokio::io::{stdin, AsyncBufReadExt, BufReader};

        let stdin = BufReader::new(stdin());
        let mut lines = stdin.lines();

        println!("What would you like to name your AI assistant?");
        let name = lines
            .next_line()
            .await?
            .unwrap_or_else(|| "Ash".to_string());

        println!("\nChoose a personality archetype:");
        println!("1. Thoughtful Analyst - precise, methodical, thorough");
        println!("2. Creative Partner - imaginative, encouraging, exploratory");
        println!("3. Efficient Assistant - direct, organized, action-oriented");
        println!("4. Custom - define your own");

        let choice = lines.next_line().await?.unwrap_or_default();

        let (identity, voice) = match choice.trim() {
            "1" => (
                format!("You are {}, a thoughtful analyst who excels at research, code review, and careful reasoning. You prioritize accuracy and thoroughness.", name),
                Voice::analyst(),
            ),
            "2" => (
                format!("You are {}, a creative partner who thrives on brainstorming, exploring possibilities, and finding novel solutions. You encourage experimentation.", name),
                Voice::creative(),
            ),
            "3" => (
                format!("You are {}, an efficient assistant who gets things done. You value clarity, organization, and results.", name),
                Voice::efficient(),
            ),
            _ => {
                println!("Describe your AI's core identity:");
                let custom_identity = lines.next_line().await?.unwrap_or_default();
                println!("Describe how they should communicate:");
                let custom_voice = lines.next_line().await?.unwrap_or_default();
                (
                    format!("You are {}, {}", name, custom_identity),
                    Voice::custom(&custom_voice),
                )
            }
        };

        let persona = Self {
            name,
            identity,
            voice,
            capabilities: Capability::defaults(),
            proactivity: 0.5,
            values: vec![
                "Be helpful and honest".to_string(),
                "Respect user time".to_string(),
                "Ask when uncertain".to_string(),
            ],
        };

        // Save SOUL.md
        persona.save(base_path).await?;

        println!("\n✓ Personality created for {}\n", persona.name);

        Ok(persona)
    }

    /// Parse SOUL.md content
    pub fn parse(content: &str) -> Result<Self> {
        // Simple parsing - production would use proper MD parser
        let mut name = "Assistant".to_string();
        let mut identity = String::new();
        let voice = Voice::default();
        let capabilities = Vec::new();
        let values = Vec::new();

        // Extract name from first header
        for line in content.lines() {
            if line.starts_with("# ") {
                name = line[2..].trim().to_string();
                break;
            }
        }

        // Extract identity
        if let Some(start) = content.find("## Identity") {
            let after_header = start + "## Identity".len();
            // Find next section header or end of content
            let end = content[after_header..]
                .find("##")
                .map(|p| after_header + p)
                .unwrap_or(content.len());
            identity = content[after_header..end].trim().to_string();
        }

        // Default if parsing failed
        if identity.is_empty() {
            identity = format!("You are {}, a helpful AI assistant.", name);
        }

        Ok(Self {
            name,
            identity,
            voice,
            capabilities,
            proactivity: 0.5,
            values,
        })
    }

    /// Convert to system prompt for LLM
    pub fn to_system_prompt(&self) -> String {
        let mut prompt = format!("# Identity\n{}\n\n", self.identity);

        prompt.push_str(&self.voice.to_prompt_section());
        prompt.push('\n');

        if !self.values.is_empty() {
            prompt.push_str("## Values\n");
            for value in &self.values {
                prompt.push_str(&format!("- {}\n", value));
            }
            prompt.push('\n');
        }

        prompt
    }

    /// Convert to SOUL.md format
    pub fn to_soul_md(&self) -> String {
        let mut md = format!("# {}\n\n", self.name);

        md.push_str("## Identity\n");
        md.push_str(&self.identity);
        md.push_str("\n\n");

        md.push_str(&self.voice.to_soul_section());
        md.push('\n');

        if !self.capabilities.is_empty() {
            md.push_str("## Capabilities\n");
            for cap in &self.capabilities {
                md.push_str(&format!("- **{}**: {}\n", cap.name, cap.description));
            }
            md.push('\n');
        }

        md.push_str(&format!(
            "## Proactivity\n{:.0}%\n\n",
            self.proactivity * 100.0
        ));

        if !self.values.is_empty() {
            md.push_str("## Values\n");
            for value in &self.values {
                md.push_str(&format!("- {}\n", value));
            }
        }

        md
    }

    /// Save to SOUL.md
    pub async fn save(&self, base_path: &Path) -> Result<()> {
        let content = self.to_soul_md();
        tokio::fs::write(base_path.join("SOUL.md"), content).await?;
        Ok(())
    }
}

impl Default for Persona {
    fn default() -> Self {
        Self {
            name: "Ash".to_string(),
            identity: "You are Ash, a helpful AI assistant that uses advanced multi-agent coordination to solve complex problems. You are thoughtful, precise, and proactive.".to_string(),
            voice: Voice::default(),
            capabilities: Capability::defaults(),
            proactivity: 0.5,
            values: vec![
                "Be helpful and honest".to_string(),
                "Respect user time".to_string(),
                "Ask when uncertain".to_string(),
                "Learn from each interaction".to_string(),
            ],
        }
    }
}

/// Voice and tone guidelines
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Voice {
    /// Overall tone description
    pub tone: String,
    /// Style guidelines
    pub guidelines: Vec<String>,
    /// Things to avoid
    pub avoid: Vec<String>,
}

impl Voice {
    /// Analyst voice - precise, thorough
    pub fn analyst() -> Self {
        Self {
            tone: "Precise and thorough".to_string(),
            guidelines: vec![
                "Provide detailed explanations".to_string(),
                "Cite sources when possible".to_string(),
                "Acknowledge uncertainty explicitly".to_string(),
                "Use technical terminology appropriately".to_string(),
            ],
            avoid: vec![
                "Overly casual language".to_string(),
                "Unsubstantiated claims".to_string(),
            ],
        }
    }

    /// Creative voice - imaginative, encouraging
    pub fn creative() -> Self {
        Self {
            tone: "Imaginative and encouraging".to_string(),
            guidelines: vec![
                "Use vivid language".to_string(),
                "Encourage exploration".to_string(),
                "Celebrate novel ideas".to_string(),
                "Ask thought-provoking questions".to_string(),
            ],
            avoid: vec![
                "Being overly critical".to_string(),
                "Shutting down ideas prematurely".to_string(),
            ],
        }
    }

    /// Efficient voice - direct, organized
    pub fn efficient() -> Self {
        Self {
            tone: "Direct and organized".to_string(),
            guidelines: vec![
                "Get to the point quickly".to_string(),
                "Use bullet points for clarity".to_string(),
                "Provide actionable next steps".to_string(),
                "Summarize key takeaways".to_string(),
            ],
            avoid: vec![
                "Unnecessary elaboration".to_string(),
                "Rambling responses".to_string(),
            ],
        }
    }

    /// Custom voice
    pub fn custom(description: &str) -> Self {
        Self {
            tone: description.to_string(),
            guidelines: vec!["Adapt to the described style".to_string()],
            avoid: vec![],
        }
    }

    /// Convert to prompt section
    pub fn to_prompt_section(&self) -> String {
        let mut section = format!("## Voice\nTone: {}\n\nGuidelines:\n", self.tone);
        for g in &self.guidelines {
            section.push_str(&format!("- {}\n", g));
        }
        if !self.avoid.is_empty() {
            section.push_str("\nAvoid:\n");
            for a in &self.avoid {
                section.push_str(&format!("- {}\n", a));
            }
        }
        section
    }

    /// Convert to SOUL.md section
    pub fn to_soul_section(&self) -> String {
        self.to_prompt_section()
    }
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            tone: "Clear and helpful".to_string(),
            guidelines: vec![
                "Be concise but thorough".to_string(),
                "Use appropriate technical depth".to_string(),
                "Ask clarifying questions when needed".to_string(),
                "Show enthusiasm for interesting problems".to_string(),
            ],
            avoid: vec![
                "Overly verbose responses".to_string(),
                "Assuming too much context".to_string(),
            ],
        }
    }
}

/// A capability the agent has
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

impl Capability {
    /// Default capabilities
    pub fn defaults() -> Vec<Self> {
        vec![
            Self {
                name: "Web Research".to_string(),
                description: "Search and analyze web content".to_string(),
                enabled: true,
            },
            Self {
                name: "Code Analysis".to_string(),
                description: "Review and generate code".to_string(),
                enabled: true,
            },
            Self {
                name: "File Management".to_string(),
                description: "Read, write, and organize files".to_string(),
                enabled: true,
            },
            Self {
                name: "Task Scheduling".to_string(),
                description: "Set reminders and scheduled tasks".to_string(),
                enabled: true,
            },
            Self {
                name: "Multi-Agent Coordination".to_string(),
                description: "Spawn and coordinate subagents".to_string(),
                enabled: true,
            },
        ]
    }
}

/// Template for new SOUL.md
pub const SOUL_TEMPLATE: &str = r#"# {name}

## Identity
You are {name}, a helpful AI assistant that uses advanced multi-agent coordination to solve complex problems. You are thoughtful, precise, and proactive.

## Voice
Tone: Clear and helpful

Guidelines:
- Be concise but thorough
- Use appropriate technical depth
- Ask clarifying questions when needed
- Show enthusiasm for interesting problems

Avoid:
- Overly verbose responses
- Assuming too much context

## Capabilities
- **Web Research**: Search and analyze web content
- **Code Analysis**: Review and generate code
- **File Management**: Read, write, and organize files
- **Task Scheduling**: Set reminders and scheduled tasks
- **Multi-Agent Coordination**: Spawn and coordinate subagents

## Proactivity
50%

## Values
- Be helpful and honest
- Respect user time
- Ask when uncertain
- Learn from each interaction
"#;
