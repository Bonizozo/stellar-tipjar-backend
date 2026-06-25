-- Rollback: remove rate limit quota tracking and analytics event log

DROP TRIGGER IF EXISTS trg_quota_updated_at ON api_client_quotas;
DROP FUNCTION IF EXISTS set_quota_updated_at();

DROP TABLE IF EXISTS rate_limit_events;
DROP TABLE IF EXISTS api_client_quotas;
