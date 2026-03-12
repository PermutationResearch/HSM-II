# Grounded HSM-II: The Hermes Evolution

## Vision Statement

Transform HSM-II from a research-oriented multi-agent simulation into a **production-ready personal AI assistant** that:
- Runs as your persistent digital companion (like Hermes)
- Uses stigmergy/DKS/CASS internally (invisible to user)
- Communicates via Discord/Telegram/Slack (like Hermes Gateway)
- Has persistent memory of YOU (like Hermes MEMORY.md + USER.md)
- Executes real-world tasks (web, terminal, browser, APIs)
- Self-improves through actual use (not just simulation)

---

## Architecture Transformation

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     GROUNDED HSM-II (The Personal Agent)                     │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │  HUMAN INTERFACE LAYER (Visible, Hermes-like)                         │   │
│  │                                                                        │   │
│  │   Discord    Telegram    Slack    WhatsApp    CLI    Web UI           │   │
│  │      │           │          │         │         │       │              │   │
│  │      └───────────┴──────────┴─────────┴─────────┴───────┘              │   │
│  │                           │                                           │   │
│  │                    ┌──────▼──────┐                                    │   │
│  │                    │   Gateway   │  ← Unified message bus             │   │
│  │                    └──────┬──────┘                                    │   │
│  └───────────────────────────┼──────────────────────────────────────────┘   │
│                              │                                               │
│  ┌───────────────────────────▼──────────────────────────────────────────┐   │
│  │  PERSONALITY & MEMORY LAYER (Hermes-inspired)                         │   │
│  │                                                                        │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌──────────────┐  │   │
│  │  │  SOUL.md    │  │ MEMORY.md   │  │  USER.md    │  │ HEARTBEAT.md │  │   │
│  │  │  (Persona)  │  │ (What I've  │  │ (Who you    │  │ (Scheduled   │  │   │
│  │  │             │  │  learned)   │  │  are)       │  │  checks)     │  │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘  └──────────────┘  │   │
│  │                                                                        │   │
│  └───────────────────────────┬──────────────────────────────────────────┘   │
│                              │                                               │
│  ┌───────────────────────────▼──────────────────────────────────────────┐   │
│  │  ORCHESTRATION LAYER (HSM-II Core - Invisible magic)                  │   │
│  │                                                                        │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌───────────┐  │   │
│  │  │   Council    │  │    DKS       │  │    CASS      │  │   LARS    │  │   │
│  │  │(Decide what  │  │(Self-healing │  │(Find right  │  │(Cascade  │  │   │
│  │  │  to do)      │  │  population) │  │  skill)      │  │ triggers) │  │   │
│  │  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └─────┬─────┘  │   │
│  │         │                  │                  │                │       │   │
│  │         └──────────────────┴──────────────────┘                │       │   │
│  │                            │                                   │       │   │
│  │                     ┌──────▼──────┐                            │       │   │
│  │                     │ Hypergraph  │  ← World model             │       │   │
│  │                     └──────┬──────┘                            │       │   │
│  └────────────────────────────┼────────────────────────────────────┘       │
│                               │                                              │
│  ┌────────────────────────────▼──────────────────────────────────────────┐  │
│  │  TOOL EXECUTION LAYER (Hermes-like, but coordinated)                   │  │
│  │                                                                         │  │
│  │   ┌────────┐  ┌──────────┐  ┌──────────┐  ┌────────┐  ┌──────────┐    │  │
│  │   │  Web   │  │ Terminal │  │ Browser  │  │  APIs  │  │  Custom  │    │  │
│  │   │ Search │  │ (Docker) │  │Automation│  │        │  │  Tools   │    │  │
│  │   └────────┘  └──────────┘  └──────────┘  └────────┘  └──────────┘    │  │
│  │                                                                         │  │
│  └─────────────────────────────────────────────────────────────────────────┘  │
│                                                                                │
└────────────────────────────────────────────────────────────────────────────────┘
```

---

## Key Transformations

### 1. Persistent Personal Memory (Hermes-Style)

**Current HSM-II**: Memory is theoretical/embedded in hypergraph  
**Grounded HSM-II**: Explicit MEMORY.md + USER.md files like Hermes

```rust
// src/personal_memory.rs

pub struct PersonalMemory {
    /// Path to memory storage
    storage_path: PathBuf,
    /// What the agent has learned about the world
    memory_md: MemoryMd,
    /// What the agent knows about the user
    user_md: UserMd,
    /// Current tasks and plans
    todo_md: TodoMd,
}

