use crate::errors::app_error::AppError;
use crate::ml::feature_extractor::Features;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    pub name: String,
    pub impact: f32,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudScore {
    pub score: f32,
    pub risk_level: RiskLevel,
    pub factors: Vec<RiskFactor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

pub struct FraudDetector {
    threshold: f32,
}

impl FraudDetector {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }

    pub async fn score_transaction(&self, features: &Features) -> Result<FraudScore, AppError> {
        let score = self.calculate_score(features);

        let risk_level = if score > self.threshold {
            RiskLevel::High
        } else if score > 0.4 {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };

        let factors = self.explain_score(features, score);

        Ok(FraudScore {
            score,
            risk_level,
            factors,
        })
    }

    fn calculate_score(&self, features: &Features) -> f32 {
        let mut score: f32 = 0.0;

        if features.amount_normalized > 2.0 {
            score += 0.3;
        }

        if features.tips_last_hour > 10 {
            score += 0.4;
        }

        if features.sender_first_tip_days_ago < 1 {
            score += 0.2;
        }

        if features.creator_avg_tip > 0.0 && features.amount > features.creator_avg_tip * 3.0 {
            score += 0.15;
        }

        score.min(1.0)
    }

    fn explain_score(&self, features: &Features, _score: f32) -> Vec<RiskFactor> {
        let mut factors = Vec::new();

        if features.amount_normalized > 2.0 {
            factors.push(RiskFactor {
                name: "unusual_amount".to_string(),
                impact: 0.3,
                description: "Transaction amount is unusually high".to_string(),
            });
        }

        if features.tips_last_hour > 10 {
            factors.push(RiskFactor {
                name: "high_velocity".to_string(),
                impact: 0.4,
                description: "Unusually high transaction frequency".to_string(),
            });
        }

        if features.sender_first_tip_days_ago < 1 {
            factors.push(RiskFactor {
                name: "new_account".to_string(),
                impact: 0.2,
                description: "Sender account is very new".to_string(),
            });
        }

        factors
    }
}
