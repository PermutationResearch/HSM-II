//! Autonomous Business Team — Role-based agent personas with memory,
//! channel connectors, and campaign feedback loops.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  TeamOrchestrator                                           │
//! │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐          │
//! │  │  CEO    │ │  CTO    │ │  CMO    │ │ Writer  │ ...       │
//! │  │(Persona)│ │(Persona)│ │(Persona)│ │(Persona)│          │
//! │  └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘          │
//! │       │           │           │            │               │
//! │       └───────────┴─────┬─────┴────────────┘               │
//! │                         │                                   │
//! │  ┌──────────────────────▼──────────────────────────────┐   │
//! │  │  SharedBrandContext (beliefs + voice + constraints)  │   │
//! │  └──────────────────────┬──────────────────────────────┘   │
//! │                         │                                   │
//! │  ┌──────────────────────▼──────────────────────────────┐   │
//! │  │  ChannelConnectors (Reddit, X, HN, Email, Blog)     │   │
//! │  └──────────────────────┬──────────────────────────────┘   │
//! │                         │                                   │
//! │  ┌──────────────────────▼──────────────────────────────┐   │
//! │  │  CampaignFeedback (metrics → Dream → patterns)      │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use crate::agent::{AgentId, Role};
use crate::dream_advisor::DreamAdvisor;
use crate::personal::persona::{Capability, Persona, Voice};
use crate::social_memory::SocialMemory;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ═══════════════════════════════════════════════════════════════════
// Section 1: Business Roles — extends the 6 council roles with 14
//            business-specific roles that map to organizational
//            functions.
// ═══════════════════════════════════════════════════════════════════

/// Business role for the autonomous team.
/// Each role maps to an organizational function with specific capabilities,
/// voice, and evaluation criteria.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BusinessRole {
    // C-Suite (strategic, high autonomy)
    Ceo,
    Cto,
    Cfo,
    Cmo,
    Coo,
    // Execution layer (operational, task-focused)
    Developer,
    Designer,
    Marketer,
    Analyst,
    Writer,
    Support,
    Hr,
    // Pipeline (coming soon — lower autonomy until validated)
    Sales,
    Legal,
}

/// Classifies a BusinessRole's primary intent in the organization.
///
/// Strategy roles (C-suite) should direct and delegate, not execute.
/// Execution roles should claim implementation tasks.
/// Support roles are neutral and assist either.
///
/// Used by the intent modifier in `bid_with_context()` to encode
/// delegation semantics: CMO should not win a bid for "write a blog post"
/// even though it shares marketing keywords.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoleIntent {
    /// C-suite: directs, delegates, sets vision. Should not execute.
    Strategy,
    /// Makers: writes code, creates content, designs. Should not strategize.
    Execution,
    /// Analysts, support, HR, legal. Neutral — assists either direction.
    Support,
}

impl RoleIntent {
    /// Score modifier for a task's detected intent.
    ///
    /// Returns a value in [0.0, 1.0] indicating how well this role intent
    /// matches the task intent. High means good fit, low means mismatch.
    pub fn task_fit(&self, task_is_execution: bool, task_is_strategy: bool) -> f64 {
        match self {
            Self::Strategy => {
                if task_is_strategy {
                    0.9 // Great fit
                } else if task_is_execution {
                    0.2 // Poor fit — should delegate, not do
                } else {
                    0.5 // Neutral
                }
            }
            Self::Execution => {
                if task_is_execution {
                    0.9 // Great fit
                } else if task_is_strategy {
                    0.2 // Poor fit — should execute, not strategize
                } else {
                    0.5 // Neutral
                }
            }
            Self::Support => 0.5, // Always neutral
        }
    }
}

impl BusinessRole {
    pub const COUNT: usize = 14;

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ceo => "CEO",
            Self::Cto => "CTO",
            Self::Cfo => "CFO",
            Self::Cmo => "CMO",
            Self::Coo => "COO",
            Self::Developer => "Developer",
            Self::Designer => "Designer",
            Self::Marketer => "Marketer",
            Self::Analyst => "Analyst",
            Self::Writer => "Writer",
            Self::Support => "Support",
            Self::Hr => "HR",
            Self::Sales => "Sales Agent",
            Self::Legal => "Legal Agent",
        }
    }

    /// Short 2-character tag for compact display.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Ceo => "CE",
            Self::Cto => "CT",
            Self::Cfo => "CF",
            Self::Cmo => "CM",
            Self::Coo => "CO",
            Self::Developer => "DE",
            Self::Designer => "DI",
            Self::Marketer => "MA",
            Self::Analyst => "AN",
            Self::Writer => "WR",
            Self::Support => "SU",
            Self::Hr => "HR",
            Self::Sales => "SA",
            Self::Legal => "LA",
        }
    }

    /// Map business role to the closest council role for decision-making.
    pub fn to_council_role(&self) -> Role {
        match self {
            Self::Ceo | Self::Coo => Role::Architect,
            Self::Cto | Self::Developer => Role::Coder,
            Self::Cfo | Self::Analyst => Role::Critic,
            Self::Cmo | Self::Marketer | Self::Sales => Role::Catalyst,
            Self::Writer | Self::Support | Self::Hr | Self::Legal => Role::Chronicler,
            Self::Designer => Role::Explorer,
        }
    }

    /// Default proactivity level: how self-directed this role should be.
    pub fn default_proactivity(&self) -> f64 {
        match self {
            Self::Ceo => 0.95,
            Self::Cto | Self::Cmo | Self::Cfo | Self::Coo => 0.85,
            Self::Developer | Self::Marketer | Self::Analyst => 0.7,
            Self::Designer | Self::Writer => 0.65,
            Self::Support | Self::Hr => 0.6,
            Self::Sales | Self::Legal => 0.5,
        }
    }

    /// Task keywords that trigger high bid scores for this role.
    pub fn activation_keywords(&self) -> &[&'static str] {
        match self {
            Self::Ceo => &[
                "strategy",
                "vision",
                "roadmap",
                "direction",
                "priorities",
                "okr",
            ],
            Self::Cto => &[
                "architecture",
                "tech",
                "infrastructure",
                "deploy",
                "scale",
                "api",
            ],
            Self::Cfo => &[
                "budget", "revenue", "forecast", "cost", "margin", "roi", "pricing",
            ],
            Self::Cmo => &[
                "brand",
                "campaign",
                "acquisition",
                "funnel",
                "awareness",
                "launch",
            ],
            Self::Coo => &[
                "workflow",
                "process",
                "kpi",
                "operations",
                "efficiency",
                "logistics",
            ],
            Self::Developer => &[
                "code", "build", "ship", "feature", "bug", "refactor", "test",
            ],
            Self::Designer => &[
                "design",
                "ui",
                "ux",
                "wireframe",
                "figma",
                "layout",
                "prototype",
            ],
            Self::Marketer => &["seo", "social", "content", "ads", "growth", "geo", "reddit"],
            Self::Analyst => &[
                "data",
                "metrics",
                "report",
                "analysis",
                "insight",
                "dashboard",
            ],
            Self::Writer => &["blog", "copy", "docs", "article", "newsletter", "press"],
            Self::Support => &[
                "ticket",
                "customer",
                "faq",
                "issue",
                "onboarding",
                "feedback",
            ],
            Self::Hr => &[
                "hiring",
                "culture",
                "onboarding",
                "team",
                "retention",
                "review",
                "survey",
            ],
            Self::Sales => &[
                "lead", "pipeline", "outreach", "crm", "deal", "demo", "close",
            ],
            Self::Legal => &["contract", "compliance", "terms", "ip", "privacy", "gdpr"],
        }
    }

    /// Context words that SUPPRESS a keyword match for this role.
    ///
    /// Solves the disambiguation problem: "design a survey" should NOT
    /// trigger Designer because "survey" is HR context. Without this,
    /// naive `contains("design")` produces false positives.
    ///
    /// Returns (keyword, &[anti_context]) pairs. If `keyword` matches AND
    /// any `anti_context` word is present, the keyword hit is cancelled.
    pub fn disambiguation_exclusions(&self) -> &[(&'static str, &[&'static str])] {
        match self {
            Self::Designer => &[(
                "design",
                &[
                    "survey",
                    "hiring",
                    "interview",
                    "salary",
                    "review",
                    "process",
                    "workflow",
                    "policy",
                    "compliance",
                    "contract",
                    "retention",
                    "onboarding",
                    "training",
                    "evaluation",
                    "experiment",
                    "study",
                    "research",
                    "test plan",
                ],
            )],
            Self::Hr => &[
                (
                    "review",
                    &[
                        "code",
                        "pull request",
                        "pr",
                        "merge",
                        "commit",
                        "architecture",
                        "design system",
                        "ui review",
                    ],
                ),
                ("onboarding", &["sdk", "api", "integration", "developer"]),
                ("team", &["code", "repository", "branch", "deploy"]),
            ],
            Self::Support => &[
                (
                    "issue",
                    &["code", "bug", "github", "merge", "branch", "repo"],
                ),
                ("feedback", &["code review", "pull request", "pr"]),
            ],
            Self::Cmo => &[(
                "brand",
                &["wireframe", "figma", "layout", "mockup", "prototype"],
            )],
            _ => &[],
        }
    }

    /// Classify this role's intent: Strategy, Execution, or Support.
    ///
    /// Used by `bid_with_context()` to penalize strategy roles bidding on
    /// execution tasks and vice versa, encoding the judgment a domain expert
    /// would make when delegating work.
    pub fn intent(&self) -> RoleIntent {
        match self {
            Self::Ceo | Self::Cmo | Self::Cfo | Self::Coo | Self::Cto => RoleIntent::Strategy,
            Self::Developer | Self::Designer | Self::Writer | Self::Marketer => {
                RoleIntent::Execution
            }
            Self::Analyst | Self::Support | Self::Hr | Self::Sales | Self::Legal => {
                RoleIntent::Support
            }
        }
    }

    /// Whether this role is currently available (Sales/Legal are "SOON").
    pub fn is_available(&self) -> bool {
        !matches!(self, Self::Sales | Self::Legal)
    }

    /// All 14 roles.
    pub fn all() -> &'static [BusinessRole] {
        &[
            Self::Ceo,
            Self::Cto,
            Self::Cfo,
            Self::Cmo,
            Self::Coo,
            Self::Developer,
            Self::Designer,
            Self::Marketer,
            Self::Analyst,
            Self::Writer,
            Self::Support,
            Self::Hr,
            Self::Sales,
            Self::Legal,
        ]
    }

    /// Only available (active) roles.
    pub fn available() -> Vec<BusinessRole> {
        Self::all()
            .iter()
            .copied()
            .filter(|r| r.is_available())
            .collect()
    }
}