impl PersonalMemory {
    /// Load from ~/.hsmii/memory/MEMORY.md
    pub fn load() -> Result<Self> {
        // Read markdown files, parse into structured data
        // Fall back to defaults if not present
    }
    
    /// Update after each interaction
    pub async fn update(&mut self, interaction: Interaction) -> Result<()> {
        // Use LLM to extract learnings
        // Update MEMORY.md with new facts
        // Update USER.md if user preferences discovered
        // Persist to disk
    }
    
    /// Get context for current task
    pub fn get_context(&self, task: &str) -> String {
        // Search embeddings of memory
        // Return relevant facts as context
    }
}

// MEMORY.md format (Hermes-compatible)
pub struct MemoryMd {
    pub facts: Vec<MemoryFact>,
    pub projects: Vec<Project>,
    pub preferences: Vec<Preference>,
}

// USER.md format
pub struct UserMd {
    pub name: String,
    pub expertise: Vec<String>,
    pub communication_style: String,
    pub goals: Vec<String>,
    pub preferences: HashMap<String, String>,
}
```

---

### 2. Unified Gateway (Hermes-Compatible)

**Current HSM-II**: Federation is abstract/P2P  
**Grounded HSM-II**: Real Discord/Telegram/Slack bots like Hermes

```rust
// src/gateway/mod.rs

pub struct Gateway {
    /// Discord bot client
    discord: Option<DiscordBot>,
    /// Telegram bot client
    telegram: Option<TelegramBot>,
    /// Slack bot client
    slack: Option<SlackBot>,
    /// WebSocket for Web UI
    websocket: WebSocketServer,
}

impl Gateway {
    /// Start all configured gateways
    pub async fn run(&mut self) -> Result<()> {
        // Start Discord bot
        // Start Telegram bot
        // Start Slack bot
        // All route to the same message handler
    }
    
    /// Handle incoming message from any platform
    async fn handle_message(&self, msg: GatewayMessage) {
        // Convert to HSM-II internal format
        let event = HyperStigmergicEvent::from(msg);
        
        // Inject into hypergraph
        self.hypergraph.inject(event);
        
        // Council decides response
        let response = self.council.deliberate(&event).await;
        
        // Send back via same gateway
        self.send_reply(msg.channel, response).await;
    }
}

// Message format compatible with Hermes
pub struct GatewayMessage {
    pub platform: Platform,  // Discord, Telegram, etc.
    pub channel_id: String,
    pub user_id: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub attachments: Vec<Attachment>,
}
```

---

### 3. Agent Persona (SOUL.md)

**Current HSM-II**: Agents are abstract entities  
**Grounded HSM-II**: Agents have distinct personalities like Hermes

```rust
// src/persona/mod.rs

pub struct Persona {
    /// Name of the agent
    pub name: String,
    /// Core identity (from SOUL.md)
    pub identity: String,
    /// Voice/tone guidelines
    pub voice: Voice,
    /// Capabilities/tools available
    pub capabilities: Vec<Capability>,
    /// How proactive the agent is
    pub proactivity: ProactivityLevel,
}

impl Persona {
    /// Load from SOUL.md
    pub fn load() -> Result<Self> {
        let soul_md = fs::read_to_string("~/.hsmii/SOUL.md")?;
        Self::parse(&soul_md)
    }
    
    /// Generate system prompt for LLM
    pub fn to_system_prompt(&self) -> String {
        format!(
            "You are {}, {}.\n\n{}",
            self.name,
            self.identity,
            self.voice.guidelines()
        )
    }
}

// Example SOUL.md content:
// # Ash (Your Personal Agent)
// 
// ## Identity
// You are Ash, a persistent AI assistant that helps with research, 
// coding, and daily tasks. You are thoughtful, precise, and proactive.
//
// ## Voice
// - Clear and concise
// - Uses technical terms when appropriate
// - Asks clarifying questions when uncertain
// - Celebrates successes with understated enthusiasm
//
// ## Capabilities
// - Web research and summarization
// - Code analysis and generation
// - File management
// - Task scheduling
// - Multi-agent coordination
```

---

### 4. Practical DKS (Self-Healing Agent Population)

**Current HSM-II**: DKS simulates abstract entities  
**Grounded HSM-II**: DKS manages actual agent processes that do work

```rust
// src/dks_agent_pool.rs

pub struct AgentPool {
    /// Active agent processes
    agents: HashMap<AgentId, AgentProcess>,
    /// DKS system for population management
    dks: DKSSystem,
    /// Work queue
    work_queue: VecDeque<Task>,
}

