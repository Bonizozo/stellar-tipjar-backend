-- Create saga_workflows table
CREATE TABLE IF NOT EXISTS saga_workflows (
    id UUID PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    state JSONB NOT NULL,
    status VARCHAR(50) NOT NULL,
    error_message TEXT,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

-- Create saga_step_executions table for monitoring
CREATE TABLE IF NOT EXISTS saga_step_executions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workflow_id UUID NOT NULL REFERENCES saga_workflows(id) ON DELETE CASCADE,
    step_id VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL,
    duration_ms BIGINT NOT NULL,
    executed_at TIMESTAMP NOT NULL
);

-- Create indexes for saga queries
CREATE INDEX IF NOT EXISTS idx_saga_workflows_status ON saga_workflows(status);
CREATE INDEX IF NOT EXISTS idx_saga_workflows_created_at ON saga_workflows(created_at);
CREATE INDEX IF NOT EXISTS idx_saga_step_executions_workflow_id ON saga_step_executions(workflow_id);
CREATE INDEX IF NOT EXISTS idx_saga_step_executions_executed_at ON saga_step_executions(executed_at);
