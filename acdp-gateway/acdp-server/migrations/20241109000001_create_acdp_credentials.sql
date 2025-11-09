-- ACDP Credentials Table
--
-- Stores ACDP credentials issued by the gateway.
-- This table will be added to Rauthy's PostgreSQL database.

CREATE TABLE IF NOT EXISTS acdp_credentials (
    -- Primary key
    credential_id UUID PRIMARY KEY,

    -- Credential metadata
    credential_type INTEGER NOT NULL, -- 0=IdentityBound, 1=Anonymous, 2=Hybrid
    principal_subject TEXT, -- NULL for anonymous
    principal_issuer TEXT, -- NULL for anonymous
    agent_id TEXT NOT NULL,

    -- Credential data (JSON serialized ACDPCredential)
    credential_data TEXT NOT NULL,

    -- Rate limiting
    max_presentations BIGINT NOT NULL,
    presentations_used BIGINT NOT NULL DEFAULT 0,

    -- Timestamps
    issued_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,

    -- Delegation
    parent_credential_id UUID REFERENCES acdp_credentials(credential_id),

    -- Revocation
    revoked BOOLEAN NOT NULL DEFAULT false,
    revoked_at TIMESTAMPTZ,
    revocation_reason TEXT,

    -- Audit
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_acdp_credentials_agent_id ON acdp_credentials(agent_id);
CREATE INDEX IF NOT EXISTS idx_acdp_credentials_principal ON acdp_credentials(principal_subject, principal_issuer);
CREATE INDEX IF NOT EXISTS idx_acdp_credentials_expires_at ON acdp_credentials(expires_at);
CREATE INDEX IF NOT EXISTS idx_acdp_credentials_parent ON acdp_credentials(parent_credential_id);

-- Updated timestamp trigger
CREATE OR REPLACE FUNCTION update_acdp_credentials_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_acdp_credentials_updated_at
    BEFORE UPDATE ON acdp_credentials
    FOR EACH ROW
    EXECUTE FUNCTION update_acdp_credentials_updated_at();
