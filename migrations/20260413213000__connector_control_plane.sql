CREATE TABLE IF NOT EXISTS company_connectors (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    connector_key TEXT NOT NULL,
    label TEXT NOT NULL,
    provider_key TEXT NOT NULL,
    base_url TEXT,
    auth_mode TEXT NOT NULL DEFAULT 'api_key',
    credential_provider_key TEXT,
    policy JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'configured',
    last_success_at TIMESTAMPTZ,
    last_failure_at TIMESTAMPTZ,
    last_error TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (company_id, connector_key)
);

CREATE INDEX IF NOT EXISTS idx_company_connectors_company
    ON company_connectors (company_id, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_company_connectors_provider
    ON company_connectors (company_id, provider_key);

CREATE TABLE IF NOT EXISTS company_connector_oauth_states (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    connector_id UUID REFERENCES company_connectors(id) ON DELETE CASCADE,
    state_id TEXT NOT NULL UNIQUE,
    oauth_state TEXT NOT NULL,
    callback_verified BOOLEAN NOT NULL DEFAULT FALSE,
    connected BOOLEAN NOT NULL DEFAULT FALSE,
    expires_at TIMESTAMPTZ,
    callback_error TEXT,
    token_meta JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_connector_oauth_company
    ON company_connector_oauth_states (company_id, created_at DESC);
