# Run your company with **durable agents** — HSM-II, Company OS, and operator chat

**Browseable docs site:** The same handbook is built with VitePress under **`docs-site/`** (`cd docs-site && npm install && npm run dev`). Edit **this file** as the canonical source; `npm run dev` / `npm run build` there runs **`sync-docs.mjs`**, which also mirrors listed docs as **`/guide/reference/…`** pages and plain **`/llm/*.md`** files (see **`docs-site/sync-manifest.json`**).

This guide is written in the spirit of product docs like [Paperclip](https://docs.paperclip.ing/#/): a **control plane** for AI-assisted operations, **delegated work on tasks**, and **human-in-the-loop** where risk requires it. HSM-II is the **runtime + action layer**; **Company OS** is the **ledger and graph in Postgres**; **agent-chat** is the **operator-facing conversation surface** in the Company Console that drives the same execution paths as the rest of the system.

If you only read one other file after this: **`docs/company-os/COMPANY_OS_AGENT_CHAT_LAUNCH.md`** (one command to bring everything up).

## What this is, in one breath {#what-this-is-one-breath}

**The pitch that matches what you’re actually building.**

This stack is for people who want **coding and ops agents that can see their own world**—the **repo**, the **runtime state**, **skills**, **instructions**, what ran, what failed—not a model in a vacuum reading only the last user message.

From there the natural move is **self-improvement with receipts**: the agent can **propose changes** to how it works (skills, prompts, wiring), you **approve or reject**, and Company OS keeps the kind of **trail** that makes “who changed what, when, after which run” answerable. **That supervision + audit trail is what we recommend** before anything touches production. If you accept more risk, the same tool plane is where **direct mutation** of config or code paths can live—but then you own the blast radius.

**Company OS** is the missing layer for “agent as company”: **one place** to run **multiple companies** (yours, clients, sandboxes), **adapt agents and playbooks** per company, and let work **compound**—tasks, runs, memory, governance—instead of evaporating when the tab closes.

**Where people want this to go** (and what the substrate is for): teams building toward **autonomous operating companies**—agents that don’t only answer tickets but **grow** how the business runs (and, when you wire the tools and policy, **outbound** work like campaigns or partnerships). **Today’s repo** is the **control plane + execution + ledger** for that story; it is **not** a turnkey “AI runs my ads while I sleep” product until **you** connect models, keys, skills, and governance you trust.

**Models and keys:** **Ollama** on your machine, or **your** API keys (default path **OpenRouter**; other lanes in **`company-os-up.sh`**). **`COMPANY_OS_AGENT_CHAT_LAUNCH.md`** + **`.env.example`** for the boring truth.

### Why you’d actually open the repo {#why-try-it}

- **You get to watch the agent work**—stream, tools, runs—so “it hallucinated confidence” isn’t your only signal.
- **Sandbox company vs production company**—same product, different boundary in the DB; experiment without poisoning live history.
- **Console + agent-chat share one spine**—no split-brain between “what the UI knows” and “what chat knows.”
- **Skills and action layer**—repo **`skills/`**, **`AGENTS.md`**, **`docs/HSMII_ACTION_LAYER.md`** when you’re ready to arm agents for real side effects under policy you set.

**The three names:** **HSM-II** = agent OS + tools + execution; **Company OS** = ledger + APIs + multi-company workspace; **agent-chat** = operator conversation **into** that same engine.

When someone asks “how do I run it?” → **`docs/company-os/COMPANY_OS_AGENT_CHAT_LAUNCH.md`** and **`.env.example`**.

---

## In 60 seconds — scope and what to do {#in-60-seconds}

**What this guide fully covers:** the **Company OS + Company Console** product path — Postgres + **`hsm_console`** (`/api/company/…`), Next **`web/company-console`**, and **operator agent-chat** calling the same **`execute-worker`** / skill flows as the rest of that UI. **It does not catalog every binary or experiment in the repo** (eval harnesses, other agents, Telegram paths, etc.); for the **wider HSM-II** tool plane and MCP, use **`README.md`** and **`docs/HSMII_ACTION_LAYER.md`**.

| If you are… | Do this one thing | How you know it worked |
|-------------|-------------------|-------------------------|
| A **human** trying the stack | From repo root: **`bash scripts/company-os-up.sh`**, then open **http://127.0.0.1:3050** | Console loads; **`curl -sfS http://127.0.0.1:3847/api/company/health`** |
| An **LLM** asked to bring up or debug the stack | Read **`docs/company-os/COMPANY_OS_AGENT_CHAT_LAUNCH.md`** (or **`DOCS_ORIGIN/llm/COMPANY_OS_AGENT_CHAT_LAUNCH.md`** on the docs site) **before** inventing ports | Your answer names **`company-os-up.sh`**, **3050** / **3847** (or env overrides), Postgres, and **`.env`** keys from that file |
| An **LLM** changing **how agent-chat runs** | Trace **Next** `app/api/agent-chat-reply` / `stream` → **`web/company-console/app/lib/agent-chat-server.ts`** (and related) → upstream **`HSM_CONSOLE_URL`** → **`/api/company/tasks/.../execute-worker`** | You cite **routes**, **telemetry wait** env vars (see **`.env.example`**), and **`web/company-console/app/lib/operator-chat-timeouts.ts`** |
| An **LLM** extending **tools / MCP / connectors** | Read **`docs/HSMII_ACTION_LAYER.md`** and **`docs/company-os/connectors-sessions-and-triggers.md`** | You name **Rust modules** or **HTTP routes** from those docs, not guessed APIs |

**Eval / meta-harness vs live chat:** **`hsm-eval`**, **`hsm_meta_harness`**, and the **Python `scripts/meta-harness/`** harness **measure** behavior; they are **not** automatically the same config as production agent-chat — see **`docs/EVAL_AND_META_HARNESS.md`**.

---

## Quick links

| I want to… | Go to |
|------------|--------|
| The main pitch (agents + companies + direction) | [What this is, in one breath](#what-this-is-one-breath) |
| Why open the repo | [Why you’d actually open the repo](#why-try-it) |
| Orient in 60s (human or LLM) | [In 60 seconds](#in-60-seconds) |
| Start workspace + API + chat locally | `bash scripts/company-os-up.sh` — see [Launch](#bring-up-company-os-one-command) |
| Understand what HSM-II *is* as a system | [HSM-II capabilities](#what-hsm-ii-is-capabilities) |
| Understand Company OS vs “just chat” | [Company OS](#company-os-the-control-plane) |
| Understand how operator chat executes work | [Agent-chat](#agent-chat-how-the-operator-drives-work) |
| Smoke-test chat + API quality | `docs/EVAL_AND_META_HARNESS.md` (Python meta-harness section), `scripts/company-os-agent-chat-meta-harness-smoke.sh` |
| Graph, memory, approvals, spend | `docs/company-os/world-model-and-intelligence.md`, `docs/company-os/ops-overview-api.md` |
| Connectors, sessions, triggers | `docs/company-os/connectors-sessions-and-triggers.md` |
| Eval binaries vs live runtime | `docs/EVAL_AND_META_HARNESS.md` |

---

## What HSM-II is (capabilities)

**HSM-II (Hyper-Stigmergic Morphogenesis II)** is an **agent operating system**: not only an LLM chat loop, but a **tool plane**, **policy**, **observable execution**, and **recovery** when context windows or processes reset.

At a glance (expanded in **`docs/HSMII_ACTION_LAYER.md`**):

| Layer | What you get |
|--------|----------------|
| **Tools** | Rust-native tools (shell, git, files, browser helpers, HTTP, Company OS CRUD, etc.) plus optional **HTTP MCP** registration from manifests. |
| **Sandboxing** | Configurable execution isolation (e.g. host `srt`, Docker-backed paths) so “run this” does not mean “run anything.” |
| **Company OS** | Postgres-backed **companies, tasks, goals, agent runs, memory, governance, spend** — exposed under **`/api/company/…`** from **`hsm_console`**. |
| **Console** | **`web/company-console`** (Next.js): workspace UI, rails, operator **agent-chat** — proxies to the Rust API. |
| **Learning / eval (offline)** | **`hsm-eval`**, **`hsm_meta_harness`**, **`hsm_outer_loop`** — improve configs and benchmarks **without** assuming they are live until wired — see **`docs/EVAL_AND_META_HARNESS.md`**. |

The rows above describe **HSM-II as a whole**. The rest of **this guide** goes deeper on **Company OS + agent-chat** only; treat **`docs/HSMII_ACTION_LAYER.md`** as the complement when your work is mostly tools/MCP/sandboxing outside the console.

**Accuracy note:** HSM-II is **self-hostable** and **repo-first**. It is not positioned here as a hosted “infinite SaaS catalog” product; it *is* positioned as a **serious action + governance substrate** you can run yourself. See the scope callouts in **`docs/HSMII_ACTION_LAYER.md`**.

---

## Company OS (the “control plane”)

Think of **Company OS** as the **system of record** for work an organization delegates to agents (Paperclip’s “hire agents, delegate tasks, approve the risky ones” maps cleanly to this idea):

- **Tasks and goals** — units of work with state, ownership, and trail.
- **Agents** — rostered agents with personas, tools, and policies.
- **Agent runs** — durable records of executions (success, error, summaries, linkage to tasks).
- **Memory** — shared vs per-agent scopes, search, export — see **`docs/issues/company-memory-shared-pool-and-roadmap.md`**.
- **Governance / approvals-shaped flows** — escalations and human-visible state surface through APIs and UI (details evolve with product; **`docs/company-os/ops-overview-api.md`** describes the unified **ops overview** and recovery story).
- **Spend and budgets** — operational cost visibility where implemented.

**Canonical truth lives in Postgres and HTTP APIs**, not in “whatever the model last said.” Chat is a **front door**; the **graph** is authoritative — see **`docs/agent-os-program/OPERATING_SUMMARY.md`**.

---

## Bring up Company OS (one command)

From the **repository root**:

```bash
bash scripts/company-os-up.sh
```

**Defaults (when you do not override ports):**

| Surface | URL |
|---------|-----|
| **Company Console** (workspace + operator UI + agent-chat) | http://127.0.0.1:3050 |
| **Company OS API** (`hsm_console`) | http://127.0.0.1:3847 |

**Prerequisites** (summarized; full table in **`docs/company-os/COMPANY_OS_AGENT_CHAT_LAUNCH.md`**):

- Rust + Node
- Postgres (Docker compose from the script, or **`HSM_COMPANY_OS_DATABASE_URL`**)
- **`.env`** with **`OPENROUTER_API_KEY`** or **`HSM_OPENROUTER_API_KEY`** for the default **OpenRouter** execution path (or configure **Ollama** per script header)

**Execution backends (important):** the same script configures **`HSM_EXECUTION_BACKEND`**. Default is **`openrouter`** (native Rust worker loop with free/cheap models unless you override). Set **`HSM_EXECUTION_BACKEND=claude`** when you want the **TypeScript `claude-harness` + executor** path (local Claude CLI, harness on **3848** by default). The script’s banner prints which lane is active.

**Health check:**

```bash
curl -sfS http://127.0.0.1:3847/api/company/health
```

Stop: **Ctrl+C** stops Next + `hsm_console`; Postgres container is usually **left running** by design — see launch doc for DB teardown.

---

## Agent-chat (how the operator drives work)

**Agent-chat** is the **operator’s conversational interface** in the Company Console. It is not a separate toy runtime: it calls **Next.js API routes** that orchestrate **`hsm_console`** — task context, **`execute-worker`**, skills, streaming telemetry — same patterns as the rest of the product.

**Worker prompt contract (introspection + governance):** every **`execute-worker`** run (from chat **or** Company OS auto-dispatch) gets the same injected block that (a) maps **repo / skills / machine / ledger** inspection to the **native tools** already on the agent, (b) spells a **propose → `company_task_requires_human` → direct `write`/`edit`/`bash`** ladder for self-change, and (c) states that **outbound “grow the business”** actions only exist through **tools you wire**—no hidden autopilot. Auto-dispatch now sets **`thread_workspace_root`** to the process cwd when available so file tools align with manual worker runs. Implementation: **`company_worker_exec_identity_markdown`** in **`src/company_os/mod.rs`**.

**Typical flow:**

1. Operator works in a **task** context in the UI (persona, company, thread state).
2. Messages hit **`POST /api/agent-chat-reply`** or **`POST /api/agent-chat-reply/stream`** (NDJSON stream for live phases, tool events, completion).
3. The server resolves **persona → agent config**, may route to **worker-first** or **chat-first** depending on intent and env (e.g. **`HSM_OPERATOR_CHAT_WORKER_FIRST`**).
4. When a **skill** or **execution** path is chosen, the stack dispatches to **`hsm_console`** (e.g. **`/api/company/tasks/:id/execute-worker`**) and **waits for telemetry** within configured caps — see **`web/company-console/app/lib/operator-chat-timeouts.ts`** and **`.env.example`** for **`HSM_OPERATOR_CHAT_TELEMETRY_WAIT_*`**.
5. **Finalize** paths and stream shapes are scored by the **Python meta-harness** for CI-style smoke (composite score from finalize, errors, tools, answer length) — **`scripts/meta-harness/evaluate_turn.py`**, documented in **`docs/EVAL_AND_META_HARNESS.md`**.

**Mental model:** Paperclip-style “delegate and watch” → here, **delegate in chat**, **observe in stream + agent runs + task trail**, **approve or repair** via governance surfaces when the workflow requires it.

---

## Skills, memory, and “how does the agent know what to do?”

- **Agent Skills** — markdown playbooks under **`skills/`** (and optional **`HSM_SKILL_EXTERNAL_DIRS`**). **`scripts/company-os-up.sh`** prepends **`./skills`** when absent so **`skill_md_read`** and catalog tools see repo skills — see **`AGENTS.md`** and **`docs/AGENT_SKILLS.md`**.
- **Company memory** — structured retrieval and append via tools and HTTP APIs (**`company_memory_search`**, **`company_memory_append`**, etc.) — see the issue/roadmap doc linked above.
- **Context recovery** — agents re-entering after restarts should lean on **`GET …/ops/overview`** and task **`llm-context`**, not on fragile session-only state — **`docs/company-os/ops-overview-api.md`**.

---

## How this compares to reading Paperclip’s docs

| Paperclip idea | HSM-II / Company OS analogue |
|----------------|------------------------------|
| Control plane for AI-run companies | **`hsm_console`** + Postgres Company OS + **`web/company-console`** |
| Hire / configure agents | Company **agents** roster, personas, tool policies |
| Delegate tasks | **Tasks**, **goals**, **execute-worker**, skill dispatch |
| Approve risky work | **Governance / approvals-shaped** states and ops overview (see API + UI as shipped in your branch) |
| Watch work happen | **Agent runs**, stream events, task trail, spend |

Paperclip’s site is a polished hosted handbook; **this repo’s truth is the code + the `docs/` tree**. Treat this file as the **onboarding map**; follow the linked documents for migrations, env vars, and edge cases.

---

## Document hub — raw markdown, benchmarks, LLM copy-paste, MCP, skills {#document-hub}

Use this section when you want **machine-ingestible specs**, **integration / benchmark context**, **paste-ready prompts**, **MCP-based discovery**, or **repo skills** that steer implementation.

### On-site markdown (browse + LLM fetch)

The **VitePress documentation site** in **`docs-site/`** mirrors these files as real pages **and** publishes **identical markdown bytes** under **`/llm/*.md`** (static files copied from `docs/` on every `npm run dev` / `npm run build`). Use **`DOCS_ORIGIN`** as the site origin (e.g. `http://127.0.0.1:5173` locally, or your deployed host).

| Topic | Browse (rendered) | Markdown for LLM (plain `GET`) | Source in repo |
|-------|-------------------|-------------------------------|----------------|
| **This operator guide** | [/guide/operator-handbook](/guide/operator-handbook) | `DOCS_ORIGIN/llm/HSM_II_COMPANY_OS_OPERATOR_GUIDE.md` | `docs/company-os/HSM_II_COMPANY_OS_OPERATOR_GUIDE.md` |
| **Company OS + agent-chat launch** | [/guide/reference/company-os-agent-chat-launch](/guide/reference/company-os-agent-chat-launch) | `DOCS_ORIGIN/llm/COMPANY_OS_AGENT_CHAT_LAUNCH.md` | `docs/company-os/COMPANY_OS_AGENT_CHAT_LAUNCH.md` |
| **Action layer (tools, MCP, sandbox)** | [/guide/reference/hsmii-action-layer](/guide/reference/hsmii-action-layer) | `DOCS_ORIGIN/llm/HSMII_ACTION_LAYER.md` | `docs/HSMII_ACTION_LAYER.md` |
| **Eval vs meta-harness vs Python harness** | [/guide/reference/eval-and-meta-harness](/guide/reference/eval-and-meta-harness) | `DOCS_ORIGIN/llm/EVAL_AND_META_HARNESS.md` | `docs/EVAL_AND_META_HARNESS.md` |
| **Benchmark stack (three tracks)** | [/guide/reference/benchmark-stack](/guide/reference/benchmark-stack) | `DOCS_ORIGIN/llm/BENCHMARK_STACK.md` | `docs/BENCHMARK_STACK.md` |
| **HSM-native / SMB benchmark** | [/guide/reference/hsm-native-bench](/guide/reference/hsm-native-bench) | `DOCS_ORIGIN/llm/HSM_NATIVE_BENCH.md` | `docs/HSM_NATIVE_BENCH.md` |
| **World model & intelligence** | [/guide/reference/world-model-and-intelligence](/guide/reference/world-model-and-intelligence) | `DOCS_ORIGIN/llm/world-model-and-intelligence.md` | `docs/company-os/world-model-and-intelligence.md` |
| **Ops overview API (recovery surface)** | [/guide/reference/ops-overview-api](/guide/reference/ops-overview-api) | `DOCS_ORIGIN/llm/ops-overview-api.md` | `docs/company-os/ops-overview-api.md` |
| **Connectors, sessions, triggers** | [/guide/reference/connectors-sessions-and-triggers](/guide/reference/connectors-sessions-and-triggers) | `DOCS_ORIGIN/llm/connectors-sessions-and-triggers.md` | `docs/company-os/connectors-sessions-and-triggers.md` |
| **Agent Skills (repo convention)** | [/guide/reference/agent-skills](/guide/reference/agent-skills) | `DOCS_ORIGIN/llm/AGENT_SKILLS.md` | `docs/AGENT_SKILLS.md` |

The mirror list is declared in **`docs-site/sync-manifest.json`** (add rows there to sync more docs). **GitHub raw** remains available if you prefer: `https://raw.githubusercontent.com/PermutationResearch/HSM-II/main/<repo-path>`.

**Relative paths:** if `DOCS_ORIGIN` is the same browser origin as this handbook, you can use only the path, e.g. **`/llm/HSMII_ACTION_LAYER.md`**.

### Integration benchmark (what to run when)

| Track | Measures | Entry / doc |
|-------|-----------|-------------|
| **LongMemEval** | Cross-session memory fidelity | `hsm_longmemeval` / **`docs/BENCHMARK_STACK.md` §1**, **`docs/LONGMEMEVAL.md`** (if present) |
| **YC-Bench** | Long-horizon company outcomes | `external_integrations/auto-harness-yc-bench/`, **`docs/BENCHMARK_STACK.md` §2**, **`docs/YC_BENCH.md`** (if present) |
| **HSM-native (SMB)** | Stigmergic memory, belief revision, handoffs | `hsm-native-eval`, **`docs/HSM_NATIVE_BENCH.md`**, **`docs/BENCHMARK_STACK.md`** §3 |
| **Company OS agent-chat (live)** | Next + `hsm_console` stream quality | **`docs/EVAL_AND_META_HARNESS.md`** (Python `scripts/meta-harness/`), **`scripts/company-os-agent-chat-meta-harness-smoke.sh`** |
| **Rust meta-harness / eval** | Harness JSON search vs baseline | **`docs/EVAL_AND_META_HARNESS.md`**, binaries `hsm_meta_harness`, `hsm-eval` |

**Rule of thumb:** pick **one** track per question — memory fidelity vs long-horizon vs native SMB vs **live** Company OS chat — then wire product changes only after you can **re-run** the same command and compare artifacts under `runs/` (often gitignored; copy summaries into `docs/` when you need history in git — see **`docs/HSM_NATIVE_BENCH.md`**).

### Copy for LLM (per document)

Paste the block into your coding agent. Replace **`DOCS_ORIGIN`** with the documentation site origin (e.g. `http://127.0.0.1:5173` when running `npm run dev` in `docs-site/`, or your deployed URL). Each block tells the model to **`GET` the `/llm/*.md` URL** (plain markdown, same bytes as `docs/` in the repo) before changing code.

<details>
<summary>Copy for LLM — operator guide (this document)</summary>

```
You are working in the HSM-II monorepo (Hyper-Stigmergic Morphogenesis II).

Task: Understand HSM-II capabilities, Company OS, and operator agent-chat at a product level.

Before writing or changing code, HTTP GET the full markdown (do not rely on memory of older chats):
DOCS_ORIGIN/llm/HSM_II_COMPANY_OS_OPERATOR_GUIDE.md

Deliverable: A short plan listing (1) which services to run locally, (2) which HTTP surfaces matter for Company OS, (3) where agent-chat calls into hsm_console, (4) open questions you still need from the repo tree.
```

</details>

<details>
<summary>Copy for LLM — Company OS + agent-chat launch</summary>

```
You are working in the HSM-II monorepo.

Task: Bring up Company OS so the workspace UI and operator agent-chat work locally.

HTTP GET the full launch spec (env, ports, Postgres, one-liner script):
DOCS_ORIGIN/llm/COMPANY_OS_AGENT_CHAT_LAUNCH.md

Deliverable: Exact commands you will run from repo root, expected URLs (3050 / 3847 unless overridden), and a checklist of env vars you verified from .env — no invented alternate entrypoints.
```

</details>

<details>
<summary>Copy for LLM — action layer & MCP integration</summary>

```
You are working in the HSM-II monorepo.

Task: Integrate or extend the action layer (native tools + HTTP MCP), staying within documented scope.

HTTP GET:
DOCS_ORIGIN/llm/HSMII_ACTION_LAYER.md

Deliverable: (1) Which mode applies (native vs MCP-backed), (2) concrete files you will touch (e.g. mcp_bridge, tool registry), (3) what you explicitly will NOT claim (hosted catalog, universal OAuth, etc.), (4) minimal tests or smoke steps.
```

</details>

<details>
<summary>Copy for LLM — eval, meta-harness, agent-chat harness</summary>

```
You are working in the HSM-II monorepo.

Task: Choose the correct eval or harness path and wire or document results without confusing live runtime with offline eval.

HTTP GET:
DOCS_ORIGIN/llm/EVAL_AND_META_HARNESS.md

Deliverable: One paragraph per tool (hsm-eval, hsm_meta_harness, hsm_outer_loop, Python scripts/meta-harness) stating when to use it; note explicitly that promoted best_config is not live until integrated.
```

</details>

<details>
<summary>Copy for LLM — benchmark stack & integration points</summary>

```
You are working in the HSM-II monorepo.

Task: Map benchmark tracks to integration directories and commands for a regression or new feature.

HTTP GET:
DOCS_ORIGIN/llm/BENCHMARK_STACK.md
and, if SMB work: DOCS_ORIGIN/llm/HSM_NATIVE_BENCH.md

Deliverable: Table of benchmark name → binary or script → primary output path → what “green” means for that track.
```

</details>

<details>
<summary>Copy for LLM — world model & Company OS graph</summary>

```
You are working in the HSM-II monorepo.

Task: Align a feature with the Company OS world model (Postgres graph, company_id, APIs).

HTTP GET:
DOCS_ORIGIN/llm/world-model-and-intelligence.md

Deliverable: For your feature, list the company-scoped tables or routes it must touch and any explicit non-goals from the doc.
```

</details>

<details>
<summary>Copy for LLM — connectors / sessions / triggers</summary>

```
You are working in the HSM-II monorepo.

Task: Extend or debug the connector session and trigger layer on top of Company OS.

HTTP GET:
DOCS_ORIGIN/llm/connectors-sessions-and-triggers.md

Deliverable: API paths you will use or add, data model fields that must stay company-scoped, and migration or rollout order.
```

</details>

<details>
<summary>Copy for LLM — ops overview API (recovery & governance)</summary>

```
You are working in the HSM-II monorepo.

Task: Use or extend the unified Company OS ops overview endpoint for context recovery, governance visibility, or operator dashboards.

HTTP GET:
DOCS_ORIGIN/llm/ops-overview-api.md

Deliverable: Which top-level JSON keys you rely on, how an agent should re-enter after restart, and what you will not duplicate from chat transcripts.
```

</details>

<details>
<summary>Copy for LLM — Agent Skills convention</summary>

```
You are working in the HSM-II monorepo.

Task: Add or reorganize Agent Skills so models can discover and load SKILL.md playbooks correctly.

HTTP GET:
DOCS_ORIGIN/llm/AGENT_SKILLS.md

Deliverable: Where skills live, how HSM_SKILL_EXTERNAL_DIRS interacts with scripts/company-os-up.sh, and naming/description rules so tool routing picks the right skill.
```

</details>

<details>
<summary>Copy for LLM — generic (any other raw doc URL)</summary>

```
You are working in the HSM-II monorepo.

Task: <DESCRIBE YOUR TASK IN ONE SENTENCE>.

HTTP GET this specification in full before editing code or config (use DOCS_ORIGIN + /llm/… from the table above, or GitHub raw if the docs site is not running):
<DOCS_ORIGIN/llm/FILE.md or https://raw.githubusercontent.com/PermutationResearch/HSM-II/main/<repo-path>>

Deliverable: Summary of constraints from the doc + a minimal patch plan + how you will verify (command or HTTP check).
```

</details>

### MCP tools (discover integration & implementation)

Use MCP when you want **structured tool lists**, **live calls to configured servers**, or **HTTP MCP parity** without reading the whole Rust tree first.

| Surface | Use it to… | Where to learn |
|---------|------------|----------------|
| **Cursor MCP** | List enabled servers, read tool schemas before `call_mcp_tool` | Project `mcps/` descriptors (when present) + Cursor MCP docs |
| **Hermes `mcporter` skill** | List/configure/call MCP servers from CLI (`mcporter`) | `.claude/skills/hermes-main/mcp/mcporter/SKILL.md` |
| **Hermes `native-mcp` skill** | Native MCP client patterns (stdio/HTTP, tool injection) | `.claude/skills/hermes-main/mcp/native-mcp/SKILL.md` |
| **Optional FastMCP** | Build or wrap Python MCP servers | `.claude/skills/hermes-optional/mcp/fastmcp/SKILL.md` |
| **Rust MCP bridge** | How HSM-II registers HTTP MCP tools from manifests | **`src/tools/mcp_bridge.rs`** (see also **`docs/HSMII_ACTION_LAYER.md`**) |
| **Company OS connector + OpenAPI/MCP ingestion** | Product integration plane for accounts, sessions, triggers | **`docs/company-os/connectors-sessions-and-triggers.md`** |

**Workflow:** use **MCP tool descriptors** (or `mcporter tools <server>`) to confirm names and JSON shapes → read the **raw markdown** row for the feature area → implement against **`hsm_console`** / **`web/company-console`** as the doc specifies.

### Skills (recommendations)

| Goal | Skill (read `SKILL.md`) |
|------|-------------------------|
| Tool names, schemas, progressive disclosure for LLMs | **`skills/llm-tool-skill-reasoning/SKILL.md`** |
| Experiment specs, checklists, measurement loops | **`skills/research-experiment-loop/SKILL.md`** |
| RLM-style prediction / policy hints | **`skills/predict-rlm/SKILL.md`** |
| Repo-wide skill layout, `HSM_SKILL_EXTERNAL_DIRS` | **`docs/AGENT_SKILLS.md`** (and **`AGENTS.md`** in repo root) |
| MCP CLI operations | **`.claude/skills/hermes-main/mcp/mcporter/SKILL.md`** |
| MCP client integration in agents | **`.claude/skills/hermes-main/mcp/native-mcp/SKILL.md`** |
| Hermes Agent umbrella (profiles, tools, gateway) | **`.claude/skills/hermes-main/autonomous-ai-agents/hermes-agent/SKILL.md`** |
| Writing implementation plans with file paths | **`.claude/skills/hermes-main/software-development/writing-plans/SKILL.md`** |

`scripts/company-os-up.sh` prepends **`./skills`** when `HSM_SKILL_EXTERNAL_DIRS` does not already include it so **`skill_md_read`** sees these playbooks in Company OS runs.

---

## Related reading (order suggested)

1. **`docs/company-os/COMPANY_OS_AGENT_CHAT_LAUNCH.md`** — pasteable LLM block, env, ports.  
2. **`docs/HSMII_ACTION_LAYER.md`** — tools, MCP, sandbox, honesty about scope.  
3. **`docs/company-os/world-model-and-intelligence.md`** — graph and intelligence alignment.  
4. **`docs/EVAL_AND_META_HARNESS.md`** — eval vs meta-harness vs agent-chat Python harness.  
5. **`docs/agent-os-program/OPERATING_SUMMARY.md`** — long-horizon operating discipline.

---

*Documentation in this repository is for operators and contributors shipping HSM-II. For Paperclip’s own product, see [Paperclip documentation](https://docs.paperclip.ing/#/).*
