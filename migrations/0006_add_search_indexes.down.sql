DROP TRIGGER IF EXISTS creators_search_vector_trigger ON creators;
DROP FUNCTION IF EXISTS creators_search_vector_update();
DROP INDEX IF EXISTS idx_creators_username_trgm;
DROP INDEX IF EXISTS idx_creators_search_vector;
ALTER TABLE creators DROP COLUMN IF EXISTS search_vector;
