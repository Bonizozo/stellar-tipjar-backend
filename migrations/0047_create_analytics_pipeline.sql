-- analytics_windows: 1-minute tumbling window buckets per creator
CREATE TABLE IF NOT EXISTS analytics_windows (
    creator_username TEXT        NOT NULL,
    bucket           TIMESTAMPTZ NOT NULL,
    total_stroops    BIGINT      NOT NULL DEFAULT 0,
    tip_count        BIGINT      NOT NULL DEFAULT 0,
    PRIMARY KEY (creator_username, bucket)
);

-- analytics_alerts: fired alert records
CREATE TABLE IF NOT EXISTS analytics_alerts (
    id               BIGSERIAL   PRIMARY KEY,
    creator_username TEXT        NOT NULL,
    alert_type       TEXT        NOT NULL,
    value_stroops    BIGINT      NOT NULL,
    fired_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (creator_username, alert_type, fired_at)
);
