-- Create message queue tables for RabbitMQ integration

CREATE TABLE IF NOT EXISTS message_queue (
    id UUID PRIMARY KEY,
    message_type VARCHAR(255) NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL,
    retry_count INTEGER NOT NULL DEFAULT 0,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    processed_at TIMESTAMP WITH TIME ZONE,
    created_by VARCHAR(255)
);

CREATE TABLE IF NOT EXISTS dead_letter_queue (
    id UUID PRIMARY KEY,
    original_message_id UUID NOT NULL,
    message_type VARCHAR(255) NOT NULL,
    payload JSONB NOT NULL,
    error_message TEXT NOT NULL,
    failed_at TIMESTAMP WITH TIME ZONE NOT NULL,
    retry_count INTEGER NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_message_queue_status ON message_queue(status);
CREATE INDEX idx_message_queue_created_at ON message_queue(created_at);
CREATE INDEX idx_message_queue_type ON message_queue(message_type);
CREATE INDEX idx_dlq_failed_at ON dead_letter_queue(failed_at);
CREATE INDEX idx_dlq_original_message ON dead_letter_queue(original_message_id);
