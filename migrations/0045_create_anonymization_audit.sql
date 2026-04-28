CREATE TABLE IF NOT EXISTS anonymization_audit (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_type VARCHAR(100) NOT NULL,
    entity_id VARCHAR(255) NOT NULL,
    fields_anonymized TEXT[] NOT NULL DEFAULT '{}',
    reason TEXT NOT NULL,
    performed_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_anonymization_audit_entity ON anonymization_audit(entity_type, entity_id);
CREATE INDEX idx_anonymization_audit_created_at ON anonymization_audit(created_at DESC);
