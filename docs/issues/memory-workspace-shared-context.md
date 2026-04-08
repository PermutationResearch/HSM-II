# Issue: Memory today, workspace attachments, and company-wide shared context

**Shipped in tree (canonical — update here when capabilities change; avoid duplicating in §4/§12):**

- **Pool & API:** Postgres `company_memory_entries` (`scope` shared | agent, `kind` general | **broadcast**, heuristic `summary_l0`/`summary_l1` on create), REST CRUD, list search with **Postgres FTS + ILIKE** (`q=`), **`GET …/memory/export.md`**, bundle export/import.
- **Task context:** `workspace_attachment_paths`, **`context_notes`** + **`POST …/stigmergic-note`**, `GET …/tasks/:id/llm-context` merges **context_markdown** + **heading TOC** + **shared** (broadcast first) + **agent** pool + task block (paths, handoff notes, `hsmii_home`, tool hints) + workforce **agent profile**; JSON includes **`company_id`**, **`context_notes`**.
- **Tools:** **`company_memory_search`** / **`company_memory_append`** (`src/tools/company_os_tools.rs`, `register_all_tools`).
- **Console:** Memory panel (search, kind, export link), task path chips, **per-task run strip** (status, tool count, `log_tail`) + handoff note + append to run log.

**See also:** Playbooks are **by project**; **`visions.md`** is the north star — [`docs/company-os/playbooks-projects-and-visions.md`](../company-os/playbooks-projects-and-visions.md). Canonical company graph vs intelligence APIs — [`docs/company-os/world-model-and-intelligence.md`](../company-os/world-model-and-intelligence.md). **Tracker-ready FAQ + APR/tool defaults:** [`ISSUE-company-memory-shared-vs-agent.md`](./ISSUE-company-memory-shared-vs-agent.md).

