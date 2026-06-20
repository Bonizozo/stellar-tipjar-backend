-- Subscription tiers defined by creators
CREATE TABLE IF NOT EXISTS subscription_tiers (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_username TEXT        NOT NULL REFERENCES creators(username) ON DELETE CASCADE,
    name             TEXT        NOT NULL,
    description      TEXT,
    price_xlm        TEXT        NOT NULL,          -- monthly price in XLM
    is_active        BOOLEAN     NOT NULL DEFAULT true,
    position         INT         NOT NULL DEFAULT 0, -- display order
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tiers_creator ON subscription_tiers(creator_username) WHERE is_active = true;

-- Benefits attached to a tier
CREATE TABLE IF NOT EXISTS tier_benefits (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tier_id     UUID NOT NULL REFERENCES subscription_tiers(id) ON DELETE CASCADE,
    description TEXT NOT NULL,
    position    INT  NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_benefits_tier ON tier_benefits(tier_id);

-- Active and historical subscriptions
CREATE TABLE IF NOT EXISTS subscriptions (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    tier_id          UUID        NOT NULL REFERENCES subscription_tiers(id),
    creator_username TEXT        NOT NULL,
    subscriber_ref   TEXT        NOT NULL,          -- wallet address or user identifier
    status           TEXT        NOT NULL DEFAULT 'active'
                                 CHECK (status IN ('active', 'cancelled', 'expired', 'past_due')),
    started_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    current_period_start TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    current_period_end   TIMESTAMPTZ NOT NULL,      -- set to +30 days on creation/renewal
    cancelled_at     TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tier_id, subscriber_ref)                -- one active sub per tier per subscriber
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_subscriber ON subscriptions(subscriber_ref);
CREATE INDEX IF NOT EXISTS idx_subscriptions_creator    ON subscriptions(creator_username);
CREATE INDEX IF NOT EXISTS idx_subscriptions_renewal    ON subscriptions(current_period_end)
    WHERE status = 'active';

-- Payment history for subscription renewals
CREATE TABLE IF NOT EXISTS subscription_payments (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    subscription_id  UUID        NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
    amount_xlm       TEXT        NOT NULL,
    transaction_hash TEXT,
    status           TEXT        NOT NULL DEFAULT 'pending'
                                 CHECK (status IN ('pending', 'completed', 'failed')),
    period_start     TIMESTAMPTZ NOT NULL,
    period_end       TIMESTAMPTZ NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sub_payments_subscription ON subscription_payments(subscription_id);
