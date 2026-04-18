-- Company OS Manager UX improvements:
-- task_events audit log, chat_threads for conversational continuity,
-- webhook_url on companies, blocked_by_task_id on tasks, goal_coverage_stats.

-- ─────────────────────────────────────────────
-- 1. Task event log (replaces "check the DB to find out what happened")
-- ─────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS task_events (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    task_id     UUID        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    company_id  UUID        NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    event_type  TEXT        NOT NULL,   -- 'state_change','tool_call','stigmergic_note','created','assigned'
    actor       TEXT        NOT NULL DEFAULT 'system',
    payload     JSONB       NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_task_events_task_time
    ON task_events (task_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_task_events_company_time
    ON task_events (company_id, created_at DESC);

-- ─────────────────────────────────────────────
-- 2. Chat threads (conversational continuity across agent-chat calls)
-- ─────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS chat_threads (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id  UUID        NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    actor       TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS chat_thread_messages (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    thread_id   UUID        NOT NULL REFERENCES chat_threads(id) ON DELETE CASCADE,
    role        TEXT        NOT NULL,   -- 'user' | 'assistant'
    content     TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_chat_thread_messages_thread_time
    ON chat_thread_messages (thread_id, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_chat_threads_company
    ON chat_threads (company_id, updated_at DESC);

-- ─────────────────────────────────────────────
-- 3. Webhook URL on companies (push notifications on human escalation)
-- ─────────────────────────────────────────────
ALTER TABLE companies
    ADD COLUMN IF NOT EXISTS webhook_url TEXT;

-- ─────────────────────────────────────────────
-- 4. blocked_by_task_id on tasks (dependency tracking)
-- ─────────────────────────────────────────────
ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS blocked_by_task_id UUID REFERENCES tasks(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_tasks_blocked_by
    ON tasks (blocked_by_task_id) WHERE blocked_by_task_id IS NOT NULL;

-- ─────────────────────────────────────────────
-- 5. Goal coverage stats (hourly KPI, surfaces in dashboard)
-- ─────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS goal_coverage_stats (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id       UUID        NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    window_days      INT         NOT NULL DEFAULT 7,
    total_tasks      INT         NOT NULL DEFAULT 0,
    tasks_with_goal  INT         NOT NULL DEFAULT 0,
    coverage_pct     NUMERIC(5,2) NOT NULL DEFAULT 0,
    computed_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_goal_coverage_stats_company_time
    ON goal_coverage_stats (company_id, computed_at DESC);
