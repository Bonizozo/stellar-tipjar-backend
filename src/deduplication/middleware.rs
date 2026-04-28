use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::http::{HeaderMap, StatusCode};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::Instant;

use super::service::{DeduplicationService, RequestRecord, DeduplicationError};
use super::fingerprint::RequestFingerprint;

pub struct DeduplicationMiddlewareState {
    pub deduplication_service: Arc<DeduplicationService>,
}

pub async fn deduplication_middleware(
    State(state): State<DeduplicationMiddlewareState>,
    request: Request,
    next: Next,
) -> Result<Response, Response> {
    let start_time = Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let query_string = request.uri().query().unwrap_or("").to_string();
    
    // Extract request body (only for methods that typically have bodies)
    let (mut request, body_bytes) = if matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
        extract_body(request).await?
    } else {
        (request, Vec::new())
    };

    let body_str = String::from_utf8_lossy(&body_bytes);
    
    // Extract headers
    let headers = extract_headers(request.headers());
    
    // Extract query parameters
    let query_params = extract_query_params(&query_string);
    
    // Extract client information
    let client_id = headers.get("x-client-id").cloned();
    let idempotency_key = headers.get("idempotency-key").cloned();

    // Generate fingerprint
    let fingerprint = state.deduplication_service.generate_fingerprint(
        &method,
        &path,
        &body_str,
        &headers,
        &query_params,
        client_id.as_deref(),
        idempotency_key.as_deref(),
    );

    // Check if request has been processed
    match state.deduplication_service.is_request_processed(&fingerprint).await {
        Ok(Some(record)) => {
            // Request already processed, return cached response
            tracing::info!("Duplicate request detected: {}", fingerprint.hash);
            
            let response = create_duplicate_response(&record);
            return Ok(response);
        }
        Ok(None) => {
            // Request not processed, continue with processing
            tracing::debug!("New request: {}", fingerprint.hash);
        }
        Err(e) => {
            tracing::warn!("Deduplication check failed: {}", e);
            // Continue processing even if deduplication fails
        }
    }

    // Process the request
    let mut response = next.run(request).await;
    let processing_time = start_time.elapsed().as_millis() as u64;

    // Record the processed request
    let response_status = response.status().as_u16();
    let response_body = extract_response_body(&mut response).await;

    let record = RequestRecord::new(
        fingerprint.clone(),
        response_status,
        processing_time,
        client_id,
    )
    .with_response_body(&response_body);

    // Store the record asynchronously (don't block the response)
    let deduplication_service = state.deduplication_service.clone();
    tokio::spawn(async move {
        if let Err(e) = deduplication_service.record_request(record).await {
            tracing::error!("Failed to record request: {}", e);
        }
    });

    Ok(response)
}

async fn extract_body(mut request: Request) -> Result<(Request, Vec<u8>), Response> {
    use axum::body::Bytes;
    use http_body_util::BodyExt;
    
    let (parts, body) = request.into_parts();
    
    match body.collect().await {
        Ok(collected) => {
            let bytes = collected.to_bytes();
            let body_vec = bytes.to_vec();
            
            // Recreate the request with the body
            let new_body = axum::body::Body::from(bytes);
            let new_request = Request::from_parts(parts, new_body);
            
            Ok((new_request, body_vec))
        }
        Err(e) => {
            tracing::error!("Failed to extract request body: {}", e);
            Err((StatusCode::BAD_REQUEST, "Invalid request body").into_response())
        }
    }
}

fn extract_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut header_map = HashMap::new();
    
    for (name, value) in headers {
        if let Ok(name_str) = name.as_str() {
            if let Ok(value_str) = value.to_str() {
                header_map.insert(name_str.to_string(), value_str.to_string());
            }
        }
    }
    
    header_map
}

fn extract_query_params(query_string: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    
    if query_string.is_empty() {
        return params;
    }
    
    for pair in query_string.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            params.insert(
                urlencoding::decode(key).unwrap_or(key).to_string(),
                urlencoding::decode(value).unwrap_or(value).to_string(),
            );
        }
    }
    
    params
}

async fn extract_response_body(response: &mut Response) -> String {
    use axum::body::Body;
    use http_body_util::BodyExt;
    
    // This is a simplified implementation
    // In a real implementation, you'd need to handle response body extraction more carefully
    // as it might be streamed
    
    // For now, return empty string as we can't easily extract response body without consuming it
    String::new()
}

fn create_duplicate_response(record: &RequestRecord) -> Response {
    let body = serde_json::json!({
        "error": "duplicate_request",
        "message": "This request has already been processed",
        "request_id": record.fingerprint.hash,
        "processed_at": record.processed_at,
        "response_status": record.response_status,
        "processing_time_ms": record.processing_time_ms,
        "idempotency_key": record.fingerprint.idempotency_key,
    });

    // If the original request was successful, return the original status
    // Otherwise, return conflict status
    let status = if record.response_status < 400 {
        StatusCode::OK
    } else {
        StatusCode::CONFLICT
    };

    // Add headers to indicate this is a cached response
    let mut response = (status, axum::Json(body)).into_response();
    response.headers_mut().insert(
        "X-Cached-Response",
        "true".parse().unwrap()
    );
    response.headers_mut().insert(
        "X-Original-Status",
        record.response_status.to_string().parse().unwrap()
    );
    response.headers_mut().insert(
        "X-Processed-At",
        record.processed_at.to_rfc3339().parse().unwrap()
    );

    response
}

#[derive(Clone)]
pub struct DeduplicationMiddlewareFactory {
    deduplication_service: Arc<DeduplicationService>,
}

