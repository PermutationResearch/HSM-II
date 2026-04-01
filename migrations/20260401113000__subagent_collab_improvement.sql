-- Sub-agent orchestration, collaboration handoffs, and self-improvement gates.

ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS parent_task_id UUID REFERENCES tasks(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS spawned_by_rule_id UUID;

CREATE INDEX IF NOT EXISTS idx_tasks_company_parent_task ON tasks(company_id, parent_task_id);

CREATE TABLE IF NOT EXISTS task_spawn_rules (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    trigger_state       TEXT NOT NULL DEFAULT 'open',
    title_pattern       TEXT,
    owner_persona       TEXT,
    max_subtasks        INT NOT NULL DEFAULT 3,
    subagent_persona    TEXT NOT NULL,
    handoff_contract    JSONB NOT NULL DEFAULT '{}',
    review_contract     JSONB NOT NULL DEFAULT '{}',
    active              BOOLEAN NOT NULL DEFAULT true,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (max_subtasks >= 1 AND max_subtasks <= 20)
);

CREATE INDEX IF NOT EXISTS idx_spawn_rules_company_active
    ON task_spawn_rules(company_id, active, trigger_state);

CREATE TABLE IF NOT EXISTS task_handoffs (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    task_id             UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    from_agent          TEXT NOT NULL,
    to_agent            TEXT NOT NULL,
    handoff_contract    JSONB NOT NULL DEFAULT '{}',
    review_contract     JSONB NOT NULL DEFAULT '{}',
    status              TEXT NOT NULL DEFAULT 'pending_review',
    notes               TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    reviewed_at         TIMESTAMPTZ,
    reviewed_by         TEXT,
    CHECK (status IN ('pending_review', 'accepted', 'rejected'))
);

CREATE INDEX IF NOT EXISTS idx_task_handoffs_company_task
    ON task_handoffs(company_id, task_id, created_at DESC);

CREATE TABLE IF NOT EXISTS improvement_runs (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    title               TEXT NOT NULL,
    scope               TEXT NOT NULL,
    baseline_meta       JSONB NOT NULL DEFAULT '{}',
    candidate_meta      JSONB NOT NULL DEFAULT '{}',
    gate_contract       JSONB NOT NULL DEFAULT '{}',
    metrics_meta        JSONB NOT NULL DEFAULT '{}',
    status              TEXT NOT NULL DEFAULT 'proposed',
    decision_reason     TEXT,
    decided_by          TEXT,
    decided_at          TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (status IN ('proposed', 'promoted', 'reverted'))
);

CREATE INDEX IF NOT EXISTS idx_improvement_runs_company_status
    ON improvement_runs(company_id, status, created_at DESC);
