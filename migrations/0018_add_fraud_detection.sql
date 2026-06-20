-- Create fraud detection logs table
CREATE TABLE IF NOT EXISTS fraud_detection_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tip_id UUID NOT NULL,
    fraud_score FLOAT NOT NULL,
    risk_level VARCHAR(20) NOT NULL,
    factors JSONB NOT NULL DEFAULT '[]',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Create index for fraud logs
CREATE INDEX IF NOT EXISTS idx_fraud_logs_tip_id ON fraud_detection_logs(tip_id);
CREATE INDEX IF NOT EXISTS idx_fraud_logs_risk_level ON fraud_detection_logs(risk_level);
CREATE INDEX IF NOT EXISTS idx_fraud_logs_created_at ON fraud_detection_logs(created_at);