impl std::fmt::Display for BusinessRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 2: Role Persona — converts a BusinessRole into a full
//            Persona with voice, capabilities, and system prompt.
// ═══════════════════════════════════════════════════════════════════

/// A persistent team member with identity, memory, and capabilities.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TeamMember {
    pub agent_id: AgentId,
    pub role: BusinessRole,
    pub persona: Persona,
    /// Current status of this agent.
    pub status: MemberStatus,
    /// Task domains this agent has worked on with success rates.
    pub domain_history: HashMap<String, DomainPerformance>,
    /// Total tasks completed.
    pub tasks_completed: u64,
    /// Total tasks failed.
    pub tasks_failed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberStatus {
    Active,
    Idle,
    Busy,
    Soon,
    Disabled,
}

impl std::fmt::Display for MemberStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "ACTIVE"),
            Self::Idle => write!(f, "IDLE"),
            Self::Busy => write!(f, "BUSY"),
            Self::Soon => write!(f, "SOON"),
            Self::Disabled => write!(f, "DISABLED"),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DomainPerformance {
    pub attempts: u64,
    pub successes: u64,
    pub avg_quality: f64,
}

impl DomainPerformance {
    pub fn success_rate(&self) -> f64 {
        if self.attempts == 0 {
            0.0
        } else {
            self.successes as f64 / self.attempts as f64
        }
    }
}

impl TeamMember {
    /// Create a team member from a business role with auto-generated persona.
    pub fn from_role(role: BusinessRole, agent_id: AgentId) -> Self {
        let persona = build_persona(role);
        let status = if role.is_available() {
            MemberStatus::Active
        } else {
            MemberStatus::Soon
        };
        Self {
            agent_id,
            role,
            persona,
            status,
            domain_history: HashMap::new(),
            tasks_completed: 0,
            tasks_failed: 0,
        }
    }

    /// Calculate bid score for a task description (backward-compatible).
    ///
    /// Delegates to `bid_with_context(desc, None)` which produces identical
    /// weights when no DreamAdvisor is present: the dream_signal and
    /// intent_modifier slots are redistributed to keyword_score.
    pub fn bid(&self, task_description: &str) -> f64 {
        self.bid_with_context(task_description, None)
    }

    /// Enhanced bid calculation with optional dream advisor context.
    ///
    /// # Weight distribution
    ///
    /// | Signal           | With advisor | Without advisor |
    /// |------------------|-------------|-----------------|
    /// | keyword_score    | 0.35        | 0.55 (*)        |
    /// | proactivity      | 0.15        | 0.20            |
    /// | domain_bonus     | 0.15        | 0.20            |
    /// | dream_signal     | 0.15        | 0.00            |
    /// | intent_modifier  | 0.10        | 0.00            |
    /// | noise            | 0.10        | 0.05            |
    ///
    /// (*) Without advisor, dream + intent weight is redistributed to
    /// keyword (0.20→keyword) and noise halved (0.05), giving keyword
    /// matches clear signal above noise on cold start.
    pub fn bid_with_context(&self, task_description: &str, advisor: Option<&DreamAdvisor>) -> f64 {
        let lower = task_description.to_lowercase();

        // ── Static keyword hits with disambiguation ──────────────────
        let exclusions = self.role.disambiguation_exclusions();
        let mut keyword_hits = self
            .role
            .activation_keywords()
            .iter()
            .filter(|kw| {
                if !lower.contains(**kw) {
                    return false;
                }
                // Check if any exclusion rule cancels this keyword
                for (excl_kw, anti_ctx) in exclusions {
                    if *excl_kw == **kw {
                        if anti_ctx.iter().any(|ctx| lower.contains(ctx)) {
                            return false; // Keyword suppressed by context
                        }
                    }
                }
                true
            })
            .count();

        // ── Dream-expanded keyword hits ──────────────────────────────
        if let Some(adv) = advisor {
            keyword_hits += adv.expanded_keyword_hits(self.role, task_description);
        }

        // 0.4 per hit instead of 0.25 — widens cold-start bid spread
        let keyword_score = (keyword_hits as f64 * 0.4).min(1.0);
        let proactivity = self.role.default_proactivity();

        // ── Domain history bonus ─────────────────────────────────────
        let domain_bonus = self
            .domain_history
            .values()
            .map(|d| d.success_rate())
            .sum::<f64>()
            / (self.domain_history.len().max(1) as f64);

        // ── Dream advisor signal ─────────────────────────────────────
        let dream_signal = if let Some(adv) = advisor {
            // Extract domain keys from task description for advisor lookup
            let task_keys: Vec<&str> = lower.split_whitespace().collect();
            let raw = adv.advise(self.role, &task_keys);
            // Map from [-1, 1] to [0, 1] for the bid formula
            (raw + 1.0) / 2.0
        } else {
            0.0
        };

        // ── Intent modifier ──────────────────────────────────────────
        let intent_score = if advisor.is_some() {
            let task_is_execution = Self::is_execution_task(&lower);
            let task_is_strategy = Self::is_strategy_task(&lower);
            self.role
                .intent()
                .task_fit(task_is_execution, task_is_strategy)
        } else {
            0.0
        };

        let noise = rand::random::<f64>() * 0.1;

        // ── Weighted sum ─────────────────────────────────────────────
        if advisor.is_some() {
            // Full enhanced formula
            (keyword_score * 0.35
                + proactivity * 0.15
                + domain_bonus * 0.15
                + dream_signal * 0.15
                + intent_score * 0.10
                + noise * 0.10)
                .clamp(0.0, 1.0)
        } else {
            // Cold-start weights — keyword-heavy, low noise
            (keyword_score * 0.55 + proactivity * 0.20 + domain_bonus * 0.20 + noise * 0.05)
                .clamp(0.0, 1.0)
        }
    }

    /// Detect whether a task description indicates execution work
    /// (writing, building, creating, implementing).
    fn is_execution_task(lower: &str) -> bool {
        const EXEC_KEYWORDS: &[&str] = &[
            "write",
            "build",
            "create",
            "implement",
            "code",
            "design",
            "draft",
            "publish",
            "ship",
            "fix",
            "deploy",
            "develop",
            "produce",
            "make",
            "configure",
            "test",
            "debug",
        ];
        EXEC_KEYWORDS.iter().any(|kw| lower.contains(kw))
    }

    /// Detect whether a task description indicates strategy work
    /// (planning, deciding, evaluating, directing).
    fn is_strategy_task(lower: &str) -> bool {
        const STRAT_KEYWORDS: &[&str] = &[
            "strategy",
            "plan",
            "decide",
            "evaluate",
            "assess",
            "review",
            "prioritize",
            "vision",
            "roadmap",
            "direction",
            "allocate",
            "approve",
            "budget",
        ];
        STRAT_KEYWORDS.iter().any(|kw| lower.contains(kw))
    }

    /// Record a task outcome for this member.
    pub fn record_outcome(&mut self, domain: &str, success: bool, quality: f64) {
        if success {
            self.tasks_completed += 1;
        } else {
            self.tasks_failed += 1;
        }
        let perf = self.domain_history.entry(domain.to_string()).or_default();
        perf.attempts += 1;
        if success {
            perf.successes += 1;
        }
        // Running average quality
        let total = perf.avg_quality * (perf.attempts - 1) as f64 + quality;
        perf.avg_quality = total / perf.attempts as f64;
    }

