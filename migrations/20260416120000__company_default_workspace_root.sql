-- Implicit workspace binding: default repo root for new tasks and execute-worker fallback.
ALTER TABLE companies ADD COLUMN IF NOT EXISTS default_workspace_root TEXT;

COMMENT ON COLUMN companies.default_workspace_root IS
    'Absolute path on the hsm_console host used when a task has no workspace_attachment_paths; also tried after task paths in execute-worker. Set via PATCH /companies/:id or IDE bridge.';
