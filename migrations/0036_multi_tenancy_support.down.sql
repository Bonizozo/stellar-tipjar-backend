-- Rollback multi-tenancy support
DROP INDEX IF EXISTS idx_uploads_tenant_id;
DROP INDEX IF EXISTS idx_api_usage_tenant_id;
DROP INDEX IF EXISTS idx_tips_tenant_id;
DROP INDEX IF EXISTS idx_creators_tenant_id;

ALTER TABLE uploads DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE api_usage DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE tips DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE creators DROP COLUMN IF EXISTS tenant_id;

DROP INDEX IF EXISTS idx_tenant_analytics_tenant_period;
DROP TABLE IF EXISTS tenant_analytics;

DROP INDEX IF EXISTS idx_tenants_status;
DROP INDEX IF EXISTS idx_tenants_org_id;
DROP TABLE IF EXISTS tenants;
