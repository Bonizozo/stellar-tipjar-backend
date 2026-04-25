-- Create tenants table for multi-tenancy support
CREATE TABLE IF NOT EXISTS tenants (
    id UUID PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    organization_id VARCHAR(255) NOT NULL,
    tier VARCHAR(50) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'active',
    config JSONB,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL,
    provisioned_at TIMESTAMP WITH TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_tenants_org_id ON tenants(organization_id);
CREATE INDEX idx_tenants_status ON tenants(status);

-- Create tenant_analytics table
CREATE TABLE IF NOT EXISTS tenant_analytics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    period VARCHAR(50) NOT NULL,
    total_tips BIGINT DEFAULT 0,
    total_revenue DECIMAL(20, 8) DEFAULT 0,
    active_creators BIGINT DEFAULT 0,
    active_supporters BIGINT DEFAULT 0,
    api_calls BIGINT DEFAULT 0,
    storage_used_mb BIGINT DEFAULT 0,
    recorded_at TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE INDEX idx_tenant_analytics_tenant_period ON tenant_analytics(tenant_id, period);

-- Add tenant_id to existing tables
ALTER TABLE creators ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE tips ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE api_usage ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;
ALTER TABLE uploads ADD COLUMN IF NOT EXISTS tenant_id UUID REFERENCES tenants(id) ON DELETE CASCADE;

CREATE INDEX idx_creators_tenant_id ON creators(tenant_id);
CREATE INDEX idx_tips_tenant_id ON tips(tenant_id);
CREATE INDEX idx_api_usage_tenant_id ON api_usage(tenant_id);
CREATE INDEX idx_uploads_tenant_id ON uploads(tenant_id);
