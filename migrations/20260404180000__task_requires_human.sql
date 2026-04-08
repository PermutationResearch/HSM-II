-- Agent-escalated human review: tasks appear in human_inbox queue when true or state is waiting_admin/blocked.
ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS requires_human BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX IF NOT EXISTS idx_tasks_company_human_inbox
    ON tasks (company_id)
    WHERE requires_human = TRUE
      AND state NOT IN ('done', 'closed', 'cancelled');