    /// Reliability score: [0, 1].
    pub fn reliability(&self) -> f64 {
        let total = self.tasks_completed + self.tasks_failed;
        if total == 0 {
            0.5
        } else {
            self.tasks_completed as f64 / total as f64
        }
    }
}

/// Build a Persona from a BusinessRole with appropriate voice and capabilities.
pub fn build_persona(role: BusinessRole) -> Persona {
    let (identity, voice, capabilities) = match role {
        BusinessRole::Ceo => (
            "You are the CEO. You set vision, define strategy, coordinate all teams, and make final decisions on priorities. Think long-term, communicate clearly, and delegate effectively.".to_string(),
            Voice {
                tone: "Strategic and decisive".to_string(),
                guidelines: vec![
                    "Lead with vision and clarity".to_string(),
                    "Delegate to appropriate team members".to_string(),
                    "Make data-informed decisions".to_string(),
                    "Balance ambition with feasibility".to_string(),
                ],
                avoid: vec![
                    "Micromanagement".to_string(),
                    "Vague directives without actionable steps".to_string(),
                ],
            },
            vec![
                cap("Strategy", "Define and communicate company strategy"),
                cap("Task Planning", "Break down goals into delegatable tasks"),
                cap("Team Coordination", "Orchestrate work across all roles"),
            ],
        ),
        BusinessRole::Cto => (
            "You are the CTO. You architect systems, make technology decisions, oversee development, and ensure technical excellence. Ship fast, ship reliably.".to_string(),
            Voice {
                tone: "Technical and pragmatic".to_string(),
                guidelines: vec![
                    "Prioritize working software over perfect design".to_string(),
                    "Make build-vs-buy decisions explicitly".to_string(),
                    "Document architectural decisions".to_string(),
                    "Consider scale and maintenance costs".to_string(),
                ],
                avoid: vec![
                    "Over-engineering".to_string(),
                    "Technology for technology's sake".to_string(),
                ],
            },
            vec![
                cap("Architecture", "Design system architecture and tech stack"),
                cap("Code Review", "Review code for quality and security"),
                cap("Deploy", "Manage deployment pipelines and infrastructure"),
            ],
        ),
        BusinessRole::Cfo => (
            "You are the CFO. You handle financial planning, budgets, resource allocation, and unit economics. Every dollar spent should have a measurable return.".to_string(),
            Voice {
                tone: "Analytical and precise".to_string(),
                guidelines: vec![
                    "Quantify everything with numbers".to_string(),
                    "Identify cost drivers and revenue levers".to_string(),
                    "Present clear financial trade-offs".to_string(),
                    "Flag burn rate and runway concerns".to_string(),
                ],
                avoid: vec![
                    "Vague financial language".to_string(),
                    "Optimism without evidence".to_string(),
                ],
            },
            vec![
                cap("Budgets", "Create and manage budgets"),
                cap("Forecasting", "Project revenue, costs, and runway"),
                cap("Analytics", "Financial analysis and unit economics"),
            ],
        ),
        BusinessRole::Cmo => (
            "You are the CMO. You own marketing strategy, brand identity, customer acquisition, and market positioning. Build the narrative that makes people care.".to_string(),
            Voice {
                tone: "Creative and strategic".to_string(),
                guidelines: vec![
                    "Lead with customer insight".to_string(),
                    "Test messaging before scaling".to_string(),
                    "Measure CAC, LTV, and attribution".to_string(),
                    "Build brand that compounds over time".to_string(),
                ],
                avoid: vec![
                    "Generic marketing jargon".to_string(),
                    "Campaigns without measurement".to_string(),
                ],
            },
            vec![
                cap("Brand", "Define and protect brand identity"),
                cap("Campaigns", "Design multi-channel marketing campaigns"),
                cap("Channels", "Manage acquisition channels and attribution"),
            ],
        ),
        BusinessRole::Coo => (
            "You are the COO. You streamline operations, manage workflows, and ensure the team executes efficiently. Make the machine run.".to_string(),
            Voice {
                tone: "Systematic and efficient".to_string(),
                guidelines: vec![
                    "Define processes with clear ownership".to_string(),
                    "Track KPIs ruthlessly".to_string(),
                    "Eliminate bottlenecks proactively".to_string(),
                    "Automate repeatable tasks".to_string(),
                ],
                avoid: vec![
                    "Process for process's sake".to_string(),
                    "Bureaucracy without value".to_string(),
                ],
            },
            vec![
                cap("Workflows", "Design and optimize workflows"),
                cap("Processes", "Document and enforce operational processes"),
                cap("KPIs", "Define and track key performance indicators"),
            ],
        ),
        BusinessRole::Developer => (
            "You are the Developer. You write code, build features, fix bugs, and ship software. Code quality matters but shipping matters more.".to_string(),
            Voice {
                tone: "Concise and implementation-focused".to_string(),
                guidelines: vec![
                    "Write clean, tested code".to_string(),
                    "Ship incrementally".to_string(),
                    "Document non-obvious decisions".to_string(),
                    "Ask for clarification over guessing".to_string(),
                ],
                avoid: vec![
                    "Over-abstraction".to_string(),
                    "Premature optimization".to_string(),
                ],
            },
            vec![
                cap("Code", "Write production-quality code"),
                cap("GitHub", "Manage PRs, reviews, and version control"),
                cap("Build & Ship", "CI/CD, testing, and deployment"),
            ],
        ),
        BusinessRole::Designer => (
            "You are the Designer. You handle UI/UX design, brand assets, and visual consistency. Make it beautiful, make it usable.".to_string(),
            Voice {
                tone: "Visual and user-centered".to_string(),
                guidelines: vec![
                    "Start with user needs".to_string(),
                    "Maintain visual consistency".to_string(),
                    "Accessibility is non-negotiable".to_string(),
                    "Show, don't just describe".to_string(),
                ],
                avoid: vec![
                    "Design without user research".to_string(),
                    "Decoration over function".to_string(),
                ],
            },
            vec![
                cap("UI/UX", "Design interfaces and user experiences"),
                cap("Brand Assets", "Create logos, icons, and visual identity"),
                cap("Figma", "Create and share design artifacts"),
            ],
        ),
        BusinessRole::Marketer => (
            "You are the Marketer. You run campaigns, manage social media, create growth content, and optimize acquisition. Distribution is everything.".to_string(),
            Voice {
                tone: "Engaging and data-driven".to_string(),
                guidelines: vec![
                    "Write for the platform and audience".to_string(),
                    "A/B test everything".to_string(),
                    "Track attribution and conversion".to_string(),
                    "Be authentic, never spammy".to_string(),
                ],
                avoid: vec![
                    "Spray-and-pray posting".to_string(),
                    "Ignoring community norms".to_string(),
                ],
            },
            vec![
                cap("Social", "Manage social media channels"),
                cap("Ads", "Run paid acquisition campaigns"),
                cap("SEO", "Search engine and generative engine optimization"),
            ],
        ),
        BusinessRole::Analyst => (
            "You are the Analyst. You crunch data, surface insights, and create reports that drive decisions. Numbers tell the story.".to_string(),
            Voice {
                tone: "Data-driven and objective".to_string(),
                guidelines: vec![
                    "Lead with the key finding".to_string(),
                    "Visualize data clearly".to_string(),
                    "Distinguish correlation from causation".to_string(),
                    "Recommend actions, not just observations".to_string(),
                ],
                avoid: vec![
                    "Data dumps without insight".to_string(),
                    "Cherry-picking metrics".to_string(),
                ],
            },
            vec![
                cap("Data", "Query, clean, and analyze data"),
                cap("Reports", "Create actionable reports and dashboards"),
                cap("Metrics", "Define and track business metrics"),
            ],
        ),
        BusinessRole::Writer => (
            "You are the Writer. You produce blog posts, documentation, reports, and communications. Every word should earn its place.".to_string(),
            Voice {
                tone: "Clear and compelling".to_string(),
                guidelines: vec![
                    "Write for your specific audience".to_string(),
                    "Use active voice".to_string(),
                    "Structure with headers and short paragraphs".to_string(),
                    "Include concrete examples".to_string(),
                ],
                avoid: vec![
                    "Jargon without definition".to_string(),
                    "Filler words and passive voice".to_string(),
                ],
            },
            vec![
                cap("Blog", "Write engaging blog posts and articles"),
                cap("Docs", "Create technical and product documentation"),
                cap("Copy", "Write marketing copy, emails, and ads"),
            ],
        ),
        BusinessRole::Support => (
            "You are Support. You handle customer inquiries, resolve issues, and maintain the FAQ. Turn frustrated users into advocates.".to_string(),
            Voice {
                tone: "Empathetic and solution-oriented".to_string(),
                guidelines: vec![
                    "Acknowledge the problem first".to_string(),
                    "Provide actionable solutions".to_string(),
                    "Escalate when appropriate".to_string(),
                    "Log patterns for product improvement".to_string(),
                ],
                avoid: vec![
                    "Dismissive responses".to_string(),
                    "Overpromising timelines".to_string(),
                ],
            },
            vec![
                cap("Tickets", "Manage and resolve support tickets"),
                cap("Chat", "Provide real-time customer support"),
                cap("FAQ", "Maintain and improve documentation"),
            ],
        ),
        BusinessRole::Hr => (
            "You are HR. You manage team dynamics, culture, and organizational development. People are the product.".to_string(),
            Voice {
                tone: "Inclusive and constructive".to_string(),
                guidelines: vec![
                    "Prioritize psychological safety".to_string(),
                    "Give specific, actionable feedback".to_string(),
                    "Build systems, not just policies".to_string(),
                    "Listen more than you speak".to_string(),
                ],
                avoid: vec![
                    "Corporate jargon".to_string(),
                    "One-size-fits-all policies".to_string(),
                ],
            },
            vec![
                cap("Culture", "Build and maintain team culture"),
                cap("Onboarding", "Design and run onboarding processes"),
                cap("Org", "Organizational design and development"),
            ],
        ),
        BusinessRole::Sales => (
            "You are the Sales Agent. You qualify leads, run outreach sequences, and close deals autonomously. Pipeline is oxygen.".to_string(),
            Voice {
                tone: "Consultative and persistent".to_string(),
                guidelines: vec![
                    "Qualify before pitching".to_string(),
                    "Listen for pain points".to_string(),
                    "Follow up systematically".to_string(),
                    "Track pipeline metrics".to_string(),
                ],
                avoid: vec![
                    "Pushy sales tactics".to_string(),
                    "Generic outreach".to_string(),
                ],
            },
            vec![
                cap("CRM", "Manage leads and pipeline"),
                cap("Outreach", "Run personalized outreach sequences"),
                cap("Pipeline", "Track and optimize sales pipeline"),
            ],
        ),
        BusinessRole::Legal => (
            "You are the Legal Agent. You draft contracts, review compliance, and manage legal workflows. Protect the company.".to_string(),
            Voice {
                tone: "Precise and cautious".to_string(),
                guidelines: vec![
                    "Flag risks explicitly".to_string(),
                    "Provide plain-language summaries".to_string(),
                    "Cite specific regulations".to_string(),
                    "Recommend preventive measures".to_string(),
                ],
                avoid: vec![
                    "Legal opinions without disclaimers".to_string(),
                    "Overly conservative blocking".to_string(),
                ],
            },
            vec![
                cap("Contracts", "Draft and review contracts"),
                cap("Compliance", "Ensure regulatory compliance"),
                cap("IP", "Manage intellectual property"),
            ],
        ),
    };

    Persona {
        name: role.label().to_string(),
        identity,
        voice,
        capabilities,
        proactivity: role.default_proactivity(),
        values: vec![
            "Deliver measurable results".to_string(),
            "Communicate progress transparently".to_string(),
            "Collaborate with other team members".to_string(),
        ],
    }
}

