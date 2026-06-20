-- Create tenants table
CREATE TABLE IF NOT EXISTS tenants (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL UNIQUE,
    config JSONB NOT NULL DEFAULT '{}',
    quotas JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Add tenant_id to creators table
ALTER TABLE creators ADD COLUMN IF NOT EXISTS tenant_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE creators ADD CONSTRAINT fk_creators_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;

-- Add tenant_id to tips table
ALTER TABLE tips ADD COLUMN IF NOT EXISTS tenant_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE tips ADD CONSTRAINT fk_tips_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE;

-- Create indexes for tenant isolation
CREATE INDEX IF NOT EXISTS idx_creators_tenant_id ON creators(tenant_id);
CREATE INDEX IF NOT EXISTS idx_tips_tenant_id ON tips(tenant_id);
CREATE INDEX IF NOT EXISTS idx_creators_tenant_username ON creators(tenant_id, username);

-- Create tenant quotas tracking table
CREATE TABLE IF NOT EXISTS tenant_quotas_usage (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    resource_type VARCHAR(50) NOT NULL,
    usage_count INT NOT NULL DEFAULT 0,
    reset_at TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(tenant_id, resource_type, reset_at)
);

CREATE INDEX IF NOT EXISTS idx_tenant_quotas_usage ON tenant_quotas_usage(tenant_id, resource_type);
