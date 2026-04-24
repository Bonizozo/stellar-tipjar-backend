use crate::currency::CurrencyService;
use crate::db::connection::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

/// GET /currencies/rates
/// Returns all current exchange rates (base: USD).
async fn rates(
    State(state): State<Arc<AppState>>,
    axum::Extension(svc): axum::Extension<Arc<CurrencyService>>,
) -> impl IntoResponse {
    let mut redis = state.redis.clone();
    match svc.get_rates(redis.as_mut()).await {
        Ok(r) => Json(r).into_response(),
        Err(e) => {
            tracing::error!("get_rates: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "exchange rate service unavailable"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct ConvertQuery {
    from: String,
    to: String,
    amount: f64,
}

/// GET /currencies/convert?from=XLM&to=EUR&amount=100
async fn convert(
    State(state): State<Arc<AppState>>,
    axum::Extension(svc): axum::Extension<Arc<CurrencyService>>,
    Query(q): Query<ConvertQuery>,
) -> impl IntoResponse {
    let mut redis = state.redis.clone();
    let rates = match svc.get_rates(redis.as_mut()).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("convert get_rates: {}", e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": "exchange rate service unavailable"})),
            )
                .into_response();
        }
    };

    match svc.convert(&rates, &q.from, &q.to, q.amount) {
        Ok(result) => Json(result).into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/currencies/rates", get(rates))
        .route("/currencies/convert", get(convert))
}
