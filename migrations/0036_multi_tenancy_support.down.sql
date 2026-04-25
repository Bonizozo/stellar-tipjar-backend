DROP INDEX IF EXISTS idx_tenants_name;
DROP INDEX IF EXISTS idx_tips_creator_tenant;
DROP INDEX IF EXISTS idx_creators_tenant_id;
ALTER TABLE creators DROP COLUMN IF EXISTS tenant_id;
DROP TABLE IF EXISTS tenant_configs;
DROP TABLE IF EXISTS tenants;
