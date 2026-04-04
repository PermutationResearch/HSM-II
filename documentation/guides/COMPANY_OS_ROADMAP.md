# Company OS roadmap (Paperclip-class surface on HSM-II)

This repo is strong on **agents, tools, evals, DSPy/GEPA**, and **file-based** packs—but light on a **unified company control plane**. External reference for the target shape: [paperclipai/paperclip](https://github.com/paperclipai/paperclip) (“orchestration for … agents”: goals, org, tickets, budgets, governance, multi-company, heartbeats).

This document maps **your gap table** to **concrete phases** and repo touchpoints.

## North-star architecture

| Layer | Choice | Notes |
|--------|--------|--------|
| **UI** | Next.js (extend `web/company-console`) or new `web/company-os` | Task board, company switcher, budgets, governance timeline; mobile-friendly layout |
| **API** | Extend `hsm_console` (Axum) or sidecar service | Same Rust codebase as `EnhancedPersonalAgent` for shared auth + spend hooks later |
| **Store** | PostgreSQL v1 (`documentation/schemas/company_os_v1.sql`) | Company-scoped tables; migrations via your preferred runner |
| **Workers** | Existing: `personal_agent`, Hermes, `hsm_a2a_adapter` | Register as `agent_bindings` rows; heartbeats hit API |

Sync path: **import/export** `config/operations.yaml` ↔ DB (`operations_yaml_snapshots` + ETL scripts) so current file workflow keeps working.

## Implemented in this repo (MVP)

- **Migrations:** `migrations/20250401120000__company_os.sql` (applied automatically when the URL below is set).
- **API (Axum, `hsm_console`):** `GET /api/company/health`, `GET|POST /api/company/companies`, `GET|POST .../companies/:id/goals`, `GET|POST .../companies/:id/tasks`, `POST /api/company/tasks/:id/checkout`.
- **Env:** `HSM_COMPANY_OS_DATABASE_URL` — if unset, company routes return 503 with a JSON hint.
- **Console UI:** `web/company-console` → **Company OS** tab (list companies, create company, list/create tasks).
- **Dashboard:** `GET /api/console/stats` includes `tasks_in_progress` from Postgres when configured and `company_os: true/false`.

Still **not** done: spend ledger writes on LLM calls, governance UI, mobile shell, template marketplace import/export.

## Phase map (gap → work)

### 1. Company OS UI (task-manager, dashboard, portfolio)

- **MVP:** Company list + task list + task detail (spec, goal breadcrumb, state) + spend summary panel (read-only from `spend_events`).
- **Repo:** new routes under `web/company-console` (or parallel app); `NEXT_PUBLIC_API_BASE` → console Axum port.
- **API:** `GET/POST /api/company/companies`, `.../tasks`, `.../tasks/:id/checkout`, `.../goals` tree.

### 2. Persistence (Postgres, migrations, company-scoped entities)

- **Deliverable:** Apply `documentation/schemas/company_os_v1.sql` with a migration tool; seed script for dev.
- **Rust:** optional `src/company_os/` module with `sqlx` or thin HTTP client if API is separate process.

### 3. Multi-company in one deployment

- **Every query** filtered by `company_id`; JWT or session carries active company; optional “portfolio” view = list companies + aggregate health.
- **Align with** existing pattern: `HSMII_HOME` per company can sit in `companies.hsmii_home` for agent filesystem isolation.

### 4. Atomic task checkout + budget hard stop

- **Checkout:** `UPDATE tasks SET checked_out_by = $agent, checked_out_until = now()+ttl, state = 'in_progress' WHERE id = $id AND checked_out_by IS NULL RETURNING *`.
- **Spend:** Before `OllamaClient` / OpenAI calls in one choke point, `INSERT spend_events` + `SELECT sum(amount) ... budget window` → fail closed if over cap (`hard_stop`).
- **Repo touchpoint:** `src/llm/` or `enhanced_agent` wrapper—**this is the main greenfield engineering** beyond schema/UI.

### 5. Goal ancestry on every task

- **DB:** `goals.parent_goal_id` + `tasks.goal_ancestry` JSON array (or maintain closure table later).
- **API:** On task create, resolve lineage from primary goal; inject into prompts for `EnhancedPersonalAgent` when `task_id` is passed (header or tool).

### 6. Governance UX (board, pause, rollback)

- **MVP:** `governance_events` log + UI filters; actions call same API as harness-style approvals where possible.
- **Rollback:** store versioned blobs (e.g. `company_config_revisions`) in a follow-up migration; v1 can be “export YAML snapshot + restore.”

### 7. BYO agent wiring (polished)

- **DB:** `agent_bindings` + heartbeat endpoint `POST /api/company/agents/:id/heartbeat`.
- **Product:** “Add agent” form: runtime kind, endpoint, secret; docs linking Hermes / A2A / OpenClaw-style runners.

### 8. Mobile / portfolio

- **Responsive** Company OS UI + read-heavy APIs; defer native app.

### 9. Templates marketplace

- **Near-term:** `hsm-business-pack init …` + “export company bundle” (pack + `operations.yaml` + seed goals/tasks JSON).
- **Later:** curated registry (out of scope for core repo unless you add `templates/company_bundles/`).

## Order of implementation (recommended)

1. **Schema + migrations + seed**  
2. **Axum CRUD for companies/goals/tasks** (behind API key)  
3. **Minimal UI: company + task list**  
4. **Checkout + spend ledger writes** (even if budget enforcement is “warn only” first)  
5. **Wire one agent path** (e.g. personal_agent accepts `HSM_COMPANY_ID` + injects goal context)  
6. **Budget middleware** on LLM calls  
7. **Governance log + pauses**

## Related docs

- `documentation/guides/ENTERPRISE_OPS_PLANE.md` — ops concepts + `operations.yaml`  
- `documentation/guides/BUSINESS_PACK.md` — personas and pack injection  
- `templates/hsmii/operations.example.yaml` — file-first ops before DB is mandatory  
