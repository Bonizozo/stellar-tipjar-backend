-- Add creator profile fields: avatar, social links, categories, tags
ALTER TABLE creators ADD COLUMN IF NOT EXISTS avatar_url TEXT;
ALTER TABLE creators ADD COLUMN IF NOT EXISTS social_links JSONB NOT NULL DEFAULT '[]';
ALTER TABLE creators ADD COLUMN IF NOT EXISTS categories TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE creators ADD COLUMN IF NOT EXISTS tags TEXT[] NOT NULL DEFAULT '{}';