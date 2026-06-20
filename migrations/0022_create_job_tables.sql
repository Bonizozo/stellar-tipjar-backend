-- Job runs tracking table
CREATE TABLE IF NOT EXISTS job_runs (
    id SERIAL PRIMARY KEY,
    job_name VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL,
    error_message TEXT,
    duration_ms BIGINT,
    run_at TIMESTAMP NOT NULL DEFAULT NOW(),
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_job_runs_name_run_at ON job_runs(job_name, run_at DESC);
CREATE INDEX idx_job_runs_status ON job_runs(status);

-- Daily summaries table
CREATE TABLE IF NOT EXISTS daily_summaries (
    id SERIAL PRIMARY KEY,
    date DATE NOT NULL UNIQUE,
    total_tips BIGINT NOT NULL DEFAULT 0,
    total_amount DECIMAL(20, 7) NOT NULL DEFAULT 0,
    unique_creators INTEGER NOT NULL DEFAULT 0,
    unique_tippers INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_daily_summaries_date ON daily_summaries(date DESC);

-- Hourly analytics table
CREATE TABLE IF NOT EXISTS hourly_analytics (
    id SERIAL PRIMARY KEY,
    hour TIMESTAMP NOT NULL UNIQUE,
    total_tips BIGINT NOT NULL DEFAULT 0,
    total_amount DECIMAL(20, 7) NOT NULL DEFAULT 0,
    avg_amount DECIMAL(20, 7),
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_hourly_analytics_hour ON hourly_analytics(hour DESC);
