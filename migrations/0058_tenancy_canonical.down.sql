DROP INDEX IF EXISTS idx_tenants_active;
DROP INDEX IF EXISTS idx_tenants_slug;
DROP INDEX IF EXISTS idx_tips_tenant_id;
DROP INDEX IF EXISTS idx_creators_tenant_id;
ALTER TABLE tips DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE creators DROP COLUMN IF EXISTS tenant_id;
DROP TABLE IF EXISTS tenant_configs;
DROP TABLE IF EXISTS tenants;
