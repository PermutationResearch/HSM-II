-- Skills imported from Paperclip/companies.sh packs, stored as importable templates.
-- Each row is one skills/<slug>/SKILL.md parsed from a pack on disk.
CREATE TABLE company_skills (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id          UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    slug                TEXT NOT NULL,
    name                TEXT NOT NULL DEFAULT '',
    description         TEXT NOT NULL DEFAULT '',
    body                TEXT NOT NULL DEFAULT '',
    skill_path          TEXT NOT NULL,
    source              TEXT NOT NULL DEFAULT 'paperclip/v1',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_company_skill_slug UNIQUE (company_id, slug)
);

CREATE INDEX idx_company_skills_company ON company_skills(company_id);
