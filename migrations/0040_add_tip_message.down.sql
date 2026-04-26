ALTER TABLE tips
    DROP COLUMN IF EXISTS message,
    DROP COLUMN IF EXISTS message_visibility;
