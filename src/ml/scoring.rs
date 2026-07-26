use crate::errors::app_error::AppError;
use crate::ml::feature_extractor::FeatureExtractor;
use crate::ml::fraud_detector::FraudDetector;
use crate::ml::fraud_detector::FraudScore;
use crate::models::tip::Tip;
use redis::aio::ConnectionManager;
use std::sync::Arc;

pub struct RealtimeFraudScorer {
    detector: Arc<FraudDetector>,
    feature_extractor: Arc<FeatureExtractor>,
    redis: ConnectionManager,
}

impl RealtimeFraudScorer {
    pub fn new(
        detector: Arc<FraudDetector>,
        feature_extractor: Arc<FeatureExtractor>,
        redis: ConnectionManager,
    ) -> Self {
        Self {
            detector,
            feature_extractor,
            redis,
        }
    }

    pub async fn score_tip(&self, tip: &Tip) -> Result<FraudScore, AppError> {
        let cache_key = format!("fraud_score:{}", tip.id);

        if let Ok(cached) = redis::cmd("GET")
            .arg(&cache_key)
            .query_async::<Option<String>>(&mut self.redis.clone())
            .await
        {
            if let Some(cached_json) = cached {
                if let Ok(score) = serde_json::from_str::<FraudScore>(&cached_json) {
                    return Ok(score);
                }
            }
        }

        let features = self.feature_extractor.extract_features(tip).await?;
        let score = self.detector.score_transaction(&features).await?;

        let _ = redis::cmd("SETEX")
            .arg(&cache_key)
            .arg(3600)
            .arg(serde_json::to_string(&score).unwrap_or_default())
            .query_async::<()>(&mut self.redis.clone())
            .await;

        Ok(score)
    }
}
