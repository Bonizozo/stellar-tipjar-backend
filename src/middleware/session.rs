use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::http::{HeaderMap, StatusCode, header};
use std::sync::Arc;
use crate::security::session_management::{SessionManager, SessionData, SessionError};

pub struct SessionMiddlewareState {
    pub session_manager: Arc<SessionManager>,
}

pub async fn session_middleware(
    State(state): State<SessionMiddlewareState>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    let headers = request.headers();
    let path = request.uri().path();
    
    // Skip session check for certain paths
    if should_skip_session_check(path) {
        return Ok(next.run(request).await);
    }

    // Try to get session from cookie or header
    let session_id = extract_session_id(headers);
    
    if let Some(session_id) = session_id {
        // Validate session
        match state.session_manager.validate_session(&session_id).await {
            Ok(Some(session)) => {
                // Add session data to request extensions
                request.extensions_mut().insert(session);
                
                // Update session cookie with new expiration
                let mut response = next.run(request).await;
                update_session_cookie(&mut response, &session_id);
                return Ok(response);
            }
            Ok(None) => {
                tracing::warn!("Session not found: {}", session_id);
                return Err(create_session_error_response(SessionError::SessionNotFound(session_id)));
            }
            Err(e) => {
                tracing::warn!("Session validation failed: {}", e);
                return Err(create_session_error_response(e));
            }
        }
    } else {
        // No session provided - this could be a public endpoint
        if requires_authentication(path) {
            return Err(create_session_error_response(SessionError::SessionNotFound("no_session".to_string())));
        } else {
            return Ok(next.run(request).await);
        }
    }
}

fn should_skip_session_check(path: &str) -> bool {
    let skip_paths = [
        "/health",
        "/metrics",
        "/api/v1/auth/login",
        "/api/v1/auth/register",
        "/api/v1/auth/refresh",
        "/docs",
        "/swagger-ui",
        "/openapi.json",
    ];
    
    skip_paths.iter().any(|skip| path.starts_with(skip))
}

fn requires_authentication(path: &str) -> bool {
    let protected_paths = [
        "/api/v1/tips",
        "/api/v1/creators",
        "/api/v1/withdrawals",
        "/api/v1/profile",
        "/api/v1/admin",
    ];
    
    protected_paths.iter().any(|protected| path.starts_with(protected))
}

fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    // Try to get session from cookie first
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie_part in cookie_str.split(';') {
                let cookie_part = cookie_part.trim();
                if let Some((name, value)) = cookie_part.split_once('=') {
                    if name.trim() == "session_id" {
                        return Some(value.trim().to_string());
                    }
                }
            }
        }
    }
    
    // Try to get session from authorization header (Bearer token)
    if let Some(auth_header) = headers.get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    
    // Try to get session from custom header
    if let Some(session_header) = headers.get("x-session-id") {
        if let Ok(session_str) = session_header.to_str() {
            return Some(session_str.to_string());
        }
    }
    
    None
}

fn update_session_cookie(response: &mut Response, session_id: &str) {
    let cookie_value = format!(
        "session_id={}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=3600",
        session_id
    );
    
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie_value.parse().unwrap()
    );
}

fn create_session_error_response(error: SessionError) -> Response {
    let (status, message) = match error {
        SessionError::SessionNotFound(_) => {
            (StatusCode::UNAUTHORIZED, "Session not found or expired".to_string())
        }
        SessionError::SessionExpired(_) => {
            (StatusCode::UNAUTHORIZED, "Session has expired".to_string())
        }
        SessionError::InvalidSessionFormat(_) => {
            (StatusCode::UNAUTHORIZED, "Invalid session format".to_string())
        }
        SessionError::RedisError(msg) => {
            tracing::error!("Redis error in session middleware: {}", msg);
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
        }
        SessionError::CreationFailed(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "Session creation failed".to_string())
        }
    };

    let body = serde_json::json!({
        "error": "session_error",
        "message": message,
        "timestamp": chrono::Utc::now()
    });

    (status, axum::Json(body)).into_response()
}

#[derive(Clone)]
pub struct SessionMiddlewareFactory {
    session_manager: Arc<SessionManager>,
}

impl SessionMiddlewareFactory {
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self { session_manager }
    }

    pub fn middleware(&self) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, Response>> + Send>> + Clone {
        let session_manager = self.session_manager.clone();
        move |request: Request, next: Next| {
            let session_manager = session_manager.clone();
            Box::pin(async move {
                let state = SessionMiddlewareState { session_manager };
                session_middleware(State(state), request, next).await
            })
        }
    }
}

