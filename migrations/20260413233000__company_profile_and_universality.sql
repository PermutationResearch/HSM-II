CREATE TABLE IF NOT EXISTS company_profiles (
    company_id UUID PRIMARY KEY REFERENCES companies(id) ON DELETE CASCADE,
    industry TEXT NOT NULL DEFAULT 'general',
    business_model TEXT NOT NULL DEFAULT 'services',
    channel_mix JSONB NOT NULL DEFAULT '[]'::jsonb,
    compliance_level TEXT NOT NULL DEFAULT 'standard',
    size_tier TEXT NOT NULL DEFAULT 'solo',
    inferred BOOLEAN NOT NULL DEFAULT TRUE,
    profile_source TEXT NOT NULL DEFAULT 'system_inference',
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT company_profiles_size_tier_chk CHECK (size_tier IN ('solo', 'team', 'org'))
);

CREATE TABLE IF NOT EXISTS company_template_adoption_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    template_key TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_company_template_adoption_events_company_created
    ON company_template_adoption_events (company_id, created_at DESC);
