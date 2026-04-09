-- Self-improvement loop persistence:
-- failure telemetry, proposals/replay/apply lifecycle, reusable skills, weekly nudges.

CREATE TABLE IF NOT EXISTS run_failure_events (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id         UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    run_id             UUID REFERENCES agent_runs(id) ON DELETE SET NULL,
    task_id            UUID REFERENCES tasks(id) ON DELETE SET NULL,
    company_agent_id   UUID REFERENCES company_agents(id) ON DELETE SET NULL,
    source             TEXT NOT NULL DEFAULT 'run_terminal',
    failure_class      TEXT NOT NULL,
    confidence         REAL NOT NULL DEFAULT 0.5,
    evidence           JSONB NOT NULL DEFAULT '{}'::jsonb,
    classifier_version TEXT NOT NULL DEFAULT 'v1',
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_run_failure_company_created
    ON run_failure_events (company_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_run_failure_class
    ON run_failure_events (company_id, failure_class, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_run_failure_run
    ON run_failure_events (run_id) WHERE run_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS self_improvement_proposals (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    failure_event_id    UUID REFERENCES run_failure_events(id) ON DELETE SET NULL,
    proposal_type       TEXT NOT NULL DEFAULT 'instruction_patch',
    target_surface      TEXT NOT NULL,
    patch_kind          TEXT NOT NULL,
    proposed_patch      JSONB NOT NULL DEFAULT '{}'::jsonb,
    rationale           TEXT NOT NULL DEFAULT '',
    status              TEXT NOT NULL DEFAULT 'proposed',
    auto_apply_eligible BOOLEAN NOT NULL DEFAULT false,
    replay_report       JSONB,
    replay_passed       BOOLEAN,
    replayed_at         TIMESTAMPTZ,
    applied_at          TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT self_improvement_proposals_status_chk CHECK (
        status IN (
            'proposed',
            'replay_passed',
            'replay_failed',
            'applied',
            'rejected',
            'rolled_back'
        )
    )
);

CREATE INDEX IF NOT EXISTS idx_self_improve_proposals_company_created
    ON self_improvement_proposals (company_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_self_improve_proposals_status
    ON self_improvement_proposals (company_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_self_improve_proposals_failure
    ON self_improvement_proposals (failure_event_id) WHERE failure_event_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS self_improvement_applies (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id        UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    proposal_id       UUID NOT NULL REFERENCES self_improvement_proposals(id) ON DELETE CASCADE,
    gate_mode         TEXT NOT NULL DEFAULT 'low_risk_auto',
    approved_by       TEXT,
    outcome           TEXT NOT NULL DEFAULT 'applied',
    rollback_reason   TEXT,
    evidence          JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    rolled_back_at    TIMESTAMPTZ,
    CONSTRAINT self_improvement_applies_outcome_chk CHECK (
        outcome IN ('applied', 'blocked', 'failed', 'rolled_back')
    )
);

CREATE INDEX IF NOT EXISTS idx_self_improve_applies_company_created
    ON self_improvement_applies (company_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_self_improve_applies_proposal
    ON self_improvement_applies (proposal_id);

CREATE TABLE IF NOT EXISTS self_improvement_skills (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id        UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    proposal_id       UUID REFERENCES self_improvement_proposals(id) ON DELETE SET NULL,
    slug              TEXT NOT NULL,
    title             TEXT NOT NULL,
    body_markdown     TEXT NOT NULL,
    target_surface    TEXT NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (company_id, slug)
);

CREATE INDEX IF NOT EXISTS idx_self_improve_skills_company_updated
    ON self_improvement_skills (company_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS self_improvement_nudges (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id        UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    period_start      TIMESTAMPTZ NOT NULL,
    period_end        TIMESTAMPTZ NOT NULL,
    summary           JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (company_id, period_start, period_end)
);

CREATE INDEX IF NOT EXISTS idx_self_improve_nudges_company_created
    ON self_improvement_nudges (company_id, created_at DESC);
