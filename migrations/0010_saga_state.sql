CREATE TABLE IF NOT EXISTS saga_instances (
    id UUID PRIMARY KEY,
    saga_type TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    context JSONB NOT NULL DEFAULT '{}',
    current_step INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS saga_step_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    saga_id UUID NOT NULL REFERENCES saga_instances(id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL,
    step_name TEXT NOT NULL,
    state TEXT NOT NULL,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_saga_instances_state ON saga_instances(state);
CREATE INDEX idx_saga_step_logs_saga_id ON saga_step_logs(saga_id);
