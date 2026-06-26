-- Migration: add unique partial index on creators.email
--
-- Ensures no two non-null emails are identical, preventing duplicate
-- registration with the same email address.  NULL values are ignored so
-- that creators without an email are not affected.

CREATE UNIQUE INDEX IF NOT EXISTS idx_creators_email
    ON creators (email)
    WHERE email IS NOT NULL;
