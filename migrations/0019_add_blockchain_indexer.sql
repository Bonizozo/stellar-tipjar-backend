-- Create indexed events table
CREATE TABLE IF NOT EXISTS indexed_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id VARCHAR(255) NOT NULL UNIQUE,
    event_type VARCHAR(50) NOT NULL,
    transaction_hash VARCHAR(255) NOT NULL,
    ledger_sequence INT NOT NULL,
    processed_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Add indexer state tracking
CREATE TABLE IF NOT EXISTS indexer_state (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    cursor VARCHAR(255) NOT NULL,
    latest_ledger INT NOT NULL,
    is_synced BOOLEAN DEFAULT FALSE,
    last_updated TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Add confirmations column to tips
ALTER TABLE tips ADD COLUMN IF NOT EXISTS confirmations INT DEFAULT 0;
ALTER TABLE tips ADD COLUMN IF NOT EXISTS indexed_at TIMESTAMP WITH TIME ZONE;

-- Create indexes for indexer
CREATE INDEX IF NOT EXISTS idx_indexed_events_event_id ON indexed_events(event_id);
CREATE INDEX IF NOT EXISTS idx_indexed_events_ledger ON indexed_events(ledger_sequence);
CREATE INDEX IF NOT EXISTS idx_indexed_events_created_at ON indexed_events(created_at);
CREATE INDEX IF NOT EXISTS idx_tips_confirmations ON tips(confirmations);
