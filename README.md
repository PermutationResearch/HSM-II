# Hyper-Stigmergic Morphogenesis II (HSM-II)

[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> **Where swarms of AI agents think together, learn from each other, and grow smarter over time.**

HSM-II is a **federated multi-agent hypergraph system** that brings emergent collective intelligence to life. Built in Rust, it combines:

🧠 **Hypergraph Memory** — A living knowledge web where agents leave "trails" (stigmergy) for others to follow  
⚖️ **Councils That Actually Deliberate** — Dynamic agent assemblies that debate, vote, and decide collectively  
🎓 **Self-Improving Skills** — Agents distill and share what they learn, like a hive mind getting smarter  
🌐 **Federated Trust** — Multiple HSM-II instances that sync, negotiate, and resolve conflicts

Think of it as *ants solving problems through pheromone trails* — except the ants are LLM-powered agents, the trails are hypergraph edges, and the colony learns to code, research, and coordinate autonomously.

**[📄 Read the Paper](https://github.com/PermutationResearch/HSM-II/blob/main/paper.pdf)** | **[🚀 Quick Start](#-quick-start)** | **[🌐 Live Demo](https://permutationresearch.github.io/HSM-II/)**

---

## 🚀 Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) 1.75+ 
- Docker (optional, for containerized deployment)
- API key for OpenAI, Anthropic, or local Ollama
- **macOS users**: Use the `.command` scripts in `scripts/macos/`
- **Linux users**: Use equivalent `cargo run` commands (see [docs/guides/COMMANDS_GUIDE.md](docs/guides/COMMANDS_GUIDE.md))

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
./scripts/macos/run-personal-agent.command
```
Your AI companion with built-in coordination capabilities.

#### 2. With Visualization
```bash
# Terminal 1: Personal agent
./scripts/macos/run-personal-agent.command

# Terminal 2: Visual hypergraph
./scripts/macos/open-hypergraphd.command
```

#### 3. Full Research Stack
```bash
# Terminal 1: Research backend
./scripts/macos/run-hyper-stigmergy-II.command

# Terminal 2: Personal agent
./scripts/macos/run-personal-agent.command --connect-hypergraph

# Browser: View hypergraph
./scripts/macos/open-hypergraphd.command
```

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         HSM-II System                           │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
│  │   Agents    │  │   Council   │  │    CASS (Skills)        │ │
│  │  ┌───────┐  │  │  ┌───────┐  │  │  ┌─────────────────┐    │ │
│  │  │Roles  │  │  │  │Debate │  │  │  │Skill Learning   │    │ │
│  │  │Drives │  │  │  │Vote   │  │  │  │Distillation     │    │ │
│  │  │Coherence│ │  │  │Evidence│ │  │  │Versioning       │    │ │
│  │  └───────┘  │  │  └───────┘  │  │  └─────────────────┘    │ │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘ │
│         │                │                    │                 │
│         └────────────────┼────────────────────┘                 │
│                          ▼                                      │
│              ┌─────────────────────┐                            │
│              │   Stigmergic Field  │                            │
│              │  (Hypergraph State) │                            │
│              └─────────────────────┘                            │
│                          │                                      │
│         ┌────────────────┼────────────────┐                     │
│         ▼                ▼                ▼                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │
│  │    DKS      │  │   Social    │  │   Federation│             │
│  │(Distributed │  │   Memory    │  │   (Multi-   │             │
│  │ Knowledge)  │  │ (Promises,  │  │   Node)     │             │
│  │             │  │ Reputation) │  │             │             │
│  └─────────────┘  └─────────────┘  └─────────────┘             │
└─────────────────────────────────────────────────────────────────┘
```

---

## ✨ Key Features

### 🧠 Core Systems

| System | Description |
|--------|-------------|
| **Hypergraph Engine** | Stigmergic morphogenesis through environmental markers |
| **Agent System** | Role-based agents with coherence scoring |
| **Council System** | Multi-agent debate, evidence-based voting, mode switching |
| **CASS** | Continuous Automated Skill Synthesis - learn and distill skills |
| **Social Memory** | Promise tracking, reputation, capability evidence |
| **DKS** | Distributed Knowledge System with selection pressure |

### 🛠️ Tools (57 Real Implementations)

| Category | Tools |
|----------|-------|
| Web/Browser | Web search, browser automation, scraping |
| File Operations | Read, write, search, analyze files |
| Shell/System | Execute commands, system info |
| Git | Clone, commit, diff, blame, search |
| API/Data | HTTP requests, JSON parsing, encoding |
| Calculations | Math, statistics, unit conversion |
| Text Processing | Regex, parsing, formatting |

### 🤖 LLM Integration

- **OpenAI** (GPT-4o, GPT-4o-mini)
- **Anthropic** (Claude models)
- **Ollama** (local models)
- **Automatic failover** between providers
- **Health monitoring** and retry logic

### 🔐 Security & Auth

- API key management with Argon2 hashing
- JWT tokens with 24h expiry
- Rate limiting per key
- Permission-based access control

---

## 📚 Documentation

| Document | Description |
|----------|-------------|
| [EASY_START.md](docs/guides/EASY_START.md) | Getting started guide |
| [DEPLOYMENT.md](docs/guides/DEPLOYMENT.md) | Production deployment |
| [COMMANDS_GUIDE.md](docs/guides/COMMANDS_GUIDE.md) | CLI commands reference |
| [IMPLEMENTATION_STATUS.md](docs/reports/IMPLEMENTATION_STATUS.md) | Feature completeness |
| [ANTIFRAGILE_ARCHITECTURE.md](docs/architecture/ANTIFRAGILE_ARCHITECTURE.md) | Architecture deep-dive |
| [PERSONAL_AGENT_README.md](docs/guides/PERSONAL_AGENT_README.md) | Personal agent guide |
| [HERMES_INTEGRATION.md](docs/integrations/HERMES_INTEGRATION.md) | Hermes bridge docs |

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
hyper-stigmergy/
├── src/
│   ├── bin/                 # Executables
│   │   ├── personal_agent.rs
│   │   ├── hypergraphd.rs
│   │   └── ...
│   ├── agent_core/          # Agent runtime
│   ├── council/             # Council decision-making
│   ├── tools/               # 57 tool implementations
│   ├── llm/                 # LLM client & providers
│   ├── dks/                 # Distributed knowledge
│   ├── cass/                # Skill learning
│   ├── federation/          # Multi-node federation
│   ├── gateways/            # Discord, Telegram, etc.
│   └── ...
├── hermes-bridge/           # Hermes integration
├── scripts/                 # Python analysis scripts
├── static/                  # Web UI
├── config/                  # Prometheus/Grafana config
├── tests/                   # Integration tests
├── Cargo.toml
├── docker-compose.yml
└── Dockerfile
```

---

## 🤝 Integration: Hermes Bridge

HSM-II includes a bridge to [Hermes Agent](https://github.com/NousResearch/hermes-agent) (by [NousResearch](https://github.com/NousResearch)) for extended capabilities:

```rust
use hermes_bridge::HermesClientBuilder;

let client = HermesClientBuilder::new()
    .endpoint("http://localhost:8000")
    .build()?;

let result = client.web_search("AI agents").await?;
```

See [hermes-bridge/README.md](hermes-bridge/README.md) for details.

---

## 📊 Metrics & Observability

Prometheus metrics available at `:9000/metrics`:

- `hsm_http_requests_total` - HTTP requests
- `hsm_llm_requests_total` - LLM API calls
- `hsm_llm_latency_milliseconds` - LLM response times
- `hsm_tool_executions_total` - Tool usage
- `hsm_failures_total` - Failed operations
- `hsm_promises_kept_total` / `hsm_promises_broken_total` - Promise tracking

---

## 🛣️ Roadmap

- [x] Core hypergraph engine
- [x] Multi-agent council system
- [x] 57 real tools
- [x] Multi-provider LLM integration
- [x] Docker deployment
- [x] Discord gateway
- [ ] Telegram/Slack gateways
- [ ] Vector database integration
- [ ] Job queue/scheduler
- [ ] Advanced web UI

---

## 📄 License

MIT License - see [LICENSE](LICENSE)

---

## 🙏 Acknowledgments

- Inspired by biological morphogenesis and stigmergic coordination in social insects
- Built with [Rust](https://rust-lang.org) and [Tokio](https://tokio.rs)
- Uses [Ollama](https://ollama.ai) for local inference

---

## 💬 Support

- Issues: [GitHub Issues](https://github.com/PermutationResearch/hyper-stigmergy/issues)
- Discussions: [GitHub Discussions](https://github.com/PermutationResearch/hyper-stigmergy/discussions)

---

**Built by Permutation Research** 🔄
