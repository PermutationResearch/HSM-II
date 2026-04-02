-- Company-wide context for LLM / agents (declaration excerpts, fee tables, policies as prose).
ALTER TABLE companies
    ADD COLUMN IF NOT EXISTS context_markdown TEXT;

COMMENT ON COLUMN companies.context_markdown IS 'Markdown injected into GET .../tasks/:id/llm-context alongside workforce agent profile; edited via PATCH company.';
