-- Enable trigram extension for fuzzy matching
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Add pre-computed tsvector column for full-text search
ALTER TABLE creators ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- Populate existing rows
UPDATE creators
SET search_vector = to_tsvector('english', username);

-- Auto-update search_vector on insert/update
CREATE OR REPLACE FUNCTION creators_search_vector_update() RETURNS trigger AS $$
BEGIN
    NEW.search_vector := to_tsvector('english', NEW.username);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER creators_search_vector_trigger
    BEFORE INSERT OR UPDATE OF username
    ON creators
    FOR EACH ROW EXECUTE FUNCTION creators_search_vector_update();

-- GIN index for full-text search
CREATE INDEX IF NOT EXISTS idx_creators_search_vector
    ON creators USING GIN(search_vector);

-- Trigram index for fuzzy/ILIKE matching
CREATE INDEX IF NOT EXISTS idx_creators_username_trgm
    ON creators USING GIN(username gin_trgm_ops);
