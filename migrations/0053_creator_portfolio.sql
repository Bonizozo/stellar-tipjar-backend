CREATE TYPE portfolio_media_type AS ENUM ('image', 'video', 'link', 'document', 'audio');

CREATE TABLE portfolio_items (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT,
    media_type portfolio_media_type NOT NULL DEFAULT 'link',
    url TEXT NOT NULL,
    thumbnail_url TEXT,
    display_order INTEGER NOT NULL DEFAULT 0,
    is_featured BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_portfolio_items_creator ON portfolio_items(creator_username, display_order);
