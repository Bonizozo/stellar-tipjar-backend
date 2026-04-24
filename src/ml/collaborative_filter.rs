use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub user_id: Uuid,
    pub tip_history: Vec<Uuid>,
    pub creator_preferences: HashMap<Uuid, f32>,
    pub engagement_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorProfile {
    pub creator_id: Uuid,
    pub content_tags: Vec<String>,
    pub popularity_score: f32,
    pub engagement_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub creator_id: Uuid,
    pub score: f32,
    pub reason: String,
}

pub struct CollaborativeFilter {
    user_profiles: HashMap<Uuid, UserProfile>,
    creator_profiles: HashMap<Uuid, CreatorProfile>,
}

impl CollaborativeFilter {
    pub fn new() -> Self {
        Self {
            user_profiles: HashMap::new(),
            creator_profiles: HashMap::new(),
        }
    }

    pub fn add_user_profile(&mut self, profile: UserProfile) {
        self.user_profiles.insert(profile.user_id, profile);
    }

    pub fn add_creator_profile(&mut self, profile: CreatorProfile) {
        self.creator_profiles.insert(profile.creator_id, profile);
    }

    pub fn get_recommendations(&self, user_id: Uuid, limit: usize) -> Vec<Recommendation> {
        let user = match self.user_profiles.get(&user_id) {
            Some(u) => u,
            None => return vec![],
        };

        let mut recommendations = Vec::new();

        for (creator_id, creator) in &self.creator_profiles {
            let similarity = self.calculate_similarity(user, creator);
            if similarity > 0.3 {
                recommendations.push(Recommendation {
                    creator_id: *creator_id,
                    score: similarity,
                    reason: "Based on your interests".to_string(),
                });
            }
        }

        recommendations.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        recommendations.truncate(limit);
        recommendations
    }

    fn calculate_similarity(&self, user: &UserProfile, creator: &CreatorProfile) -> f32 {
        let base_score = user.engagement_score * creator.engagement_rate;
        let preference_boost = user
            .creator_preferences
            .get(&creator.creator_id)
            .copied()
            .unwrap_or(0.0);
        (base_score + preference_boost) / 2.0
    }
}
