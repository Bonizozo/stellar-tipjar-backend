use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::recorder::MockRecorder;
use super::registry::{MockRegistry, MockRequest, MockResponse};
use super::templates::render_response_template;

#[derive(Clone)]
pub struct MockServer {
    pub registry: MockRegistry,
    pub recorder: MockRecorder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockServerRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterMockRequest {
    pub request: MockRequest,
    pub response: MockResponse,
}

impl MockServer {
    pub fn new() -> Self {
        Self {
            registry: MockRegistry::new(),
            recorder: MockRecorder::new(),
        }
    }

    pub fn router(self: Arc<Self>) -> Router {
        Router::new()
            .route("/_mock/entries", post(register).get(list).delete(clear))
            .route("/_mock/entries/:id", delete(remove))
            .route("/_mock/recording/enable", post(enable_recording))
            .route("/_mock/recording/disable", post(disable_recording))
            .route("/_mock/recording", get(recordings).delete(clear_recordings))
            .route("/_mock/replay", post(replay_recordings))
            .route("/{*path}", any(handle))
            .with_state(self)
    }

    pub async fn handle_request(&self, req: MockServerRequest) -> MockResponse {
        let request_path = req.path.clone();
        let request_body = req.body.clone();

        let Some((response, path_params)) = self
            .registry
            .match_http_request_with_context(
                &req.method,
                &request_path,
                request_body.as_ref(),
                Some(&req.headers),
            )
            .await
        else {
            let body = json!({
                "error": "no mock matched request",
                "method": req.method,
                "path": request_path
            });
            return MockResponse {
                status: StatusCode::NOT_FOUND.as_u16(),
                body,
                headers: HashMap::new(),
            };
        };

        let rendered =
            render_response_template(&response.body, &req.method, &req.path, &path_params);

        let final_response = MockResponse {
            status: response.status,
            body: rendered,
            headers: response.headers.clone(),
        };

        self.recorder
            .record(
                &req.method,
                &request_path,
                request_body,
                final_response.status,
                final_response.body.clone(),
            )
            .await;

        final_response
    }
}

impl Default for MockServer {
    fn default() -> Self {
        Self::new()
    }
}

async fn register(
    State(server): State<Arc<MockServer>>,
    Json(payload): Json<RegisterMockRequest>,
) -> Json<Value> {
    let id = server
        .registry
        .register(payload.request, payload.response)
        .await;
    Json(json!({ "id": id }))
}

async fn list(State(server): State<Arc<MockServer>>) -> Json<Vec<super::registry::MockEntry>> {
    Json(server.registry.list().await)
}

async fn remove(State(server): State<Arc<MockServer>>, Path(id): Path<String>) -> StatusCode {
    server.registry.remove(&id).await;
    StatusCode::NO_CONTENT
}

async fn clear(State(server): State<Arc<MockServer>>) -> StatusCode {
    server.registry.clear().await;
    StatusCode::NO_CONTENT
}

async fn enable_recording(State(server): State<Arc<MockServer>>) -> StatusCode {
    server.recorder.enable();
    StatusCode::NO_CONTENT
}

async fn disable_recording(State(server): State<Arc<MockServer>>) -> StatusCode {
    server.recorder.disable();
    StatusCode::NO_CONTENT
}

async fn recordings(
    State(server): State<Arc<MockServer>>,
) -> Json<Vec<super::recorder::RecordedInteraction>> {
    Json(server.recorder.get_all().await)
}

async fn clear_recordings(State(server): State<Arc<MockServer>>) -> StatusCode {
    server.recorder.clear().await;
    StatusCode::NO_CONTENT
}

async fn replay_recordings(State(server): State<Arc<MockServer>>) -> Json<Value> {
    let entries = server.recorder.as_mock_entries().await;
    let mut restored = 0usize;
    for (req, res) in entries {
        server.registry.register(req, res).await;
        restored += 1;
    }
    Json(json!({ "restored": restored }))
}

async fn handle(
    State(server): State<Arc<MockServer>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let headers = headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|sv| (k.as_str().to_string(), sv.to_string()))
        })
        .collect::<HashMap<_, _>>();

    let body_json = if body.is_empty() {
        None
    } else {
        serde_json::from_slice::<Value>(&body).ok()
    };

    let request = MockServerRequest {
        method: method.to_string(),
        path: uri
            .path_and_query()
            .map(|p| p.as_str().to_string())
            .unwrap_or_else(|| uri.path().to_string()),
        headers,
        body: body_json,
    };

    let matched = server.handle_request(request).await;

    (
        StatusCode::from_u16(matched.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(matched.body),
    )
}
