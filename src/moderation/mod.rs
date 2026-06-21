//! Content moderation module.
//!
//! Provides AI-powered and rule-based detection of inappropriate content,
//! spam, and policy violations, with a human review queue for flagged items.
//!
//! # Configuration
//!
//! | Environment variable   | Default | Description                                        |
//! |------------------------|---------|----------------------------------------------------|
//! | `MODERATION_ENABLED`   | `true`  | Set to `false` to bypass all moderation checks.    |
//! | `MODERATION_THRESHOLD` | `0.90`  | Confidence threshold above which content is blocked.|

pub mod ai_detector;
pub mod review_queue;
pub mod rules;

pub use ai_detector::AiDetector;
pub use review_queue::{ModerationFlag, ModerationHistoryEntry, ModerationQueueItem, ReviewQueue};
pub use rules::RulesEngine;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
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
    /// True when violations were detected or AI score exceeds the flag threshold.
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

// ── Runtime-configurable moderation settings ─────────────────────────────────

/// Serializable snapshot of the current moderation configuration,
/// returned by `GET /api/v1/admin/moderation/config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerationConfig {
    /// Whether moderation checks are active. When `false`, all content passes.
    pub enabled: bool,
    /// Confidence threshold (0.0–1.0) above which content is hard-blocked.
    pub threshold: f32,
}

/// Atomically-stored moderation configuration that can be updated at runtime
/// without restarting the service.
///
/// Threshold is stored as a `u32` bit-pattern of an `f32` so it can live in
/// an `AtomicU32`.
#[derive(Debug)]
pub struct AtomicModerationConfig {
    enabled: AtomicBool,
    /// f32 bits stored as u32 for atomic access.
    threshold_bits: AtomicU32,
}

impl AtomicModerationConfig {
    /// Load initial values from environment variables, falling back to defaults.
    pub fn from_env() -> Self {
        let enabled = std::env::var("MODERATION_ENABLED")
            .map(|v| v.to_lowercase() != "false" && v != "0")
            .unwrap_or(true);

        let threshold = std::env::var("MODERATION_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .map(|t| t.clamp(0.0, 1.0))
            .unwrap_or(0.90_f32);

        tracing::info!(
            enabled,
            threshold,
            "Moderation initialised (MODERATION_ENABLED / MODERATION_THRESHOLD)"
        );

        Self {
            enabled: AtomicBool::new(enabled),
            threshold_bits: AtomicU32::new(threshold.to_bits()),
        }
    }

    /// Read the current configuration snapshot.
    pub fn get(&self) -> ModerationConfig {
        ModerationConfig {
            enabled: self.enabled.load(Ordering::Relaxed),
            threshold: f32::from_bits(self.threshold_bits.load(Ordering::Relaxed)),
        }
    }

    /// Atomically update the configuration.
    pub fn set(&self, cfg: ModerationConfig) {
        let threshold = cfg.threshold.clamp(0.0, 1.0);
        self.enabled.store(cfg.enabled, Ordering::Relaxed);
        self.threshold_bits
            .store(threshold.to_bits(), Ordering::Relaxed);
        tracing::info!(
            enabled = cfg.enabled,
            threshold,
            "Moderation configuration updated at runtime"
        );
    }
}

// ── Service ───────────────────────────────────────────────────────────────────

/// Top-level orchestrator: runs rule-based checks, optionally follows up with AI
/// detection, and persists flagged items to the review queue.
pub struct ModerationService {
    rules: RulesEngine,
    ai: AiDetector,
    queue: ReviewQueue,
    config: Arc<AtomicModerationConfig>,
}

impl ModerationService {
    pub fn new(db: PgPool) -> Self {
        Self {
            rules: RulesEngine::new(),
            ai: AiDetector::new(),
            queue: ReviewQueue::new(db),
            config: Arc::new(AtomicModerationConfig::from_env()),
        }
    }

    /// Read the current runtime configuration.
    pub fn config(&self) -> ModerationConfig {
        self.config.get()
    }

    /// Update the runtime configuration (persists for the lifetime of the process).
    pub fn update_config(&self, cfg: ModerationConfig) {
        self.config.set(cfg);
    }

    /// Evaluate `content` of `content_type`. If flagged, the item is persisted to
    /// the review queue and associated with `content_id` when provided.
    ///
    /// When `MODERATION_ENABLED=false` this is a no-op and always returns a
    /// clean result so that development environments are not blocked.
    pub async fn check_content(
        &self,
        content: &str,
        content_type: ContentType,
        content_id: Option<Uuid>,
    ) -> ModerationResult {
        let cfg = self.config.get();

        // Short-circuit when moderation is disabled (e.g. development).
        if !cfg.enabled {
            return ModerationResult {
                is_flagged: false,
                violations: vec![],
                ai_score: None,
                ai_reasoning: None,
            };
        }

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

        // Use the configurable threshold for the soft "queue for review" gate.
        // Hard-blocking uses the same threshold at the call sites via
        // `ModerationResult::has_high_confidence_violation(threshold)`.
        let flag_threshold = cfg.threshold * 0.78; // ~78 % of block threshold triggers review
        let is_flagged = !violations.is_empty()
            || ai_score.map(|s| s > flag_threshold).unwrap_or(false);

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

    /// The current block threshold to use at call sites.
    pub fn block_threshold(&self) -> f32 {
        self.config.get().threshold
    }

    /// Returns `true` when moderation is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.get().enabled
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
