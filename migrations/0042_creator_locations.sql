CREATE EXTENSION IF NOT EXISTS postgis;

CREATE TABLE IF NOT EXISTS creator_locations (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_id   UUID NOT NULL REFERENCES creators(id) ON DELETE CASCADE,
    location     GEOGRAPHY(POINT, 4326) NOT NULL,
    label        TEXT,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (creator_id)
);

CREATE INDEX IF NOT EXISTS idx_creator_locations_gist ON creator_locations USING GIST (location);
