-- Create saga pattern tables for distributed transactions
CREATE TABLE IF NOT EXISTS saga_executions (
    id UUID PRIMARY KEY,
    saga_type VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'running',
    created_at TIMESTAMP WITH TIME ZONE NOT NULL,
    completed_at TIMESTAMP WITH TIME ZONE,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS saga_steps (
    id UUID PRIMARY KEY,
    saga_id UUID NOT NULL REFERENCES saga_executions(id) ON DELETE CASCADE,
    step_name VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL,
    input JSONB NOT NULL,
    output JSONB,
    error TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE INDEX idx_saga_executions_status ON saga_executions(status);
CREATE INDEX idx_saga_executions_type ON saga_executions(saga_type);
CREATE INDEX idx_saga_steps_saga_id ON saga_steps(saga_id);
CREATE INDEX idx_saga_steps_status ON saga_steps(status);
