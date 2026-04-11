-- Published context-repo snapshots → company_memory_entries (supermemory), with rollback pointers.

CREATE TABLE IF NOT EXISTS company_context_repo_publishes (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id              UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    session_key             TEXT NOT NULL,
    base_rel_path           TEXT NOT NULL,
    manifest_sha256         TEXT NOT NULL,
    content_sha256          TEXT NOT NULL,
    memory_id               UUID NOT NULL REFERENCES company_memory_entries(id) ON DELETE CASCADE,
    previous_publish_id     UUID REFERENCES company_context_repo_publishes(id) ON DELETE SET NULL,
    rolled_back_at          TIMESTAMPTZ,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_context_repo_pub_company_session_created
    ON company_context_repo_publishes(company_id, session_key, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_context_repo_pub_memory
    ON company_context_repo_publishes(memory_id);
