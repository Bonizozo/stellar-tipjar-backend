-- ============================================================================
-- Database Indexing Strategy
-- ============================================================================
-- Adds performance indexes for frequently queried columns, foreign keys,
-- and common query patterns. All indexes use IF NOT EXISTS to be idempotent.
-- ============================================================================

-- Index on creator username for direct lookups (login, profile page)
CREATE INDEX IF NOT EXISTS idx_creators_username ON creators(username);

-- Index on creator wallet_address for transaction verification lookups
CREATE INDEX IF NOT EXISTS idx_creators_wallet_address ON creators(wallet_address);

-- Unique index on tips transaction_hash for duplicate detection
-- (already enforced by UNIQUE constraint, but this makes it explicit)
CREATE UNIQUE INDEX IF NOT EXISTS idx_tips_transaction_hash ON tips(transaction_hash);

-- Index on tips created_at for chronological sorting (descending for "latest first")
CREATE INDEX IF NOT EXISTS idx_tips_created_at ON tips(created_at DESC);

-- Composite index for the common "get tips for a creator, newest first" query
CREATE INDEX IF NOT EXISTS idx_tips_creator_created ON tips(creator_username, created_at DESC);

-- Partial index for recent tips (last 30 days) to speed up dashboard queries
CREATE INDEX IF NOT EXISTS idx_tips_recent ON tips(created_at DESC)
WHERE created_at > NOW() - INTERVAL '30 days';

-- Index on audit_logs for admin activity queries (newest first)
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at ON audit_logs(created_at DESC);

-- Index on audit_logs admin_username for filtering by admin
CREATE INDEX IF NOT EXISTS idx_audit_logs_admin ON audit_logs(admin_username);
