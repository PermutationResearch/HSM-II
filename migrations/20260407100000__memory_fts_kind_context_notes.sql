-- Full-text search on company memory (English tsvector + GIN).
CREATE INDEX IF NOT EXISTS idx_company_memory_fts ON company_memory_entries
USING gin (
  to_tsvector(
    'english',
    coalesce(title, '') || ' ' || coalesce(body, '') || ' ' || coalesce(summary_l1, '') || ' ' || coalesce(summary_l0, '')
  )
);

-- High-priority "company line" rows merged first in llm-context (see fetch_shared_memory_addon).
ALTER TABLE company_memory_entries
  ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'general';

ALTER TABLE company_memory_entries DROP CONSTRAINT IF EXISTS company_memory_entries_kind_chk;
ALTER TABLE company_memory_entries
  ADD CONSTRAINT company_memory_entries_kind_chk CHECK (kind IN ('general', 'broadcast'));

-- Append-only handoff traces for the next assignee (stigmergic task memory).
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS context_notes JSONB NOT NULL DEFAULT '[]'::jsonb;
