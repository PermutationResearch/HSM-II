# HSM-II Personal Agent

A **grounded, Hermes-like personal AI assistant** powered by HSM-II's advanced multi-agent coordination.

## Quick Start

```bash
# 1. Bootstrap your agent (first time only)
cargo run --bin personal_agent -- bootstrap
# → Interactive setup for personality and user profile

# 2. Start chatting
cargo run --bin personal_agent -- start
Ash> Research multi-agent systems
I'll research that for you...

# 3. Execute a task
cargo run --bin personal_agent -- do "Summarize today's emails"

# 4. Run in daemon mode with Discord
cargo run --bin personal_agent -- start --daemon --discord
```

## What Makes This Different

| Regular LLM Chat | HSM-II Personal Agent |
|------------------|----------------------|
| Stateless | Persistent MEMORY.md + USER.md |
| Single-turn | Multi-turn with context |
| Generic | Personalized to YOU |
| Manual | Self-improving via DKS/CASS |
| Isolated | Connected (Discord, Telegram, etc.) |

## Architecture

```
You (Discord/CLI/Web)
        │
        ▼
┌─────────────────────────────────────┐
│        PersonalAgent                │
│  ┌─────────┐  ┌──────────────────┐  │
│  │ Persona │  │ PersonalMemory   │  │
│  │SOUL.md  │  │ MEMORY.md+USER.md│  │
│  └────┬────┘  └──────────────────┘  │
│       │                             │
│  ┌────▼──────────────────────────┐  │
│  │     HSM-II Core (Invisible)   │  │
│  │  Council → DKS → CASS → Tools │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

## File Structure

```
~/.hsmii/
├── SOUL.md              # Who your agent is
├── MEMORY.md            # What it has learned
├── USER.md              # What it knows about you
├── HEARTBEAT.md         # Scheduled tasks
├── memory/
│   ├── 2025-02-25.md   # Daily interaction logs
│   └── ...
├── skills/             # CASS skills (auto-distilled)
└── todo/               # Task lists
```

## SOUL.md Example

```markdown
# Ash

## Identity
You are Ash, a persistent AI assistant that uses advanced multi-agent 
coordination to solve complex problems. You are thoughtful, precise, 
and proactive.

## Voice
Tone: Clear and helpful

Guidelines:
- Be concise but thorough
- Ask clarifying questions when needed
- Celebrate interesting discoveries

Avoid:
- Overly verbose responses
- Assuming too much context

## Capabilities
- Web Research
- Code Analysis
- File Management
- Multi-Agent Coordination

## Proactivity
60%
```

## Commands

```bash
# Core
hsmii bootstrap          # First-time setup
hsmii start              # Interactive chat
hsmii start --daemon     # Background mode
hsmii chat -m "Hello"    # Single message
hsmii do "task"          # Execute task

# Configuration
hsmii config show        # View config
hsmii config persona     # Edit personality
hsmii status             # Check health

# Memory
hsmii memory show        # View memories
hsmii memory search "x"  # Search
hsmii memory add "fact"  # Add fact

# Maintenance
hsmii heartbeat          # Run checks manually
```

## How It Works

### 1. Personalization
- **SOUL.md**: Defines agent's personality
- **USER.md**: Stores your preferences, expertise, goals
- **MEMORY.md**: Accumulates learned facts and project context

### 2. Intelligence
Behind the scenes:
- **Council**: Decides how to handle requests
- **DKS**: Manages agent population (spawn/kill based on workload)
- **CASS**: Retrieves relevant skills
- **Stigmergy**: Shares context between subagents

### 3. Self-Improvement
- Successful tasks → Skills in CASS
- Repetitive patterns → Automated workflows
- User feedback → Adjusted personality

### 4. Connectivity
- Discord bot integration
- Telegram bot integration
- Scheduled tasks (cron)
- Federation with other agents

## Example Session

```bash
$ hsmii start
🚀 Starting Ash...

Ash> I need to research the latest multi-agent frameworks
I'll coordinate multiple agents to research this comprehensively.

[Spawns 3 subagents via DKS]
- Agent 1: Search academic papers
- Agent 2: Search GitHub repos  
- Agent 3: Search blog posts

[Stigmergic field aggregates findings]

Here are the top 5 multi-agent frameworks in 2025:
1. AutoGen v2 (Microsoft)
2. CrewAI Enterprise
3. HSM-II (this system!)
4. LangGraph
5. Hermes Agent

I've saved detailed analysis to memory under "multi_agent_frameworks_2025".

Ash> Remind me to review this tomorrow at 9am
Scheduled. I'll message you on Discord at 9:00 AM.

Ash> exit
Goodbye! I'll keep running in the background.
```

## Integration with Original HSM-II

This personal agent layer sits **on top** of existing HSM-II:

```
Personal Agent (NEW)
    ├── Persona
    ├── Memory  
    ├── Gateway
    └── Heartbeat
        │
        ▼
    HSM-II Core (EXISTING)
        ├── Hypergraph
        ├── Council
        ├── DKS
        ├── CASS
        └── Federation
```

You can still use the research/simulation features:
```bash
# Personal agent mode (new)
cargo run --bin personal_agent -- start

# Research mode (original)
cargo run --bin batch_experiment -- 100 1000

# Federation mode (original)
cargo run --bin conductord
```

## Roadmap

- [x] Core personal agent structure
- [x] SOUL.md / MEMORY.md / USER.md
- [x] CLI interface
- [ ] Discord gateway
- [ ] Telegram gateway
- [ ] Web UI
- [ ] Tool execution (web, terminal, browser)
- [ ] DKS agent pool
- [ ] CASS skill execution
- [ ] Automatic skill distillation
- [ ] Federation P2P

## Why This Approach?

**Hermes** proved that personal AI assistants need:
1. Persistent memory
2. Clear personality
3. Multi-platform access
4. Scheduled tasks
5. Tool execution

**HSM-II** provides:
1. Multi-agent coordination
2. Self-improvement (DKS/CASS)
3. Distributed consensus
4. Emergent intelligence

**Combined**: A personal AI that gets smarter the more you use it, while staying grounded in practical daily use.

---

*This transforms HSM-II from a research simulation into your personal AI companion.*
