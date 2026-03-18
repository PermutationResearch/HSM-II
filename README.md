# HSM-II — Hyper-Stigmergic Morphogenesis II

[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A multi-agent AI orchestration system built in Rust that coordinates autonomous agents through stigmergic signaling, council-based governance, and adaptive learning. Agents collaborate on complex tasks — code generation, investigation, optimization — using local LLMs (Ollama) with optional cloud provider fallback.

## What it does

- **Multi-agent orchestration**: Spawns and coordinates autonomous AI agents that communicate through a shared stigmergic field (indirect coordination via environmental signals)
- **Coder assistant**: An AI-powered coding agent with tool execution (read/write/edit/bash/grep), macOS sandbox enforcement, MCP tool providers, and WASM capability isolation
- **Council governance (Ouroboros)**: A 5-phase security gate where multiple AI "council members" vote on risky operations before allowing execution
- **Investigation engine**: Deep research workflows that decompose complex questions into multi-hop investigations
- **Federated networking**: Agents can form teams across network boundaries using Axum-based federation endpoints
- **TUI interfaces**: Terminal dashboards for monitoring agent activity, team coordination, and code sessions

## Architecture

```
src/
├── main.rs                    # Primary binary (11K+ lines) — CLI, server, orchestration entry
├── config.rs                  # Centralized constants (network, timeouts, limits, thresholds)
├── lib.rs                     # Module declarations and re-exports
│
├── agent_core/                # Core agent lifecycle, message loop, LLM config
├── coder_assistant/           # Coding agent with tool execution
│   ├── agent_loop.rs          #   Agent message loop and session management
│   ├── tools.rs               #   Tool registry and execution policy
│   ├── tool_executor.rs       #   Core dispatch: builtin vs external providers
│   ├── security_policy.rs     #   Ouroboros gate, secret boundary, audit trail
│   ├── builtin_tools.rs       #   read/write/edit/bash/grep/find/ls implementations
│   ├── external_providers.rs  #   MCP (HTTP JSON-RPC) and WASM tool providers
│   ├── sandbox.rs             #   macOS sandbox-exec enforcement
│   ├── schemas.rs             #   Tool/provider type definitions
│   ├── renderer.rs            #   Output formatting
│   ├── session.rs             #   Session persistence
│   └── streaming.rs           #   Streaming response handling
│
├── council/                   # Multi-agent deliberation and voting
├── ouroboros_compat/          # 5-phase governance gate (propose → deliberate → vote → execute → audit)
├── investigation/             # Multi-hop research engine
├── investigation_engine.rs    # Investigation orchestration
│
├── rlm.rs                     # Reinforcement Learning Module v1 (bidding, embeddings)
├── rlm_v2/                    # RLM v2 with LLM bridge
├── optimize_anything/         # Generic optimization framework
├── mirofish.rs                # Trust calibration and Bayesian scoring
│
├── federation/                # Cross-network agent communication (Axum)
├── communication/             # Inter-agent messaging
├── scheduler/                 # Cron-based task scheduling
│
├── llm/                       # Multi-provider LLM client (OpenAI, Anthropic, Ollama)
├── ollama_client.rs           # Ollama-specific client with model resolution
├── pi_ai_compat/              # Model compatibility layer
│
├── personal/                  # Personal agent with memory and personality
├── dream/                     # Dream-state processing (offline learning)
├── social_memory.rs           # Social interaction memory
│
├── hypergraph.rs              # Hypergraph data structure
├── embedded_graph_store.rs    # On-disk graph persistence
├── property_graph.rs          # Property graph implementation
├── hnsw_index.rs              # HNSW approximate nearest neighbor index
├── disk_backed_vector_index.rs # Disk-backed vector storage
│
├── auth.rs                    # JWT + Argon2 authentication
├── vault.rs                   # Secret management
├── gateways/                  # Discord and Telegram bot integrations
├── gpu/                       # Optional wgpu acceleration
│
└── bin/                       # Additional binary entry points
    ├── agentd.rs              #   Agent daemon
    ├── conductord.rs          #   Conductor daemon (team orchestration)
    ├── hypergraphd.rs         #   Hypergraph server
    ├── teamd.rs               #   Team daemon
    ├── ouroboros_gate.rs      #   Standalone governance gate CLI
    ├── investigate.rs         #   Investigation CLI
    ├── personal_agent.rs      #   Personal agent CLI
    ├── batch_experiment.rs    #   Batch experiment runner
    └── tui_codex_demo.rs      #   TUI demonstration
```

## Prerequisites

- **Rust 1.75+** (2021 edition)
- **Ollama** running locally on port 11434 (or set `OLLAMA_URL`)
- Optional: OpenAI or Anthropic API keys for cloud LLM fallback
- Optional: MySQL/MariaDB for RooDB persistence (defaults to embedded storage)

## Quick start

```bash
# Clone and build
git clone <repo-url>
cd hyper-stigmergic-morphogenesisII
cp .env.example .env
# Edit .env with your configuration

# Build all binaries
cargo build --release

# Run the primary binary (includes CLI, server modes, coder assistant)
cargo run --release

# Run specific daemons
cargo run --release --bin agentd
cargo run --release --bin conductord
cargo run --release --bin hypergraphd
```

## Environment variables

Copy `.env.example` to `.env` and configure:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `OLLAMA_URL` | No | `http://localhost:11434` | Ollama API endpoint |
| `OLLAMA_MODEL` | No | auto-detect | Override default model selection |
| `OPENAI_API_KEY` | No | — | OpenAI API key for cloud fallback |
| `ANTHROPIC_API_KEY` | No | — | Anthropic API key for cloud fallback |
| `DEFAULT_LLM_MODEL` | No | `gpt-4o-mini` | Default model when using cloud providers |
| `RUST_LOG` | No | `info` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |
| `DATABASE_URL` | No | SQLite | Database connection string |
| `HSM_MODE` | No | `production` | Runtime mode (`development`, `staging`, `production`) |
| `HSM_DATA_DIR` | No | `./data` | Data storage directory |
| `BROWSERBASE_API_KEY` | No | — | Browser automation API key |
| `CF_ACCOUNT_ID` | No | — | Cloudflare account for web search |
| `CF_API_TOKEN` | No | — | Cloudflare API token |
| `GRAFANA_PASSWORD` | No | `admin` | Grafana dashboard password |

## Key subsystems

### Coder Assistant

An AI coding agent that can read, write, edit files, execute shell commands, and use external tools:

```bash
cargo run --release -- coder  # Enter coder assistant mode
```

- **Built-in tools**: `read_file`, `write_file`, `edit_file`, `bash`, `grep`, `find_files`, `ls`
- **Security**: macOS `sandbox-exec` enforcement, secret boundary detection, network allowlists
- **External tools**: MCP providers (HTTP JSON-RPC), WASM capability-isolated tools
- **Governance**: Ouroboros 5-phase gate for dangerous operations (self-modification, network access)

### Ouroboros Governance Gate

A council of AI agents that vote on risky operations:

```bash
cargo run --release --bin ouroboros_gate -- \
  --action "modify core config" \
  --confidence-threshold 0.70
```

Phases: Propose → Deliberate → Vote → Execute → Audit

### Investigation Engine

Multi-hop research that decomposes complex questions:

```bash
cargo run --release --bin investigate -- "How does X affect Y?"
```

### Federation

Agents on different machines can form teams:

```bash
cargo run --release --bin conductord -- --port 8080  # Start conductor
cargo run --release --bin agentd -- --conductor http://localhost:8080  # Join
```

## Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Without GPU support
cargo build --release --no-default-features

# Run tests
cargo test

# Run specific test
cargo test test_name

# Check without building
cargo check

# Lint
cargo clippy
```

## Configuration constants

All magic numbers and thresholds are centralized in `src/config.rs`:

- `config::network` — URLs, ports, endpoints
- `config::timeouts` — execution and HTTP timeouts
- `config::limits` — file sizes, output lengths, line counts
- `config::thresholds` — confidence, coherence, trust scores
- `config::algorithm` — RLM parameters, agent counts, embedding dimensions
- `config::security` — secret patterns, suspicious markers, dangerous commands
- `config::paths` — default file paths
- `config::models` — default LLM model names

## License

MIT — see [LICENSE](LICENSE) for details.
