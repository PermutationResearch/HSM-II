-- Link Postgres goals to in-memory Paperclip goal ids (one-way sync target).
ALTER TABLE goals
    ADD COLUMN IF NOT EXISTS paperclip_goal_id TEXT,
    ADD COLUMN IF NOT EXISTS paperclip_snapshot JSONB NOT NULL DEFAULT '{}'::jsonb;

COMMENT ON COLUMN goals.paperclip_goal_id IS 'When set, row was upserted from Paperclip IntelligenceLayer goal id.';
COMMENT ON COLUMN goals.paperclip_snapshot IS 'Last synced assignee, tags, capabilities, priority from Paperclip (JSON).';

CREATE UNIQUE INDEX IF NOT EXISTS idx_goals_company_paperclip_goal
    ON goals (company_id, paperclip_goal_id)
    WHERE paperclip_goal_id IS NOT NULL AND btrim(paperclip_goal_id) <> '';

-- Org-level DRI assignments (first-class; can mirror Paperclip dri_registry or stand alone).
CREATE TABLE IF NOT EXISTS dri_assignments (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    dri_key             TEXT NOT NULL,
    display_name        TEXT NOT NULL,
    agent_ref           TEXT NOT NULL,
    domains             TEXT[] NOT NULL DEFAULT '{}',
    authority           JSONB NOT NULL DEFAULT '{}',
    tenure_kind         TEXT NOT NULL DEFAULT 'persistent',
    valid_from          TIMESTAMPTZ,
    valid_until         TIMESTAMPTZ,
    paperclip_dri_id    TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_dri_assignments_company_key UNIQUE (company_id, dri_key)
);

CREATE INDEX IF NOT EXISTS idx_dri_assignments_company_agent
    ON dri_assignments (company_id, agent_ref);

COMMENT ON TABLE dri_assignments IS 'Cross-cutting outcome owners (DRI); sync from Paperclip or manage via Company OS API.';
