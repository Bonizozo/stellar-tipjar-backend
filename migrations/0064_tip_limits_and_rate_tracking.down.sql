DROP INDEX IF EXISTS idx_tips_creator_recent;
DROP INDEX IF EXISTS idx_tips_tipper_ip_created;
DROP INDEX IF EXISTS idx_tips_tipper_wallet_created;

ALTER TABLE tips
    DROP COLUMN IF EXISTS tipper_ip,
    DROP COLUMN IF EXISTS tipper_wallet;

ALTER TABLE creators
    DROP COLUMN IF EXISTS max_tips_per_minute,
    DROP COLUMN IF EXISTS max_tip_amount,
    DROP COLUMN IF EXISTS min_tip_amount;
