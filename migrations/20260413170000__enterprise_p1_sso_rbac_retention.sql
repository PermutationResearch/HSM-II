-- Enterprise P1 scaffolding: SSO providers, RBAC roles/bindings, and retention policies.
-- This migration intentionally adds schema foundation only; enforcement wiring is implemented in follow-up runtime patches.

CREATE TABLE IF NOT EXISTS company_sso_providers (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id       UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    provider         TEXT NOT NULL, -- okta, azure_ad, google_workspace, custom_oidc
    issuer_url       TEXT NOT NULL,
    client_id        TEXT NOT NULL,
    auth_url         TEXT,
    token_url        TEXT,
    jwks_url         TEXT,
    scopes           TEXT NOT NULL DEFAULT 'openid profile email',
    enabled          BOOLEAN NOT NULL DEFAULT true,
    metadata         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_company_sso_provider UNIQUE (company_id, provider)
);

CREATE INDEX IF NOT EXISTS idx_company_sso_providers_company
    ON company_sso_providers(company_id, provider);

CREATE TABLE IF NOT EXISTS company_roles (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id       UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    role_key         TEXT NOT NULL, -- owner, admin, operator, auditor, etc.
    display_name     TEXT NOT NULL,
    permissions      JSONB NOT NULL DEFAULT '[]'::jsonb,
    is_system        BOOLEAN NOT NULL DEFAULT false,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_company_role_key UNIQUE (company_id, role_key)
);

CREATE INDEX IF NOT EXISTS idx_company_roles_company
    ON company_roles(company_id, role_key);

CREATE TABLE IF NOT EXISTS company_role_bindings (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id       UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    role_id          UUID NOT NULL REFERENCES company_roles(id) ON DELETE CASCADE,
    principal_type   TEXT NOT NULL, -- user, group, service
    principal_ref    TEXT NOT NULL, -- stable id from IdP or internal directory
    granted_by       TEXT,
    metadata         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_company_role_binding UNIQUE (company_id, role_id, principal_type, principal_ref)
);

CREATE INDEX IF NOT EXISTS idx_company_role_bindings_company
    ON company_role_bindings(company_id, principal_type, principal_ref);

CREATE TABLE IF NOT EXISTS company_retention_policies (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id       UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    policy_key       TEXT NOT NULL,
    subject_type     TEXT NOT NULL, -- memory, runs, governance_events, artifacts, messages
    retention_days   INTEGER NOT NULL CHECK (retention_days >= 1),
    action           TEXT NOT NULL DEFAULT 'delete', -- delete, archive, anonymize
    legal_hold       BOOLEAN NOT NULL DEFAULT false,
    enabled          BOOLEAN NOT NULL DEFAULT true,
    metadata         JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_company_retention_policy UNIQUE (company_id, policy_key)
);

CREATE INDEX IF NOT EXISTS idx_company_retention_policies_company
    ON company_retention_policies(company_id, subject_type, enabled);
