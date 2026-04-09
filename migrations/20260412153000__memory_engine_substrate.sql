-- Graph-native temporal multimodal memory substrate.
-- Keeps `company_memory_entries` as canonical facts while adding artifact/chunk layers for ingest + retrieval.

CREATE EXTENSION IF NOT EXISTS vector;

ALTER TABLE company_memory_entries
    ADD COLUMN IF NOT EXISTS primary_artifact_id UUID,
    ADD COLUMN IF NOT EXISTS source_artifact_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS chunk_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS embedding_vec vector(768);

CREATE INDEX IF NOT EXISTS idx_company_memory_primary_artifact
    ON company_memory_entries(primary_artifact_id)
    WHERE primary_artifact_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_company_memory_embedding_vec
    ON company_memory_entries
    USING ivfflat (embedding_vec vector_cosine_ops)
    WITH (lists = 100);

CREATE TABLE IF NOT EXISTS memory_artifacts (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id           UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    memory_id            UUID REFERENCES company_memory_entries(id) ON DELETE SET NULL,
    parent_artifact_id   UUID REFERENCES memory_artifacts(id) ON DELETE SET NULL,
    media_type           TEXT NOT NULL,
    source_type          TEXT NOT NULL,
    source_uri           TEXT,
    storage_uri          TEXT,
    title                TEXT,
    checksum             TEXT,
    size_bytes           BIGINT,
    extraction_status    TEXT NOT NULL DEFAULT 'queued',
    extraction_provider  TEXT,
    retry_count          INTEGER NOT NULL DEFAULT 0,
    last_error           TEXT,
    document_date        TIMESTAMPTZ,
    event_date           TIMESTAMPTZ,
    valid_from           TIMESTAMPTZ,
    valid_to             TIMESTAMPTZ,
    entity_type          TEXT,
    entity_id            TEXT,
    contains_pii         BOOLEAN NOT NULL DEFAULT false,
    redacted_text        TEXT,
    extracted_text       TEXT,
    metadata             JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT memory_artifacts_status_chk CHECK (
        extraction_status IN (
            'queued',
            'extracting',
            'chunked',
            'summarized',
            'indexed',
            'retry_waiting',
            'failed',
            'dead_letter'
        )
    )
);

CREATE INDEX IF NOT EXISTS idx_memory_artifacts_company_created
    ON memory_artifacts(company_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_memory_artifacts_company_status
    ON memory_artifacts(company_id, extraction_status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_memory_artifacts_company_memory
    ON memory_artifacts(company_id, memory_id)
    WHERE memory_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_memory_artifacts_entity
    ON memory_artifacts(company_id, entity_type, entity_id)
    WHERE entity_type IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_artifacts_source_dedupe
    ON memory_artifacts(company_id, source_type, COALESCE(source_uri, ''), COALESCE(checksum, ''));

COMMENT ON TABLE memory_artifacts IS
    'Raw or extracted source objects that justify canonical company memory facts.';
COMMENT ON COLUMN memory_artifacts.extracted_text IS
    'Normalized plain text extracted from the source (OCR/transcript/parser output).';

CREATE TABLE IF NOT EXISTS memory_chunks (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id           UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    artifact_id          UUID NOT NULL REFERENCES memory_artifacts(id) ON DELETE CASCADE,
    memory_id            UUID REFERENCES company_memory_entries(id) ON DELETE CASCADE,
    chunk_index          INTEGER NOT NULL,
    text                 TEXT NOT NULL,
    summary_l0           TEXT,
    summary_l1           TEXT,
    token_count          INTEGER NOT NULL DEFAULT 0,
    modality             TEXT NOT NULL DEFAULT 'text',
    start_offset         INTEGER,
    end_offset           INTEGER,
    page_number          INTEGER,
    time_start_ms        INTEGER,
    time_end_ms          INTEGER,
    entity_type          TEXT,
    entity_id            TEXT,
    document_date        TIMESTAMPTZ,
    event_date           TIMESTAMPTZ,
    valid_from           TIMESTAMPTZ,
    valid_to             TIMESTAMPTZ,
    source_range         JSONB NOT NULL DEFAULT '{}'::jsonb,
    metadata             JSONB NOT NULL DEFAULT '{}'::jsonb,
    contains_pii         BOOLEAN NOT NULL DEFAULT false,
    redacted_text        TEXT,
    embedding_json       JSONB,
    embedding_vec        vector(768),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT memory_chunks_unique UNIQUE (artifact_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_memory_chunks_company_memory
    ON memory_chunks(company_id, memory_id, chunk_index);
CREATE INDEX IF NOT EXISTS idx_memory_chunks_company_artifact
    ON memory_chunks(company_id, artifact_id, chunk_index);
CREATE INDEX IF NOT EXISTS idx_memory_chunks_entity
    ON memory_chunks(company_id, entity_type, entity_id)
    WHERE entity_type IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_memory_chunks_document_date
    ON memory_chunks(company_id, document_date)
    WHERE document_date IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_memory_chunks_event_date
    ON memory_chunks(company_id, event_date)
    WHERE event_date IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_memory_chunks_validity
    ON memory_chunks(company_id, valid_from, valid_to)
    WHERE valid_from IS NOT NULL OR valid_to IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_memory_chunks_fts
    ON memory_chunks
    USING gin (
        to_tsvector(
            'english',
            coalesce(text, '') || ' ' ||
            coalesce(summary_l1, '') || ' ' ||
            coalesce(summary_l0, '') || ' ' ||
            coalesce(entity_type, '') || ' ' ||
            coalesce(entity_id, '')
        )
    );
CREATE INDEX IF NOT EXISTS idx_memory_chunks_embedding_vec
    ON memory_chunks
    USING ivfflat (embedding_vec vector_cosine_ops)
    WITH (lists = 100);

COMMENT ON TABLE memory_chunks IS
    'Chunk-level searchable memory units for hybrid retrieval, provenance, and multimodal support.';
COMMENT ON COLUMN memory_chunks.embedding_vec IS
    'pgvector embedding used for nearest-neighbor retrieval.';
