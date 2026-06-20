-- Email status enum
CREATE TYPE email_status AS ENUM ('pending', 'sent', 'delivered', 'bounced', 'complained', 'failed');

-- Email deliveries tracking table
CREATE TABLE IF NOT EXISTS email_deliveries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_id UUID NOT NULL REFERENCES creators(id) ON DELETE CASCADE,
    email_type VARCHAR(100) NOT NULL,
    recipient VARCHAR(255) NOT NULL,
    subject VARCHAR(500) NOT NULL,
    status email_status NOT NULL DEFAULT 'pending',
    error_message TEXT,
    sent_at TIMESTAMP,
    delivered_at TIMESTAMP,
    bounced_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_email_deliveries_creator ON email_deliveries(creator_id, created_at DESC);
CREATE INDEX idx_email_deliveries_status ON email_deliveries(status);
CREATE INDEX idx_email_deliveries_type ON email_deliveries(email_type);

-- Email preferences table
CREATE TABLE IF NOT EXISTS email_preferences (
    creator_id UUID PRIMARY KEY REFERENCES creators(id) ON DELETE CASCADE,
    tip_notifications BOOLEAN NOT NULL DEFAULT true,
    weekly_summary BOOLEAN NOT NULL DEFAULT true,
    marketing_emails BOOLEAN NOT NULL DEFAULT true,
    unsubscribed_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_email_preferences_notifications ON email_preferences(tip_notifications) WHERE tip_notifications = true;
CREATE INDEX idx_email_preferences_weekly ON email_preferences(weekly_summary) WHERE weekly_summary = true;
