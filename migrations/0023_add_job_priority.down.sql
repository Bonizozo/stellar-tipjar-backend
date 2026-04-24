DROP INDEX IF EXISTS idx_jobs_status_priority_scheduled;
CREATE INDEX IF NOT EXISTS idx_jobs_status_scheduled ON jobs(status, scheduled_at);
ALTER TABLE jobs DROP COLUMN IF EXISTS priority;
