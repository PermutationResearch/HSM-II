-- Intelligence Layer: durable signal log + task provenance from run feedback promotion.

-- 1. Signals table: every signal the intelligence layer processes, persisted for audit.
CREATE TABLE intelligence_signals (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    kind                TEXT NOT NULL,
    source              TEXT NOT NULL DEFAULT 'intelligence_layer',
    description         TEXT NOT NULL DEFAULT '',
    severity            REAL NOT NULL DEFAULT 0.5,
    metadata            JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- composition outcome (filled in after tick processes the signal)
    composition_success BOOLEAN,
    composed_goal_id    UUID REFERENCES goals(id) ON DELETE SET NULL,
    composed_task_id    UUID REFERENCES tasks(id)  ON DELETE SET NULL,
    escalated_to        TEXT,
    -- optional link back to a paperclip in-memory signal id
    paperclip_signal_id TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at        TIMESTAMPTZ,
    CONSTRAINT intelligence_signals_kind_chk CHECK (
        kind IN (
            'capability_degraded', 'goal_stale', 'budget_overrun',
            'composition_failed', 'missing_capability', 'external_signal',
            'coherence_drop', 'agent_anomaly', 'custom',
            -- inbound from Company OS
            'task_failure_pattern', 'spend_anomaly', 'unlinked_goal'
        )
    )
);

CREATE INDEX idx_intel_signals_company_created
    ON intelligence_signals (company_id, created_at DESC);
CREATE INDEX idx_intel_signals_kind
    ON intelligence_signals (company_id, kind, created_at DESC);
CREATE INDEX idx_intel_signals_composed_task
    ON intelligence_signals (composed_task_id)
    WHERE composed_task_id IS NOT NULL;

COMMENT ON TABLE intelligence_signals IS
    'Durable log of every signal processed by the Intelligence Layer (Paperclip + Company OS inbound). '
    'Source of truth for "why was this goal/task created."';

-- 2. Task provenance: record when a task was born from a run-feedback promotion.
ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS source_run_id           UUID REFERENCES agent_runs(id)            ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS source_feedback_event_id UUID REFERENCES run_feedback_events(id)   ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS source_signal_id         UUID REFERENCES intelligence_signals(id)  ON DELETE SET NULL;

COMMENT ON COLUMN tasks.source_run_id           IS 'Set when this task was promoted from an agent run context.';
COMMENT ON COLUMN tasks.source_feedback_event_id IS 'Precise promotion source: the run_feedback_event that spawned this task.';
COMMENT ON COLUMN tasks.source_signal_id         IS 'Set when the Intelligence Layer composed this task from a signal.';

CREATE INDEX IF NOT EXISTS idx_tasks_source_run
    ON tasks (source_run_id) WHERE source_run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tasks_source_signal
    ON tasks (source_signal_id) WHERE source_signal_id IS NOT NULL;
