-- Create API gateway configuration tables
CREATE TABLE IF NOT EXISTS api_routes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    path VARCHAR(500) NOT NULL UNIQUE,
    methods VARCHAR(100) NOT NULL,
    rate_limit_rpm SMALLINT NOT NULL DEFAULT 60,
    burst_size SMALLINT NOT NULL DEFAULT 10,
    requires_auth BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS api_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    route_id UUID NOT NULL REFERENCES api_routes(id) ON DELETE CASCADE,
    version VARCHAR(50) NOT NULL,
    deprecated BOOLEAN NOT NULL DEFAULT false,
    sunset_date DATE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE TABLE IF NOT EXISTS api_rate_limits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id VARCHAR(255) NOT NULL,
    path VARCHAR(500) NOT NULL,
    request_count SMALLINT NOT NULL DEFAULT 0,
    window_start TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE INDEX idx_api_routes_path ON api_routes(path);
CREATE INDEX idx_api_versions_route_id ON api_versions(route_id);
CREATE INDEX idx_api_rate_limits_client_path ON api_rate_limits(client_id, path);
