-- Create tenants table
CREATE TABLE IF NOT EXISTS tenants (
    id UUID PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    max_creators INTEGER NOT NULL DEFAULT 100,
    max_tips_per_day INTEGER NOT NULL DEFAULT 10000,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Create tenant_configs table
CREATE TABLE IF NOT EXISTS tenant_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    features JSONB NOT NULL DEFAULT '[]',
    custom_domain VARCHAR(255),
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id)
);

-- Add tenant_id to creators if not exists
ALTER TABLE creators ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;

-- Create index for tenant queries
CREATE INDEX IF NOT EXISTS idx_creators_tenant_id ON creators(tenant_id);
CREATE INDEX IF NOT EXISTS idx_tips_creator_tenant ON tips(creator_id) WHERE creator_id IN (SELECT id FROM creators WHERE tenant_id IS NOT NULL);
CREATE INDEX IF NOT EXISTS idx_tenants_name ON tenants(name);
