use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::http::{HeaderMap, StatusCode};
use chrono::{DateTime, Utc, TimeZone};
use serde::Deserialize;
use std::sync::Arc;
use crate::security::replay_protection::{ReplayProtectionService, ReplayProtectionError};

#[derive(Debug, Deserialize)]
pub struct ReplayHeaders {
    pub x_nonce: Option<String>,
    pub x_timestamp: Option<String>,
    pub x_client_id: Option<String>,
}

impl ReplayHeaders {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            x_nonce: headers
                .get("x-nonce")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string()),
            x_timestamp: headers
                .get("x-timestamp")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string()),
            x_client_id: headers
                .get("x-client-id")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string()),
        }
    }

    pub fn validate_format(&self) -> Result<(), ReplayProtectionError> {
        if self.x_nonce.is_none() || self.x_timestamp.is_none() {
            return Err(ReplayProtectionError::MissingNonceOrTimestamp);
        }
        Ok(())
    }

    pub fn parse_timestamp(&self) -> Result<DateTime<Utc>, ReplayProtectionError> {
        let timestamp_str = self.x_timestamp.as_ref().ok_or(ReplayProtectionError::MissingNonceOrTimestamp)?;
        
        // Try parsing as Unix timestamp (seconds)
        if let Ok(seconds) = timestamp_str.parse::<i64>() {
            return Ok(Utc.timestamp_opt(seconds, 0).single().unwrap_or_else(|| Utc::now()));
        }
        
        // Try parsing as Unix timestamp (milliseconds)
        if let Ok(millis) = timestamp_str.parse::<i64>() {
            return Ok(Utc.timestamp_opt(millis / 1000, (millis % 1000 * 1_000_000) as u32).single().unwrap_or_else(|| Utc::now()));
        }
        
        // Try parsing as ISO 8601
        timestamp_str.parse::<DateTime<Utc>>()
            .map_err(|_| ReplayProtectionError::InvalidNonceFormat(format!("Invalid timestamp format: {}", timestamp_str)))
    }
}

pub async fn replay_protection_middleware(
    State(replay_service): State<Arc<ReplayProtectionService>>,
    request: Request,
    next: Next,
) -> Result<Response, Response> {
    let headers = ReplayHeaders::from_headers(request.headers());
    let path = request.uri().path();
    let method = request.method().to_string();
    
    // Only apply replay protection to configured endpoints
    if !replay_service.config.enabled_endpoints.contains(&path.to_string()) {
        return Ok(next.run(request).await);
    }

    // Validate headers format
    if let Err(e) = headers.validate_format() {
        tracing::warn!("Replay protection header validation failed: {}", e);
        return Err(error_response(e));
    }

    // Parse timestamp
    let timestamp = match headers.parse_timestamp() {
        Ok(ts) => ts,
        Err(e) => {
            tracing::warn!("Replay protection timestamp parsing failed: {}", e);
            return Err(error_response(e));
        }
    };

    // Validate request
    let client_id = headers.x_client_id.as_deref();
    let nonce = headers.x_nonce.as_ref().unwrap();

    if let Err(e) = replay_service.validate_request(nonce, timestamp, client_id, path).await {
        tracing::warn!("Replay protection validation failed: {}", e);
        return Err(error_response(e));
    }

    tracing::debug!("Replay protection validation passed for nonce: {}", nonce);
    Ok(next.run(request).await)
}

fn error_response(error: ReplayProtectionError) -> Response {
    let (status, message) = match error {
        ReplayProtectionError::NonceAlreadyUsed(nonce) => {
            (StatusCode::CONFLICT, format!("Nonce {} has already been used", nonce))
        }
        ReplayProtectionError::TimestampTooOld(timestamp) => {
            (StatusCode::BAD_REQUEST, format!("Request timestamp {} is too old", timestamp))
        }
        ReplayProtectionError::TimestampTooFuture(timestamp) => {
            (StatusCode::BAD_REQUEST, format!("Request timestamp {} is too far in the future", timestamp))
        }
        ReplayProtectionError::InvalidNonceFormat(nonce) => {
            (StatusCode::BAD_REQUEST, format!("Invalid nonce format: {}", nonce))
        }
        ReplayProtectionError::MissingNonceOrTimestamp => {
            (StatusCode::BAD_REQUEST, "Missing required headers: x-nonce and x-timestamp".to_string())
        }
        ReplayProtectionError::RedisError(msg) => {
            tracing::error!("Redis error in replay protection: {}", msg);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
        }
    };

    let body = serde_json::json!({
        "error": "replay_protection_failed",
        "message": message,
        "timestamp": Utc::now()
    });

    (status, axum::Json(body)).into_response()
}

#[derive(Clone)]
pub struct ReplayProtectionMiddlewareFactory {
    service: Arc<ReplayProtectionService>,
}

impl ReplayProtectionMiddlewareFactory {
    pub fn new(service: Arc<ReplayProtectionService>) -> Self {
        Self { service }
    }

    pub fn middleware(&self) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, Response>> + Send>> + Clone {
        let service = self.service.clone();
        move |request: Request, next: Next| {
            let service = service.clone();
            Box::pin(async move {
                replay_protection_middleware(State(service), request, next).await
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, Method};
    use axum::body::Body;
    use tower::ServiceExt;

    fn create_test_request() -> Request {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/tips")
            .header("x-nonce", "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
            .header("x-timestamp", Utc::now().timestamp().to_string())
            .header("x-client-id", "test-client")
            .body(Body::empty())
            .unwrap();
        request
    }

    #[test]
    fn test_replay_headers_extraction() {
        let mut headers = HeaderMap::new();
        headers.insert("x-nonce", HeaderValue::from_static("test-nonce"));
        headers.insert("x-timestamp", HeaderValue::from_static("1234567890"));
        headers.insert("x-client-id", HeaderValue::from_static("test-client"));

        let replay_headers = ReplayHeaders::from_headers(&headers);
        
        assert_eq!(replay_headers.x_nonce, Some("test-nonce".to_string()));
        assert_eq!(replay_headers.x_timestamp, Some("1234567890".to_string()));
        assert_eq!(replay_headers.x_client_id, Some("test-client".to_string()));
    }

    #[test]
    fn test_timestamp_parsing() {
        let headers = ReplayHeaders {
            x_nonce: Some("test-nonce".to_string()),
            x_timestamp: Some("1640995200".to_string()), // 2022-01-01 00:00:00 UTC
            x_client_id: None,
        };

        let timestamp = headers.parse_timestamp().unwrap();
        assert_eq!(timestamp.timestamp(), 1640995200);
    }

    #[tokio::test]
    async fn test_replay_protection_validation() {
        let service = Arc::new(ReplayProtectionService::without_redis());
        let nonce = ReplayProtectionService::generate_nonce();
        let timestamp = Utc::now();
        
        // This should pass validation (no Redis, so no actual nonce checking)
        let result = service.validate_request(&nonce, timestamp, None, "/api/v1/tips").await;
        assert!(result.is_ok());
    }
}
