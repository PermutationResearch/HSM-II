CREATE TABLE IF NOT EXISTS company_tool_sources (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id        UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL,
    name              TEXT NOT NULL,
    source_url        TEXT,
    auth              JSONB NOT NULL DEFAULT '{}'::jsonb,
    config            JSONB NOT NULL DEFAULT '{}'::jsonb,
    status            TEXT NOT NULL DEFAULT 'active',
    last_ingested_at  TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT company_tool_sources_kind_chk CHECK (kind IN ('openapi', 'graphql', 'mcp', 'custom')),
    CONSTRAINT company_tool_sources_status_chk CHECK (status IN ('active', 'paused', 'error')),
    CONSTRAINT uq_company_tool_sources_name UNIQUE (company_id, name)
);

CREATE INDEX IF NOT EXISTS idx_company_tool_sources_company
    ON company_tool_sources(company_id, created_at DESC);

CREATE TABLE IF NOT EXISTS company_tool_catalog (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id    UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    source_id     UUID REFERENCES company_tool_sources(id) ON DELETE SET NULL,
    tool_key      TEXT NOT NULL,
    display_name  TEXT NOT NULL,
    description   TEXT,
    schema        JSONB NOT NULL DEFAULT '{}'::jsonb,
    meta          JSONB NOT NULL DEFAULT '{}'::jsonb,
    active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_company_tool_key UNIQUE (company_id, tool_key)
);

CREATE INDEX IF NOT EXISTS idx_company_tool_catalog_company_active
    ON company_tool_catalog(company_id, active, updated_at DESC);

CREATE TABLE IF NOT EXISTS company_tool_executions (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id         UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    tool_key           TEXT NOT NULL,
    status             TEXT NOT NULL,
    args               JSONB NOT NULL DEFAULT '{}'::jsonb,
    flow               JSONB NOT NULL DEFAULT '{}'::jsonb,
    result             JSONB,
    error              TEXT,
    resume_token       TEXT,
    resumed_from       UUID REFERENCES company_tool_executions(id) ON DELETE SET NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT company_tool_executions_status_chk CHECK (
        status IN ('running', 'paused_auth', 'paused_approval', 'resumed', 'success', 'error', 'cancelled')
    )
);

CREATE INDEX IF NOT EXISTS idx_company_tool_executions_company_created
    ON company_tool_executions(company_id, created_at DESC);
