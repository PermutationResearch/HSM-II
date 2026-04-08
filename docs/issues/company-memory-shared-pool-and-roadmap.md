# Issue: Company memory — how it works today + roadmap (shared pool without duplicating per-agent state)

**Intent:** Capture how Paperclip-style **company memory** works in this repo after recent work, and propose directions for **company-wide knowledge** so operators and agents do not have to **copy the same fact into every agent’s private memory**.

---

## 1. How memory works today (Company OS)

### 1.1 Data model

- **`company_memory_entries`** (Postgres) holds two scopes:
  - **`shared`** — visible company-wide (any agent on that company can search and receive injected context subject to retrieval rules).
  - **`agent`** — tied to a specific **`company_agents.id`** (`company_agent_id`); “private” notebook for that roster agent.
- Optional **`kind`**: `general` vs **`broadcast`** (shared only). Broadcast rows sort **first** in listings and in the markdown export so urgent lines surface above routine notes.
- **`tags`**, **`source`** (e.g. `human` vs agent), optional **`summary_l0` / `summary_l1`** (auto-derived on create if omitted).

See migration `migrations/20260406120000__company_shared_memory.sql` and implementation `src/company_os/company_memory.rs`.

### 1.2 HTTP API (Company console / `hsm_console`)

| Capability | Route / method |
|------------|----------------|
| List / search | `GET /api/company/companies/:company_id/memory?scope=shared\|agent\|all&q=…` |
| Create | `POST /api/company/companies/:company_id/memory` (body: `scope`, `title`, `body`, optional `company_agent_id`, `tags`, `kind`, …) |
| Update | `PATCH …/memory/:memory_id` |
| Delete | `DELETE …/memory/:memory_id` (and POST alias for strict proxies) |
| Git-friendly export | `GET …/memory/export.md` (shared rows only, ordered with broadcast first) |

Search uses **Postgres full-text** plus **ILIKE** fallbacks on title/body/summaries.

### 1.3 Agent tools (APR / harness)

- **`company_memory_search`** — HTTP-backed; **`mode`**: `shared` (default), **`mine`** (agent scope only), **`both`**. Resolves `company_id` / `company_agent_id` from params or `GET …/tasks/:id/llm-context` when `task_id` is set (`src/tools/company_os_tools.rs`).
- **`company_memory_append`** — creates rows with explicit **`scope`** (`shared` or `agent`); no silent default in the **prompt contract** embedded in `llm-context` (see below).

### 1.4 What actually gets injected into the model (task `llm-context`)

`GET /api/company/tasks/:task_id/llm-context` assembles a **single system-side bundle** with explicit sections (manifest / tiers in `src/company_os/agents.rs`):

1. **Company `context_markdown`** (plus optional heading outline).
2. **Shared memory addon** — newest **shared** rows only, **size-capped** and **entry-limited** (`fetch_shared_memory_addon` in `company_memory.rs`).
3. **Agent memory addon** — same pattern for **`scope = agent`** rows for the resolved workforce agent (`fetch_agent_memory_addon`).
4. **Task block** — title, spec, workspace file pointers, capability refs, **stigmergic `context_notes`** (operator handoff), pack home, and **tool usage instructions** for memory read/write.

**Important operational detail:** injection is **not** “full history.” Defaults (overridable via env):

- `HSM_COMPANY_MEMORY_LLM_CONTEXT_MAX_BYTES` (default **3072**)
- `HSM_COMPANY_MEMORY_LLM_CONTEXT_ENTRY_LIMIT` (default **1** newest row per pool)

So agents are **prompted** to save durable facts and **search** when they need more than the tiny injected slice.

### 1.5 Orthogonal: in-process “Hindsight” memory (`src/memory.rs`)

The core library’s vector/BM25/graph **recall** stack is a **separate** subsystem from Company OS Postgres memory. Company mode is the right place for **durable, auditable, multi-agent** knowledge; the in-process stack is for **single-runtime** experiments unless explicitly bridged.

---

## 2. Answering the product questions directly

### 2.1 “How do we share information company-wide without updating everyone’s memories?”

**Today:** Put it in **`scope: shared`** once. Per-agent **`agent`** rows are for **private** scratch, preference, or sensitive notes. **Do not** replicate the same policy into every agent’s private scope—use **shared** (and **`kind: broadcast`** when it must win ordering).

**Gap:** Injected context only shows a **thin slice** (by design). Agents must **`company_memory_search`** (or read `export.md`) to pull depth—operators should assume **not every run “sees” the whole pool in the prompt**.

### 2.2 “How do we create shared memories?”

- **Agents:** `company_memory_append` with **`scope: "shared"`** (and optional `kind: "broadcast"`).
- **Humans:** Company console UI (e.g. shared memory panel) or direct API / `export.md` workflow.
- **Handoffs:** Task-level **`context_notes`** (stigmergic notes) are ideal for **sequence-local** context; **shared memory** is for **durable** facts that should survive past a single task.

---

## 3. Proposal: ambitious improvements (creative + practical)

### 3.1 Tiered “read path” (keep prompts small, knowledge deep)

- **L0/L1 already exist** on rows (`summary_l0`, `summary_l1`). Use them in injection: e.g. inject **only L0 lines** for N shared entries, full body on **search hit** or explicit “open memory id.”
- **Semantic / hybrid retrieval** for `company_memory_search` (embed summaries, rerank) so `q=` is not only FTS/substring.
- **Pinned memories**: operator flag “always include in injection” with a **hard byte budget** to avoid prompt blow-up.

### 3.2 Write path governance (trust without spam)

- **Roles:** who may write `shared` vs `agent` (human-only shared, or agent with review queue).
- **Provenance:** mandatory `source` + optional `task_id` / `issue_id` link for audit (“why does this memory exist?”).
- **Supersedes / deprecates:** soft-delete or `replaced_by` UUID so searches prefer current policy without losing history.

### 3.3 “Company brain” patterns (no per-agent duplication)

- **Namespaces / channels** (e.g. `#policy`, `#infra`, `#product`) via `tags` or a first-class column—filter in search and injection.
- **Digest job:** nightly (or on publish) **rollup markdown** pushed to a repo path + optional notification—**one** canonical doc instead of N agent memories.
- **Diffusion:** when an agent writes a **high-value** `agent` memory, suggest **promotion** to `shared` (operator one-click or policy auto-approve).

### 3.4 Cross-cutting context

- **Project / team scoping** (future): `company_id` + `project_id` so large orgs don’t share one flat pool.
- **Integration with issues:** link `memory_id` ↔ issue; when an issue closes, auto-append a **shared** “decision record” entry.

### 3.5 Observability

- Metrics: search QPS, zero-result rate, append rate by scope, **injection truncation** frequency (signals budget too tight or entries too fat).

---

## 4. Suggested acceptance criteria (when we implement “phase 2”)

1. Documented operator playbook: **when to use shared vs agent vs task notes** (one page, linked from console).
2. Injection strategy configurable without code change (at minimum: env + optional per-company overrides).
3. Search quality: measurable improvement on recall@k for realistic `q=` vs substring-only.
4. Optional: promotion flow from agent-private to shared with audit trail.

---

## 5. References (code)

- `src/company_os/company_memory.rs` — API, export, `fetch_*_memory_addon`
- `src/tools/company_os_tools.rs` — `company_memory_search`, `company_memory_append`
- `src/company_os/agents.rs` — `get_task_llm_context` assembly + prompt text for tools
- `migrations/20260406120000__company_shared_memory.sql` — schema

---

*This issue is descriptive + roadmap; implementation can be split into smaller tracked issues (injection L0-only, hybrid search, governance, etc.).*
