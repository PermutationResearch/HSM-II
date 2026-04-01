-- Phase 2/3/4 foundations: scheduler reliability, idempotency, safety gates, GTM scale entities.

CREATE TABLE IF NOT EXISTS request_idempotency (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    scope               TEXT NOT NULL,
    idempotency_key     TEXT NOT NULL,
    request_hash        TEXT NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(company_id, scope, idempotency_key)
);

CREATE TABLE IF NOT EXISTS automation_jobs (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    kind                TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'pending',
    payload             JSONB NOT NULL DEFAULT '{}',
    attempts            INT NOT NULL DEFAULT 0,
    max_attempts        INT NOT NULL DEFAULT 5,
    next_run_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error          TEXT,
    idempotency_key     TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (status IN ('pending', 'running', 'done', 'failed', 'dead_letter'))
);

CREATE INDEX IF NOT EXISTS idx_automation_jobs_due
    ON automation_jobs(status, next_run_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_automation_jobs_company_idem
    ON automation_jobs(company_id, kind, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS automation_dead_letters (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    job_id              UUID,
    kind                TEXT NOT NULL,
    payload             JSONB NOT NULL DEFAULT '{}',
    error               TEXT NOT NULL,
    attempts            INT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE improvement_runs
    ADD COLUMN IF NOT EXISTS min_eval_samples INT,
    ADD COLUMN IF NOT EXISTS max_regression_pct NUMERIC(8,3),
    ADD COLUMN IF NOT EXISTS high_risk_requires_reviewer BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE IF NOT EXISTS onboarding_contract_versions (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contract_id         TEXT NOT NULL,
    version             TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'active',
    schema              JSONB NOT NULL DEFAULT '{}',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (status IN ('active', 'deprecated', 'sunset'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_contract_versions_unique
    ON onboarding_contract_versions(contract_id, version);

CREATE TABLE IF NOT EXISTS connector_permission_presets (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    vertical            TEXT NOT NULL,
    connector_provider  TEXT NOT NULL,
    allowed_actions     JSONB NOT NULL DEFAULT '[]',
    blocked_actions     JSONB NOT NULL DEFAULT '[]',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(vertical, connector_provider)
);

CREATE TABLE IF NOT EXISTS company_go_live_checklists (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    item_key            TEXT NOT NULL,
    item_label          TEXT NOT NULL,
    required            BOOLEAN NOT NULL DEFAULT true,
    completed           BOOLEAN NOT NULL DEFAULT false,
    completed_by        TEXT,
    completed_at        TIMESTAMPTZ,
    notes               TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(company_id, item_key)
);
