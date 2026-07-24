-- Migration 0012: Tip Verification Pipeline
-- Adds two-phase tip state (pending_verification → confirmed/rejected),
-- tipper source account for on-chain verification, and enforces exactly-once
-- ingestion via a UNIQUE constraint on transaction_hash.

-- 1. Add the status column with a safe default for existing rows
ALTER TABLE tips
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'confirmed';

-- 2. Add tipper_source_account (nullable – populated for newly submitted tips)
ALTER TABLE tips
    ADD COLUMN IF NOT EXISTS tipper_source_account TEXT;

-- 3. Backfill: existing rows were never verified; mark them as 'confirmed'
--    because they pre-date the verification pipeline and must remain visible.
UPDATE tips SET status = 'confirmed' WHERE status IS NULL OR status = '';

-- 4. Add CHECK constraint so only valid values are stored
ALTER TABLE tips
    ADD CONSTRAINT tips_status_check
        CHECK (status IN ('pending_verification', 'confirmed', 'rejected'));

-- 5. Ensure the UNIQUE constraint on transaction_hash exists
--    (0002_create_tips.sql already added it, but guard with IF NOT EXISTS pattern
--     by creating a named index if the constraint name doesn't exist)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.table_constraints
        WHERE table_name = 'tips'
          AND constraint_type = 'UNIQUE'
          AND constraint_name = 'tips_transaction_hash_key'
    ) AND NOT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE tablename = 'tips' AND indexname = 'tips_transaction_hash_unique'
    ) THEN
        CREATE UNIQUE INDEX tips_transaction_hash_unique ON tips(transaction_hash);
    END IF;
END
$$;

-- 6. Index for fast lookup of pending tips (used by reconciliation job)
CREATE INDEX IF NOT EXISTS idx_tips_status ON tips(status);

-- 7. Index for reconciliation queries on created_at + status
CREATE INDEX IF NOT EXISTS idx_tips_status_created_at ON tips(status, created_at);
