CREATE TABLE IF NOT EXISTS company_email_operator_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    connector_key TEXT,
    mailbox TEXT NOT NULL,
    thread_id TEXT,
    message_id TEXT,
    from_address TEXT NOT NULL,
    subject TEXT NOT NULL,
    body_text TEXT NOT NULL,
    suggested_reply TEXT,
    suggested_by_agent TEXT,
    status TEXT NOT NULL DEFAULT 'pending_draft',
    owner_decision TEXT,
    decided_by TEXT,
    decided_at TIMESTAMPTZ,
    sent_at TIMESTAMPTZ,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_company_email_operator_queue_company_status
    ON company_email_operator_queue (company_id, status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_company_email_operator_queue_mailbox
    ON company_email_operator_queue (company_id, mailbox, created_at DESC);
