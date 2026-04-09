-- Memory integration: edges, versioning, entity binding, temporal metadata, provenance.
-- Closes the gap between raw memory append and a graph-aware, conflict-aware memory system.

-- ═══════════════════════════════════════════════════════════════════════════════
-- 1. MEMORY EDGES — typed relations between memory entries
-- ═══════════════════════════════════════════════════════════════════════════════

CREATE TABLE memory_edges (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id       UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    from_memory_id   UUID NOT NULL REFERENCES company_memory_entries(id) ON DELETE CASCADE,
    to_memory_id     UUID NOT NULL REFERENCES company_memory_entries(id) ON DELETE CASCADE,
    relation_type    TEXT NOT NULL,
    confidence       REAL NOT NULL DEFAULT 1.0,
    metadata         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT memory_edges_relation_chk CHECK (
        relation_type IN ('updates', 'extends', 'derives', 'contradicts', 'supports', 'supersedes', 'related')
    ),
    CONSTRAINT memory_edges_no_self CHECK (from_memory_id <> to_memory_id),
    CONSTRAINT memory_edges_confidence_range CHECK (confidence >= 0.0 AND confidence <= 1.0)
);

CREATE INDEX idx_memory_edges_company ON memory_edges(company_id);
CREATE INDEX idx_memory_edges_from ON memory_edges(from_memory_id);
CREATE INDEX idx_memory_edges_to ON memory_edges(to_memory_id);
CREATE INDEX idx_memory_edges_type ON memory_edges(company_id, relation_type);
-- Prevent exact duplicate edges
CREATE UNIQUE INDEX idx_memory_edges_unique
    ON memory_edges(from_memory_id, to_memory_id, relation_type);

-- ═══════════════════════════════════════════════════════════════════════════════
-- 2. VERSIONING + "LATEST FACT" SEMANTICS
-- ═══════════════════════════════════════════════════════════════════════════════

