-- Company OS autonomy layer: scheduler policies, universal approvals, connector ops runs, KPI loop.

CREATE TABLE IF NOT EXISTS company_autonomy_policies (
    company_id               UUID PRIMARY KEY REFERENCES companies(id) ON DELETE CASCADE,
    autonomy_enabled         BOOLEAN NOT NULL DEFAULT true,
    kill_switch              BOOLEAN NOT NULL DEFAULT false,
    quiet_hours_start_utc    SMALLINT NOT NULL DEFAULT 0,
    quiet_hours_end_utc      SMALLINT NOT NULL DEFAULT 0,
    max_concurrent_runs      INT NOT NULL DEFAULT 3,
    daily_budget_usd         NUMERIC(18,4),
    updated_by               TEXT,
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (quiet_hours_start_utc >= 0 AND quiet_hours_start_utc <= 23),
    CHECK (quiet_hours_end_utc >= 0 AND quiet_hours_end_utc <= 23),
    CHECK (max_concurrent_runs > 0)
);

CREATE TABLE IF NOT EXISTS company_schedules (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id               UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name                     TEXT NOT NULL,
    trigger_kind             TEXT NOT NULL,
    trigger_spec             JSONB NOT NULL DEFAULT '{}',
    action_type              TEXT NOT NULL,
    payload                  JSONB NOT NULL DEFAULT '{}',
    status                   TEXT NOT NULL DEFAULT 'active',
    next_run_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_run_at              TIMESTAMPTZ,
    retry_max_attempts       INT NOT NULL DEFAULT 3,
    retry_backoff_seconds    INT NOT NULL DEFAULT 60,
    requires_approval        BOOLEAN NOT NULL DEFAULT false,
    created_by               TEXT,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (trigger_kind IN ('interval_minutes','daily_hour_utc','event')),
    CHECK (status IN ('active','paused','disabled')),
    CHECK (retry_max_attempts > 0),
    CHECK (retry_backoff_seconds >= 0)
);

CREATE INDEX IF NOT EXISTS idx_company_schedules_due
    ON company_schedules (company_id, status, next_run_at);

CREATE TABLE IF NOT EXISTS company_approval_requests (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id               UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    risk_domain              TEXT NOT NULL,
    risk_level               TEXT NOT NULL,
    action_type              TEXT NOT NULL,
    subject_type             TEXT NOT NULL,
    subject_id               TEXT NOT NULL,
    request_payload          JSONB NOT NULL DEFAULT '{}',
    policy_reason            TEXT,
    status                   TEXT NOT NULL DEFAULT 'pending',
    requested_by             TEXT,
    decided_by               TEXT,
    decided_at               TIMESTAMPTZ,
    expires_at               TIMESTAMPTZ,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (status IN ('pending','approved','rejected','expired')),
    CHECK (risk_level IN ('low','medium','high','critical'))
);

CREATE INDEX IF NOT EXISTS idx_company_approval_requests_company_status
    ON company_approval_requests (company_id, status, created_at DESC);

CREATE TABLE IF NOT EXISTS company_connector_operation_runs (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id               UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    connector_id             UUID REFERENCES company_connectors(id) ON DELETE SET NULL,
    action_type              TEXT NOT NULL,
    request_payload          JSONB NOT NULL DEFAULT '{}',
    response_payload         JSONB NOT NULL DEFAULT '{}',
    status                   TEXT NOT NULL DEFAULT 'queued',
    risk_level               TEXT NOT NULL DEFAULT 'medium',
    approval_request_id      UUID REFERENCES company_approval_requests(id) ON DELETE SET NULL,
    actor                    TEXT,
    error                    TEXT,
    started_at               TIMESTAMPTZ,
    finished_at              TIMESTAMPTZ,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (status IN ('queued','running','succeeded','failed','waiting_approval','blocked')),
    CHECK (risk_level IN ('low','medium','high','critical'))
);

CREATE INDEX IF NOT EXISTS idx_company_connector_operation_runs_company
    ON company_connector_operation_runs (company_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_company_connector_operation_runs_status
    ON company_connector_operation_runs (status, created_at DESC);

CREATE TABLE IF NOT EXISTS company_kpi_targets (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id               UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    kpi_key                  TEXT NOT NULL,
    target_value             NUMERIC(18,4) NOT NULL,
    direction                TEXT NOT NULL DEFAULT 'up',
    window_days              INT NOT NULL DEFAULT 30,
    enabled                  BOOLEAN NOT NULL DEFAULT true,
    owner                    TEXT,
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (direction IN ('up','down')),
    CHECK (window_days > 0),
    UNIQUE (company_id, kpi_key)
);

CREATE TABLE IF NOT EXISTS company_kpi_snapshots (
    id                       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id               UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    kpi_key                  TEXT NOT NULL,
    value                    NUMERIC(18,4) NOT NULL,
    snapshot_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    source                   TEXT NOT NULL DEFAULT 'system',
    meta                     JSONB NOT NULL DEFAULT '{}',
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_company_kpi_snapshots_company_key_time
    ON company_kpi_snapshots (company_id, kpi_key, snapshot_at DESC);
