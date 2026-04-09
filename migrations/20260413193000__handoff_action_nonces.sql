CREATE TABLE IF NOT EXISTS handoff_action_nonces (
    nonce TEXT PRIMARY KEY,
    handoff_id UUID NOT NULL REFERENCES task_handoffs(id) ON DELETE CASCADE,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    used_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_handoff_action_nonces_handoff
    ON handoff_action_nonces (handoff_id, used_at DESC);
