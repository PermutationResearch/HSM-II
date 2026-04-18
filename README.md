# HSM-II — Company OS

[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

> **Every task your agents run makes the next one cheaper. The system compounds — it doesn't reset.**

Most AI agent setups are stateless pipelines. You run a task, it finishes, and everything learned evaporates. The next time you run the same kind of task, you start from zero.

HSM-II is different. It's a **multi-agent operating system** built in Rust where agents operate under a five-phase control loop, govern each other, track spend, and distill every execution into durable knowledge that gets reused automatically.

**[📄 Paper](./documentation/paper.pdf)** · **[🌐 Live Demo](https://permutationresearch.github.io/HSM-II/)** · **[🔄 Operating Loop](./docs/company-os/operating-loop.md)** · **[📋 Task SOP](./company-files/sop/task_lifecycle_sop.md)**

---

## What You Actually Get

**A company staffed by AI agents** — with a real org chart, budget controls, governance, and a quality gate that blocks shipping until sign-off is complete.

| What you have today | What you get with HSM-II |
|---------------------|--------------------------|
| Agents that forget everything after each run | Agents that distill every execution into reusable skills |
| No way to know if agent output is safe to ship | Sovereign Gate — a separate verifier blocks anything with open approvals |
| Costs you can't track or control | Per-agent spend tracking with hard-stop budgets |
| One agent doing everything | Specialists coordinated by a Council with Debate mode for high-stakes decisions |
| Manual re-runs when something breaks | Automatic Repair — failures re-enter the loop as new signals |

---

## The Operating Loop

Every unit of work — a task, a goal, a heartbeat, an incoming message — follows the same five-phase loop:

```
Signal → Frame → Execute → Gate → Compound
```

| Phase | What happens | Hard exit condition |
|-------|-------------|---------------------|
| **Signal** | Work enters; DRI assigned; duplicate check | Task has an owner, no duplicate open |
| **Frame** | Council challenges the approach before any code is written | Framing artifact exists; complexity assessed |
| **Execute** | Workers run in isolated git worktrees; spend tracked per agent | Deliverable produced; spend within budget |
| **Gate** | A *different* agent verifies; nothing ships with open approvals | Verifier signed off; `approvals_pending = 0` |
| **Compound** | Successful traces promoted to memory and versioned skills | Skill upserted; task closed as `completed` |

When any phase fails, **Repair** re-hydrates context from Postgres and re-enters with `repair: true`. Two consecutive failures auto-escalate to governance with `escalation_reason` set — visible in the operator console immediately.

Context is rebuilt from durable storage on every entry. The system does not rely on in-memory state surviving between runs.

---

## Company OS

The Company OS is a **Postgres-backed multi-tenant control plane** (~200 REST endpoints) that gives each company running on HSM-II its own isolated operating environment:

- **Org chart** — agents with roles, DRI assignments, reporting structure
- **Goals and tasks** — Postgres-backed, lifecycle-tracked, linked to capability refs
- **Heartbeats** — scheduled checks with persisted runtime state
- **Budget controls** — per-role monthly limits with hard-stop enforcement at task checkout
- **Governance log** — every approval, escalation, and phase failure recorded
- **Spend ledger** — grouped by kind and agent ref
- **Audit trail** — `memory/task_trail.jsonl` per company with full task history

### Company Packs

HSM-II ships with **18 pre-built company configurations** from the Paperclip/companies.sh ecosystem — engineering teams, research labs, creative studios, capital groups. Each pack includes agents, skills, SOPs, and a governance structure.

Packs are **auto-updated at point of use**: every time you load a company, HSM-II pulls the latest skill and agent files from the upstream GitHub source before reading the pack.

```
Signal comes in → company loaded → upstream files fetched → agents read → task runs
```

Install a pack, and the next time you run that company it's already up to date. No manual `paperclip install` step needed.

---

## How Agents Get Smarter Over Time

**CASS — Continuous Automated Skill Synthesis:**

1. **Harvest** — successful execution traces are captured
2. **Distill** — repeated patterns become named skills with slugs
3. **Gate** — skills pass a consensus jury before promotion
4. **Compound** — next agent that hits the same problem uses the distilled skill instead of reasoning from scratch

The result: the first time HSM-II solves a problem takes the full loop. The tenth time costs a fraction.

---

## Memory Architecture

HSM-II uses a **layered memory stack** — not a single store. Each layer has a different durability, scope, and purpose. Together they ensure that knowledge compounds and context survives crashes, restarts, and context window flushes.

### Layer 1 — Hypergraph World Model (in-process, snapshotted)

The primary in-process store. Agents share a **hypergraph** where edges can connect multiple nodes at once, enabling relational knowledge that a flat vector store can't represent.

- **Beliefs** — claims with confidence scores, supporting/contradicting evidence, ownership scopes (`local` / `shared` / `restricted`)
- **Hyperedges** — multi-participant connections with weights, provenance chains, and federation scope
- **Experiences** — timestamped execution records with L0/L1/L2 tiered abstractions
- **Stigmergic coordination** — agents detect each other's work through state changes, not direct messages

Persisted to `world_state.ladybug.bincode` via a write-ahead log with atomic rename. On crash, the WAL replays and the full state is rebuilt.

### Layer 2 — PostgreSQL (durable, company-scoped)

Every Company OS tenant has an isolated slice of Postgres. This is where task lifecycle state, memory entries, governance events, and spend live — and where context is rebuilt after any failure.

Key tables: `tasks`, `company_memory_entries` (scope: local/shared), `agent_runs`, `governance_events`, `spend_events`, `store_promotions`, `approvals`.

Memory entries have `summary_l0` and `summary_l1` fields — progressive summarization so agents query at the right detail level without loading full bodies.

### Layer 3 — Task Trail (append-only audit, per company)

`memory/task_trail.jsonl` — a JSONL file written after every turn. Never overwritten, only appended.

Each entry records: skills used, council mode, tool step count, token counts, world edge/belief deltas. This is the **context recovery surface** — when an agent re-enters after a crash or context flush, it reads the task trail to reconstruct what happened before.

### Layer 4 — Skills (versioned, hash-locked)

Skills distilled from execution live in two places: a `SkillBank` in-process (with semantic embeddings for retrieval) and a `skills-lock.json` with SHA-256 content hashes. The lock prevents in-flight mutations — if the hash changes, the skill was modified and must pass the Gate again before use.

Skill retrieval uses embedding similarity + context relevance + graph centrality across the CASS semantic graph. A skill promoted last week is available to every agent today.

### Layer 5 — Heartbeat State (scheduled task persistence)

`memory/heartbeat_state.json` — persists the last-run timestamp and status of every periodic check. On restart, the system immediately knows which heartbeats are overdue without scanning logs.

### Context Recovery — No Session State Required

The single biggest memory design decision: **context lives in Postgres, not in agent memory.**

When an agent loses its context window — crash, restart, or simple flush — it re-enters by calling:

```
GET /api/company/companies/:id/ops/overview
```

This returns the full operational picture: recent task trail, open governance events, heartbeat state, task counts by phase, current spend vs. budget. The agent reconstructs context **deterministically** from durable storage. Nothing is assumed to survive between runs.

This is how HSM-II handles the problem every long-running agent hits: context decay. The answer isn't a bigger context window. It's durable storage queried on demand.

### Memory Scoping

| Scope | Who sees it | Written by |
|-------|-------------|-----------|
| `local` | Agent or task only | Worker during execution |
| `shared` | All agents in the company | Promoted from local on task close |
| `restricted` | Governance-audited access | Escalation events, approvals |
| `federated` | Cross-company (if federation enabled) | Belief propagation engine |

---

## Council Deliberation

When decisions matter, agents form a Council:

| Mode | When it activates | How it works |
|------|-------------------|--------------|
| **Simple** | Low complexity, high urgency | Single agent with coherence check |
| **Orchestrate** | Medium complexity | Lead agent coordinates specialists |
| **Debate** | High stakes, contested approach | Full deliberation with evidence contracts and voting |

Every Frame phase runs through a Council before any code is written.

---

## Quick Start

### Prerequisites

- **Rust** — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **PostgreSQL** — for Company OS (the personal agent runs without it)
- An LLM: **Ollama** (local/free), **Anthropic Claude**, or **OpenAI GPT-4**

### Option A: Local with Ollama (no API key needed)

```bash
ollama pull llama3.2

git clone https://github.com/PermutationResearch/HSM-II.git
cd HSM-II
cargo run --bin personal_agent -- bootstrap
cargo run --bin personal_agent -- start
```

### Option B: Claude

```bash
git clone https://github.com/PermutationResearch/HSM-II.git
cd HSM-II
cargo run --bin personal_agent -- bootstrap
export ANTHROPIC_API_KEY="sk-ant-..."
cargo run --bin personal_agent -- start
```

### Option C: GPT-4 / OpenAI-compatible

```bash
git clone https://github.com/PermutationResearch/HSM-II.git
cd HSM-II
cargo run --bin personal_agent -- bootstrap
export OPENAI_API_KEY="sk-..."
cargo run --bin personal_agent -- start
```

The agent listens on **port 3000** (REST API) and **port 3847** (Company OS console API) by default.

---

## Company OS Console

The console API is embedded in `personal_agent`. Set `NEXT_PUBLIC_API_BASE=http://127.0.0.1:3847` to connect the web console.

**Core endpoints:**

| Endpoint | What it returns |
|----------|----------------|
| `GET /api/company/companies` | All companies with status |
| `GET /api/company/companies/:id/ops/overview` | Full ops snapshot: goals, tasks, budgets, heartbeats, spend, audit, governance |
| `POST /api/company/companies/:id/import-pack` | Load a company pack (triggers auto-fetch from upstream) |
| `GET /api/company/companies/:id/tasks` | Task list with lifecycle state |
| `POST /api/company/companies/:id/tasks/:tid/checkout` | Assign DRI and lock task for execution |

The `/ops/overview` endpoint is also the **context recovery surface** — when an agent loses session state, it re-enters by calling this endpoint before doing anything else. Context lives in Postgres, not in agent memory.

---

## Using Telegram (optional)

Telegram is one way to send signals into the loop — not the main interface.

1. Get a bot token from [@BotFather](https://t.me/BotFather)
2. Start the agent with the token:

```bash
TELEGRAM_TOKEN="your-token" cargo run --bin personal_agent -- start --telegram --daemon
```

Useful bot commands once running:

| Command | Effect |
|---------|--------|
| `/ralph <task>` | Code generation with worker-reviewer loop |
| `/rlm <text>` | Process large documents |
| `/model list` | Show available LLM models |
| `/tool list` | List all 60+ available tools |

---

## LLM Providers & Failover

| Provider | Models | Key variable |
|----------|--------|-------------|
| **Ollama** | Any local model (Llama, Mistral, Qwen…) | `OLLAMA_HOST`, `OLLAMA_PORT` |
| **Anthropic** | Claude 3.5 Sonnet, Claude 3 Opus | `ANTHROPIC_API_KEY` |
| **OpenAI** | GPT-4o, GPT-4o-mini | `OPENAI_API_KEY` |
| **OpenRouter** | Qwen3 Coder (free tier) | `OPENROUTER_API_KEY` |
| **Any compatible** | Groq, Together, Mistral… | `OPENAI_BASE_URL` |

If one provider fails, the system automatically tries the next. No single point of failure.

---

## YC-Bench Results

[YC-Bench](https://collinear-ai.github.io/yc-bench/) is a long-horizon deterministic benchmark for LLM agents. The agent operates a simulated AI startup for one simulated year, interacting exclusively through a CLI against a discrete-event simulation. It is designed to be hard — not a chatbot test, but a sustained decision-making challenge.

### What the simulation tests

The agent starts with **$200,000** and manages a company with 8 employees across the full year. It must:

- Accept and prioritize tasks from a client marketplace (4 domains: training, inference, research, data engineering)
- Assign the right employees to the right tasks — employee salaries grow with every assignment, so poor allocation compounds into cash flow problems
- Build client trust over time (reduces future work requirements, unlocks higher-reward tasks)
- Detect and avoid **adversarial "RAT" clients** — hidden bad actors who inflate work after acceptance and make deadlines nearly impossible, but offer high rewards to lure greedy agents
- Manage runway across hundreds of turns, with context truncated to 20 turns — the only memory persistence is a scratchpad injected into the system prompt

**Why it's hard:** payroll grows monotonically, context truncates after 20 turns, and adversarial clients are invisible until you've already failed them. Most agents either burn payroll on inactivity, get trapped by RAT clients, or run out of context and lose state.

### Our runs

We ran all 18 HSM-II company packs against the medium preset (1-year horizon, seeds 1–10) using **Qwen3.6-plus:free** via OpenRouter — the free tier, zero cost per run. 186 total runs.

| Rank | Company pack | Runs | Avg final funds | Peak run | Completed full year |
|------|-------------|------|-----------------|----------|---------------------|
| 1 | agency-agents | 13 | $969,389 | $1,189,275 | 3 / 13 |
| 2 | aeon-intelligence | 13 | $780,582 | $1,059,497 | 3 / 13 |
| 3 | kdense-science-lab | 10 | $702,146 | $702,146 | 0 / 10 |
| 4 | agentsys-engineering | 10 | $694,972 | $1,043,654 | 3 / 10 |
| 5 | apex-systems | 10 | $675,028 | $1,011,137 | 4 / 10 |
| 6 | clawteam-capital | 10 | $602,584 | $1,075,768 | 1 / 10 |
| 7 | fullstack-forge | 10 | $497,415 | $503,578 | 0 / 10 |
| 8 | trail-of-bits-security | 10 | $419,801 | $430,911 | 0 / 10 |
| 9 | compound-engineering-co | 10 | $380,082 | $383,570 | 0 / 10 |
| 10 | redoak-review | 10 | $362,659 | $397,328 | 0 / 10 |
| 11 | gstack | 10 | $319,912 | $324,500 | 0 / 10 |
| 12 | clawteam-engineering | 10 | $318,626 | $360,086 | 0 / 10 |
| 13 | minimax-studio | 10 | $303,145 | $313,101 | 0 / 10 |
| 14 | clawteam-research-lab | 10 | $264,819 | $264,819 | 0 / 10 |

Starting capital: $200,000. Average profit across all 186 runs: **+$249,509** (+125% ROI).  
Best single run: agency-agents seed 1 — **$1,189,275** (5.9× starting capital, 976 turns).

### What drives the differences

Each company pack includes a system prompt built from that company's `VISION.md`, agent briefings, and skill descriptions. The benchmark is measuring whether a company's documented operating philosophy, skill set, and context actually translates into better decision-making under adversarial conditions.

Packs that perform well tend to have explicit guidance on: client vetting, task prioritization, employee efficiency, and cash flow management. Packs with generic or thin context tend to get trapped by RAT clients or go idle waiting for tasks.

The "Completed full year" column is the hardest bar. Most runs end with `terminal_reason: error` — the model gets stuck in a loop after repeated tool failures. Only 14 out of 186 runs reached the horizon end. Agency-agents, apex-systems, agentsys-engineering, aeon-intelligence, and clawteam-capital are the packs whose context helped the agent sustain coherent decision-making for the full simulation.

### Running the benchmark yourself

```bash
export OPENROUTER_API_KEY=sk-or-v1-...

# Single seed across all 18 packs
cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed7.json

# Full grid (seeds 7-10)
for seed in 7 8 9 10; do
  cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed${seed}.json
done
```

Results write to `runs/external_batch_<timestamp>.json` and aggregate via `GET /api/companies-sh/yc-bench`.

---

## Built-In Tool Suite (60+ tools)

| Category | Capabilities |
|----------|-------------|
| **Web & Browser** | Search, scrape, automate, read PDFs |
| **File Operations** | Read, write, search, analyze any file type |
| **Shell & System** | Execute commands, gather system info |
| **Git** | Clone, commit, diff, blame, search |
| **APIs & Data** | HTTP requests, JSON, encoding |
| **Text Processing** | Regex, parse, format, diff |

All tools are real implementations — agents can actually modify files, browse the web, and run commands.

---

## Architecture

Five living layers over one shared world model:

```
┌─────────────────────────────────────┐
│  Federation layer (trust, sync)     │
├─────────────────────────────────────┤
│  Intelligence layer (Paperclip IL)  │
├─────────────────────────────────────┤
│  Execution layer (workers, tools)   │
├─────────────────────────────────────┤
│  Reasoning layer (council, CASS)    │
├─────────────────────────────────────┤
│  World model (hypergraph, Postgres) │
└─────────────────────────────────────┘
```

Machine-readable blueprint: [`architecture/hsm-ii-blueprint.ron`](architecture/hsm-ii-blueprint.ron)  
Generated reference: [`ARCHITECTURE.generated.md`](ARCHITECTURE.generated.md) (verified by `cargo test --lib`)

---

## Security

- **API Key Management** — Argon2-hashed, revocable
- **JWT Authentication** — 24-hour expiring tokens
- **Rate Limiting** — per-key quota enforcement
- **Permission Levels** — Read / Write / Admin
- **Spend Hard-Stops** — budget limits enforced at task checkout; agent cannot proceed past limit

---

## Documentation

| Doc | What it covers |
|-----|----------------|
| [Operating Loop](./docs/company-os/operating-loop.md) | Phase definitions, entry conditions, exit artifacts |
| [Task Lifecycle SOP](./company-files/sop/task_lifecycle_sop.md) | API-level walkthrough of every phase |
| [Ops Overview API](./docs/company-os/ops-overview-api.md) | The unified ops endpoint and context recovery |
| [EASY_START.md](documentation/guides/EASY_START.md) | Get running in 5 minutes |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Curated blueprint notes |
| [EVAL_AND_META_HARNESS.md](docs/EVAL_AND_META_HARNESS.md) | Benchmarking and harness tooling |

---

## License

MIT — see [LICENSE](LICENSE)

---

**Built by Permutation Research**
