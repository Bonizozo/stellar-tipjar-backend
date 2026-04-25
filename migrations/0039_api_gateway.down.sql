-- Rollback API gateway
DROP INDEX IF EXISTS idx_api_rate_limits_client_path;
DROP INDEX IF EXISTS idx_api_versions_route_id;
DROP INDEX IF EXISTS idx_api_routes_path;

DROP TABLE IF EXISTS api_rate_limits;
DROP TABLE IF EXISTS api_versions;
DROP TABLE IF EXISTS api_routes;
