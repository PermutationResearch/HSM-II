-- Company-defined labels for tasks/issues (stored on tasks as capability_refs kind=label).
CREATE TABLE company_issue_labels (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id      UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    description     TEXT,
    sort_order      INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT company_issue_labels_slug_chk CHECK (slug ~ '^[a-z0-9][a-z0-9_-]{0,47}$'),
    UNIQUE (company_id, slug)
);

CREATE INDEX idx_company_issue_labels_company ON company_issue_labels(company_id);
