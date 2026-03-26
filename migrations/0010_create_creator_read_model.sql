CREATE TABLE creator_read_model (
    id              UUID        PRIMARY KEY,
    username        TEXT        NOT NULL UNIQUE,
    wallet_address  TEXT        NOT NULL,
    tip_count       BIGINT      NOT NULL DEFAULT 0,
    registered_at   TIMESTAMPTZ NOT NULL
);
