-- Enable pg_trgm extension for fuzzy matching
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Profile columns referenced by full-text/fuzzy search below, but never
-- previously created by any migration.
ALTER TABLE creators ADD COLUMN IF NOT EXISTS display_name TEXT;
ALTER TABLE creators ADD COLUMN IF NOT EXISTS bio TEXT;

-- Tips were keyed by creator_username; search joins tips to creators by id.
ALTER TABLE tips ADD COLUMN IF NOT EXISTS creator_id UUID REFERENCES creators(id) ON DELETE CASCADE;
UPDATE tips t SET creator_id = c.id FROM creators c WHERE t.creator_username = c.username AND t.creator_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_tips_creator_id ON tips(creator_id);

-- Add search vector columns to creators table
ALTER TABLE creators ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- Create GIN index for full-text search on creators
CREATE INDEX IF NOT EXISTS idx_creators_search_vector ON creators USING GIN(search_vector);

-- Create trigram indexes for fuzzy matching
CREATE INDEX IF NOT EXISTS idx_creators_username_trgm ON creators USING GIN(username gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_creators_display_name_trgm ON creators USING GIN(display_name gin_trgm_ops);

-- Update search vector for existing creators
UPDATE creators SET search_vector = 
    setweight(to_tsvector('english', COALESCE(username, '')), 'A') ||
    setweight(to_tsvector('english', COALESCE(display_name, '')), 'B') ||
    setweight(to_tsvector('english', COALESCE(bio, '')), 'C');

-- Create trigger to automatically update search vector
CREATE OR REPLACE FUNCTION creators_search_vector_update() RETURNS trigger AS $$
BEGIN
    NEW.search_vector := 
        setweight(to_tsvector('english', COALESCE(NEW.username, '')), 'A') ||
        setweight(to_tsvector('english', COALESCE(NEW.display_name, '')), 'B') ||
        setweight(to_tsvector('english', COALESCE(NEW.bio, '')), 'C');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS creators_search_vector_trigger ON creators;
CREATE TRIGGER creators_search_vector_trigger
    BEFORE INSERT OR UPDATE ON creators
    FOR EACH ROW
    EXECUTE FUNCTION creators_search_vector_update();

-- Add search vector columns to tips table
ALTER TABLE tips ADD COLUMN IF NOT EXISTS search_vector tsvector;

-- Create GIN index for full-text search on tips
CREATE INDEX IF NOT EXISTS idx_tips_search_vector ON tips USING GIN(search_vector);

-- Update search vector for existing tips
UPDATE tips SET search_vector = 
    setweight(to_tsvector('english', COALESCE(message, '')), 'A');

-- Create trigger to automatically update tips search vector
CREATE OR REPLACE FUNCTION tips_search_vector_update() RETURNS trigger AS $$
BEGIN
    NEW.search_vector := 
        setweight(to_tsvector('english', COALESCE(NEW.message, '')), 'A');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS tips_search_vector_trigger ON tips;
CREATE TRIGGER tips_search_vector_trigger
    BEFORE INSERT OR UPDATE ON tips
    FOR EACH ROW
    EXECUTE FUNCTION tips_search_vector_update();

-- Create indexes for search filters
CREATE INDEX IF NOT EXISTS idx_tips_created_at_amount ON tips(created_at DESC, amount DESC);
CREATE INDEX IF NOT EXISTS idx_tips_creator_created ON tips(creator_id, created_at DESC);

-- Add verified column to creators if not exists
ALTER TABLE creators ADD COLUMN IF NOT EXISTS verified BOOLEAN DEFAULT false;
CREATE INDEX IF NOT EXISTS idx_creators_verified ON creators(verified) WHERE verified = true;
