-- Shared cross-operator thread sessions (company-scoped).

CREATE TABLE IF NOT EXISTS shared_thread_sessions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id    UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    session_key   TEXT NOT NULL,
    title         TEXT NOT NULL DEFAULT '',
    participants  JSONB NOT NULL DEFAULT '[]'::jsonb,
    state         JSONB NOT NULL DEFAULT '{}'::jsonb,
    is_active     BOOLEAN NOT NULL DEFAULT true,
    created_by    TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_shared_thread_sessions_company_key UNIQUE (company_id, session_key)
);

CREATE INDEX IF NOT EXISTS idx_shared_thread_sessions_company_updated
    ON shared_thread_sessions(company_id, updated_at DESC);