**Desktop shell (in-repo):** [`web/company-console-desktop/`](../../web/company-console-desktop/README.md) — Electron app that spawns `hsm_console` + Next standalone (same idea as [paperclip-desktop](https://github.com/aronprins/paperclip-desktop), for HSM’s company-console).

**What’s next:** only **§12 Open backlog** (unchecked items). Narrative gap vs Paperclip UI: **§7**.

---

This document is a **working issue / design note**: how memory works in **hyper-stigmergic-morphogenesisII** today, what **workspace file attachment** (Paperclip-style) is trying to solve, and **concrete directions** for shared memory—without forcing every agent to duplicate the same facts in private `MEMORY.md` files.

**Reference UX (Paperclip):** Agents have an on-disk **workspace** (folders + markdown). Creating an issue can **attach a workspace file**: a chip in the UI (“Workspace file attached”) plus a **stable path string** in the issue body (e.g. `Workspace file: workspace/content/.../file.md`). The file **stays on disk** for reuse; the issue carries a **pointer**, not a one-off paste. Optional **index files** (e.g. a curated summary or TOC) mirror the idea of a **small, fast-to-read layer** over a larger corpus—similar in spirit to layered / progressive context (abstract → overview → full) already used in code.

**Desktop shell:** Packaging the **company console** (Next.js UI + `hsm_console` API) inside a **native macOS host** (e.g. [aronprins/paperclip-desktop](https://github.com/aronprins/paperclip-desktop)) would align local workspace paths, deep links, and “open in editor” flows with how operators already work in Paperclip.

---

## 1. How memory works today (this repo)

Memory is **not one subsystem**; it is several layers that compose differently for **personal / enhanced agents** vs **Company OS** vs **graph / eval** tooling.

### 1.1 File-backed “Hermes / home directory” context

The enhanced personal agent (`src/personal/enhanced_agent.rs`) injects **truncated excerpts** from the agent home when present:

| Artifact | Role |
|----------|------|
| `MEMORY.md` | Durable operator notes; injected up to a byte budget |
| `USER.md` | User preferences / profile excerpt |
| `AGENTS.md` | Repo / multi-agent instructions |
| `prompt.template.md` | Optional system template |
| `skills/**/SKILL.md` | Skill index + on-demand reads |
| `autocontext/` | Session-to-session playbooks / hints |
| `business/pack.yaml` | Domain **BusinessPack** overlay |
| `HEARTBEAT.md` | Phased “cron” style ticks when heartbeat is enabled |
| `memory/journal/` | Optional turn journal |
| `memory/consolidated/` | **autoDream** rollups when enabled |

**Implication:** “Memory” for a running agent is often **whatever is on disk under `HSMII_HOME`**, plus **what fits in the prompt this turn**. There is **no built-in company-wide shared store** at this layer; sharing is usually **copy/paste, git, or duplicate files per agent home**.

### 1.2 In-process hybrid memory (`src/memory.rs`)

`HybridMemory` holds **typed entries** (`MemoryNetwork`: world fact, experience, entity summary, belief) with **vector + keyword + graph-ish indices**, **L0/L1/L2-style** progressive recall, and RRF-style fusion. This is **process-local** unless separately persisted (e.g. wired through Honcho / other stores).

### 1.3 World model & stigmergy (`src/hyper_stigmergy.rs`, `src/memory.rs` integration)

The **HyperStigmergicMorphogenesis** world tracks **beliefs, experiences, traces**, etc.—a different “memory” from markdown files: more **structured graph + narrative** than “notes in `MEMORY.md`”.

### 1.4 Honcho / peer representations (`src/honcho/`, `src/api/mod.rs`)

When enabled, **`/api/honcho/*`** exposes **peer representations**, **packed context** under a token budget, and **HybridMemory** stats / entries. This is a path toward **queryable, packed memory** for a **peer identity**, not yet a first-class **company namespace** shared by all workforce agents.

### 1.5 Company OS: durable *company* context, not per-agent memory

Company OS (`src/company_os/`) stores:

- **`companies.context_markdown`** — company-wide markdown blob (edited via console PATCH).
- **`hsmii_home`**, **paperclip import** — pack-derived skills / agents text in context.
- **`GET /api/company/tasks/:task_id/llm-context`** — composes **company `context_markdown`**, **recent shared `company_memory_entries`**, **recent agent-scoped entries** for the resolved `company_agents` row (if any), **current task** (spec + `workspace_attachment_paths` + `hsmii_home`), and **matched workforce agent profile** (`company_id` included for downstream tools).

**Implication:** Company-wide **facts** can live in **`context_markdown`**, the **`company_memory_entries`** pool (`scope` `shared` | `agent`), and per-task **workspace path pointers**. Runs can **query/append** the pool via **`company_memory_search`** / **`company_memory_append`** (scope enforced by the REST API).

### 1.6 Eval / memory graph (SQLite, etc.)

Eval and related modules maintain **run-scoped or graph-scoped** memory artifacts for benchmarks and tracing—these are **not** the same as production “agent recalls what the company decided last Tuesday.”

---

## 2. Problem statements

1. **Workspace file attachment** gives **durable pointers** + **reuse**; we should treat **task/issue spec** as able to reference **`hsmii_home`-relative paths** (and later validate / read via tools), not only free text.
2. **Company-wide truth** should not require **N agents × full `MEMORY.md` updates** when one policy changes.
3. **Newly spawned agents** need a **bootstrap pack**: what the company is, where repos live, which paths are canonical, who owns what—**without** manual duplication.
4. **Query model:** agents that **save** memories should optionally **write to a shared pool**; **recall** should support **`scope: agent | company`** (your APR insight).

---

## 3. Proposals (ambitious but implementable)

### 3.1 Shared memory pool (company scope)

Introduce a **first-class store** (Postgres fits existing Company OS) such as:

- **`company_memory_entries`**: `id`, `company_id`, `scope` (`shared` | `agent`), `company_agent_id` nullable, `title`, `body`, `tags[]`, `source` (human | agent | import), `created_at`, optional **`summary_l0` / `summary_l1`**, future **`embedding`** or **tsvector** for search.
- **API:** `GET/POST/PATCH/DELETE` under `/api/company/companies/:id/memory/...` with **`?scope=shared|agent|all`** and optional **`q`** (full-text + substring); `agent` requires **`company_agent_id`**.
- **Injection rule (implemented):** **`llm-context`** merges **(1)** `context_markdown`, **(2)** recent **shared** rows, **(3)** recent **agent** rows for the matched workforce agent, **(4)** task block, **(5)** agent profile addon.

**Creative twist — “index row” pattern:** Each shared topic can have a **`summary_l0`** (one line) and **`summary_l1`** (short paragraph) maintained by **heartbeat jobs** or **explicit consolidate**—the Kapathi-style “small surface over deep corpus,” aligned with existing **L0/L1/L2** machinery in `memory.rs`.

### 3.2 Propagate changes without touching every agent file

- **Single write, many readers:** Shared pool entries are **one row**; all agents **query** them. Private `MEMORY.md` only holds **personal deltas** (“I prefer briefings on Monday”).
- **Subscriptions / invalidation:** Optional **`memory_entry_versions`** or **`updated_at`** so agents can **detect staleness** and refresh summaries.
- **Governance:** Tie **high-impact** shared memories to **governance events** (Company OS already has governance logging)—e.g. “pricing policy v3” as a shared memory with audit trail.

### 3.3 Spawn / onboarding: “Company bootstrap bundle”

On **first task checkout** or **agent spawn**, inject a **fixed block** (generated once per company, cached):

- Parsed **`context_markdown`** TOC (if markdown headings).
- **`hsmii_home` path**, default **workspace roots**, **repo list** (from pack or manual config).
- **Links to shared memory queries** (“run `company_memory_search` with `mode=shared`” as a tool).

Optionally materialize **`COMPANY.md`** or **`SHARED_MEMORY_INDEX.md`** under pack import—**git-friendly**, diffable, and familiar to Paperclip users—while **Postgres remains source of truth** for search.

### 3.4 Task spec: workspace attachment fields

Extend **task** model (or spec markdown convention):

- **`workspace_attachment_paths: string[]`** (paths relative to company `hsmii_home` or agent workspace root).
- Console UI: Paperclip-like **chip + path in body** for humans; API accepts **structured list** for agents.

Tools (`read_file`, etc.) resolve paths against **configured roots** with **sandbox checks**.

### 3.5 Native macOS shell ([paperclip-desktop](https://github.com/aronprins/paperclip-desktop))

- **Embed** or **spawn** `hsm_console` and open **company-console** in a **WebView** (or system browser with localhost).
- **Deep links:** `paperclip://company/<id>/task/<id>` opens the right view.
- **Filesystem:** Expose **Reveal in Finder** for `hsmii_home` and attached workspace paths.

---

## 4. Milestones

**Completed work** is summarized in **Shipped in tree** at the top (single source of truth). **Open work** is **§12 Open backlog** only—this section avoids a second done checklist.

**Rough sequencing for what’s left:** (1) Paperclip-parity console (§7 / §12.1), (2) pool depth—vectors + summary refresh (§12.2), (3) creative graph/citations (§12.3).

---

## 5. Acceptance (archive)

Original acceptance targets for the MVP track are **met**; specifics live in **§1**, **Shipped in tree**, and the API catalog. Update this note only if you add new hard requirements.

---

## 6. Visual references (repo)

Screenshots illustrating workspace tree, “Workspace file attached” issue flow, and run log / attachment affordances are saved under:

- `.cursor/projects/Users-cno-hyper-stigmergic-morphogenesisII/assets/` (e.g. `IMG_7192-*.png`, `IMG_7193-*.png`, and related run views).

Use them when implementing console UX and when writing user-facing copy for attachment chips and path display.

---

## 7. Why we don’t have Paperclip-parity UI (yet)

This section is **narrative only** (no checkboxes): **why** the product still feels different from Paperclip. **What to build next** is **§12 Open backlog** only—edit the table below when parity improves so it stays accurate.

Paperclip (see screenshots in §6) combines **four** surfaces that feel “obvious” together:

| Surface | What it gives the operator | HSM company-console today |
|--------|----------------------------|---------------------------|
| **Per-agent workspace** | File tree (`memory/`, `MEMORY.md`, `HEARTBEAT.md`, nested content) with upload/new file | Partially aligned: pack `hsmii_home`, Memory panel, local `memory/` listing; not always the same **file-browser-first** chrome |
| **Issues + workspace attach** | “Workspace file attached” chip + **stable path** in body → pointer, not paste | **Structured** `workspace_attachment_paths` on tasks + path chips in UI; less of a **single modal** “new issue from file” flow |
| **Runs** | Sidebar of runs, status tags (Automation / Assignment / Heartbeat), **token + $** per run, expandable tool log | **Per-task** run strip + `log_tail` + operator append exists; **no** company-wide **Runs** sidebar with **$ / tok** yet |
| **Human-in-the-loop** | Inline comment (“This was a bad decision”) in the run stream | Handoff notes + run-log append from the task row; **not** the same **per-step interrupt** on a live orchestrated stream |

So: **backend direction** (shared pool, task paths, `llm-context`) is ahead of or beside **Paperclip-shaped product UI**. Closing the gap is mostly **frontend + run orchestration**—see **§12**.

---

## 8. APR recall model (make “shared vs mine” explicit)

When an agent **saves** a memory (tool or heartbeat):

- **`scope: agent`** → row in `company_memory_entries` (or file under agent workspace) visible only to that agent’s queries.
- **`scope: shared`** → same table, `scope=shared`; **all** authorized agents see it when they query the pool (subject to caps).

When an agent **queries** memory (APR / tool):

| Mode | Behavior |
|------|----------|
| **`mine`** | Only `scope=agent` for this `agent_id` (+ optional local `MEMORY.md` excerpt). |
| **`shared`** | Only `scope=shared` for the company (ranked by relevance to task/query). |
| **`both`** | Merge with a **budget**: e.g. top-K shared + top-K private, or RRF-style fusion (same spirit as `HybridMemory` in `src/memory.rs`). |

**Server must enforce scope** so a model cannot “guess” another agent’s private rows by prompt injection.

**Shipped:** tools **`company_memory_search`** / **`company_memory_append`** in `src/tools/company_os_tools.rs` (HTTP to the same REST routes). Tool **`company_memory_search`** only issues list calls with **`shared`**, **`agent` + `company_agent_id`**, or **both**—never **`scope=all`**.

---

## 9. Company-wide truth without updating N× `MEMORY.md`

**Core idea:** treat **shared memory** as the **broadcast layer**; per-agent files as **scratch + style**.

1. **Single write, many readers** — Policy changes as **one** shared entry (or versioned row); agents **pull** via query, not **copy** into 12 `MEMORY.md` files.
2. **`context_markdown` + shared pool** — Human-edited “constitution” stays in `context_markdown`; **operational** facts (“COM-500 fix merged”, “API base URL”) live in **searchable** shared entries with tags.
3. **Index rows (`summary_l0` / `summary_l1`)** — Heartbeat or batch job **refreshes short surfaces** so every agent gets **the same one-liner** without re-reading long bodies (§3.1).
4. **`updated_at` / version in prompt** — Inject “shared memory snapshot as of **timestamp**” so agents know when to **re-query** after governance events.
5. **Optional git mirror** — Export `SHARED_MEMORY_INDEX.md` from DB for **diff-friendly** review; Postgres (or API) remains **source of truth** for recall.
6. **“Company line” channel** — High-signal announcements (release, incident) as **typed** shared memories (`kind: broadcast`) always included in **first bucket** of context until expired.

---

## 10. More creative directions (optional / later)

- **Stigmergic memory** — *Partially shipped:* `context_notes` + `POST …/stigmergic-note` + `llm-context` merge. Extensions: richer schema, caps policy, UI polish.
- **Memory DAG** — Shared entries **supersede** or **support** other entries (like beliefs in `hyper_stigmergy`); query returns **resolved** view (“current pricing policy” follows edges).
- **Role-aware retrieval** — CMO queries bias **brand/customer** tags; engineer bias **repo/path** tags—same pool, different ranking.
- **Consent to promote** — Private note → **suggest shared** with one click in console (human or lead agent approves).
- **Negative shared memory** (“do not repeat”) — Explicit **anti-patterns** company-wide, small token cost, high priority in merge.
- **Cross-run citations** — Runs log links **memory entry IDs** used that step (audit + debugging), similar to Paperclip run ↔ issue linkage.

---

## 11. One-line answers (for issue description)

- **How do we share information company-wide without updating everyone’s memories?**  
  **Write once to `scope=shared` (or `context_markdown` for static constitution); agents query the pool—private `MEMORY.md` only for personal deltas.**

- **How do we create shared memories?**  
  **API/console CRUD on `company_memory_entries` with `scope=shared`, plus optional agent tools that *append* with server-enforced scope; optional export to markdown for review.**

- **Why don’t we have the same UI as Paperclip?**  
  We prioritized durable APIs (`llm-context`, paths, shared pool). The remaining *console/orchestration* slice is **§7** (why) and **§12** (what to build).

---

## 12. Open backlog (unchecked only)

**Done items are not listed here**—they live in **Shipped in tree** at the top and in git history. **§7** explains the Paperclip gap in prose; **§12** is the single actionable checklist.

### Console & orchestration (Paperclip parity)

- [ ] File-browser-first **workspace** per agent or company pack.
- [ ] Single **“new issue / task + attach file”** modal (path chip + body); structured paths on tasks already exist.
- [ ] **Runs sidebar** (company- or agent-scoped): status tags, **token + $**, expandable tool log—not only the per-task strip.
- [ ] **Inline interrupt** on a **live** orchestrated run stream (vs handoff note + run-log append today).

### Pool, search, bootstrap

- [ ] **Heartbeat / LLM refresh** of `summary_l0` / `summary_l1` for existing rows (heuristic-on-create already shipped).
- [ ] **Vector / semantic** search (embeddings or pgvector).
- [ ] **Dedicated checkout/spawn hook** that injects bootstrap (beyond what `llm-context` already adds).

### Creative / product

- [ ] **Memory DAG** (supersedes / supports) and resolved read.
- [ ] **Broadcast `kind` TTL** / expiry in context merge.
- [ ] **Cross-run citations** (memory entry IDs ↔ run steps).
- [ ] **Role-aware retrieval**, **promote private → shared**, **negative shared memory**.
