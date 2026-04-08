# Sketch: agent run id, feedback events, task promotion (Company OS)

**Goal:** Model **execution runs** (e.g. harness / Paperclip Nexus), **human feedback** on a run (optionally anchored to a step), and **optional promotion** to a normal **`tasks`** row—without overloading `task_run_snapshots`, which today is **one telemetry row per task** (`migrations/20260402140000__task_run_snapshots.sql`), not a replay log.

**Principles**

- **`tasks`** remain the unit of assignable work (`owner_persona`, `checked_out_by`, `context_notes`, `llm-context`).
- **Runs** are **sessions** (many per task over time, or ad-hoc runs with no task).
- **Feedback** is append-only **events** on a run; **promotion** creates a **new task** and back-links for audit.

---

## 1. Entities

### 1.1 `agent_runs` (new)

| Column | Type | Notes |
|--------|------|--------|
| `id` | UUID PK | Canonical **run id** in this DB. |
| `company_id` | UUID FK → `companies` | |
| `task_id` | UUID NULL FK → `tasks` | Optional: run is executing *this* task. |
| `company_agent_id` | UUID NULL FK → `company_agents` | Workforce agent (e.g. “Nexus”). |
| `external_run_id` | TEXT NULL | Idempotent sync from Paperclip / harness (`UNIQUE (company_id, external_run_id)` WHERE NOT NULL). |
| `external_system` | TEXT NOT NULL DEFAULT `'hsm'` | `'paperclip'`, `'harness'`, … |
| `status` | TEXT | `running` \| `success` \| `error` \| `cancelled` |
| `started_at` | TIMESTAMPTZ | |
| `finished_at` | TIMESTAMPTZ NULL | |
| `summary` | TEXT NULL | Short outcome for sidebar. |
| `meta` | JSONB | Tool counts, model, cost, branch, worktree path, etc. |

**Indexes:** `(company_id, started_at DESC)`, `(task_id)`, `(company_id, company_agent_id, started_at DESC)`.

### 1.2 `run_feedback_events` (new)

| Column | Type | Notes |
|--------|------|--------|
| `id` | UUID PK | |
| `run_id` | UUID FK → `agent_runs` ON DELETE CASCADE | |
| `company_id` | UUID FK | Denormalized for RLS / listing (same as run’s company). |
| `step_index` | INT NULL | Optional 0-based step in timeline (matches Paperclip UI). |
| `step_external_id` | TEXT NULL | If the UI uses opaque step ids. |
| `actor` | TEXT NOT NULL | `operator:email` or `agent:…`. |
| `kind` | TEXT NOT NULL | `comment` \| `correction` \| `blocker` \| `praise` (extend as needed). |
| `body` | TEXT NOT NULL | Feedback text. |
| `created_at` | TIMESTAMPTZ | |
| `spawned_task_id` | UUID NULL FK → `tasks` | Set when this event **promoted** a task. |

**Indexes:** `(run_id, created_at)`, `(company_id, created_at DESC)`, partial `(spawned_task_id) WHERE spawned_task_id IS NOT NULL`.

### 1.3 `tasks` (optional columns)

Add optional provenance (migration):

| Column | Type | Notes |
|--------|------|--------|
| `source_run_id` | UUID NULL FK → `agent_runs` | Task created from a run context. |
| `source_feedback_event_id` | UUID NULL FK → `run_feedback_events` | Precise promotion source. |

**Constraint:** if `source_feedback_event_id` is set, `source_run_id` should match that event’s `run_id` (enforce in app or CHECK via trigger).

### 1.4 Relation to existing tables

- **`task_run_snapshots`:** Keep as **live strip** telemetry for the **current** task execution; optionally update `task_run_snapshots` from the **latest** `agent_runs` row for `task_id` for UI convenience, or leave separate—do not conflate schemas.
- **`governance_events`:** Optional duplicate **emit** on `promote` / `feedback` for analytics; source of truth for feedback remains `run_feedback_events`.

---

## 2. API (REST, `hsm_console` style)

Base: `/api/company/companies/:company_id/…`

| Method | Path | Body / query | Result |
|--------|------|--------------|--------|
| `POST` | `/agent-runs` | `{ task_id?, company_agent_id?, external_run_id?, external_system?, meta? }` | `201` + `{ run }` (create or return existing if `external_run_id` unique hit) |
| `PATCH` | `/agent-runs/:run_id` | `{ status?, summary?, meta?, finished_at? }` | `{ run }` |
| `GET` | `/agent-runs` | `?task_id=`, `?company_agent_id=`, `limit` | `{ runs: [...] }` |
| `GET` | `/agent-runs/:run_id` | | `{ run, feedback: [...] }` or nested |
| `POST` | `/agent-runs/:run_id/feedback` | `{ body, kind?, step_index?, step_external_id?, actor }` | `201` + `{ event }` |
| `POST` | `/agent-runs/:run_id/feedback/:event_id/promote-task` | `{ title?, specification?, owner_persona?, priority? }` | `201` + `{ task }` — creates **`tasks`** row, sets `source_*`, sets `spawned_task_id` on event |

**Auth:** Same as other Company OS routes (bearer if configured).

**Idempotency:** `POST /agent-runs` with same `(company_id, external_system, external_run_id)` returns **200** with existing run + `idempotent: true` in the JSON body.

---

## 3. Tooling (for agents)

- **`company_run_feedback_append`** — HTTP tool: `run_id`, `body`, optional `step_index` (mirrors `POST …/feedback`).
- **`company_promote_feedback_to_task`** — `run_id`, `event_id`, `title`, `specification`.

Resolve `company_id` / `run_id` from `task_id` + `llm-context` when bound to a task.

---

## 4. Sync from Paperclip (optional)

- **Inbound:** Webhook or poll: upsert `agent_runs` by `external_run_id`, append feedback as `run_feedback_events`.
- **Outbound:** On promote, POST Paperclip issue API if you want a mirrored COM-* ticket (reuse existing `goal` / issue mapping patterns).

---

## 5. Minimal MVP order

1. Migration: `agent_runs` + `run_feedback_events` + optional `tasks` columns.
2. CRUD + promote endpoint in `src/company_os/` (new `runs.rs` or `agent_runs.rs`).
3. Register routes + catalog entry in `company_os/mod.rs`.
4. Company console: list recent runs for an agent; feedback composer; “Create task from this”.

---

## 6. Implemented (this repo)

- **Migration:** `migrations/20260409120000__agent_runs.sql` — tables `agent_runs` and `run_feedback_events` (promotion uses `spawned_task_id` only; no `tasks` provenance columns yet).
- **Handlers:** `src/company_os/agent_runs.rs`, merged in `company_os::router()`.
- **Catalog:** entries under `company_os_api_catalog_endpoints()` in `src/company_os/mod.rs`.
- **HTTP tools:** `company_run_feedback_append`, `company_promote_feedback_to_task` in `src/tools/company_os_tools.rs`, registered in `register_all_tools`.

---

*This is a design sketch; naming (`agent_runs` vs `harness_runs`) can follow your product naming.*
