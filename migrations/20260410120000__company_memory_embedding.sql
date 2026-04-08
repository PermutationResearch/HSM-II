-- Dense embeddings for hybrid retrieval (JSON array of f32; portable without pgvector).
ALTER TABLE company_memory_entries
  ADD COLUMN IF NOT EXISTS embedding_json JSONB;

COMMENT ON COLUMN company_memory_entries.embedding_json IS
  'Optional embedding vector (JSON array of floats), e.g. 768-dim from nomic-embed-text; used with FTS + RRF.';