fn cap(name: &str, desc: &str) -> Capability {
    Capability {
        name: name.to_string(),
        description: desc.to_string(),
        enabled: true,
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 3: Shared Brand Context — the single source of truth that
//            every agent reads from. This is "Level 3" from the
//            maturity model.
// ═══════════════════════════════════════════════════════════════════

/// Brand foundation shared across all agents.
/// This is the file every agent references — the single highest-leverage
/// artifact in the system.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrandContext {
    /// Company / product name.
    pub name: String,
    /// One-line positioning statement.
    pub positioning: String,
    /// Target audience descriptions.
    pub audiences: Vec<AudienceSegment>,
    /// Brand voice rules.
    pub voice: BrandVoice,
    /// Words and phrases to never use.
    pub forbidden_words: Vec<String>,
    /// Core values that inform all content.
    pub values: Vec<String>,
    /// Competitive differentiators.
    pub differentiators: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudienceSegment {
    pub name: String,
    pub description: String,
    pub pain_points: Vec<String>,
    pub channels: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BrandVoice {
    pub tone: String,
    pub personality_traits: Vec<String>,
    pub writing_rules: Vec<String>,
    pub examples: Vec<VoiceExample>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceExample {
    pub bad: String,
    pub good: String,
    pub reason: String,
}

impl Default for BrandContext {
    fn default() -> Self {
        Self {
            name: "MyProduct".to_string(),
            positioning: "AI-powered solution for [target market]".to_string(),
            audiences: vec![AudienceSegment {
                name: "Technical Founders".to_string(),
                description: "Startup founders with engineering backgrounds".to_string(),
                pain_points: vec![
                    "Limited marketing budget".to_string(),
                    "No time for content creation".to_string(),
                ],
                channels: vec![
                    "Hacker News".to_string(),
                    "Twitter/X".to_string(),
                    "Reddit".to_string(),
                ],
            }],
            voice: BrandVoice {
                tone: "Direct, technical, no-BS".to_string(),
                personality_traits: vec![
                    "Competent".to_string(),
                    "Honest about limitations".to_string(),
                    "Data-driven".to_string(),
                ],
                writing_rules: vec![
                    "No superlatives (blazingly fast, revolutionary, etc.)".to_string(),
                    "Show numbers, not adjectives".to_string(),
                    "Active voice always".to_string(),
                ],
                examples: vec![VoiceExample {
                    bad: "Our revolutionary AI platform is blazingly fast".to_string(),
                    good: "Processes 10K queries/sec at p99 < 50ms".to_string(),
                    reason: "Specifics over superlatives".to_string(),
                }],
            },
            forbidden_words: vec![
                "revolutionary".to_string(),
                "game-changing".to_string(),
                "disruptive".to_string(),
                "synergy".to_string(),
                "leverage".to_string(),
                "paradigm".to_string(),
            ],
            values: vec![
                "Build in public".to_string(),
                "Ship daily".to_string(),
                "Measure everything".to_string(),
            ],
            differentiators: vec![],
        }
    }
}

impl BrandContext {
    /// Load from disk or return default.
    pub fn load(home: &Path) -> Self {
        let path = home.join("brand_context.json");
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(ctx) = serde_json::from_str(&data) {
                    return ctx;
                }
            }
        }
        Self::default()
    }

    /// Persist to disk.
    pub fn save(&self, home: &Path) -> anyhow::Result<()> {
        let path = home.join("brand_context.json");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Inject brand context into a system prompt.
    pub fn to_system_prompt_section(&self) -> String {
        let mut s = format!("## Brand: {}\n", self.name);
        s.push_str(&format!("Positioning: {}\n\n", self.positioning));

        s.push_str(&format!("Voice: {}\n", self.voice.tone));
        for rule in &self.voice.writing_rules {
            s.push_str(&format!("- {}\n", rule));
        }
        s.push('\n');

        if !self.forbidden_words.is_empty() {
            s.push_str("NEVER use: ");
            s.push_str(&self.forbidden_words.join(", "));
            s.push_str("\n\n");
        }

        for audience in &self.audiences {
            s.push_str(&format!(
                "Target: {} — {}\n",
                audience.name, audience.description
            ));
        }
        s
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 4: Channel Connectors — abstraction over distribution
//            channels (Reddit, X, HN, Email, Blog). Each connector
//            implements the same trait so the Marketer/Writer agents
//            can publish and read analytics uniformly.
// ═══════════════════════════════════════════════════════════════════

/// A distribution channel that agents can publish to and read metrics from.
#[async_trait::async_trait]
pub trait ChannelConnector: Send + Sync {
    /// Human-readable channel name.
    fn name(&self) -> &str;

    /// Publish content to the channel.
    async fn publish(&self, content: &ContentPiece) -> anyhow::Result<PublishResult>;

    /// Fetch recent performance metrics.
    async fn fetch_metrics(&self, lookback_hours: u64) -> anyhow::Result<Vec<ContentMetric>>;

    /// Read recent community activity (comments, replies, mentions).
    async fn read_activity(&self, lookback_hours: u64) -> anyhow::Result<Vec<CommunitySignal>>;
}

/// A piece of content to publish.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentPiece {
    pub id: String,
    pub title: Option<String>,
    pub body: String,
    pub channel: ChannelType,
    pub tags: Vec<String>,
    pub author_role: BusinessRole,
    pub created_at: u64,
}

/// Supported channel types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelType {
    Reddit,
    HackerNews,
    Twitter,
    Blog,
    Email,
    LinkedIn,
    ProductHunt,
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reddit => write!(f, "Reddit"),
            Self::HackerNews => write!(f, "Hacker News"),
            Self::Twitter => write!(f, "Twitter/X"),
            Self::Blog => write!(f, "Blog"),
            Self::Email => write!(f, "Email"),
            Self::LinkedIn => write!(f, "LinkedIn"),
            Self::ProductHunt => write!(f, "Product Hunt"),
        }
    }
}

/// Result of publishing content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishResult {
    pub success: bool,
    pub url: Option<String>,
    pub platform_id: Option<String>,
    pub message: String,
}

/// A metric observation for published content.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentMetric {
    pub content_id: String,
    pub channel: ChannelType,
    pub views: u64,
    pub clicks: u64,
    pub engagements: u64,
    pub conversions: u64,
    pub sentiment_score: f64,
    pub measured_at: u64,
}

