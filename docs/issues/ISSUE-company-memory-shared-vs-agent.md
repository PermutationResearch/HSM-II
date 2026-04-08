# Issue: Company memory — how it works today, APR/tool recall, and richer shared knowledge

**Type:** design / product / documentation  
**Canonical deep dive (architecture, backlog, Paperclip parity):** [`memory-workspace-shared-context.md`](./memory-workspace-shared-context.md)

This note is written to **paste into a tracker** or PR description: plain-language **current behavior**, **FAQ**, and **ambitious but grounded** improvements—especially around **company-wide facts** vs **per-agent scratch space**, and how **autonomous runs (APR)** should **search** and **write** memory.

---

## Executive summary

| Layer | What it is |
|--------|------------|
| **Postgres `company_memory_entries`** | Durable rows per company: `scope = shared` (company pool) or `agent` (+ `company_agent_id`). Optional `kind = broadcast` (shared only)—merged first in task context. Heuristic `summary_l0` / `summary_l1` on create. |
| **Passive context** | `GET …/tasks/:id/llm-context` concatenates company `context_markdown` (plus heading TOC), **recent shared** memories (~20), **recent agent-scoped** memories for the resolved workforce agent (~20), task block (spec, `workspace_attachment_paths`, handoff notes, `hsmii_home`), tool hints (`company_memory_search` / `company_memory_append`), and agent profile. |
| **Active tools** | `company_memory_search` (`mode`: `shared` \| `mine` \| `both`) and `company_memory_append` (`scope`: `shared` \| `agent`, required). |
| **Operator UI** | Company console: **shared** pool panel; per-agent **Memory** tab for **agent-scoped** CRUD; `GET …/memory/export.md` for a git-friendly shared index. |

**Important nuance for “APR only sees its own memories”:**  
`company_memory_search` **defaults `mode` to `shared`** when omitted (`src/tools/company_os_tools.rs`). So **ad-hoc search** is **company-wide by default**, not private. **`mine`** restricts to `scope=agent` for the resolved agent; **`both`** returns two JSON buckets (`shared` + `mine`).  

What *is* private by default is **writing**: `company_memory_append` **requires an explicit `scope`**—there is no default. If runbooks or model habits bias toward `scope=agent`, knowledge stays siloed unless operators or prompts steer toward **`shared`**.

---

## 1. How memory works today (operator view)

### 1.1 Three places “company truth” can live

1. **`companies.context_markdown`** — Human-edited “constitution”: stable, versioned in your normal company workflow; injected whole (with TOC) into `llm-context`. Best for **slow-changing** norms.
2. **`company_memory_entries` with `scope=shared`** — **Many short rows**: searchable (FTS + ILIKE), tagged, optional **broadcast** priority. Best for **operational** facts that change often (“incident X”, “API base for staging”, “COM-500 fix merged”).
3. **Per-task `workspace_attachment_paths`** — **Pointers** to files under `hsmii_home`, not copies. Best for **artifacts** and deep context without stuffing the DB.

Per-agent **`scope=agent`** rows are for **preferences, working notes, and facts that should not leak** to other agents on the same company.

### 1.2 What the model sees without calling tools

On each task context build, the server already **injects a slice** of shared + agent memories (recency-limited). That is **not** a full search; it is a **fixed preview**. Anything not in that slice still requires **`company_memory_search`** (or reading workspace files).

### 1.3 What the model can do with tools

- **`company_memory_search`:** Query the API list endpoint with `q` (optional). Modes:
  - **`shared`** — only the company pool (default).
  - **`mine`** — only this agent’s rows (needs `company_agent_id` or task-bound `llm-context` resolution).
  - **`both`** — parallel queries; useful when the task blends **policy** (shared) with **personal habit** (mine).
- **`company_memory_append`:** Creates a row; **`scope` is required** (`shared` or `agent`). **`kind: broadcast`** only valid for `shared`.

Server-side validation ensures agents cannot read or write other agents’ private rows by spoofing IDs in ways the API rejects.

---

## 2. FAQ

### How do we share information company-wide without updating everyone’s memories?

**Do not copy** the same paragraph into twelve `MEMORY.md` files or twelve agent-scoped rows.

- **Write once** to **`scope=shared`** (or update **`context_markdown`** if it is truly constitutional).
- **Rely on injection + search:** Every agent’s `llm-context` already includes **recent shared** rows; for older or niche facts, models should run **`company_memory_search`** (default mode is already **`shared`**).
- **Optional:** Use **`kind: broadcast`** for high-signal lines that should stay near the top of the merged block until you add TTL (see backlog in the canonical doc).

### How do we create shared memories?