impl DeduplicationMiddlewareFactory {
    pub fn new(deduplication_service: Arc<DeduplicationService>) -> Self {
        Self { deduplication_service }
    }

    pub fn middleware(&self) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, Response>> + Send>> + Clone {
        let deduplication_service = self.deduplication_service.clone();
        move |request: Request, next: Next| {
            let deduplication_service = deduplication_service.clone();
            Box::pin(async move {
                let state = DeduplicationMiddlewareState { deduplication_service };
                deduplication_middleware(State(state), request, next).await
            })
        }
    }
}

// Extension trait to easily get deduplication info from request
pub trait RequestDeduplicationExt {
    fn fingerprint(&self) -> Option<&RequestFingerprint>;
    fn is_duplicate(&self) -> bool;
}

impl RequestDeduplicationExt for Request {
    fn fingerprint(&self) -> Option<&RequestFingerprint> {
        self.extensions().get::<RequestFingerprint>()
    }

    fn is_duplicate(&self) -> bool {
        self.extensions().contains::<RequestFingerprint>()
    }
}

// Helper functions for manual deduplication
pub struct DeduplicationHelper {
    deduplication_service: Arc<DeduplicationService>,
}

impl DeduplicationHelper {
    pub fn new(deduplication_service: Arc<DeduplicationService>) -> Self {
        Self { deduplication_service }
    }

    pub async fn check_and_record_request(
        &self,
        method: &str,
        path: &str,
        body: &str,
        headers: &HashMap<String, String>,
        query_params: &HashMap<String, String>,
        client_id: Option<&str>,
        idempotency_key: Option<&str>,
    ) -> Result<Option<RequestRecord>, DeduplicationError> {
        // Generate fingerprint
        let fingerprint = self.deduplication_service.generate_fingerprint(
            method,
            path,
            body,
            headers,
            query_params,
            client_id,
            idempotency_key,
        );

        // Check if already processed
        if let Some(record) = self.deduplication_service.is_request_processed(&fingerprint).await? {
            return Ok(Some(record));
        }

        // Not processed, return None
        Ok(None)
    }

    pub async fn manually_record_request(
        &self,
        method: &str,
        path: &str,
        body: &str,
        headers: &HashMap<String, String>,
        query_params: &HashMap<String, String>,
        client_id: Option<&str>,
        idempotency_key: Option<&str>,
        response_status: u16,
        processing_time_ms: u64,
        response_body: Option<&str>,
    ) -> Result<(), DeduplicationError> {
        let fingerprint = self.deduplication_service.generate_fingerprint(
            method,
            path,
            body,
            headers,
            query_params,
            client_id,
            idempotency_key,
        );

        let mut record = RequestRecord::new(
            fingerprint,
            response_status,
            processing_time_ms,
            client_id.map(|s| s.to_string()),
        );

        if let Some(body) = response_body {
            record = record.with_response_body(body);
        }

        self.deduplication_service.record_request(record).await
    }

    pub async fn get_client_request_history(
        &self,
        client_id: &str,
    ) -> Result<Vec<RequestRecord>, DeduplicationError> {
        self.deduplication_service.get_client_records(client_id).await
    }

    pub async fn clear_deduplication_cache(&self) -> Result<u64, DeduplicationError> {
        self.deduplication_service.clear_all_records().await
    }

    pub async fn get_deduplication_statistics(&self) -> Result<super::service::DeduplicationStats, DeduplicationError> {
        self.deduplication_service.get_deduplication_stats().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, Method};
    use axum::body::Body;

    fn create_test_request() -> Request {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/tips?amount=100")
            .header("content-type", "application/json")
            .header("x-client-id", "test-client")
            .header("idempotency-key", "test-key-123")
            .body(Body::from("{\"amount\":100}"))
            .unwrap();
        request
    }

    #[test]
    fn test_header_extraction() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("x-client-id", HeaderValue::from_static("test-client"));

        let extracted = extract_headers(&headers);
        assert_eq!(extracted.get("content-type"), Some(&"application/json".to_string()));
        assert_eq!(extracted.get("x-client-id"), Some(&"test-client".to_string()));
    }

    #[test]
    fn test_query_param_extraction() {
        let query_string = "amount=100&currency=USD&timestamp=123456789";
        let params = extract_query_params(query_string);

        assert_eq!(params.get("amount"), Some(&"100".to_string()));
        assert_eq!(params.get("currency"), Some(&"USD".to_string()));
        assert_eq!(params.get("timestamp"), Some(&"123456789".to_string()));
    }

    #[test]
    fn test_empty_query_param_extraction() {
        let params = extract_query_params("");
        assert!(params.is_empty());

        let params = extract_query_params("single=value");
        assert_eq!(params.len(), 1);
        assert_eq!(params.get("single"), Some(&"value".to_string()));
    }

    #[tokio::test]
    async fn test_deduplication_helper() {
        let service = Arc::new(DeduplicationService::without_redis());
        let helper = DeduplicationHelper::new(service);

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        let mut query_params = HashMap::new();
        query_params.insert("amount".to_string(), "100".to_string());

        let result = helper.check_and_record_request(
            "POST",
            "/api/v1/tips",
            "{\"amount\":100}",
            &headers,
            &query_params,
            Some("test-client"),
            Some("test-key"),
        ).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // No Redis, so no record found
    }

    #[test]
    fn test_request_deduplication_ext() {
        let request = create_test_request();
        
        // Initially no fingerprint
        assert!(request.fingerprint().is_none());
        assert!(!request.is_duplicate());
    }
}
