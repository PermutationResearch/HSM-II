# Hyper-Stigmergic Morphogenesis II (HSM-II)

[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> **Where swarms of AI agents think together, learn from each other, and grow smarter over time.**

HSM-II is a **federated multi-agent hypergraph system** that brings emergent collective intelligence to life. Built in Rust, it enables autonomous AI agents to coordinate without central control, learn from collective experience, and solve complex problems through shared knowledge structures.

Think of it as *ants solving problems through pheromone trails* — except the ants are LLM-powered agents, the trails are hypergraph edges, and the colony learns to code, research, and coordinate autonomously.

**[📄 Read the Paper](./documentation/paper.pdf)** | **[🚀 Quick Start](#-quick-start)** | **[🌐 Live Demo](https://permutationresearch.github.io/HSM-II/)**

---

## 🚀 Quick Start

### Step 1: Create Your Telegram Bot (2 minutes)

You need a Telegram bot token before anything else:

1. Open Telegram and search for **[@BotFather](https://t.me/BotFather)**
2. Send `/newbot`
3. Choose a name (e.g. "My HSM-II Bot") and a username (e.g. `my_hsmii_bot`)
4. BotFather gives you a token like `7123456789:AAF1k...` — **save this, you'll need it below**

---

### Step 2: Install Rust

**macOS / Linux:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

**Windows:**
Download and run [rustup-init.exe](https://rustup.rs/), then restart your terminal.

Verify it worked:
```bash
rustc --version
```

---

### Step 3: Choose Your LLM → Clone → Run

Pick **one** option below. All three end with a working Telegram bot.

#### Option A: Local with Ollama (Free, Private — your data stays on your machine)

**Install Ollama:**

| Platform | Command |
|----------|---------|
| macOS | `brew install ollama` |
| Linux | `curl -fsSL https://ollama.com/install.sh \| sh` |
| Windows | Download from [ollama.com/download](https://ollama.com/download) |

**Then run these commands:**
```bash
# Start Ollama and pull a model (pick any — it auto-detects)
ollama pull llama3.2

# Clone, bootstrap, and start the bot
git clone https://github.com/PermutationResearch/HSM-II.git
cd HSM-II
cargo run --bin personal_agent -- bootstrap
TELEGRAM_TOKEN="PASTE_YOUR_TOKEN_HERE" cargo run --bin personal_agent -- start --telegram --daemon
```

> 💡 **Note:** Ollama usually starts automatically after install. If you get a connection error, run `ollama serve` first.

---

#### Option B: Claude (Anthropic API)

Get your API key from [console.anthropic.com](https://console.anthropic.com/)

```bash
git clone https://github.com/PermutationResearch/HSM-II.git
cd HSM-II
cargo run --bin personal_agent -- bootstrap

export ANTHROPIC_API_KEY="sk-ant-PASTE_YOUR_KEY_HERE"
TELEGRAM_TOKEN="PASTE_YOUR_TOKEN_HERE" cargo run --bin personal_agent -- start --telegram --daemon
```

---

#### Option C: GPT-4 (OpenAI API) or Any OpenAI-Compatible API

Get your API key from [platform.openai.com](https://platform.openai.com/)

```bash
git clone https://github.com/PermutationResearch/HSM-II.git
cd HSM-II
cargo run --bin personal_agent -- bootstrap

export OPENAI_API_KEY="sk-PASTE_YOUR_KEY_HERE"
TELEGRAM_TOKEN="PASTE_YOUR_TOKEN_HERE" cargo run --bin personal_agent -- start --telegram --daemon
```

**Using Groq, Together, Mistral, or another OpenAI-compatible provider?** Just add the base URL:
```bash
export OPENAI_BASE_URL="https://api.groq.com/openai/v1"
```

---

### Step 4: Talk to Your Bot

Open Telegram, find your bot, and send it a message. That's it.

**Useful commands inside the chat:**

| Command | What it does |
|---------|-------------|
| `/model list` | Show available LLM models |
| `/model claude` | Switch to Claude |
| `/model gpt-4` | Switch to GPT-4 |
| `/ralph <task>` | Code generation with worker-reviewer loop |
| `/rlm <text>` | Process large documents |
| `/tool list` | Show available tools (60+) |
| `/tool <name> <args>` | Run a specific tool |

---

### Environment Variables Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `TELEGRAM_TOKEN` | *(required)* | Your Telegram bot token from BotFather |
| `OLLAMA_HOST` | `http://localhost` | Ollama server address |
| `OLLAMA_PORT` | `11434` | Ollama server port |
| `OLLAMA_MODEL` | `auto` (detects installed) | Force a specific Ollama model |
| `OPENROUTER_API_KEY` | *(optional)* | For `qwencoder:480b-cloud` – routes to OpenRouter (Qwen3 Coder free tier) |
| `ANTHROPIC_API_KEY` | *(optional)* | Anthropic API key for Claude |
| `OPENAI_API_KEY` | *(optional)* | OpenAI API key for GPT-4 |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | Custom OpenAI-compatible endpoint |

### Troubleshooting

| Problem | Fix |
|---------|-----|
| `cargo: command not found` | Run `source ~/.cargo/env` or restart your terminal |
| `Cannot reach Ollama` | Run `ollama serve` to start it manually |
| `No models found in Ollama` | Run `ollama pull llama3.2` (or any model) |
| Bot doesn't respond | Check the terminal for errors; make sure `TELEGRAM_TOKEN` is correct |
| Want to start fresh | `rm -f world_state.ladybug*.bincode ~/.hsmii/config.json` then `cargo run --bin personal_agent -- bootstrap` |

### Other Ways to Run

#### With Visualization Dashboard
```bash
cargo run --bin personal_agent -- start --telegram &
cargo run --bin hypergraphd
```

#### Full Research Stack
```bash
cargo run --release &
cargo run --bin personal_agent -- start --telegram &
cargo run --bin hypergraphd
```

#### External Rust Harnesses
`hsm_outer_loop` can now build and run external Rust harnesses from JSON specs, including checked-out side projects such as `claw-code`.

```bash
cargo run --bin hsm_outer_loop -- external-batch --spec config/external_claw_code.example.json
```

The external spec supports:
- `labels`: structured metadata for later comparison (`company_pack`, `preset`, `seed`, `benchmark`)
- `setup_commands`: run build steps before the benchmark command
- `cwd`: run inside the external repo workspace
- `env`: inject per-harness environment variables

Example: point [`external_claw_code.example.json`](/Users/cno/hyper-stigmergic-morphogenesisII/config/external_claw_code.example.json) at your local `claw-code/rust` checkout, then let `hsm_outer_loop` build `claw-cli` and smoke-test the release binary inside the harness pipeline.

For long-horizon startup stress tests, [`external_yc_bench.example.json`](/Users/cno/hyper-stigmergic-morphogenesisII/config/external_yc_bench.example.json) shows how to run `yc-bench` and tag the result with `company_pack`, `preset`, and `seed` so you can compare marketplace companies using the same scenario.

Full marketplace grids (18 Paperclip-class packs, `hsm_market_*`, medium preset) live in `config/external_yc_bench_seed7.json` … `seed10.json`. Edit each file’s `command` (path to `uv`), `cwd` (your local `yc-bench` checkout), and export `OPENROUTER_API_KEY` in the shell (`env` in those specs is empty so the child inherits your environment). Then run, for example:

```bash
export OPENROUTER_API_KEY=sk-or-v1-...
cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed9.json
cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed10.json
```

Results are written under `runs/external_batch_<timestamp>.json` and picked up by the company console `GET /api/companies-sh/yc-bench` aggregator.

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

HSM-II is documented as **one world model** with **five living layers** (world, reasoning, execution, intelligence, federation). The machine-readable blueprint lives in [`architecture/hsm-ii-blueprint.ron`](architecture/hsm-ii-blueprint.ron); curated notes and commands are in [**ARCHITECTURE.md**](ARCHITECTURE.md). The **exact** Markdown emitted by `blueprint_markdown()` is checked in as [`ARCHITECTURE.generated.md`](ARCHITECTURE.generated.md) and verified by `cargo test --lib`—regenerate with `./scripts/generate-architecture-md.sh` after RON edits. **GET** `/api/architecture` returns that blueprint plus optional runtime counts when the API has a mounted world. The thin dashboard at `web/` includes **`/architecture`** (server fetch to `HSM_API_URL`). Generate a report locally with `cargo run -q --bin hsm_archviz` (from the repo root).

---

## 📚 Documentation

| Document | What You'll Learn |
|----------|-------------------|
| [EASY_START.md](documentation/guides/EASY_START.md) | Get running in 5 minutes |
| [DEPLOYMENT.md](documentation/guides/DEPLOYMENT.md) | Production deployment guide |
| [COMMANDS_GUIDE.md](documentation/guides/COMMANDS_GUIDE.md) | CLI reference |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Curated blueprint notes (Mermaid + API links) |
| [ARCHITECTURE.generated.md](ARCHITECTURE.generated.md) | Generated from RON; must match `cargo test --lib` |
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
