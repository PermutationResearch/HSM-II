-- Per-task run telemetry for dashboard live strip (tool counts, log tail, status).
CREATE TABLE IF NOT EXISTS task_run_snapshots (
    task_id     UUID PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    company_id  UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    run_status  TEXT NOT NULL DEFAULT 'idle',
    tool_calls  INT NOT NULL DEFAULT 0,
    log_tail    TEXT NOT NULL DEFAULT '',
    finished_at TIMESTAMPTZ,
    CONSTRAINT task_run_snapshots_status_chk CHECK (
        run_status IN ('idle', 'running', 'success', 'error')
    )
);

CREATE INDEX IF NOT EXISTS idx_task_run_snapshots_company ON task_run_snapshots(company_id);