// Extension trait to easily get session data from request
pub trait RequestSessionExt {
    fn session(&self) -> Option<&SessionData>;
    fn session_mut(&mut self) -> Option<&mut SessionData>;
}

impl RequestSessionExt for Request {
    fn session(&self) -> Option<&SessionData> {
        self.extensions().get::<SessionData>()
    }

    fn session_mut(&mut self) -> Option<&mut SessionData> {
        self.extensions_mut().get_mut::<SessionData>()
    }
}

// Helper functions for creating and managing sessions
pub struct SessionHelper {
    session_manager: Arc<SessionManager>,
}

impl SessionHelper {
    pub fn new(session_manager: Arc<SessionManager>) -> Self {
        Self { session_manager }
    }

    pub async fn create_session_response(
        &self,
        user_id: &str,
        client_id: Option<&str>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<Response, SessionError> {
        let session = self.session_manager.create_session(user_id, client_id, ip_address, user_agent).await?;
        
        let cookie_value = format!(
            "session_id={}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=3600",
            session.session_id
        );

        let body = serde_json::json!({
            "session_id": session.session_id,
            "user_id": session.user_id,
            "created_at": session.created_at,
            "expires_at": session.expires_at
        });

        let mut response = (StatusCode::OK, axum::Json(body)).into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            cookie_value.parse().unwrap()
        );

        Ok(response)
    }

    pub async fn logout_response(&self, session_id: &str) -> Result<Response, SessionError> {
        self.session_manager.delete_session(session_id).await?;

        let cookie_value = "session_id=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0";
        
        let mut response = (StatusCode::OK, axum::Json(serde_json::json!({"message": "Logged out successfully"}))).into_response();
        response.headers_mut().insert(
            header::SET_COOKIE,
            cookie_value.parse().unwrap()
        );

        Ok(response)
    }

    pub async fn refresh_session_response(&self, session_id: &str) -> Result<Response, SessionError> {
        let session = self.session_manager.refresh_session(session_id, None).await?;
        
        if let Some(session) = session {
            let cookie_value = format!(
                "session_id={}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=3600",
                session.session_id
            );

            let body = serde_json::json!({
                "session_id": session.session_id,
                "user_id": session.user_id,
                "expires_at": session.expires_at
            });

            let mut response = (StatusCode::OK, axum::Json(body)).into_response();
            response.headers_mut().insert(
                header::SET_COOKIE,
                cookie_value.parse().unwrap()
            );

            Ok(response)
        } else {
            Err(SessionError::SessionNotFound(session_id.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderValue, Method};
    use axum::body::Body;

    fn create_test_request_with_session(session_id: &str) -> Request {
        let mut request = Request::builder()
            .method(Method::GET)
            .uri("/api/v1/profile")
            .header("Cookie", format!("session_id={}", session_id))
            .body(Body::empty())
            .unwrap();
        request
    }

    fn create_test_request_without_session() -> Request {
        Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn test_session_id_extraction_from_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", HeaderValue::from_static("session_id=test123; other=value"));

        let session_id = extract_session_id(&headers);
        assert_eq!(session_id, Some("test123".to_string()));
    }

    #[test]
    fn test_session_id_extraction_from_auth_header() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer test123"));

        let session_id = extract_session_id(&headers);
        assert_eq!(session_id, Some("test123".to_string()));
    }

    #[test]
    fn test_session_id_extraction_from_custom_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-session-id", HeaderValue::from_static("test123"));

        let session_id = extract_session_id(&headers);
        assert_eq!(session_id, Some("test123".to_string()));
    }

    #[test]
    fn test_should_skip_session_check() {
        assert!(should_skip_session_check("/health"));
        assert!(should_skip_session_check("/api/v1/auth/login"));
        assert!(should_skip_session_check("/docs"));
        assert!(!should_skip_session_check("/api/v1/profile"));
        assert!(!should_skip_session_check("/api/v1/tips"));
    }

    #[test]
    fn test_requires_authentication() {
        assert!(!requires_authentication("/health"));
        assert!(!requires_authentication("/api/v1/auth/login"));
        assert!(requires_authentication("/api/v1/profile"));
        assert!(requires_authentication("/api/v1/tips"));
        assert!(requires_authentication("/api/v1/admin/users"));
    }

    #[tokio::test]
    async fn test_session_helper_without_redis() {
        let session_manager = Arc::new(SessionManager::without_redis());
        let helper = SessionHelper::new(session_manager);

        // This should work even without Redis
        let result = helper.create_session_response("user123", None, None, None).await;
        assert!(result.is_ok());
    }
}
