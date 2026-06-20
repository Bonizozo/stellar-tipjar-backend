CREATE TABLE IF NOT EXISTS categories (
    id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    slug TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS creator_categories (
    creator_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    category_id      UUID NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    PRIMARY KEY (creator_username, category_id)
);

CREATE TABLE IF NOT EXISTS tags (
    id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS creator_tags (
    creator_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    tag_id           UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (creator_username, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_creator_categories_creator ON creator_categories(creator_username);
CREATE INDEX IF NOT EXISTS idx_creator_categories_category ON creator_categories(category_id);
CREATE INDEX IF NOT EXISTS idx_creator_tags_creator ON creator_tags(creator_username);
CREATE INDEX IF NOT EXISTS idx_creator_tags_tag ON creator_tags(tag_id);
