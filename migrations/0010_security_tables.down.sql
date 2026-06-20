DROP INDEX IF EXISTS idx_oauth2_accounts_user_id;
DROP INDEX IF EXISTS idx_security_audit_logs_created_at;
DROP INDEX IF EXISTS idx_security_audit_logs_user_id;
DROP TABLE IF EXISTS security_audit_logs;
DROP TABLE IF EXISTS oauth2_accounts;
DROP TABLE IF EXISTS user_roles;
