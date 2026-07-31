//! AI-powered content moderation via the Anthropic Claude API.
//!
//! Requires `ANTHROPIC_API_KEY` to be set. If the variable is absent the
//! detector reports itself as disabled and the [`ModerationService`] will
//! skip this step entirely, falling back to rule-based checks only.

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{Violation, ViolationType};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
// Use the fast, cost-effective model for high-throughput moderation.
const MODEL: &str = "claude-haiku-4-5-20251001";

/// The result returned by the AI detector for a single piece of content.
pub struct AiDetectionResult {
    /// Overall harm probability in the range 0.0–1.0.
    pub score: f32,
    /// Human-readable explanation from the model.
    pub reasoning: String,
    /// Structured violations extracted from the AI response.
    pub violations: Vec<Violation>,
}

// ── Anthropic API request/response shapes ────────────────────────────────────

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<ApiMessage<'a>>,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: String,
}

// ── Expected JSON schema inside the model's reply ────────────────────────────

#[derive(Deserialize)]
struct ModerationResponse {
    is_flagged: bool,
    score: f32,
    reasoning: String,
    violations: Vec<AiViolation>,
}

#[derive(Deserialize)]
struct AiViolation {
    #[serde(rename = "type")]
    violation_type: String,
    description: String,
    confidence: f32,
}

impl AiViolation {
    fn into_violation(self) -> Violation {
        let violation_type = match self.violation_type.as_str() {
            "spam" => ViolationType::Spam,
            "hate_speech" => ViolationType::HateSpeech,
            "personal_information" => ViolationType::PersonalInformation,
            "policy_violation" => ViolationType::PolicyViolation,
            _ => ViolationType::InappropriateContent,
        };
        Violation {
            violation_type,
            description: self.description,
            confidence: self.confidence.clamp(0.0, 1.0),
        }
    }
}

// ── Detector ──────────────────────────────────────────────────────────────────

pub struct AiDetector {
    client: Client,
    api_key: Option<String>,
}

impl AiDetector {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
        }
    }

    /// Returns `true` when an API key is available.
    pub fn is_enabled(&self) -> bool {
        self.api_key.is_some()
    }

    /// Send `content` to the Claude API and return a structured moderation result.
    pub async fn analyze(&self, content: &str) -> anyhow::Result<AiDetectionResult> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("ANTHROPIC_API_KEY not configured"))?;

        let prompt = build_moderation_prompt(content);

        let request_body = ApiRequest {
            model: MODEL,
            max_tokens: 512,
            messages: vec![ApiMessage {
                role: "user",
                content: &prompt,
            }],
        };

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error {}: {}", status, body);
        }

        let api_response: ApiResponse = response.json().await?;

        let text = api_response
            .content
            .into_iter()
            .next()
            .map(|b| b.text)
            .unwrap_or_default();

        parse_moderation_response(&text)
    }
}

fn build_moderation_prompt(content: &str) -> String {
    format!(
        r#"You are a content moderation assistant for a creator tipping platform.
Evaluate the following user-submitted content for policy violations.

Content to evaluate:
<content>{}</content>

Respond with a JSON object using exactly this schema (no extra keys, no markdown):
{{
  "is_flagged": <true|false>,
  "score": <0.0 to 1.0 harm probability>,
  "reasoning": "<one sentence explanation>",
  "violations": [
    {{
      "type": "<spam|inappropriate_content|hate_speech|personal_information|policy_violation>",
      "description": "<specific description>",
      "confidence": <0.0 to 1.0>
    }}
  ]
}}

Return an empty violations array when the content is acceptable."#,
        content
    )
}

fn parse_moderation_response(text: &str) -> anyhow::Result<AiDetectionResult> {
    // Strip any accidental markdown fences the model may add.
    let json_str = text
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let parsed: ModerationResponse = serde_json::from_str(json_str).map_err(|e| {
        anyhow::anyhow!(
            "Failed to parse moderation response: {} — raw: {}",
            e,
            json_str
        )
    })?;

    let violations: Vec<Violation> = parsed
        .violations
        .into_iter()
        .map(|v| v.into_violation())
        .collect();

    Ok(AiDetectionResult {
        score: parsed.score.clamp(0.0, 1.0),
        reasoning: parsed.reasoning,
        violations,
    })
}
