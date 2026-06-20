CREATE TABLE IF NOT EXISTS collaboration_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    document_id UUID NOT NULL,
    user_id UUID NOT NULL,
    op_type VARCHAR(20) NOT NULL,
    position BIGINT NOT NULL DEFAULT 0,
    content TEXT NOT NULL DEFAULT '',
    length BIGINT NOT NULL DEFAULT 0,
    clock BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_collab_history_document_id ON collaboration_history(document_id, clock ASC);
CREATE INDEX idx_collab_history_user_id ON collaboration_history(user_id);
