-- Rollback: remove unique index on creators.email

DROP INDEX IF EXISTS idx_creators_email;
