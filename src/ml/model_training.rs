use super::collaborative_filter::{CreatorProfile, UserProfile};
use std::collections::HashMap;
use uuid::Uuid;

pub struct TrainingPipeline {
    batch_size: usize,
    learning_rate: f32,
    epochs: usize,
}

impl TrainingPipeline {
    pub fn new(batch_size: usize, learning_rate: f32, epochs: usize) -> Self {
        Self {
            batch_size,
            learning_rate,
            epochs,
        }
    }

    pub async fn train_model(
        &self,
        user_profiles: Vec<UserProfile>,
        creator_profiles: Vec<CreatorProfile>,
    ) -> anyhow::Result<TrainedModel> {
        tracing::info!(
            "Starting training with {} users and {} creators",
            user_profiles.len(),
            creator_profiles.len()
        );

        let mut model = TrainedModel::new();

        for epoch in 0..self.epochs {
            let mut total_loss = 0.0;

            for chunk in user_profiles.chunks(self.batch_size) {
                for user in chunk {
                    let loss = self.compute_loss(user, &creator_profiles);
                    total_loss += loss;
                }
            }

            let avg_loss = total_loss / user_profiles.len() as f32;
            tracing::debug!("Epoch {}: avg_loss = {}", epoch, avg_loss);
            model.losses.push(avg_loss);
        }

        tracing::info!("Training completed");
        Ok(model)
    }

    fn compute_loss(&self, user: &UserProfile, creators: &[CreatorProfile]) -> f32 {
        let mut loss = 0.0;
        for creator in creators {
            let predicted = user.engagement_score * creator.engagement_rate;
            let actual = user
                .creator_preferences
                .get(&creator.creator_id)
                .copied()
                .unwrap_or(0.0);
            loss += (predicted - actual).powi(2);
        }
        loss / creators.len() as f32
    }
}

pub struct TrainedModel {
    pub losses: Vec<f32>,
    pub weights: HashMap<String, f32>,
}

impl TrainedModel {
    pub fn new() -> Self {
        Self {
            losses: Vec::new(),
            weights: HashMap::new(),
        }
    }

    pub fn get_accuracy(&self) -> f32 {
        if self.losses.is_empty() {
            return 0.0;
        }
        let final_loss = self.losses.last().unwrap_or(&0.0);
        (1.0 - final_loss.min(1.0)).max(0.0)
    }
}
