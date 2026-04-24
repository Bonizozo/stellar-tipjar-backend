DROP TABLE IF EXISTS tenant_quotas_usage;
DROP INDEX IF EXISTS idx_creators_tenant_username;
DROP INDEX IF EXISTS idx_tips_tenant_id;
DROP INDEX IF EXISTS idx_creators_tenant_id;
ALTER TABLE tips DROP CONSTRAINT IF EXISTS fk_tips_tenant;
ALTER TABLE tips DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE creators DROP CONSTRAINT IF EXISTS fk_creators_tenant;
ALTER TABLE creators DROP COLUMN IF EXISTS tenant_id;
DROP TABLE IF EXISTS tenants;
