-- Paperclip-style workforce agents (name, role, org chart, adapter, budget, briefing).
CREATE TABLE company_agents (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name                TEXT NOT NULL,
    role                TEXT NOT NULL DEFAULT 'worker',
    title               TEXT,
    capabilities        TEXT,
    reports_to          UUID REFERENCES company_agents(id) ON DELETE SET NULL,
    adapter_type        TEXT,
    adapter_config      JSONB NOT NULL DEFAULT '{}',
    budget_monthly_cents INTEGER,
    briefing            TEXT,
    status              TEXT NOT NULL DEFAULT 'active',
    sort_order          INT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_company_agent_name UNIQUE (company_id, name)
);

CREATE INDEX idx_company_agents_company ON company_agents(company_id);
CREATE INDEX idx_company_agents_reports ON company_agents(reports_to);
