CREATE TABLE IF NOT EXISTS campaign_contributions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    campaign_id UUID NOT NULL REFERENCES campaigns(id) ON DELETE CASCADE,
    tip_id UUID NOT NULL REFERENCES tips(id) ON DELETE CASCADE,
    matched_amount TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_campaign_contributions_campaign
    ON campaign_contributions(campaign_id);
CREATE INDEX IF NOT EXISTS idx_campaign_contributions_tip
    ON campaign_contributions(tip_id);
