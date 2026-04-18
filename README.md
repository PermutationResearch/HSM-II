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

### What is YC-Bench

[YC-Bench](https://collinear-ai.github.io/yc-bench/) is a long-horizon deterministic benchmark that tests whether an LLM agent can run a simulated AI startup profitably over one full year. The agent interacts exclusively through a CLI — no shortcuts, no cheating, just hundreds of sequential decisions against a discrete-event engine that enforces real economic consequences.

This is not a coding benchmark or a question-answering test. It is a **sustained decision-making benchmark under adversarial conditions and memory constraints**. An agent that can't manage cash flow, detect bad clients, and retain strategy across hundreds of turns will fail — regardless of how well it reasons on isolated problems.

---

### The Simulation — What the Agent Has to Manage

The agent starts with **$200,000** and manages a company with **8 employees** for a simulated calendar year. Every mechanic in the simulation has compounding consequences.

#### The task marketplace

Clients post tasks across four domains: `training`, `inference`, `research`, and `data engineering`. Each task has:
- A **reward** (earned only if completed before deadline)
- A **deadline** (activated the moment the agent accepts — the clock starts immediately)
- A **work quantity** that employees must complete
- A **prestige requirement** (higher prestige unlocks higher-reward tasks)

The agent must browse the marketplace, accept tasks strategically, assign employees, dispatch work, and advance the simulation clock.

#### The payroll trap — the most punishing mechanic

This is the mechanic that separates naive agents from good ones.

Every employee has a **salary that grows with every task assignment**. Each time a task completes, all assigned employees get a salary bump. An agent that assigns all 8 employees to every task grows its monthly payroll **~2.7× faster** than one that assigns selectively.

```
Month 1:  ~$38,000/month payroll
Month 6:  ~$55,000/month if selective assignment
Month 6:  ~$70,000+/month if all-8 assigned to everything
```

At $70K/month payroll, the business must earn over $840K/year just to break even. The agent needs enough revenue to outpace the salary growth it is itself creating. Agents that blindly maximize short-term task speed by throwing all employees at everything compound their own cost structure into insolvency.

The benchmark specifically rewards agents that think about **which employees to assign** based on domain productivity (each employee has different skill levels per domain) rather than brute-forcing with all 8.

#### RAT clients — adversarial detection under uncertainty

35% of clients in the simulation are **adversarial "RAT" clients**. Their identifying behavior:

- They offer top-tier rewards to attract greedy agents
- After acceptance, they **inflate the work quantity**, making the deadline nearly impossible
- Failing a deadline costs 35% of the advertised reward as a penalty **plus** a prestige reduction
- Their adversarial status is **hidden** — the agent cannot see it in advance

The agent must infer which clients are adversarial from failure patterns over time. A client that has caused two consecutive deadline failures is almost certainly a RAT. The correct strategy: check `client history` before accepting from a new client, and blacklist clients after confirmed failures.

Agents that chase the highest-reward tasks without tracking client history end up in a spiral: accept RAT task → fail → lose prestige → can't access good tasks → accept more RAT tasks.

#### Trust mechanics — compounding rewards for loyalty

Completing tasks for the same client builds **trust**, which:
- Reduces future work quantity by up to **50%** (half the work for the same reward)
- Unlocks higher-tier tasks from that client
- Increases effective reward-per-hour

But trust is **fragile and exclusive**: completing tasks for one client causes trust to decay with all other clients. Spreading attention too thin means no client ever trusts you enough to reduce work. The optimal strategy focuses on 2–3 vetted, trusted, non-RAT clients rather than promiscuously accepting from everyone.

#### The memory constraint — context truncation to 20 turns

The agent's conversation history is hard-truncated to **20 turns**. Older turns are dropped. The only mechanism for retaining information across the truncation boundary is a **persistent scratchpad** injected into the system prompt each turn.

Agents that don't use the scratchpad lose all client history, employee productivity notes, and strategic decisions the moment the window rolls past them. They re-identify RAT clients they already blacklisted, reassign employees they already benchmarked, and forget strategies that were working.

This makes the benchmark a direct test of **whether company context survives a rolling context window** — which is exactly what HSM-II is built to ensure.

---

### What We Were Testing

Each of the 18 company packs we benchmarked comes with a system prompt assembled from:
- `VISION.md` — the company's operating philosophy and priorities
- Agent briefings — role descriptions with domain expertise and decision heuristics
- Skill files — documented procedures, e.g., how to vet clients, manage cash flow, assign work

The question we were asking: **does company-specific context actually change how an agent behaves in a sustained adversarial simulation, or is it noise?**

The secondary question: **which dimensions of context matter most?** Explicit client-vetting procedures? Employee efficiency heuristics? Risk management philosophy? Cash flow guidance?

We ran the same benchmark — same seeds, same model, same simulation parameters — across all 18 packs. The model was **Qwen3.6-plus:free** via OpenRouter (free tier, $0.00 cost per run). Any difference in performance comes from the company context, not model capability.

---

### Results — 186 Runs, 18 Company Packs, Seeds 1–10

| Rank | Company pack | Runs | Completed full year | Avg final funds | Best single run | Avg payroll at end |
|------|-------------|------|---------------------|-----------------|-----------------|-------------------|
| 1 | **apex-systems** | 10 | **4 / 10** | $607,163 | $1,011,137 | $61,090/mo |
| 2 | **agency-agents** | 13 | **3 / 13** | $564,152 | $1,189,275 | $71,035/mo |
| 3 | **kdense-science-lab** | 10 | 0 / 10 | $532,392 | $979,667 | $54,565/mo |
| 4 | **product-compass-consulting** | 10 | 0 / 10 | $454,899 | $808,024 | — |
| 5 | **donchitos-game-studio** | 10 | 0 / 10 | $436,844 | $934,064 | — |
| 6 | **clawteam-capital** | 10 | 1 / 10 | $426,755 | $1,075,768 | $54,025/mo |
| 7 | **aeon-intelligence** | 13 | 3 / 13 | $421,876 | $1,059,497 | $68,485/mo |
| 8 | **superpowers** | 10 | 0 / 10 | $412,101 | $974,715 | — |
| 9 | **agentsys-engineering** | 10 | 3 / 10 | $405,801 | $1,043,654 | $63,655/mo |
| 10 | **redoak-review** | 10 | 0 / 10 | $373,360 | $800,648 | $48,517/mo |
| 11 | **clawteam-research-lab** | 10 | 0 / 10 | $359,923 | $1,062,191 | $41,605/mo |
| 12 | **trail-of-bits-security** | 10 | 0 / 10 | $322,838 | $581,061 | $49,525/mo |
| 13 | **fullstack-forge** | 10 | 0 / 10 | $300,237 | $687,456 | $49,885/mo |
| 14 | **clawteam-engineering** | 10 | 0 / 10 | $295,045 | $379,938 | $44,845/mo |
| 15 | **compound-engineering-co** | 10 | 0 / 10 | $278,102 | $550,995 | $44,575/mo |
| 16 | **minimax-studio** | 10 | 0 / 10 | $276,761 | $513,224 | $43,495/mo |
| 17 | **gstack** | 10 | 0 / 10 | $265,427 | $393,273 | $43,945/mo |
| 18 | **taches-creative** | 10 | 0 / 10 | $253,414 | $524,283 | — |

**Starting capital: $200,000. All 18 packs produced positive average returns.**  
Average final funds across all runs: **$388,172** (+94% on starting capital).  
Best single run: agency-agents — **$1,189,275** (5.9× starting capital, 976 turns, full year completed).  
Most full-year completions: apex-systems — **4 out of 10 runs** survived to the 1-year horizon.

---

### What the Results Prove

**1. Company context changes agent behavior in measurable ways across hundreds of turns.**

The spread between #1 (apex-systems, $607K avg) and #18 (taches-creative, $253K avg) is $354K — on the same model, same seeds, same simulation. That gap is entirely explained by the quality and specificity of the company context each pack provides. The model is identical. The instruction set is not.

**2. The "full year completed" metric is the hardest signal.**

Reaching the 1-year horizon requires coherent strategy across 900–2,400 turns with a 20-turn context window. The packs that sustained full-year completion (apex-systems 40%, agency-agents 23%, agentsys-engineering 30%, aeon-intelligence 23%, clawteam-capital 10%) all share a common trait: their VISION and agent briefings contain **explicit decision heuristics** — not just "be a good company" but "when a client causes two consecutive failures, stop accepting from them" and "assign employees by domain productivity, not by headcount."

**3. Payroll discipline is the deciding factor between mediocre and great runs.**

Look at the end-payroll column. Top performers end with $54K–$71K/month payroll — roughly 1.5–1.9× their starting payroll. This means they still grew payroll (grew the business), but they kept it proportional to revenue. Packs without explicit payroll guidance let the model assign all 8 employees to every task, which grows payroll 2.7× faster and eventually makes the business insolvent regardless of revenue.

**4. Free-tier models can run this benchmark effectively at zero cost.**

All 186 runs used **Qwen3.6-plus:free** on OpenRouter. Total compute cost: $0.00. This makes the benchmark fully reproducible by anyone with an OpenRouter account. The bottleneck is not model capability — it is the quality of the operational context given to the agent.

---

### Run It Yourself

```bash
export OPENROUTER_API_KEY=sk-or-v1-...

# Run all 18 packs, one seed
cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed7.json

# Full grid (seeds 7–10, ~72 runs)
for seed in 7 8 9 10; do
  cargo run --bin hsm_outer_loop -- external-batch --spec config/external_yc_bench_seed${seed}.json
done
```

Results write to `runs/external_batch_<timestamp>.json` and aggregate via:
```
GET /api/companies-sh/yc-bench
```

Each result file contains the full simulation transcript, time-series funds/payroll/prestige data, and per-turn command logs — queryable for deeper analysis.

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
