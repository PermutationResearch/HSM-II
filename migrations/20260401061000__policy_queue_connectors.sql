-- Day 1-2 foundation: policy engine, queue/SLA fields, connectors, stricter governance metadata.

ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS due_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS sla_policy TEXT,
    ADD COLUMN IF NOT EXISTS escalate_after TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS status_reason TEXT;

CREATE INDEX IF NOT EXISTS idx_tasks_company_due_at ON tasks(company_id, due_at);
CREATE INDEX IF NOT EXISTS idx_tasks_company_escalate_after ON tasks(company_id, escalate_after);
CREATE INDEX IF NOT EXISTS idx_tasks_company_priority_due ON tasks(company_id, priority DESC, due_at);

ALTER TABLE governance_events
    ADD COLUMN IF NOT EXISTS severity TEXT NOT NULL DEFAULT 'info',
    ADD COLUMN IF NOT EXISTS decision TEXT;

CREATE TABLE IF NOT EXISTS policy_rules (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    action_type     TEXT NOT NULL,
    risk_level      TEXT NOT NULL,
    amount_min      NUMERIC(18,4),
    amount_max      NUMERIC(18,4),
    decision_mode   TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (decision_mode IN ('auto', 'admin_required', 'blocked')),
    CHECK (risk_level IN ('low', 'medium', 'high', 'critical')),
    CHECK (amount_min IS NULL OR amount_max IS NULL OR amount_min <= amount_max)
);

CREATE INDEX IF NOT EXISTS idx_policy_rules_company_action_risk
    ON policy_rules(company_id, action_type, risk_level);

CREATE TABLE IF NOT EXISTS connector_accounts (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    provider        TEXT NOT NULL,
    account_ref     TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active',
    config_meta     JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(company_id, provider, account_ref)
);

CREATE INDEX IF NOT EXISTS idx_connector_accounts_company_provider
    ON connector_accounts(company_id, provider);
