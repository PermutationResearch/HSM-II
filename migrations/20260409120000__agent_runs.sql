-- Agent execution runs + human feedback events (Paperclip Nexus-style); optional task promotion.
CREATE TABLE agent_runs (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    task_id             UUID REFERENCES tasks(id) ON DELETE SET NULL,
    company_agent_id    UUID REFERENCES company_agents(id) ON DELETE SET NULL,
    external_run_id     TEXT,
    external_system     TEXT NOT NULL DEFAULT 'hsm',
    status              TEXT NOT NULL DEFAULT 'running',
    started_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at         TIMESTAMPTZ,
    summary             TEXT,
    meta                JSONB NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT agent_runs_status_chk CHECK (
        status IN ('running', 'success', 'error', 'cancelled')
    ),
    CONSTRAINT agent_runs_external_nonempty CHECK (
        external_run_id IS NULL OR btrim(external_run_id) <> ''
    )
);

CREATE UNIQUE INDEX uq_agent_runs_external
    ON agent_runs (company_id, external_system, external_run_id)
    WHERE external_run_id IS NOT NULL;

CREATE INDEX idx_agent_runs_company_started ON agent_runs (company_id, started_at DESC);
CREATE INDEX idx_agent_runs_task ON agent_runs (task_id);
CREATE INDEX idx_agent_runs_company_agent ON agent_runs (company_id, company_agent_id, started_at DESC);

CREATE TABLE run_feedback_events (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    run_id              UUID NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    step_index          INT,
    step_external_id    TEXT,
    actor               TEXT NOT NULL,
    kind                TEXT NOT NULL DEFAULT 'comment',
    body                TEXT NOT NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    spawned_task_id     UUID REFERENCES tasks(id) ON DELETE SET NULL,
    CONSTRAINT run_feedback_kind_chk CHECK (
        kind IN ('comment', 'correction', 'blocker', 'praise')
    )
);

CREATE INDEX idx_run_feedback_run ON run_feedback_events (run_id, created_at);
CREATE INDEX idx_run_feedback_company ON run_feedback_events (company_id, created_at DESC);
CREATE INDEX idx_run_feedback_spawned ON run_feedback_events (spawned_task_id)
    WHERE spawned_task_id IS NOT NULL;