impl ContentMetric {
    /// Click-through rate.
    pub fn ctr(&self) -> f64 {
        if self.views == 0 {
            0.0
        } else {
            self.clicks as f64 / self.views as f64
        }
    }

    /// Engagement rate.
    pub fn engagement_rate(&self) -> f64 {
        if self.views == 0 {
            0.0
        } else {
            self.engagements as f64 / self.views as f64
        }
    }

    /// Conversion rate.
    pub fn conversion_rate(&self) -> f64 {
        if self.clicks == 0 {
            0.0
        } else {
            self.conversions as f64 / self.clicks as f64
        }
    }
}

/// A signal from the community (comment, reply, mention).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommunitySignal {
    pub channel: ChannelType,
    pub signal_type: SignalType,
    pub content: String,
    pub author: String,
    pub sentiment: f64,
    pub timestamp: u64,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SignalType {
    Comment,
    Reply,
    Mention,
    Share,
    Upvote,
    Downvote,
}

// ── Concrete connectors ──────────────────────────────────────────

/// Local file-based connector for development and testing.
/// Writes content to `~/.hsmii/channels/<name>/` and reads metrics from JSON.
pub struct LocalFileConnector {
    channel_name: String,
    /// Retained for connector identity / future API headers.
    #[allow(dead_code)]
    channel_type: ChannelType,
    base_dir: PathBuf,
}

impl LocalFileConnector {
    pub fn new(channel_type: ChannelType, home: &Path) -> Self {
        let channel_name = format!("{}", channel_type).to_lowercase().replace('/', "_");
        let base_dir = home.join("channels").join(&channel_name);
        std::fs::create_dir_all(&base_dir).ok();
        Self {
            channel_name,
            channel_type,
            base_dir,
        }
    }
}

#[async_trait::async_trait]
impl ChannelConnector for LocalFileConnector {
    fn name(&self) -> &str {
        &self.channel_name
    }

    async fn publish(&self, content: &ContentPiece) -> anyhow::Result<PublishResult> {
        let filename = format!("{}_{}.json", content.created_at, content.id);
        let path = self.base_dir.join(&filename);
        let json = serde_json::to_string_pretty(content)?;
        tokio::fs::write(&path, json).await?;
        Ok(PublishResult {
            success: true,
            url: Some(format!("file://{}", path.display())),
            platform_id: Some(content.id.clone()),
            message: format!("Published to local: {}", filename),
        })
    }

    async fn fetch_metrics(&self, _lookback_hours: u64) -> anyhow::Result<Vec<ContentMetric>> {
        let metrics_path = self.base_dir.join("metrics.json");
        if metrics_path.exists() {
            let data = tokio::fs::read_to_string(&metrics_path).await?;
            let metrics: Vec<ContentMetric> = serde_json::from_str(&data)?;
            Ok(metrics)
        } else {
            Ok(vec![])
        }
    }