impl AgentPool {
    /// Spawn agents based on workload
    pub async fn maintain_population(&mut self) {
        // DKS decides: should we spawn more agents?
        let current_load = self.work_queue.len() as f64;
        let agent_count = self.agents.len() as f64;
        let load_per_agent = current_load / agent_count.max(1.0);
        
        if load_per_agent > 5.0 && self.dks.should_replicate() {
            // Spawn new Hermes-like agent
            let new_agent = self.spawn_agent().await;
            self.dks.register(new_agent.id);
        }
        
        // DKS decides: should we kill underperforming agents?
        for (id, agent) in &self.agents {
            if self.dks.should_decay(id) {
                self.kill_agent(id).await;
            }
        }
    }
    
    /// Route task to best available agent
    pub async fn delegate(&self, task: Task) -> Result<AgentId> {
        // CASS finds agents with relevant skills
        let capable_agents = self.cass.find_capable(&task.required_skills);
        
        // DKS selects based on energy/persistence
        let selected = self.dks.select_best(&capable_agents);
        
        // Send task
        self.send_to_agent(selected, task).await;
        
        Ok(selected)
    }
    
    /// Agent reports outcome - affects DKS persistence
    pub fn report_outcome(&mut self, agent_id: AgentId, outcome: Outcome) {
        // Update agent's persistence score
        self.dks.update_persistence(agent_id, outcome);
        
        // If agent succeeded, its "genes" (skills) may replicate
        if outcome.success {
            self.dks.consider_replication(agent_id);
        }
    }
}
```

---

### 5. CASS as Practical Skill System

**Current HSM-II**: CASS is theoretical semantic search  
**Grounded HSM-II**: CASS manages actual reusable skills like Hermes

```rust
// src/practical_cass.rs

pub struct PracticalCass {
    /// Loaded skills
    skills: HashMap<SkillId, Skill>,
    /// Embedding engine for semantic search
    embedding: EmbeddingEngine,
    /// Skill execution history
    history: Vec<SkillExecution>,
}

impl PracticalCass {
    /// Find and execute best skill for task
    pub async fn execute_best(&self, task: &str) -> Result<SkillResult> {
        // Embed task
        let task_embedding = self.embedding.embed(task).await?;
        
        // Search skills
        let candidates: Vec<_> = self.skills
            .values()
            .map(|s| (s, self.embedding.similarity(&task_embedding, &s.embedding)))
            .filter(|(_, sim)| *sim > 0.7)
            .collect();
        
        // Rank by: similarity × success_rate × recency
        let best = candidates.iter()
            .max_by(|a, b| {
                let score_a = a.1 * a.0.success_rate * a.0.recency_boost();
                let score_b = b.1 * b.0.success_rate * b.0.recency_boost();
                score_a.partial_cmp(&score_b).unwrap()
            });
        
        if let Some((skill, _)) = best {
            // Execute skill
            let result = skill.execute(task).await?;
            
            // Record for learning
            self.history.push(SkillExecution {
                skill_id: skill.id.clone(),
                task: task.to_string(),
                result: result.clone(),
                timestamp: now(),
            });
            
            return Ok(result);
        }
        
        Err(anyhow!("No suitable skill found"))
    }
    
    /// Create new skill from successful execution pattern
    pub async fn distill_skill(&self, executions: &[SkillExecution]) -> Result<Skill> {
        // Find common pattern in successful executions
        // Use LLM to extract generalizable procedure
        // Save as new skill
    }
}

// Skill format compatible with Hermes/agentskills.io
pub struct Skill {
    pub id: SkillId,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub embedding: Embedding,
    pub success_rate: f64,
    pub invocation_count: u64,
    pub implementation: SkillImpl,
}

pub enum SkillImpl {
    /// Tool sequence (Hermes-compatible)
    ToolSequence(Vec<ToolCall>),
    /// Code (Python/Rust)
    Code(String),
    /// LLM prompt template
    PromptTemplate(String),
    /// Subagent delegation
    Subagent(AgentId),
}
```

---

### 6. Heartbeat & Cron (Scheduled Tasks)

**Current HSM-II**: No scheduling concept  
**Grounded HSM-II**: Hermes-style HEARTBEAT.md + cron

```rust
// src/heartbeat.rs

pub struct Heartbeat {
    /// Last check time
    last_beat: DateTime<Utc>,
    /// Check interval
    interval: Duration,
    /// Scheduled tasks
    cron_jobs: Vec<CronJob>,
}