| Channel | How |
|--------|-----|
| **Console** | Shared memory panel on the workspace → create row with `scope=shared` (and optional **broadcast**). |
| **Agent tool** | `company_memory_append` with `scope: "shared"` (and optional `kind`, `tags`). |
| **API** | `POST /api/company/companies/{company_id}/memory` with JSON body (same fields as console). |
| **Review / git** | `GET …/memory/export.md` for a markdown export of **shared** rows (Postgres remains source of truth for recall). |

### When should a run use `mine` vs `shared` vs `both`?

| Situation | Suggested mode |
|-----------|----------------|
| Policy, URLs, org-wide decisions | **`shared`** (or rely on injected shared block + search with `q`) |
| Personal preference, experimental note | **`mine`** / append with `scope=agent` |
| Task needs “what we decided as a company” **and** “how I usually work” | **`both`** |

---

## 3. Gaps and improvement proposals (creative + implementable)

These extend (not replace) **§10–§12** in [`memory-workspace-shared-context.md`](./memory-workspace-shared-context.md).

### 3.1 Make “save” defaults intentional (APR / runbooks)

**Problem:** Append has **no default `scope`**; models may choose `agent` whenever they feel “this is my note,” fragmenting knowledge.

**Ideas:**

- **Runbook text** in task templates: “Durable facts visible to the whole company → `scope=shared`; only use `agent` for personal preference or secrets.”
- **Tool description tweak (optional):** Add one line: *Prefer `shared` for policies and facts another agent would need; use `agent` only for private preference or sensitive detail.*
- **Governance hook:** Optional `requires_approval` or “suggest shared” queue for rows promoted from private → shared (human or lead agent approves).

### 3.2 Stronger shared-pool **retrieval** (beyond recent-20 injection)

**Problem:** Injection is **recency**, not **relevance** to the current task.

**Ideas:**

- **Task-conditioned search:** Before main reasoning, APR calls `company_memory_search` with `q` derived from task title + spec keywords (or a tiny planner step).
- **Embeddings / pgvector:** Semantic search over `shared` (and optionally `agent`) with caps—matches the “interesting” direction you called out for shared pool querying.
- **Role-aware ranking:** Same pool, different sort bias by `company_agents.role` or tags (marketing vs engineering)—see canonical doc §10.

### 3.3 “Single write, many readers” UX

- **Version or supersede:** Mark a shared row as **superseded_by** another ID so queries return a **resolved** policy, not three conflicting paragraphs.
- **Staleness token in context:** Inject “shared memory snapshot as of `updated_at` …” so the model knows when to re-query after governance events.

### 3.4 Cross-surface observability

- Log **which memory entry IDs** were injected or returned by tool calls into run metadata (audit + debugging), aligned with Paperclip-style run ↔ artifact linkage.

### 3.5 Operator ergonomics

- **One-click “promote to shared”** from an agent-scoped row in console (dup or move, with optional redaction).
- **Bulk import** from markdown or from Paperclip export into `scope=shared` with tag `imported`.

---

## 4. Acceptance criteria (for a future PR that “closes” this issue)

- [ ] Tracker links this doc + canonical doc; no duplicate “shipped” checklist (single source: top of [`memory-workspace-shared-context.md`](./memory-workspace-shared-context.md)).
- [ ] Runbooks / default APR prompts state **when to use `shared` vs `agent` on append** and **when to use `both` on search**.
- [ ] (Optional) Metrics: fraction of `company_memory_append` calls with `scope=shared` vs `agent` per company (privacy-preserving aggregate).

---

## 5. Code map (for implementers)

| Concern | Location |
|---------|----------|
| REST + DB | `src/company_os/company_memory.rs` |
| `llm-context` merge order | `src/company_os/agents.rs` (`fetch_shared_memory_addon`, `fetch_agent_memory_addon`, task block) |
| Tools | `src/tools/company_os_tools.rs` (`CompanyMemorySearchTool`, `CompanyMemoryAppendTool`) |
| Pack hint snippet | `templates/company-os/AGENTS.snippet.md` |

---

## 6. One-line answers (copy-paste for issue body)

- **Company-wide without N memory files:** Use **`scope=shared`** rows and **`context_markdown`** for static constitution; agents **read** via injection + **`company_memory_search`** (default **shared**).  
- **Create shared memories:** Console shared panel, **`company_memory_append` with `scope: shared`**, or **POST** company memory API; optional **`export.md`** for review.  
- **“They only query their own”:** **Not accurate for `company_memory_search`**—**default mode is `shared`**. **`mine`** is opt-in; **`both`** merges pools. **Silos come from append habits**, not from search defaults.
