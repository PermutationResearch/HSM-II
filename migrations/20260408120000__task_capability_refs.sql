-- Explicit links from tasks to atomic capabilities (skills, SOPs, tools, packs, agent templates).
ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS capability_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

COMMENT ON COLUMN tasks.capability_refs IS
    'JSON array of {kind, ref} objects (kind: skill|sop|tool|pack|agent); surfaced in API and llm-context.';
