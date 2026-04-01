-- Company OS v1 (see documentation/schemas/company_os_v1.sql)
CREATE TABLE companies (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug            TEXT NOT NULL UNIQUE,
    display_name    TEXT NOT NULL,
    hsmii_home      TEXT,
    metadata        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE goals (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    parent_goal_id      UUID REFERENCES goals(id) ON DELETE SET NULL,
    title               TEXT NOT NULL,
    description         TEXT,
    status              TEXT NOT NULL DEFAULT 'active',
    sort_order          INT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_goals_company ON goals(company_id);
CREATE INDEX idx_goals_parent ON goals(parent_goal_id);

CREATE TABLE tasks (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    primary_goal_id     UUID REFERENCES goals(id) ON DELETE SET NULL,
    goal_ancestry       JSONB NOT NULL DEFAULT '[]',
    title               TEXT NOT NULL,
    specification       TEXT,
    state               TEXT NOT NULL DEFAULT 'open',
    owner_persona       TEXT,
    assignee_agent_id   TEXT,
    checked_out_by      TEXT,
    checked_out_until   TIMESTAMPTZ,
    priority            INT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_tasks_company_state ON tasks(company_id, state);
CREATE INDEX idx_tasks_goal ON tasks(primary_goal_id);

CREATE TABLE budgets (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    scope           TEXT NOT NULL,
    scope_ref       TEXT,
    kind            TEXT NOT NULL,
    cap_amount      NUMERIC(18,4) NOT NULL,
    window_start    DATE NOT NULL,
    window_end      DATE,
    hard_stop       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_budgets_company ON budgets(company_id);

CREATE TABLE spend_events (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    budget_id       UUID REFERENCES budgets(id) ON DELETE SET NULL,
    task_id         UUID REFERENCES tasks(id) ON DELETE SET NULL,
    agent_ref       TEXT,
    kind            TEXT NOT NULL,
    amount          NUMERIC(18,4) NOT NULL,
    unit            TEXT NOT NULL DEFAULT 'usd',
    external_ref    TEXT,
    meta            JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_spend_company_time ON spend_events(company_id, created_at);

CREATE TABLE governance_events (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    actor           TEXT NOT NULL,
    action          TEXT NOT NULL,
    subject_type    TEXT NOT NULL,
    subject_id      TEXT NOT NULL,
    payload         JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_gov_company ON governance_events(company_id, created_at DESC);

CREATE TABLE agent_bindings (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    label           TEXT NOT NULL,
    runtime_kind    TEXT NOT NULL,
    endpoint        TEXT,
    config          JSONB NOT NULL DEFAULT '{}',
    status          TEXT NOT NULL DEFAULT 'active',
    last_heartbeat  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_agents_company ON agent_bindings(company_id);

CREATE TABLE operations_yaml_snapshots (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    content_yaml    TEXT NOT NULL,
    content_hash    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
