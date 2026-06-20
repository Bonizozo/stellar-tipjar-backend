CREATE TABLE events (
    sequence_number BIGSERIAL PRIMARY KEY,
    aggregate_id    UUID        NOT NULL,
    event_type      TEXT        NOT NULL,
    event_data      JSONB       NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_events_aggregate_id ON events (aggregate_id);
CREATE INDEX idx_events_event_type   ON events (event_type);
