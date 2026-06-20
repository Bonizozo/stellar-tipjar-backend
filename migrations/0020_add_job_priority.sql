-- Add priority column to jobs table for job priority levels
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS priority INTEGER NOT NULL DEFAULT 0;

-- Update the status+scheduled index to include priority for efficient dequeue ordering
DROP INDEX IF EXISTS idx_jobs_status_scheduled;
CREATE INDEX idx_jobs_status_priority_scheduled ON jobs(status, priority DESC, scheduled_at ASC);
