//! Content moderation module.
//!
//! Provides AI-powered and rule-based detection of inappropriate content,
//! spam, and policy violations, with a human review queue for flagged items.

pub mod ai_detector;
pub mod review_queue;
pub mod rules;

pub use ai_detector::AiDetector;
pub use review_queue::{ModerationFlag, ModerationHistoryEntry, ModerationQueueItem, ReviewQueue};
pub use rules::RulesEngine;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

/// Categories of policy violations the moderation system can detect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ViolationType {
    InappropriateContent,
    Spam,
    HateSpeech,
    PersonalInformation,
    PolicyViolation,
}

/// A single detected violation with a confidence score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    pub violation_type: ViolationType,
    pub description: String,
    /// 0.0 (uncertain) – 1.0 (certain)
    pub confidence: f32,
}

/// The kind of content being evaluated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Username,
    TipMessage,
    CreatorBio,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::Username => "username",
            ContentType::TipMessage => "tip_message",
            ContentType::CreatorBio => "creator_bio",
        }
    }
}

/// The combined result of running all moderation checks against a piece of content.
#[derive(Debug, Clone)]
pub struct ModerationResult {
    /// True when violations were detected or AI score exceeds the block threshold.
    pub is_flagged: bool,
    pub violations: Vec<Violation>,
    /// Aggregate harm probability returned by the AI detector (0.0–1.0).
    pub ai_score: Option<f32>,
    /// Free-form reasoning from the AI detector.
    pub ai_reasoning: Option<String>,
}

impl ModerationResult {
    /// Returns true when any violation has confidence above the given threshold,
    /// or when the AI score alone exceeds it. Used to decide whether to hard-block
    /// a request instead of merely queuing it for human review.
    pub fn has_high_confidence_violation(&self, threshold: f32) -> bool {
        self.violations.iter().any(|v| v.confidence >= threshold)
            || self.ai_score.map(|s| s >= threshold).unwrap_or(false)
    }
}

/// Top-level orchestrator: runs rule-based checks, optionally follows up with AI
/// detection, and persists flagged items to the review queue.
pub struct ModerationService {
    rules: RulesEngine,
    ai: AiDetector,
    queue: ReviewQueue,
}

impl ModerationService {
    pub fn new(db: PgPool) -> Self {
        Self {
            rules: RulesEngine::new(),
            ai: AiDetector::new(),
            queue: ReviewQueue::new(db),
        }
    }

    /// Evaluate `content` of `content_type`. If flagged, the item is persisted to
    /// the review queue and associated with `content_id` when provided.
    pub async fn check_content(
        &self,
        content: &str,
        content_type: ContentType,
        content_id: Option<Uuid>,
    ) -> ModerationResult {
        // 1. Fast rule-based pass — never makes network calls.
        let mut violations = self.rules.check(content, &content_type);

        // 2. AI detection when a key is configured.
        let (ai_score, ai_reasoning) = if self.ai.is_enabled() {
            match self.ai.analyze(content).await {
                Ok(ai_result) => {
                    violations.extend(ai_result.violations);
                    (Some(ai_result.score), Some(ai_result.reasoning))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "AI moderation check failed, using rules only");
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        let is_flagged = !violations.is_empty()
            || ai_score.map(|s| s > 0.7).unwrap_or(false);

        let result = ModerationResult {
            is_flagged,
            violations,
            ai_score,
            ai_reasoning,
        };

        // 3. Persist to review queue if flagged.
        if is_flagged {
            if let Err(e) = self
                .queue
                .enqueue(content, &content_type, content_id, &result)
                .await
            {
                tracing::error!(error = %e, "Failed to enqueue flagged content for review");
            }
        }

        result
    }

    /// Manually flag content for review (called from user-facing routes).
    pub async fn flag(
        &self,
        content_type: &str,
        content_id: Uuid,
        content_text: &str,
        reason: &str,
        flagged_by: &str,
    ) -> anyhow::Result<Uuid> {
        self.queue
            .flag(content_type, content_id, content_text, reason, flagged_by)
            .await
    }

    /// Expose the review queue so admin handlers can call it directly.
    pub fn queue(&self) -> &ReviewQueue {
        &self.queue
    }
}
