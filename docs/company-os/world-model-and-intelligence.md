# Company world model, capabilities, and intelligence

This document is the **contract** for how “what the company knows” and “what runs” fit together. It exists so the console, agents rail, and issues stay **one game**: Postgres Company OS keyed by **`company_id`**, not a parallel in-memory product.

---

## 1. World model (company) — canonical operational graph

Treat **Postgres Company OS** as the **single source of operational truth** for a company:

| Area | Role in the graph |
|------|-------------------|
| **Tasks** | Work units: state, checkout, `requires_human`, attachments, `context_notes`, **`capability_refs`** (links to skills/SOPs/tools/packs/agents), spawn/handoffs, spend links. |
| **Runs / telemetry** | Honest signals: terminal status, tool usage, logs — surfaced on tasks and in governance-style events where wired. |
| **`company_memory_entries`** | Durable shared and agent-scoped facts; search/append via API and tools. |
| **`companies.context_markdown`** | Company-wide narrative the LLM layer always sees (with memory and task context). |
| **`GET …/tasks/:id/llm-context`** | Composed **read model** for agents: context markdown, memory pool, task block, workforce profile, headings/TOC as implemented. |
| **Goals, governance events, spend** | Alignment, policy trail, and cost — same `company_id`. |

Anything labeled **“intelligence”** in product terms should **read and write through this graph** (or explicitly **sync into** it), not maintain a second ledger the UI pretends is equally real.

---

## 2. Capabilities — atomic building blocks

These are **already modular** in the product; the rule is to **link them into the world model**, not float them beside it:

- **Tools & skills** — registered and invoked with company/task context.
- **Packs** — bootstrap templates; after import, **roster and skills** should resolve to **Postgres** (`company_agents`, adapter config, etc.).
- **Workforce agents** — rows with roles, budgets, profiles; **`llm-context`** and task checkout should resolve **persona → agent row** where possible.
- **SOPs / playbooks** — procedures tied to **projects and tasks** (see [playbooks-projects-and-visions.md](./playbooks-projects-and-visions.md)).

**Direction:** task and agent APIs (and UIs) should carry **explicit references** (IDs, personas, skill refs) so the graph is queryable — “who is on this,” “what skills apply,” “which SOP governs this stream.”

**Implemented in tree:** `tasks.capability_refs` (JSONB array of `{ "kind", "ref" }` or create-body strings normalized to `kind: skill`). Set at **create**, **PATCH `/api/company/tasks/{id}/context`**, and **bundle import/export**; copied to **spawned subtasks**; merged into **`GET …/tasks/{id}/llm-context`** and the **Intelligence** workflow feed when updated (`task_capability_refs_updated`). **Pack bridge:** `POST …/import-paperclip-home` pulls on-disk agents/skills into Postgres for the company.

**Paperclip → Postgres (goals & DRIs):** `goals.paperclip_goal_id` + `paperclip_snapshot` for upserts; **`POST /api/company/companies/{id}/sync/paperclip-goals`** (optional JSON `{ "goals": [...] }`, or empty body when `hsm_console` runs with in-process `IntelligenceLayer`); **`POST …/sync/paperclip-dris`** same for `{ "dris": [...] }`. **`dri_assignments`** table + **`GET/POST …/dri-assignments`** and **`PATCH/DELETE …/dri-assignments/{row_id}`** for first-class org DRIs (including manual rows).

---

## 3. Intelligence layer — two APIs, one truth

| Surface | Scope | Use |
|---------|--------|-----|
| **`/api/company/companies/{company_id}/…`** | Per company | **Canonical**: goals, tasks, memory, spend, intelligence summary, `llm-context`, etc. |
| **`/api/paperclip/*`** (proxied in dev) | Global / in-memory demo | **Optional**: composition, routing experiments, demos — **not** a second company dashboard of record. |

**Allowed patterns:**

1. **Embed** composition/routing **inside** Company OS (per `company_id`) so all state lives in Postgres; or  
2. **Sync** Paperclip-style state **into** Postgres (goals, signals, DRIs) so operators still have **one** store and **one** UI truth.

**Anti-pattern:** two first-class UIs that each imply their own goals/signals/registry without migration or sync — that splits hierarchy (alignment, backlog, who is on what) across “Postgres truth” and “optional global layer.”

Workspace **Intelligence** in the company console should prefer **`GET …/intelligence/summary`** (and related company routes), not Paperclip-only views.

---

## 4. Interfaces — edges of the same model

| Interface | Expectation |
|-----------|-------------|
| **Company console** | Reads/writes **`/api/company/…`** for the selected company; copy should not imply a global parallel state is authoritative. |
| **Agents rail** | Chat and task actions **mutate** the same task graph (checkout, notes, runs) for that `company_id`. |
| **Issues / my-work** | Same task list and states as the API; no shadow issue system. |

If a feature cannot point at a **`company_id`** and a concrete row or API path, it is not yet part of the world model — it is integration or demo debt.

---

## See also

- [Intelligence layer & DRI alignment](./intelligence-layer-dri-alignment.md) — composer vs ledger vs edge (DRIs); integration checklist for external intelligence.  
- [Memory, workspace attachments, shared context](../issues/memory-workspace-shared-context.md) — `company_memory`, `context_notes`, `llm-context`, tools.  
- [Playbooks, projects, and visions](./playbooks-projects-and-visions.md) — how SOPs sit on the task/project graph.
