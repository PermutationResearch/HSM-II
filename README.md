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

## YC-Bench: Can an AI Agent Run a Profitable Startup for a Full Year?

> **186 runs. 18 AI companies. 1 free model. $0 compute cost.**
> Every single company turned a profit. The best run returned **5.9× the starting capital**.
> The only variable between top and bottom: **the quality of the company context**.

---

### The Benchmark

[YC-Bench](https://collinear-ai.github.io/yc-bench/) drops an LLM agent into a discrete-event business simulation and forces it to make hundreds of real decisions — hire employees, accept client contracts, manage cash, detect bad actors — across a full simulated year, with no human in the loop.

**This is not a coding test or a Q&A benchmark.** It tests whether an agent can maintain coherent strategy across hundreds of turns, under adversarial conditions, with a hard memory limit. That's the same problem HSM-II is built to solve.

---

### What Makes This Hard

The simulation starts the agent with **$200,000** and **8 employees**. By the end of the year it either has more money or it's bankrupt. Four mechanics make this brutally difficult:

| Mechanic | What it does | How agents die |
|----------|-------------|----------------|
| **Payroll escalation** | Every completed task raises every assigned employee's salary. Assign all 8 to everything and monthly payroll grows **2.7×** faster than selective assignment. | Payroll outpaces revenue → insolvent by month 3 |
| **RAT clients (35% of market)** | Hidden adversarial clients inflate task scope after acceptance, guaranteeing deadline failure and a 35% penalty + prestige hit | Accept 3 RAT tasks → trust collapses → capacity halved → death spiral |
| **Trust multiplier** | Working with trusted clients reduces their work quantity by up to **50%** for the same reward. Spread too thin → no trust → no efficiency gains | Chasing every client → 0 trust depth → constant high-effort tasks |
| **20-turn context window** | The agent's memory is wiped every 20 turns. Client history, payroll notes, strategy — all gone unless persisted externally | Re-hires fired employees, re-accepts blocked RAT clients, loses strategy mid-game |

The context window mechanic is the direct reason HSM-II exists. Company context — VISION, agent briefings, decision heuristics — is injected into every turn, surviving the truncation. That's what we benchmarked.

---

### What We Tested

We ran **18 different company packs** through the same simulation: same model, same seeds, same parameters. The only difference between each run was the **company context** — the VISION.md, agent role definitions, and decision heuristics loaded into the system prompt.

**The question:** does company-specific context actually change how an agent behaves across hundreds of turns, or is it noise?

**The model:** `Qwen3.6-plus:free` via OpenRouter — free tier, $0.00 per run, $0.00 total across all 186 runs.

---

### Results — 186 Runs · 18 Company Packs · Seeds 1–10

**Starting capital: $200,000 · Duration: 1 simulated year · Model: Qwen3.6-plus:free (cost: $0)**

| # | Company Pack | Runs | 🏁 Full Year | Avg Return | Best Run | Avg ROI |
|---|-------------|------|-------------|------------|----------|---------|
| 🥇 1 | **apex-systems** | 10 | **4 / 10** | $607K | $1.01M | **3.0×** |
| 🥈 2 | **agency-agents** | 13 | **3 / 13** | $564K | $1.19M | **2.8×** |
| 🥉 3 | **kdense-science-lab** | 10 | 0 / 10 | $532K | $980K | **2.7×** |
| 4 | **product-compass-consulting** | 10 | 0 / 10 | $455K | $808K | 2.3× |
| 5 | **donchitos-game-studio** | 10 | 0 / 10 | $437K | $934K | 2.2× |
| 6 | **clawteam-capital** | 10 | **1 / 10** | $427K | $1.08M | 2.1× |
| 7 | **aeon-intelligence** | 13 | **3 / 13** | $422K | $1.06M | 2.1× |
| 8 | **superpowers** | 10 | 0 / 10 | $412K | $975K | 2.1× |
| 9 | **agentsys-engineering** | 10 | **3 / 10** | $406K | $1.04M | 2.0× |
| 10 | **redoak-review** | 10 | 0 / 10 | $373K | $801K | 1.9× |
| 11 | **clawteam-research-lab** | 10 | 0 / 10 | $360K | $1.06M | 1.8× |
| 12 | **trail-of-bits-security** | 10 | 0 / 10 | $323K | $581K | 1.6× |
| 13 | **fullstack-forge** | 10 | 0 / 10 | $300K | $687K | 1.5× |
| 14 | **clawteam-engineering** | 10 | 0 / 10 | $295K | $380K | 1.5× |
| 15 | **compound-engineering-co** | 10 | 0 / 10 | $278K | $551K | 1.4× |
| 16 | **minimax-studio** | 10 | 0 / 10 | $277K | $513K | 1.4× |
| 17 | **gstack** | 10 | 0 / 10 | $265K | $393K | 1.3× |
| 18 | **taches-creative** | 10 | 0 / 10 | $253K | $524K | 1.3× |

**🏁 Full Year** = agent survived all 52 weeks with positive funds (900–2,400 turns depending on pace).  
**Avg Return** = average final cash balance across all runs for that pack.  
**Avg ROI** = average final balance ÷ $200K starting capital.

---

### Key Numbers

| Metric | Value |
|--------|-------|
| Total runs | **186** |
| Packs with positive average return | **18 / 18 (100%)** |
| Average return across all 186 runs | **$388K (+94% on $200K)** |
| Best single run | **agency-agents: $1,189,275 (5.9×)** |
| Spread between #1 and #18 (same model, same seeds) | **$354K — explained entirely by context quality** |
| Full-year completions | **14 out of 186 runs (7.5%)** |
| Compute cost | **$0.00** |

---

### What the Results Prove

**The gap is not the model — it's the instructions.**

apex-systems averaged **3.0×** return. taches-creative averaged **1.3×**. Same model. Same random seeds. Same simulation engine. The $354K difference in average outcome is entirely explained by the quality and specificity of the company context loaded into the system prompt.

**Explicit decision heuristics are what separate survivors from collapses.**

Every pack that completed the full year — apex-systems (40%), agentsys-engineering (30%), agency-agents (23%), aeon-intelligence (23%), clawteam-capital (10%) — shares one trait: their VISION and agent briefings contain **mechanical if/then rules**, not just philosophy. Not "be disciplined about clients" but "check `tasks_failed` before every acceptance; block anyone with any failure history, permanently." The difference is specificity.

**Payroll discipline shows up in the numbers.**

Top performers end the year at **$54K–$71K/month payroll** — they grew the business (payroll went up) but kept it proportional to revenue. Packs without explicit payroll guidance let the agent assign all 8 employees to every task, which inflates payroll 2.7× faster and silently makes the business insolvent around month 4–6, even with solid revenue.

**A free model is sufficient. The bottleneck is context quality.**

All 186 runs used `Qwen3.6-plus:free` at zero cost. The benchmark is fully reproducible by anyone with an OpenRouter account. Model capability is not what separates the runs — the quality of what you load into the agent's context is.

---

### Run It Yourself

```bash
export OPENROUTER_API_KEY=sk-or-v1-...

# Single seed, all 18 packs
cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed7.json

# Full grid — seeds 7–10, ~72 runs
for seed in 7 8 9 10; do
  cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed${seed}.json
done
```

Results write to `runs/external_batch_<timestamp>.json`. Aggregate view:
```
GET /api/companies-sh/yc-bench
```

Each result contains the full simulation transcript, time-series funds/payroll/prestige data, and per-turn command logs.

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
