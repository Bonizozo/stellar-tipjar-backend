ALTER TABLE creators
    ADD COLUMN IF NOT EXISTS min_tip_amount TEXT,
    ADD COLUMN IF NOT EXISTS max_tip_amount TEXT,
    ADD COLUMN IF NOT EXISTS max_tips_per_minute INTEGER;

ALTER TABLE tips
    ADD COLUMN IF NOT EXISTS tipper_wallet TEXT,
    ADD COLUMN IF NOT EXISTS tipper_ip TEXT;

CREATE INDEX IF NOT EXISTS idx_tips_tipper_wallet_created ON tips(tipper_wallet, created_at DESC) WHERE tipper_wallet IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tips_tipper_ip_created ON tips(tipper_ip, created_at DESC) WHERE tipper_ip IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_tips_creator_recent ON tips(creator_username, created_at DESC);
