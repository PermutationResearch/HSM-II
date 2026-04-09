-- Store promotion pipeline: track artifacts promoted from RooDB / Ladybug into Postgres Company OS.

CREATE TABLE store_promotions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    source_store    TEXT NOT NULL CHECK (source_store IN ('roodb', 'ladybug', 'sqlite')),
    source_id       TEXT NOT NULL,
    source_snapshot JSONB NOT NULL DEFAULT '{}'::jsonb,
    target_table    TEXT NOT NULL,
    target_id       UUID,
    promoted_by     TEXT NOT NULL DEFAULT 'system',
    status          TEXT NOT NULL DEFAULT 'promoted' CHECK (status IN ('promoted', 'rolled_back', 'superseded')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_store_promotions_company ON store_promotions(company_id);
CREATE INDEX idx_store_promotions_source ON store_promotions(source_store, source_id);
CREATE INDEX idx_store_promotions_target ON store_promotions(target_table, target_id);
CREATE UNIQUE INDEX idx_store_promotions_dedupe ON store_promotions(company_id, source_store, source_id)
    WHERE status = 'promoted';

COMMENT ON TABLE store_promotions IS
    'Audit trail for artifacts promoted from RooDB/Ladybug/SQLite into Postgres Company OS.';
COMMENT ON COLUMN store_promotions.source_snapshot IS
    'JSON copy of the source row at promotion time (for rollback / diff).';

-- Composite index for list_promotions ORDER BY created_at DESC (I6)
CREATE INDEX idx_store_promotions_list
    ON store_promotions (company_id, created_at DESC);

-- Proposal dedupe: prevent concurrent generate from creating duplicates per failure event (I4)
CREATE UNIQUE INDEX IF NOT EXISTS idx_self_improve_proposals_failure_dedupe
    ON self_improvement_proposals (company_id, failure_event_id)
    WHERE failure_event_id IS NOT NULL AND status NOT IN ('rejected', 'rolled_back');
