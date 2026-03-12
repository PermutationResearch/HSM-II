# Hyper-Stigmergic Morphogenesis II (HSM-II)

[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> **Where swarms of AI agents think together, learn from each other, and grow smarter over time.**

HSM-II is a **federated multi-agent hypergraph system** that brings emergent collective intelligence to life. Built in Rust, it enables autonomous AI agents to coordinate without central control, learn from collective experience, and solve complex problems through shared knowledge structures.

Think of it as *ants solving problems through pheromone trails* — except the ants are LLM-powered agents, the trails are hypergraph edges, and the colony learns to code, research, and coordinate autonomously.

**[📄 Read the Paper](./documentatio./documentation/paper.pdf)** | **[🚀 Quick Start](#-quick-start)** | **[🌐 Live Demo](https://permutationresearch.github.io/HSM-II/)**

---

## 🚀 Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) 1.75+ 
- Docker (optional, for containerized deployment)
- API key for OpenAI, Anthropic, or local Ollama
- **macOS users**: Use the `.command` scripts in `agent_tools/scripts/macos/`
- **Linux users**: Use equivalent `cargo run` commands (see [documentation/guides/COMMANDS_GUIDE.md](documentation/guides/COMMANDS_GUIDE.md))

### Installation

```bash
# Clone the repository
git clone https://github.com/PermutationResearch/HSM-II.git
cd HSM-II

# Configure environment
cp .env.example .env
# Edit .env with your API keys

# Build and run
cargo run --release
```

### Three Ways to Use HSM-II

#### 1. Personal Agent (Easiest)
```bash
./agent_tools/scripts/macos/run-personal-agent.command
```
Your AI companion with built-in coordination capabilities.

#### 2. With Visualization
```bash
# Terminal 1: Personal agent
./agent_tools/scripts/macos/run-personal-agent.command

# Terminal 2: Visual hypergraph
./agent_tools/scripts/macos/open-hypergraphd.command
```

#### 3. Full Research Stack
```bash
# Terminal 1: Research backend
./agent_tools/scripts/macos/run-hyper-stigmergy-II.command

# Terminal 2: Personal agent
./agent_tools/scripts/macos/run-personal-agent.command --connect-hypergraph

# Browser: View hypergraph
./agent_tools/scripts/macos/open-hypergraphd.command
```

---

## 🧠 What HSM-II Does

### Shared Memory Through Hypergraphs

HSM-II stores knowledge as a **hypergraph** — a web where edges can connect multiple nodes at once. Agents read and write to this shared structure:

- **Beliefs** — What agents think about the world
- **Hyperedges** — Connections between multiple beliefs (emergent insights)
- **Ontological Tags** — Categories for organizing knowledge
- **Visibility Scopes** — Local, Shared, or Restricted access levels

```
Agent A ──believes──► "Neural networks are effective"
                           │
                           │ (hyperedge)
                           ▼
Agent B ──believes──► "For image classification" ◄───believes─── Agent C
                           │
                           │ (hyperedge)
                           ▼
                    "But require lots of data"
```

### Stigmergic Coordination

Like ants leaving pheromone trails, agents leave "trails" in the hypergraph:

1. **Agent solves a problem** → Creates/updates beliefs
2. **Other agents detect changes** → Read the updated structure
3. **Collective learning emerges** → No direct communication needed

### Multi-Agent Council Deliberation

When decisions matter, agents form **Councils**:

| Mode | Use Case | How It Works |
|------|----------|--------------|
| **Simple** | Low complexity, high urgency | Single agent decides with coherence check |
| **Orchestrate** | Medium complexity | Leader agent coordinates specialists |
| **Debate** | High complexity, high stakes | Full deliberation with evidence and voting |

Councils use **evidence contracts** — agents must provide proof for their positions.

---

## 🎓 Continuous Learning & Skill Improvement

### CASS: Continuous Automated Skill Synthesis

HSM-II doesn't just execute tasks — it **learns from them**:

1. **Harvest** — Successful agent trajectories are captured
2. **Distill** — Common patterns become reusable skills
3. **Promote** — Skills pass through consensus jury validation
4. **Version** — Skills evolve with semantic versioning

### DKS: Distributed Knowledge System

Knowledge spreads through the agent population like genetic evolution:

- **Selection Pressure** — Better-performing knowledge survives
- **Replication** — Successful patterns spread to other agents
- **Mutation** — Variations are tested and rewarded
- **Flux** — Knowledge flows between local and shared scopes

---

## 🌐 Federation & Multi-Node Coordination

Multiple HSM-II instances can connect and form a **federation**:

- **Trust Dynamics** — Bayesian trust scoring between nodes
- **Conflict Resolution** — When beliefs diverge, councils negotiate
- **Knowledge Sync** — Selective merging of hypergraph structures
- **Anti-fragile** — The system improves under stress

```
┌─────────────┐      Trust Edges      ┌─────────────┐
│  HSM-II     │◄─────────────────────►│  HSM-II     │
│  Node A     │    (confidence: 0.85) │  Node B     │
│  (Toronto)  │                       │  (London)   │
└─────────────┘                       └─────────────┘
       │                                     │
       │         ┌─────────────┐             │
       └────────►│  HSM-II     │◄────────────┘
                 │  Node C     │
                 │  (Tokyo)    │
                 └─────────────┘
```

---

## 🛠️ Built-In Tool Suite (62+ Tools)

Agents come with real-world capabilities out of the box:

| Category | What Agents Can Do |
|----------|-------------------|
| **Web & Browser** | Search, scrape, automate browsers, read PDFs |
| **File Operations** | Read, write, search, analyze any file type |
| **Shell & System** | Execute commands, gather system info |
| **Git Operations** | Clone, commit, diff, blame, search repositories |
| **APIs & Data** | HTTP requests, JSON parsing, encoding/decoding |
| **Calculations** | Math, statistics, unit conversions |
| **Text Processing** | Regex, parsing, formatting, diffing |

Tools are **real implementations**, not mocks. Agents can actually modify files, browse websites, and run commands.

---

## 🤖 LLM Integration & Provider Failover

HSM-II works with multiple LLM providers with automatic failover:

- **OpenAI** — GPT-4o, GPT-4o-mini
- **Anthropic** — Claude 3.5 Sonnet, Claude 3 Opus
- **Ollama** — Local models (Llama, Mistral, etc.)

If one provider fails, the system automatically switches to another. No single point of failure.

---

## 🔐 Security & Access Control

- **API Key Management** — Argon2-hashed, revocable keys
- **JWT Authentication** — 24-hour expiring tokens
- **Rate Limiting** — Per-key quota enforcement
- **Permission Levels** — Read, Write, Admin access control

---

## 🏗️ System Architecture

```
╔═══════════════════════════════════════════════════════════════════════╗
║                         HSM-II SYSTEM                                 ║
╠═══════════════════════════════════════════════════════════════════════╣
║                                                                       ║
║  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                ║
║  │    AGENTS    │  │   COUNCIL    │  │     CASS     │                ║
║  │              │  │              │  │   (Skills)   │                ║
║  │ • Roles      │  │ • Debate     │  │              │                ║
║  │ • Drives     │  │ • Vote       │  │ • Harvest    │                ║
║  │ • Coherence  │  │ • Evidence   │  │ • Distill    │                ║
║  │ • Beliefs    │  │ • Decide     │  │ • Promote    │                ║
║  └──────────────┘  └──────────────┘  └──────────────┘                ║
║         │                 │                 │                         ║
║         └─────────────────┼─────────────────┘                         ║
║                           ▼                                           ║
║              ┌──────────────────────────┐                            ║
║              │   HYPERGRAPH MEMORY      │                            ║
║              │   (Stigmergic Field)     │                            ║
║              │                          │                            ║
║              │ • Nodes (beliefs)        │                            ║
║              │ • Hyperedges (emergent)  │                            ║
║              │ • Ontological tags       │                            ║
║              │ • Visibility scopes      │                            ║
║              └──────────────────────────┘                            ║
║                           │                                           ║
║         ┌─────────────────┼─────────────────┐                         ║
║         ▼                 ▼                 ▼                         ║
║  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                ║
║  │     DKS      │  │    SOCIAL    │  │  FEDERATION  │                ║
║  │              │  │    MEMORY    │  │              │                ║
║  │ • Selection  │  │              │  │ • Trust      │                ║
║  │ • Replication│  │ • Promises   │  │ • Conflict   │                ║
║  │ • Mutation   │  │ • Reputation │  │ • Sync       │                ║
║  │ • Flux       │  │ • Evidence   │  │ • Consensus  │                ║
║  └──────────────┘  └──────────────┘  └──────────────┘                ║
║                                                                       ║
╚═══════════════════════════════════════════════════════════════════════╝
```

---

## 📚 Documentation

| Document | What You'll Learn |
|----------|-------------------|
| [EASY_START.md](documentation/guides/EASY_START.md) | Get running in 5 minutes |
| [DEPLOYMENT.md](documentation/guides/DEPLOYMENT.md) | Production deployment guide |
| [COMMANDS_GUIDE.md](documentation/guides/COMMANDS_GUIDE.md) | CLI reference |
| [ANTIFRAGILE_ARCHITECTURE.md](documentation/architecture/ANTIFRAGILE_ARCHITECTURE.md) | System design deep-dive |
| [PERSONAL_AGENT_README.md](documentation/guides/PERSONAL_AGENT_README.md) | Your AI companion |
| [HERMES_INTEGRATION.md](documentation/integrations/HERMES_INTEGRATION.md) | Connect to Hermes Agent |

---

## 🐳 Docker Deployment

```bash
# Full stack with monitoring
docker-compose up -d

# Check health
curl http://localhost:8080/health

# View metrics
curl http://localhost:9000/metrics
```

Services:
- **HSM-II**: Main application (port 8080)
- **Ollama**: Local LLM inference (port 11434)
- **Prometheus**: Metrics (port 9090)
- **Grafana**: Dashboards (port 3000)

---

## 📊 Observability & Metrics

HSM-II exposes Prometheus metrics for monitoring:

| Metric | What It Tracks |
|--------|---------------|
| `hsm_coherence_growth` | Agent synchronization over time |
| `hsm_llm_requests_total` | LLM API call volume |
| `hsm_council_decisions_total` | Council voting patterns |
| `hsm_skills_harvested` | Skills learned from experience |
| `hsm_promises_kept_total` / `hsm_promises_broken_total` | Social memory integrity |

---

## 🧪 Testing

```bash
# Run all tests
cargo test

# Run library tests only
cargo test --lib

# Run with logging
RUST_LOG=debug cargo test
```

---

## 📁 Project Structure

```
HSM-II/
├── src/                    Core Rust implementation
│   ├── agent_core/         Agent runtime & lifecycle
│   ├── council/            Deliberation & voting
│   ├── tools/              62+ tool implementations
│   ├── llm/                LLM clients & failover
│   ├── dks/                Distributed knowledge
│   ├── cass/               Skill learning
│   ├── federation/         Multi-node coordination
│   └── gateways/           Discord, web, etc.
├── documentation/          Guides, architecture, reports
├── external_integrations/  Third-party connections (Hermes)
├── infrastructure/         Prometheus, Grafana, CI/CD
├── agent_tools/            Scripts & visual-explainer
├── web_interface/          Web UI & visualization
└── test_suite/             Integration tests
```

---

## 🤝 Hermes Agent Integration

HSM-II bridges to [Hermes Agent](https://github.com/NousResearch/hermes-agent) (by [NousResearch](https://github.com/NousResearch)) for extended capabilities:

```rust
use hermes_bridge::HermesClientBuilder;

let client = HermesClientBuilder::new()
    .endpoint("http://localhost:8000")
    .build()?;

let result = client.web_search("AI agents").await?;
```

---

## 🛣️ Roadmap

- [x] Core hypergraph memory engine
- [x] Multi-agent council system
- [x] 62+ real tools
- [x] Multi-provider LLM with failover
- [x] Docker deployment
- [x] Hermes Agent integration
- [x] Telegram bot
- [x] Job queue/scheduler
- [ ] Vector database integration
- [ ] Advanced web dashboard

---

## 📄 License

MIT License - see [LICENSE](LICENSE)

---

## 🙏 Acknowledgments

- Inspired by biological morphogenesis and stigmergic coordination in social insects
- Built with [Rust](https://rust-lang.org) and [Tokio](https://tokio.rs)
- Uses [Ollama](https://ollama.ai) for local inference

---

**Built by Permutation Research** 🔄
