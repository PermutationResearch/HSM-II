-- Company-wide shared memory pool + workspace file pointers on tasks.
CREATE TABLE company_memory_entries (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    scope               TEXT NOT NULL CHECK (scope IN ('shared', 'agent')),
    company_agent_id    UUID REFERENCES company_agents(id) ON DELETE SET NULL,
    title               TEXT NOT NULL,
    body                TEXT NOT NULL DEFAULT '',
    tags                TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    source              TEXT NOT NULL DEFAULT 'human',
    summary_l0          TEXT,
    summary_l1          TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_company_memory_company ON company_memory_entries(company_id);
CREATE INDEX idx_company_memory_company_scope ON company_memory_entries(company_id, scope);
CREATE INDEX idx_company_memory_agent ON company_memory_entries(company_id, company_agent_id);

ALTER TABLE tasks ADD COLUMN IF NOT EXISTS workspace_attachment_paths JSONB NOT NULL DEFAULT '[]'::jsonb;
