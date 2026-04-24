-- Matching campaign system for sponsors to amplify creator tips.
CREATE TABLE IF NOT EXISTS campaigns (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sponsor_name TEXT NOT NULL,
    creator_username TEXT NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    match_ratio TEXT NOT NULL DEFAULT '1.0',
    per_tip_cap TEXT NOT NULL DEFAULT '0',
    total_budget TEXT NOT NULL,
    remaining_budget TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    starts_at TIMESTAMPTZ,
    ends_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS campaign_matches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    campaign_id UUID NOT NULL REFERENCES campaigns(id) ON DELETE CASCADE,
    tip_id UUID NOT NULL REFERENCES tips(id) ON DELETE CASCADE,
    matched_amount TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_campaigns_creator_active
    ON campaigns(creator_username, active, starts_at, ends_at, created_at);
CREATE INDEX IF NOT EXISTS idx_campaign_matches_tip ON campaign_matches(tip_id);