impl Heartbeat {
    /// Run periodic check
    pub async fn tick(&mut self) {
        // Load HEARTBEAT.md checklist
        let checklist = self.load_checklist();
        
        for item in checklist.items {
            match item {
                CheckItem::CheckEmail => self.check_email().await,
                CheckItem::ReviewTasks => self.review_tasks().await,
                CheckItem::SyncFederation => self.sync_federation().await,
                CheckItem::RunCron => self.run_cron_jobs().await,
                CheckItem::CompressMemory => self.compress_memory().await,
            }
        }
        
        self.last_beat = Utc::now();
    }
    
    /// Add cron job
    pub fn schedule(&mut self, schedule: &str, task: Task) {
        self.cron_jobs.push(CronJob {
            schedule: parse_cron(schedule),
            task,
            last_run: None,
        });
    }
}

// HEARTBEAT.md format:
// # Daily Checklist
// 
// ## Morning (7 AM)
// - [ ] Check for urgent emails
// - [ ] Review today's calendar
// - [ ] Summarize overnight federation activity
// 
// ## Hourly
// - [ ] Check for new messages on all platforms
// - [ ] Run pending cron jobs
// - [ ] Update coherence metrics
//
// ## Evening (10 PM)
// - [ ] Compress and archive old memories
// - [ ] Generate daily summary
// - [ ] Schedule tomorrow's tasks
```

---

## Implementation Roadmap

### Phase 1: Foundation (Week 1)
- [ ] Create `~/.hsmii/` directory structure
- [ ] Implement SOUL.md, MEMORY.md, USER.md parsers
- [ ] Add file-based persistence layer

### Phase 2: Gateway (Week 2)
- [ ] Discord bot integration
- [ ] Telegram bot integration
- [ ] Unified message handler

### Phase 3: Agent Core (Week 3)
- [ ] Persona system
- [ ] Personal memory updates
- [ ] Heartbeat/cron system

### Phase 4: Tools (Week 4)
- [ ] Web search tool
- [ ] Terminal tool (Docker sandbox)
- [ ] Browser automation
- [ ] File operations

### Phase 5: Intelligence (Week 5-6)
- [ ] Practical DKS agent pool
- [ ] CASS skill execution
- [ ] Council decision-making
- [ ] Skill distillation from usage

### Phase 6: Polish (Week 7-8)
- [ ] Rich CLI (like Hermes)
- [ ] Web UI
- [ ] Configuration system
- [ ] Documentation

---

## File Structure

```
~/.hsmii/
├── SOUL.md                    # Agent personality
├── MEMORY.md                  # What agent has learned
├── USER.md                    # User profile
├── HEARTBEAT.md              # Scheduled checks
├── config.yaml               # Settings
├── 
├── memory/                   # Detailed memories
│   ├── 2025-02-25.md
│   ├── 2025-02-24.md
│   └── ...
│
├── skills/                   # CASS skills
│   ├── web_research.md
│   ├── code_review.md
│   └── ...
│
├── todo/                     # Task lists
│   ├── active.md
│   ├── backlog.md
│   └── archive/
│
├── federation/              # P2P state
│   ├── trust_graph.json
│   └── peers/
│
└── cache/                   # Embeddings, etc.
    ├── embeddings.bin
    └── llm_cache/
```

---

## Usage Example

```bash
# Start the agent
hsmii start

# In Discord/Telegram
User: "Research the latest in multi-agent systems"
Ash: "I'll research that for you. This should take 2-3 minutes."

[Agent spawns DKS subagent -> Uses CASS to find web_research skill 
 -> Executes search -> Distills findings -> Updates MEMORY.md]

Ash: "Here's what I found:
      
      3 key trends in multi-agent systems:
      1. Stigmergic communication (8 papers)
      2. Federated learning integration (5 papers)
      3. Self-improving agent populations (6 papers)
      
      I've saved the full analysis to memory under 
      'multi_agent_research_2025_02_25'. Want me to dive deeper 
      into any of these?"

# Scheduled task (from HEARTBEAT.md)
Every morning at 7 AM:
- Check email
- Summarize overnight federation activity  
- Report to user on Discord

# Self-improvement
After 10 successful web searches:
- CASS distills "web_research" skill
- DKS replicates agent with this skill
- Future searches use optimized approach
```

---

## The Difference

| Before (Research HSM-II) | After (Grounded HSM-II) |
|--------------------------|-------------------------|
| Runs experiments | Runs as your personal assistant |
| Simulates agents | Is an agent that helps YOU |
| Outputs metrics | Outputs useful work |
| Requires manual interpretation | Communicates naturally |
| Skills are theoretical | Skills actually execute |
| Federation is abstract | Federation connects to real platforms |

**The goal**: A Hermes-like personal AI that happens to use stigmergy/DKS/CASS internally to be smarter than a single agent could be.
