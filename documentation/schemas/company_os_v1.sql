-- Company OS v1 — PostgreSQL schema sketch for multi-company orchestration.
-- Aligns with gaps vs Paperclip-style product: org, goals, tasks, budgets, governance, BYO agents.
-- Apply with your migration runner (sqlx, refinery, etc.); IDs are UUID text-compatible.

BEGIN;

CREATE TABLE IF NOT EXISTS companies (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug            TEXT NOT NULL UNIQUE,
    display_name    TEXT NOT NULL,
    hsmii_home      TEXT,           -- optional filesystem profile root
    metadata        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Goal hierarchy: mission → objective → key result (flex depth via parent_goal_id).
CREATE TABLE IF NOT EXISTS goals (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    parent_goal_id      UUID REFERENCES goals(id) ON DELETE SET NULL,
    title               TEXT NOT NULL,
    description         TEXT,
    status              TEXT NOT NULL DEFAULT 'active', -- active|paused|done|cancelled
    sort_order          INT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_goals_company ON goals(company_id);
CREATE INDEX IF NOT EXISTS idx_goals_parent ON goals(parent_goal_id);

-- Operational unit of work; ties to goal lineage for “why.”
CREATE TABLE IF NOT EXISTS tasks (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    primary_goal_id     UUID REFERENCES goals(id) ON DELETE SET NULL,
    -- Denormalized ancestry for cheap reads (e.g. JSON array of goal UUIDs root→leaf).
    goal_ancestry       JSONB NOT NULL DEFAULT '[]',
    title               TEXT NOT NULL,
    specification       TEXT,       -- acceptance criteria, I/O, forbidden actions
    state               TEXT NOT NULL DEFAULT 'open', -- open|in_progress|blocked|done|cancelled
    owner_persona       TEXT,       -- e.g. business pack persona key
    assignee_agent_id   TEXT,       -- BYO agent instance id / external ref
    checked_out_by      TEXT,       -- must be cleared on complete/fail; supports atomic claim
    checked_out_until   TIMESTAMPTZ,
    priority            INT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tasks_company_state ON tasks(company_id, state);
CREATE INDEX IF NOT EXISTS idx_tasks_goal ON tasks(primary_goal_id);

-- Atomic checkout: enforce in application with UPDATE ... WHERE id = $1 AND checked_out_by IS NULL RETURNING *.

-- Budget caps (monthly or custom window).
CREATE TABLE IF NOT EXISTS budgets (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    scope           TEXT NOT NULL,  -- company|role|agent
    scope_ref       TEXT,           -- role slug or agent id when scope ≠ company
    kind            TEXT NOT NULL,  -- llm_usd|api_usd|...
    cap_amount      NUMERIC(18,4) NOT NULL,
    window_start    DATE NOT NULL,
    window_end      DATE,
    hard_stop       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_budgets_company ON budgets(company_id);

-- Spend ledger — middleware appends before each LLM/tool spend.
CREATE TABLE IF NOT EXISTS spend_events (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    budget_id       UUID REFERENCES budgets(id) ON DELETE SET NULL,
    task_id         UUID REFERENCES tasks(id) ON DELETE SET NULL,
    agent_ref       TEXT,
    kind            TEXT NOT NULL,
    amount          NUMERIC(18,4) NOT NULL,
    unit            TEXT NOT NULL DEFAULT 'usd',
    external_ref    TEXT,           -- provider invoice id / request id
    meta            JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_spend_company_time ON spend_events(company_id, created_at);

-- Governance: approvals, pauses, config pins (extend as needed).
CREATE TABLE IF NOT EXISTS governance_events (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    actor           TEXT NOT NULL,   -- human email or system
    action          TEXT NOT NULL,   -- approve|deny|pause_agent|resume|rollback_config|...
    subject_type    TEXT NOT NULL,   -- task|agent|budget|company_config
    subject_id      TEXT NOT NULL,
    payload         JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_gov_company ON governance_events(company_id, created_at DESC);

-- BYO agent registration (heartbeat endpoint, webhook, CLI).
CREATE TABLE IF NOT EXISTS agent_bindings (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    label           TEXT NOT NULL,
    runtime_kind    TEXT NOT NULL,  -- hermes|openclaw|http|cursor|custom
    endpoint        TEXT,           -- URL or command hint
    config          JSONB NOT NULL DEFAULT '{}',
    status          TEXT NOT NULL DEFAULT 'active', -- active|paused|terminated
    last_heartbeat  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_agents_company ON agent_bindings(company_id);

-- Optional: sync from / export to operations.yaml
CREATE TABLE IF NOT EXISTS operations_yaml_snapshots (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    content_yaml    TEXT NOT NULL,
    content_hash    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMIT;
