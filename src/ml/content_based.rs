use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentProfile {
    pub creator_id: Uuid,
    pub tags: Vec<String>,
    pub category: String,
    pub quality_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentRecommendation {
    pub creator_id: Uuid,
    pub score: f32,
    pub matched_tags: Vec<String>,
}

pub struct ContentBasedFilter {
    content_profiles: Vec<ContentProfile>,
}

impl ContentBasedFilter {
    pub fn new() -> Self {
        Self {
            content_profiles: Vec::new(),
        }
    }

    pub fn add_content(&mut self, profile: ContentProfile) {
        self.content_profiles.push(profile);
    }

    pub fn get_recommendations(
        &self,
        user_tags: &[String],
        limit: usize,
    ) -> Vec<ContentRecommendation> {
        let mut recommendations = Vec::new();

        for content in &self.content_profiles {
            let matched_tags: Vec<String> = content
                .tags
                .iter()
                .filter(|tag| user_tags.contains(tag))
                .cloned()
                .collect();

            if !matched_tags.is_empty() {
                let score = (matched_tags.len() as f32 / user_tags.len().max(1) as f32)
                    * content.quality_score;

                recommendations.push(ContentRecommendation {
                    creator_id: content.creator_id,
                    score,
                    matched_tags,
                });
            }
        }

        recommendations.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        recommendations.truncate(limit);
        recommendations
    }
}
