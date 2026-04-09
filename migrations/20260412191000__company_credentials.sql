-- Company-scoped service credentials for operator-connected tools and MCP-like integrations.

CREATE TABLE IF NOT EXISTS company_credentials (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id       UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    provider_key     TEXT NOT NULL,
    label            TEXT NOT NULL DEFAULT '',
    env_var          TEXT,
    secret_value     TEXT NOT NULL,
    masked_preview   TEXT NOT NULL DEFAULT '',
    notes            TEXT,
    status           TEXT NOT NULL DEFAULT 'connected',
    metadata         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_company_credentials_provider UNIQUE (company_id, provider_key),
    CONSTRAINT company_credentials_status_chk CHECK (status IN ('connected', 'missing', 'error'))
);

CREATE INDEX IF NOT EXISTS idx_company_credentials_company
    ON company_credentials(company_id, provider_key);

COMMENT ON TABLE company_credentials IS
    'Operator-managed API credentials and connection metadata for company tools/integrations.';
