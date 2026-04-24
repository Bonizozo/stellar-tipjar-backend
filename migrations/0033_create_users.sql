-- Standalone users table for multi-role authentication.
-- Creators continue to use the creators table; this table supports
-- supporter and admin accounts that are not tied to a creator profile.

CREATE TYPE user_role AS ENUM ('creator', 'supporter', 'admin', 'moderator');

CREATE TABLE IF NOT EXISTS users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email           TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    role            user_role NOT NULL DEFAULT 'supporter',
    -- Optional link to a creator profile for creator-role users
    creator_id      UUID REFERENCES creators(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_users_email ON users (email);
CREATE INDEX idx_users_role  ON users (role);