ALTER TABLE company_memory_entries
    ADD COLUMN IF NOT EXISTS supersedes_memory_id UUID REFERENCES company_memory_entries(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS is_latest            BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS version              INTEGER NOT NULL DEFAULT 1;

-- When a memory supersedes another, mark the predecessor as not latest
CREATE INDEX idx_memory_latest ON company_memory_entries(company_id, is_latest) WHERE is_latest = true;
CREATE INDEX idx_memory_supersedes ON company_memory_entries(supersedes_memory_id) WHERE supersedes_memory_id IS NOT NULL;

COMMENT ON COLUMN company_memory_entries.supersedes_memory_id IS
    'Points to the memory entry this one replaces. The chain forms a version history.';
COMMENT ON COLUMN company_memory_entries.is_latest IS
    'False when a newer memory has superseded this one. Retrieval prefers is_latest=true.';
COMMENT ON COLUMN company_memory_entries.version IS
    'Monotonic version counter within a supersession chain.';

-- ═══════════════════════════════════════════════════════════════════════════════
-- 3. DUAL TEMPORAL METADATA
-- ═══════════════════════════════════════════════════════════════════════════════

ALTER TABLE company_memory_entries
    ADD COLUMN IF NOT EXISTS document_date TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS event_date    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS valid_from    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS valid_to      TIMESTAMPTZ;

CREATE INDEX idx_memory_document_date ON company_memory_entries(company_id, document_date) WHERE document_date IS NOT NULL;
CREATE INDEX idx_memory_event_date ON company_memory_entries(company_id, event_date) WHERE event_date IS NOT NULL;
CREATE INDEX idx_memory_validity ON company_memory_entries(company_id, valid_from, valid_to) WHERE valid_from IS NOT NULL;

COMMENT ON COLUMN company_memory_entries.document_date IS
    'When the source document was authored (e.g. meeting notes from 2024-01-15).';
COMMENT ON COLUMN company_memory_entries.event_date IS
    'When the event described by this memory occurred (may differ from document_date).';
COMMENT ON COLUMN company_memory_entries.valid_from IS
    'Start of the temporal validity window for this fact. NULL = always valid.';
COMMENT ON COLUMN company_memory_entries.valid_to IS
    'End of the temporal validity window. NULL = still valid.';

-- ═══════════════════════════════════════════════════════════════════════════════
-- 4. ENTITY / PROFILE BINDING
-- ═══════════════════════════════════════════════════════════════════════════════

ALTER TABLE company_memory_entries
    ADD COLUMN IF NOT EXISTS entity_type TEXT,
    ADD COLUMN IF NOT EXISTS entity_id   TEXT;

CREATE INDEX idx_memory_entity ON company_memory_entries(company_id, entity_type, entity_id)
    WHERE entity_type IS NOT NULL;

COMMENT ON COLUMN company_memory_entries.entity_type IS
    'Bound entity kind: user, org, project, task, agent, goal, dri, capability.';
COMMENT ON COLUMN company_memory_entries.entity_id IS
    'ID of the bound entity (UUID string, slug, or external ref).';

-- ═══════════════════════════════════════════════════════════════════════════════
-- 5. PROVENANCE MODEL
-- ═══════════════════════════════════════════════════════════════════════════════

ALTER TABLE company_memory_entries
    ADD COLUMN IF NOT EXISTS source_type TEXT,
    ADD COLUMN IF NOT EXISTS source_uri  TEXT,
    ADD COLUMN IF NOT EXISTS chunk_id    TEXT,
    ADD COLUMN IF NOT EXISTS source_range JSONB;

COMMENT ON COLUMN company_memory_entries.source_type IS
    'Origin format: pdf, web, audio, image, code, chat, api, manual.';
COMMENT ON COLUMN company_memory_entries.source_uri IS
    'URI or path to the source document.';
COMMENT ON COLUMN company_memory_entries.chunk_id IS
    'Identifier of the chunk within the source (e.g. page number, section id).';
COMMENT ON COLUMN company_memory_entries.source_range IS
    'Byte/line/page range within source: {"start_line": 10, "end_line": 25} or {"page": 3}.';

-- ═══════════════════════════════════════════════════════════════════════════════
-- 6. SAFETY / TENANCY GUARDS
-- ═══════════════════════════════════════════════════════════════════════════════

-- Ensure memory_edges respect company boundaries (both endpoints same company)
CREATE OR REPLACE FUNCTION check_memory_edge_tenant() RETURNS trigger AS $$
DECLARE
    from_cid UUID;
    to_cid   UUID;
BEGIN
    SELECT company_id INTO from_cid FROM company_memory_entries WHERE id = NEW.from_memory_id;
    SELECT company_id INTO to_cid   FROM company_memory_entries WHERE id = NEW.to_memory_id;
    IF from_cid IS DISTINCT FROM to_cid OR from_cid IS DISTINCT FROM NEW.company_id THEN
        RAISE EXCEPTION 'memory_edges: tenant mismatch — from=% to=% edge=%', from_cid, to_cid, NEW.company_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_memory_edge_tenant
    BEFORE INSERT OR UPDATE ON memory_edges
    FOR EACH ROW EXECUTE FUNCTION check_memory_edge_tenant();

-- PII/secret redaction flag (ingestion gate — set by ingestion pipeline, enforced in retrieval)
ALTER TABLE company_memory_entries
    ADD COLUMN IF NOT EXISTS contains_pii     BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS redacted_body    TEXT;

COMMENT ON COLUMN company_memory_entries.contains_pii IS
    'Set by ingestion pipeline if PII/secrets detected. Retrieval may redact or skip.';
COMMENT ON COLUMN company_memory_entries.redacted_body IS
    'Body with PII/secrets masked. Used in place of body when contains_pii=true.';

-- ═══════════════════════════════════════════════════════════════════════════════
-- 7. FTS INDEX REFRESH (include new columns)
-- ═══════════════════════════════════════════════════════════════════════════════

-- Drop old FTS index and recreate with entity_type for faceted search
DROP INDEX IF EXISTS idx_company_memory_fts;
CREATE INDEX idx_company_memory_fts ON company_memory_entries
USING gin (
    to_tsvector(
        'english',
        coalesce(title, '') || ' ' ||
        coalesce(body, '') || ' ' ||
        coalesce(summary_l1, '') || ' ' ||
        coalesce(summary_l0, '') || ' ' ||
        coalesce(entity_type, '') || ' ' ||
        coalesce(entity_id, '')
    )
);
