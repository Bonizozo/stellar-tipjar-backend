ALTER TABLE creators DROP COLUMN IF EXISTS is_verified;
DROP INDEX IF EXISTS idx_verifications_creator;
DROP INDEX IF EXISTS idx_verifications_status;
DROP TABLE IF EXISTS creator_verifications;