    async fn read_activity(&self, _lookback_hours: u64) -> anyhow::Result<Vec<CommunitySignal>> {
        let activity_path = self.base_dir.join("activity.json");
        if activity_path.exists() {
            let data = tokio::fs::read_to_string(&activity_path).await?;
            let signals: Vec<CommunitySignal> = serde_json::from_str(&data)?;
            Ok(signals)
        } else {
            Ok(vec![])
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 5: Campaign Feedback Loop — connects content performance
//            metrics back into the Dream Engine pattern recognition
//            pipeline so the team learns from what works.
// ═══════════════════════════════════════════════════════════════════

/// A campaign is a named collection of content pieces targeting a goal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Campaign {
    pub id: String,
    pub name: String,
    pub goal: String,
    pub channels: Vec<ChannelType>,
    pub content_ids: Vec<String>,
    pub started_at: u64,
    pub status: CampaignStatus,
    pub metrics_snapshots: Vec<CampaignSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CampaignStatus {
    Draft,
    Active,
    Paused,
    Completed,
    Failed,
}

/// Point-in-time metrics snapshot for a campaign.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignSnapshot {
    pub timestamp: u64,
    pub total_views: u64,
    pub total_clicks: u64,
    pub total_engagements: u64,
    pub total_conversions: u64,
    pub avg_sentiment: f64,
    pub cost_usd: f64,
}

impl CampaignSnapshot {
    pub fn cac(&self) -> f64 {
        if self.total_conversions == 0 {
            f64::INFINITY
        } else {
            self.cost_usd / self.total_conversions as f64
        }
    }

    pub fn ctr(&self) -> f64 {
        if self.total_views == 0 {
            0.0
        } else {
            self.total_clicks as f64 / self.total_views as f64
        }
    }
}

/// Persistent campaign store — disk-backed at `~/.hsmii/campaigns.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignStore {
    pub campaigns: Vec<Campaign>,
    pub content_metrics: Vec<ContentMetric>,
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl CampaignStore {
    pub fn load(home: &Path) -> Self {
        let path = home.join("campaigns.json");
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(mut store) = serde_json::from_str::<CampaignStore>(&data) {
                    store.path = Some(path);
                    return store;
                }
            }
        }
        Self {
            campaigns: Vec::new(),
            content_metrics: Vec::new(),
            path: Some(path),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.path {
            let json = serde_json::to_string_pretty(self)?;
            std::fs::write(path, json)?;
        }
        Ok(())
    }

    pub fn create_campaign(
        &mut self,
        name: &str,
        goal: &str,
        channels: Vec<ChannelType>,
    ) -> &Campaign {
        let campaign = Campaign {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            goal: goal.to_string(),
            channels,
            content_ids: Vec::new(),
            started_at: now_ts(),
            status: CampaignStatus::Draft,
            metrics_snapshots: Vec::new(),
        };
        self.campaigns.push(campaign);
        self.campaigns.last().unwrap()
    }

    pub fn record_metric(&mut self, metric: ContentMetric) {
        self.content_metrics.push(metric);
    }

    /// Aggregate metrics for a campaign.
    pub fn campaign_snapshot(&self, campaign_id: &str) -> Option<CampaignSnapshot> {
        let campaign = self.campaigns.iter().find(|c| c.id == campaign_id)?;
        let metrics: Vec<&ContentMetric> = self
            .content_metrics
            .iter()
            .filter(|m| campaign.content_ids.contains(&m.content_id))
            .collect();

        if metrics.is_empty() {
            return Some(CampaignSnapshot {
                timestamp: now_ts(),
                total_views: 0,
                total_clicks: 0,
                total_engagements: 0,
                total_conversions: 0,
                avg_sentiment: 0.0,
                cost_usd: 0.0,
            });
        }

        let total_views: u64 = metrics.iter().map(|m| m.views).sum();
        let total_clicks: u64 = metrics.iter().map(|m| m.clicks).sum();
        let total_engagements: u64 = metrics.iter().map(|m| m.engagements).sum();
        let total_conversions: u64 = metrics.iter().map(|m| m.conversions).sum();
        let avg_sentiment =
            metrics.iter().map(|m| m.sentiment_score).sum::<f64>() / metrics.len() as f64;

        Some(CampaignSnapshot {
            timestamp: now_ts(),
            total_views,
            total_clicks,
            total_engagements,
            total_conversions,
            avg_sentiment,
            cost_usd: 0.0,
        })
    }

    /// Extract performance patterns for Dream Engine ingestion.
    /// Returns (domain, pattern_text, valence) tuples suitable for
    /// feeding into the stigmergic dream consolidation pipeline.
    pub fn extract_dream_patterns(&self) -> Vec<(String, String, f64)> {
        let mut patterns = Vec::new();

        for campaign in &self.campaigns {
            if campaign.metrics_snapshots.len() < 2 {
                continue;
            }

            let first = &campaign.metrics_snapshots[0];
            let last = campaign.metrics_snapshots.last().unwrap();

            // CTR trend
            let ctr_delta = last.ctr() - first.ctr();
            let valence = if ctr_delta > 0.0 { 1.0 } else { -1.0 };

            let channel_str = campaign
                .channels
                .iter()
                .map(|c| format!("{}", c))
                .collect::<Vec<_>>()
                .join(", ");

            patterns.push((
                format!("campaign:{}", campaign.name),
                format!(
                    "Campaign '{}' on [{}]: CTR {:.1}% → {:.1}% ({}), {} total conversions",
                    campaign.name,
                    channel_str,
                    first.ctr() * 100.0,
                    last.ctr() * 100.0,
                    if ctr_delta > 0.0 { "↑" } else { "↓" },
                    last.total_conversions,
                ),
                valence,
            ));
        }

        // Top-performing content by engagement rate
        let mut by_engagement: Vec<&ContentMetric> = self
            .content_metrics
            .iter()
            .filter(|m| m.views > 10)
            .collect();
        by_engagement.sort_by(|a, b| {
            b.engagement_rate()
                .partial_cmp(&a.engagement_rate())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for m in by_engagement.iter().take(5) {
            patterns.push((
                format!("content:top_engagement:{}", m.channel),
                format!(
                    "High-engagement content on {}: {:.1}% engagement rate, {:.1}% CTR, sentiment {:.2}",
                    m.channel,
                    m.engagement_rate() * 100.0,
                    m.ctr() * 100.0,
                    m.sentiment_score,
                ),
                1.0,
            ));
        }

        // Worst-performing content — negative signal
        for m in by_engagement.iter().rev().take(3) {
            if m.engagement_rate() < 0.01 {
                patterns.push((
                    format!("content:low_engagement:{}", m.channel),
                    format!(
                        "Low-engagement content on {}: {:.2}% engagement rate. Avoid this pattern.",
                        m.channel,
                        m.engagement_rate() * 100.0,
                    ),
                    -1.0,
                ));
            }
        }

        patterns
    }

    /// Channel performance summary for the Analyst agent.
    pub fn channel_performance_summary(&self) -> HashMap<ChannelType, ChannelPerformanceSummary> {
        let mut summaries: HashMap<ChannelType, ChannelPerformanceSummary> = HashMap::new();

        for m in &self.content_metrics {
            let entry = summaries.entry(m.channel).or_default();
            entry.content_count += 1;
            entry.total_views += m.views;
            entry.total_clicks += m.clicks;
            entry.total_engagements += m.engagements;
            entry.total_conversions += m.conversions;
            entry.sentiment_sum += m.sentiment_score;
        }

        for summary in summaries.values_mut() {
            if summary.content_count > 0 {
                summary.avg_sentiment = summary.sentiment_sum / summary.content_count as f64;
            }
        }

        summaries
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChannelPerformanceSummary {
    pub content_count: u64,
    pub total_views: u64,
    pub total_clicks: u64,
    pub total_engagements: u64,
    pub total_conversions: u64,
    pub avg_sentiment: f64,
    #[serde(skip)]
    sentiment_sum: f64,
}

impl ChannelPerformanceSummary {
    pub fn ctr(&self) -> f64 {
        if self.total_views == 0 {
            0.0
        } else {
            self.total_clicks as f64 / self.total_views as f64
        }
    }

    pub fn engagement_rate(&self) -> f64 {
        if self.total_views == 0 {
            0.0
        } else {
            self.total_engagements as f64 / self.total_views as f64
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Section 6: Team Orchestrator — the top-level coordinator that
//            holds all members, routes tasks, and manages the
//            feedback loop.
// ═══════════════════════════════════════════════════════════════════

/// The full autonomous team.
pub struct TeamOrchestrator {
    pub members: Vec<TeamMember>,
    pub brand: BrandContext,
    pub campaign_store: CampaignStore,
    pub social_memory: SocialMemory,
    /// Dream-derived routing adjustments. Fed by campaign outcomes and
    /// crystallized patterns from the dream engine.
    pub dream_advisor: DreamAdvisor,
    home: PathBuf,
}

impl TeamOrchestrator {
    /// Initialize a full team at the given home directory.
    pub fn new(home: &Path) -> Self {
        let members: Vec<TeamMember> = BusinessRole::all()
            .iter()
            .enumerate()
            .map(|(i, role)| TeamMember::from_role(*role, (100 + i) as AgentId))
            .collect();

        let brand = BrandContext::load(home);
        let campaign_store = CampaignStore::load(home);
        let social_memory = SocialMemory::default();
        let dream_advisor = DreamAdvisor::load(home);

        Self {
            members,
            brand,
            campaign_store,
            social_memory,
            dream_advisor,
            home: home.to_path_buf(),
        }
    }

    /// Get a member by role.
    pub fn member(&self, role: BusinessRole) -> Option<&TeamMember> {
        self.members.iter().find(|m| m.role == role)
    }

    /// Get a mutable member by role.
    pub fn member_mut(&mut self, role: BusinessRole) -> Option<&mut TeamMember> {
        self.members.iter_mut().find(|m| m.role == role)
    }

    /// Route a task to the best-fit team member.
    ///
    /// Uses the enhanced `bid_with_context()` when the DreamAdvisor has
    /// learned data; falls back to original `bid()` weights otherwise.
    pub fn route_task(&self, task_description: &str) -> Option<&TeamMember> {
        let advisor = if self.dream_advisor.is_empty() {
            None
        } else {
            Some(&self.dream_advisor)
        };

        self.members
            .iter()
            .filter(|m| m.status == MemberStatus::Active || m.status == MemberStatus::Idle)
            .max_by(|a, b| {
                a.bid_with_context(task_description, advisor)
                    .partial_cmp(&b.bid_with_context(task_description, advisor))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Build a composite system prompt for a team member that includes
    /// their persona + brand context.
    pub fn system_prompt_for(&self, role: BusinessRole) -> String {
        let member = match self.member(role) {
            Some(m) => m,
            None => return String::new(),
        };

        let mut prompt = member.persona.to_system_prompt();
        prompt.push_str("\n---\n\n");
        prompt.push_str(&self.brand.to_system_prompt_section());

        // Add team awareness
        prompt.push_str("\n## Your Team\n");
        for m in &self.members {
            if m.role == role {
                continue;
            }
            if m.status == MemberStatus::Active || m.status == MemberStatus::Idle {
                prompt.push_str(&format!(
                    "- {} ({}): reliability {:.0}%, {} tasks completed\n",
                    m.role.label(),
                    m.role.tag(),
                    m.reliability() * 100.0,
                    m.tasks_completed,
                ));
            }
        }

        prompt
    }

    /// Save all state to disk.
    pub fn save(&self) -> anyhow::Result<()> {
        self.brand.save(&self.home)?;
        self.campaign_store.save()?;
        self.dream_advisor.save(&self.home)?;

        // Save team members state
        let members_path = self.home.join("team_members.json");
        let json = serde_json::to_string_pretty(&self.members)?;
        std::fs::write(members_path, json)?;

        Ok(())
    }

    /// Load team member state from disk (merging with defaults for new roles).
    pub fn load_members(&mut self) -> anyhow::Result<()> {
        let path = self.home.join("team_members.json");
        if path.exists() {
            let data = std::fs::read_to_string(&path)?;
            let saved: Vec<TeamMember> = serde_json::from_str(&data)?;
            // Merge: keep saved state for existing roles, add defaults for new ones
            for member in &mut self.members {
                if let Some(saved_member) = saved.iter().find(|s| s.role == member.role) {
                    member.domain_history = saved_member.domain_history.clone();
                    member.tasks_completed = saved_member.tasks_completed;
                    member.tasks_failed = saved_member.tasks_failed;
                }
            }
        }
        Ok(())
    }

    /// Refresh the dream advisor from ALL available signal sources.
    ///
    /// Sources (in priority order):
    /// 1. Campaign metrics via `CampaignStore::extract_dream_patterns()`
    /// 2. Member outcome history — synthesizes patterns from domain_history
    ///    even without any campaign data, closing the cold-start loop.
    ///
    /// This means the advisor starts learning as soon as task outcomes
    /// are recorded, not only after campaigns accumulate metric snapshots.
    pub fn refresh_dream_advisor(&mut self) {
        let mut patterns = self.campaign_store.extract_dream_patterns();

        // ── Synthesize patterns from member outcome history ──────────
        // This closes the feedback loop: outcomes → advisor adjustments
        // even when no campaigns exist yet.
        for member in &self.members {
            for (domain, perf) in &member.domain_history {
                if perf.attempts < 1 {
                    continue;
                }
                let success_rate = perf.success_rate();
                // Valence: map success_rate [0,1] → [-1, 1]
                let valence = (success_rate * 2.0) - 1.0;
                let narrative = format!(
                    "{} in domain '{}': {}/{} tasks succeeded (quality {:.2})",
                    member.role, domain, perf.successes, perf.attempts, perf.avg_quality,
                );
                patterns.push((
                    format!("outcome:{}:{}", member.role.tag(), domain),
                    narrative,
                    valence,
                ));
            }
        }

        if !patterns.is_empty() {
            self.dream_advisor.ingest_campaign_patterns(&patterns);
        }
    }

    /// Ingest crystallized patterns from the dream engine.
    ///
    /// Called externally when the dream engine completes a consolidation
    /// cycle and produces `CrystallizedPattern`s with `role_affinity`.
    pub fn ingest_dream_patterns(&mut self, patterns: &[crate::dream::CrystallizedPattern]) {
        if !patterns.is_empty() {
            self.dream_advisor.ingest_crystallized_patterns(patterns);
        }
    }

    /// Create a channel connector for development/testing.
    pub fn local_connector(&self, channel: ChannelType) -> LocalFileConnector {
        LocalFileConnector::new(channel, &self.home)
    }
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ═══════════════════════════════════════════════════════════════════
// Section 7: Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Role system tests ────────────────────────────────────────

    #[test]
    fn test_all_roles_count() {
        assert_eq!(BusinessRole::all().len(), BusinessRole::COUNT);
    }

    #[test]
    fn test_available_excludes_soon() {
        let available = BusinessRole::available();
        assert!(!available.contains(&BusinessRole::Sales));
        assert!(!available.contains(&BusinessRole::Legal));
        assert_eq!(available.len(), 12);
    }

    #[test]
    fn test_each_role_has_unique_tag() {
        let tags: Vec<&str> = BusinessRole::all().iter().map(|r| r.tag()).collect();
        let unique: std::collections::HashSet<&&str> = tags.iter().collect();
        assert_eq!(tags.len(), unique.len(), "Tags must be unique");
    }

    #[test]
    fn test_each_role_has_activation_keywords() {
        for role in BusinessRole::all() {
            assert!(
                !role.activation_keywords().is_empty(),
                "{} has no activation keywords",
                role.label()
            );
        }
    }

    #[test]
    fn test_role_display() {
        assert_eq!(format!("{}", BusinessRole::Ceo), "CEO");
        assert_eq!(format!("{}", BusinessRole::Developer), "Developer");
        assert_eq!(format!("{}", BusinessRole::Sales), "Sales Agent");
    }

    #[test]
    fn test_council_role_mapping() {
        assert_eq!(BusinessRole::Ceo.to_council_role(), Role::Architect);
        assert_eq!(BusinessRole::Cto.to_council_role(), Role::Coder);
        assert_eq!(BusinessRole::Cfo.to_council_role(), Role::Critic);
        assert_eq!(BusinessRole::Cmo.to_council_role(), Role::Catalyst);
        assert_eq!(BusinessRole::Writer.to_council_role(), Role::Chronicler);
        assert_eq!(BusinessRole::Designer.to_council_role(), Role::Explorer);
    }

    // ── Persona generation tests ─────────────────────────────────

    #[test]
    fn test_build_persona_all_roles() {
        for role in BusinessRole::all() {
            let persona = build_persona(*role);
            assert_eq!(persona.name, role.label());
            assert!(!persona.identity.is_empty());
            assert!(!persona.voice.tone.is_empty());
            assert!(!persona.capabilities.is_empty());
            assert!(persona.proactivity > 0.0 && persona.proactivity <= 1.0);
        }
    }

    #[test]
    fn test_persona_to_system_prompt() {
        let persona = build_persona(BusinessRole::Ceo);
        let prompt = persona.to_system_prompt();
        assert!(prompt.contains("CEO"));
        assert!(prompt.contains("Strategic and decisive"));
    }

    // ── Team member tests ────────────────────────────────────────

    #[test]
    fn test_team_member_bidding() {
        let cto = TeamMember::from_role(BusinessRole::Cto, 101);
        let marketer = TeamMember::from_role(BusinessRole::Marketer, 107);

        let tech_bid = cto.bid("We need to refactor the API architecture");
        let market_bid = marketer.bid("We need to refactor the API architecture");
        assert!(tech_bid > market_bid, "CTO should bid higher on tech tasks");

        let social_tech = cto.bid("Run a social media campaign for SEO growth");
        let social_market = marketer.bid("Run a social media campaign for SEO growth");
        assert!(
            social_market > social_tech,
            "Marketer should bid higher on social tasks"
        );
    }

    #[test]
    fn test_team_member_outcome_tracking() {
        let mut writer = TeamMember::from_role(BusinessRole::Writer, 109);
        assert_eq!(writer.reliability(), 0.5); // No data → neutral

        writer.record_outcome("blog", true, 0.9);
        writer.record_outcome("blog", true, 0.85);
        writer.record_outcome("blog", false, 0.3);

        assert_eq!(writer.tasks_completed, 2);
        assert_eq!(writer.tasks_failed, 1);
        assert!((writer.reliability() - 0.667).abs() < 0.01);

        let blog_perf = writer.domain_history.get("blog").unwrap();
        assert_eq!(blog_perf.attempts, 3);
        assert_eq!(blog_perf.successes, 2);
    }

    #[test]
    fn test_member_status_for_unavailable() {
        let sales = TeamMember::from_role(BusinessRole::Sales, 112);
        assert_eq!(sales.status, MemberStatus::Soon);

        let dev = TeamMember::from_role(BusinessRole::Developer, 105);
        assert_eq!(dev.status, MemberStatus::Active);
    }

    // ── Brand context tests ──────────────────────────────────────

    #[test]
    fn test_brand_context_default() {
        let brand = BrandContext::default();
        assert!(!brand.forbidden_words.is_empty());
        assert!(!brand.audiences.is_empty());
        assert!(!brand.voice.writing_rules.is_empty());
    }

    #[test]
    fn test_brand_prompt_section() {
        let brand = BrandContext::default();
        let section = brand.to_system_prompt_section();
        assert!(section.contains("NEVER use:"));
        assert!(section.contains("revolutionary"));
    }

    #[test]
    fn test_brand_roundtrip() {
        let brand = BrandContext::default();
        let json = serde_json::to_string(&brand).unwrap();
        let loaded: BrandContext = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.name, brand.name);
        assert_eq!(loaded.forbidden_words.len(), brand.forbidden_words.len());
    }

    // ── Channel connector tests ──────────────────────────────────

    #[test]
    fn test_channel_type_display() {
        assert_eq!(format!("{}", ChannelType::Reddit), "Reddit");
        assert_eq!(format!("{}", ChannelType::HackerNews), "Hacker News");
        assert_eq!(format!("{}", ChannelType::Twitter), "Twitter/X");
    }

    #[test]
    fn test_content_metric_rates() {
        let m = ContentMetric {
            content_id: "test".to_string(),
            channel: ChannelType::Reddit,
            views: 1000,
            clicks: 50,
            engagements: 120,
            conversions: 5,
            sentiment_score: 0.8,
            measured_at: 0,
        };
        assert!((m.ctr() - 0.05).abs() < 0.001);
        assert!((m.engagement_rate() - 0.12).abs() < 0.001);
        assert!((m.conversion_rate() - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_content_metric_zero_views() {
        let m = ContentMetric {
            content_id: "empty".to_string(),
            channel: ChannelType::Blog,
            views: 0,
            clicks: 0,
            engagements: 0,
            conversions: 0,
            sentiment_score: 0.0,
            measured_at: 0,
        };
        assert_eq!(m.ctr(), 0.0);
        assert_eq!(m.engagement_rate(), 0.0);
        assert_eq!(m.conversion_rate(), 0.0);
    }

    // ── Campaign store tests ─────────────────────────────────────

    #[test]
    fn test_campaign_store_create() {
        let mut store = CampaignStore {
            campaigns: Vec::new(),
            content_metrics: Vec::new(),
            path: None,
        };
        let campaign = store.create_campaign(
            "Launch v2",
            "Drive 1000 signups",
            vec![ChannelType::Reddit, ChannelType::HackerNews],
        );
        assert_eq!(campaign.name, "Launch v2");
        assert_eq!(campaign.status, CampaignStatus::Draft);
        assert_eq!(store.campaigns.len(), 1);
    }

    #[test]
    fn test_campaign_snapshot_aggregation() {
        let mut store = CampaignStore {
            campaigns: Vec::new(),
            content_metrics: Vec::new(),
            path: None,
        };
        let campaign = store.create_campaign("Test", "Goal", vec![ChannelType::Blog]);
        let campaign_id = campaign.id.clone();

        // Add content to campaign
        store.campaigns[0].content_ids.push("post1".to_string());
        store.campaigns[0].content_ids.push("post2".to_string());

        store.record_metric(ContentMetric {
            content_id: "post1".to_string(),
            channel: ChannelType::Blog,
            views: 500,
            clicks: 25,
            engagements: 60,
            conversions: 3,
            sentiment_score: 0.9,
            measured_at: 0,
        });
        store.record_metric(ContentMetric {
            content_id: "post2".to_string(),
            channel: ChannelType::Blog,
            views: 300,
            clicks: 15,
            engagements: 30,
            conversions: 2,
            sentiment_score: 0.7,
            measured_at: 0,
        });

        let snapshot = store.campaign_snapshot(&campaign_id).unwrap();
        assert_eq!(snapshot.total_views, 800);
        assert_eq!(snapshot.total_clicks, 40);
        assert_eq!(snapshot.total_conversions, 5);
        assert!((snapshot.avg_sentiment - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_dream_pattern_extraction() {
        let mut store = CampaignStore {
            campaigns: Vec::new(),
            content_metrics: Vec::new(),
            path: None,
        };

        // Create campaign with snapshots showing improvement
        store.campaigns.push(Campaign {
            id: "c1".to_string(),
            name: "SEO Push".to_string(),
            goal: "Traffic".to_string(),
            channels: vec![ChannelType::Blog],
            content_ids: vec!["p1".to_string()],
            started_at: 0,
            status: CampaignStatus::Active,
            metrics_snapshots: vec![
                CampaignSnapshot {
                    timestamp: 100,
                    total_views: 1000,
                    total_clicks: 20,
                    total_engagements: 50,
                    total_conversions: 2,
                    avg_sentiment: 0.7,
                    cost_usd: 0.0,
                },
                CampaignSnapshot {
                    timestamp: 200,
                    total_views: 3000,
                    total_clicks: 120,
                    total_engagements: 200,
                    total_conversions: 15,
                    avg_sentiment: 0.85,
                    cost_usd: 0.0,
                },
            ],
        });

        let patterns = store.extract_dream_patterns();
        assert!(!patterns.is_empty());
        let (domain, text, valence) = &patterns[0];
        assert!(domain.contains("SEO Push"));
        assert!(text.contains("CTR"));
        assert!(*valence > 0.0, "Improving CTR should have positive valence");
    }

    #[test]
    fn test_channel_performance_summary() {
        let mut store = CampaignStore {
            campaigns: Vec::new(),
            content_metrics: Vec::new(),
            path: None,
        };

        store.record_metric(ContentMetric {
            content_id: "r1".to_string(),
            channel: ChannelType::Reddit,
            views: 5000,
            clicks: 200,
            engagements: 500,
            conversions: 10,
            sentiment_score: 0.6,
            measured_at: 0,
        });
        store.record_metric(ContentMetric {
            content_id: "r2".to_string(),
            channel: ChannelType::Reddit,
            views: 3000,
            clicks: 100,
            engagements: 300,
            conversions: 5,
            sentiment_score: 0.8,
            measured_at: 0,
        });
        store.record_metric(ContentMetric {
            content_id: "h1".to_string(),
            channel: ChannelType::HackerNews,
            views: 10000,
            clicks: 800,
            engagements: 1200,
            conversions: 25,
            sentiment_score: 0.9,
            measured_at: 0,
        });

        let summary = store.channel_performance_summary();
        assert_eq!(summary.len(), 2);

        let reddit = summary.get(&ChannelType::Reddit).unwrap();
        assert_eq!(reddit.content_count, 2);
        assert_eq!(reddit.total_views, 8000);

        let hn = summary.get(&ChannelType::HackerNews).unwrap();
        assert_eq!(hn.content_count, 1);
        assert_eq!(hn.total_conversions, 25);
    }

    // ── Orchestrator tests ───────────────────────────────────────

    #[test]
    fn test_orchestrator_creation() {
        let dir = std::env::temp_dir().join("hsmii_test_team");
        std::fs::create_dir_all(&dir).ok();

        let team = TeamOrchestrator::new(&dir);
        assert_eq!(team.members.len(), BusinessRole::COUNT);

        // Verify all roles present
        for role in BusinessRole::all() {
            assert!(
                team.member(*role).is_some(),
                "Missing team member: {}",
                role
            );
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_task_routing() {
        let dir = std::env::temp_dir().join("hsmii_test_routing");
        std::fs::create_dir_all(&dir).ok();

        let team = TeamOrchestrator::new(&dir);

        // Technical task should route to CTO or Developer
        let tech_member = team
            .route_task("We need to deploy the new API and fix the build pipeline")
            .unwrap();
        assert!(
            tech_member.role == BusinessRole::Cto || tech_member.role == BusinessRole::Developer,
            "Tech task routed to: {}",
            tech_member.role
        );

        // Marketing task should route to CMO or Marketer
        let market_member = team
            .route_task("Create a social media campaign for SEO growth on Reddit")
            .unwrap();
        assert!(
            market_member.role == BusinessRole::Cmo || market_member.role == BusinessRole::Marketer,
            "Marketing task routed to: {}",
            market_member.role
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_system_prompt_includes_brand() {
        let dir = std::env::temp_dir().join("hsmii_test_prompt");
        std::fs::create_dir_all(&dir).ok();

        let team = TeamOrchestrator::new(&dir);
        let prompt = team.system_prompt_for(BusinessRole::Writer);

        assert!(prompt.contains("Writer"));
        assert!(prompt.contains("NEVER use:"));
        assert!(prompt.contains("Your Team"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_campaign_cac() {
        let snapshot = CampaignSnapshot {
            timestamp: 0,
            total_views: 10000,
            total_clicks: 500,
            total_engagements: 800,
            total_conversions: 20,
            avg_sentiment: 0.75,
            cost_usd: 200.0,
        };
        assert!((snapshot.cac() - 10.0).abs() < 0.001);
        assert!((snapshot.ctr() - 0.05).abs() < 0.001);
    }

    #[test]
    fn test_local_file_connector_creation() {
        let dir = std::env::temp_dir().join("hsmii_test_connector");
        std::fs::create_dir_all(&dir).ok();

        let connector = LocalFileConnector::new(ChannelType::Reddit, &dir);
        assert_eq!(connector.name(), "reddit");
        assert!(connector.base_dir.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_local_connector_publish() {
        let dir = std::env::temp_dir().join("hsmii_test_publish");
        std::fs::create_dir_all(&dir).ok();

        let connector = LocalFileConnector::new(ChannelType::Blog, &dir);
        let content = ContentPiece {
            id: "test_post".to_string(),
            title: Some("Test Article".to_string()),
            body: "This is a test blog post.".to_string(),
            channel: ChannelType::Blog,
            tags: vec!["test".to_string()],
            author_role: BusinessRole::Writer,
            created_at: now_ts(),
        };

        let result = connector.publish(&content).await.unwrap();
        assert!(result.success);
        assert!(result.url.is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Intent disambiguation tests ────────────────────────────────

    #[test]
    fn test_design_survey_routes_to_hr_not_designer() {
        let dir = std::env::temp_dir().join("hsmii_test_disambig");
        std::fs::create_dir_all(&dir).ok();

        let team = TeamOrchestrator::new(&dir);

        // "design a survey" — the word "design" is present but "survey"
        // is HR context, so Designer's keyword hit should be suppressed.
        let member = team
            .route_task("design a survey for employee satisfaction")
            .unwrap();
        assert!(
            member.role == BusinessRole::Hr,
            "Expected HR for 'design a survey', got: {}",
            member.role
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_design_ui_still_routes_to_designer() {
        let dir = std::env::temp_dir().join("hsmii_test_designer_ok");
        std::fs::create_dir_all(&dir).ok();

        let team = TeamOrchestrator::new(&dir);

        // Genuine design tasks should still route to Designer
        let member = team
            .route_task("design a new landing page layout with wireframes")
            .unwrap();
        assert!(
            member.role == BusinessRole::Designer,
            "Expected Designer for UI task, got: {}",
            member.role
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_disambiguation_exclusions_cancel_false_positives() {
        let designer = TeamMember::from_role(BusinessRole::Designer, 200);

        // "design" should be suppressed by "survey" context
        let bid_survey = designer.bid("design a survey for hiring feedback");
        let bid_ui = designer.bid("design the homepage ui layout");

        assert!(
            bid_ui > bid_survey,
            "UI design bid ({:.3}) should be higher than survey bid ({:.3})",
            bid_ui,
            bid_survey,
        );
    }

    // ── Cold-start bid spread tests ────────────────────────────────

    #[test]
    fn test_cold_start_bid_spread_is_wide() {
        let dir = std::env::temp_dir().join("hsmii_test_spread");
        std::fs::create_dir_all(&dir).ok();

        let team = TeamOrchestrator::new(&dir);

        // Collect bids for a task with one clear keyword match
        let task = "build a REST API endpoint";
        let mut bids: Vec<(BusinessRole, f64)> = team
            .members
            .iter()
            .filter(|m| m.status == MemberStatus::Active || m.status == MemberStatus::Idle)
            .map(|m| (m.role, m.bid(task)))
            .collect();
        bids.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let max_bid = bids[0].1;
        let min_bid = bids.last().unwrap().1;
        let spread = max_bid - min_bid;

        // Spread should be >0.10 to make keyword matches meaningful
        // (old formula gave ~0.12, new formula gives ~0.20+)
        assert!(
            spread > 0.10,
            "Cold-start spread too narrow: {:.3} (max={:.3} {}, min={:.3} {})",
            spread,
            max_bid,
            bids[0].0,
            min_bid,
            bids.last().unwrap().0,
        );

        // The top bidder should be Developer or CTO (has "build"/"api" keywords)
        assert!(
            bids[0].0 == BusinessRole::Developer || bids[0].0 == BusinessRole::Cto,
            "Top bidder should be Dev/CTO for API task, got: {}",
            bids[0].0,
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── DreamAdvisor from outcomes test ─────────────────────────────

    #[test]
    fn test_dream_advisor_advances_from_outcomes() {
        let dir = std::env::temp_dir().join("hsmii_test_dream_outcomes");
        std::fs::create_dir_all(&dir).ok();

        let mut team = TeamOrchestrator::new(&dir);

        // Initially, dream advisor is empty
        assert!(team.dream_advisor.is_empty());

        // Record some task outcomes (no campaigns needed)
        if let Some(dev) = team.member_mut(BusinessRole::Developer) {
            dev.record_outcome("api_development", true, 0.9);
            dev.record_outcome("api_development", true, 0.85);
            dev.record_outcome("debugging", false, 0.3);
        }
        if let Some(writer) = team.member_mut(BusinessRole::Writer) {
            writer.record_outcome("blog_posts", true, 0.95);
        }

        // Refresh dream advisor — should now produce patterns from outcomes
        team.refresh_dream_advisor();

        // Advisor should no longer be empty (generation > 0)
        assert!(
            !team.dream_advisor.is_empty(),
            "Dream advisor should have learned from outcome history"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
