use super::collaborative_filter::{CollaborativeFilter, CreatorProfile, UserProfile};
use super::content_based::{ContentBasedFilter, ContentProfile};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationResult {
    pub creator_id: Uuid,
    pub score: f32,
    pub source: String,
}

pub struct RecommendationEngine {
    collaborative: CollaborativeFilter,
    content_based: ContentBasedFilter,
}

impl RecommendationEngine {
    pub fn new() -> Self {
        Self {
            collaborative: CollaborativeFilter::new(),
            content_based: ContentBasedFilter::new(),
        }
    }

    pub fn train(&mut self, user_profiles: Vec<UserProfile>, creator_profiles: Vec<CreatorProfile>) {
        for user in user_profiles {
            self.collaborative.add_user_profile(user);
        }
        for creator in creator_profiles {
            self.collaborative.add_creator_profile(creator);
        }
    }

    pub fn add_content(&mut self, profile: ContentProfile) {
        self.content_based.add_content(profile);
    }

    pub fn get_recommendations(
        &self,
        user_id: Uuid,
        user_tags: &[String],
        limit: usize,
    ) -> Vec<RecommendationResult> {
        let mut results = Vec::new();

        // Get collaborative filtering recommendations
        let collab_recs = self.collaborative.get_recommendations(user_id, limit);
        for rec in collab_recs {
            results.push(RecommendationResult {
                creator_id: rec.creator_id,
                score: rec.score,
                source: "collaborative".to_string(),
            });
        }

        // Get content-based recommendations
        let content_recs = self.content_based.get_recommendations(user_tags, limit);
        for rec in content_recs {
            if !results.iter().any(|r| r.creator_id == rec.creator_id) {
                results.push(RecommendationResult {
                    creator_id: rec.creator_id,
                    score: rec.score,
                    source: "content_based".to_string(),
                });
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results.truncate(limit);
        results
    }

    pub fn predict_cold_start(&self, user_tags: &[String], limit: usize) -> Vec<RecommendationResult> {
        let content_recs = self.content_based.get_recommendations(user_tags, limit);
        content_recs
            .into_iter()
            .map(|rec| RecommendationResult {
                creator_id: rec.creator_id,
                score: rec.score,
                source: "cold_start".to_string(),
            })
            .collect()
    }
}
