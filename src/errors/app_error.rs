use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

use super::{DatabaseError, StellarError, ValidationError};

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Stellar(#[from] StellarError),
    #[error(transparent)]
    Validation(#[from] ValidationError),
    #[error("creator not found")]
    CreatorNotFound { username: String },
    #[error("unauthorized")]
    Unauthorized { message: String },
    #[error("forbidden")]
    Forbidden { message: String },
    #[error("conflict")]
    Conflict { code: &'static str, message: String },
    #[error("internal server error")]
    Internal,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl AppError {
    pub fn internal() -> Self {
        Self::Internal
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized {
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden {
            message: message.into(),
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::Database(err) => match err {
                DatabaseError::NotFound { .. } => StatusCode::NOT_FOUND,
                DatabaseError::UniqueViolation { .. } => StatusCode::CONFLICT,
                DatabaseError::QueryFailed => StatusCode::INTERNAL_SERVER_ERROR,
            },
            Self::Stellar(err) => match err {
                StellarError::TransactionNotFound { .. } => StatusCode::UNPROCESSABLE_ENTITY,
                StellarError::InvalidTransaction { .. } => StatusCode::UNPROCESSABLE_ENTITY,
                StellarError::NetworkUnavailable => StatusCode::BAD_GATEWAY,
                StellarError::CircuitBreakerOpen => StatusCode::SERVICE_UNAVAILABLE,
            },
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::CreatorNotFound { .. } => StatusCode::NOT_FOUND,
            Self::Unauthorized { .. } => StatusCode::UNAUTHORIZED,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn body(&self) -> ErrorBody {
        match self {
            Self::Database(err) => {
                if matches!(err, DatabaseError::QueryFailed) {
                    ErrorBody {
                        code: err.code(),
                        message: "Internal server error".to_string(),
                        details: None,
                    }
                } else {
                    ErrorBody {
                        code: err.code(),
                        message: err.message(),
                        details: Some(err.details()),
                    }
                }
            }
            Self::Stellar(err) => ErrorBody {
                code: err.code(),
                message: err.message(),
                details: Some(err.details()),
            },
            Self::Validation(err) => ErrorBody {
                code: err.code(),
                message: err.message(),
                details: Some(err.details()),
            },
            Self::CreatorNotFound { username } => ErrorBody {
                code: "CREATOR_NOT_FOUND",
                message: "Creator not found".to_string(),
                details: Some(serde_json::json!({ "username": username })),
            },
            Self::Unauthorized { message } => ErrorBody {
                code: "UNAUTHORIZED",
                message: message.clone(),
                details: None,
            },
            Self::Forbidden { message } => ErrorBody {
                code: "FORBIDDEN",
                message: message.clone(),
                details: None,
            },
            Self::Conflict { code, message } => ErrorBody {
                code,
                message: message.clone(),
                details: None,
            },
            Self::Internal => ErrorBody {
                code: "INTERNAL_ERROR",
                message: "Internal server error".to_string(),
                details: None,
            },
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        let mapped = DatabaseError::from_sqlx(&value);
        if matches!(mapped, DatabaseError::QueryFailed) {
            tracing::error!(error = %value, "Database operation failed");
        }
        Self::Database(mapped)
    }
}

impl From<reqwest::Error> for AppError {
    fn from(_value: reqwest::Error) -> Self {
        Self::Stellar(StellarError::NetworkUnavailable)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        tracing::error!(error = %value, "Internal error propagated");
        Self::Internal
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(error = %self, "Request failed");
        } else {
            tracing::warn!(error = %self, "Request rejected");
        }

        let body = self.body();
        (status, Json(ErrorResponse { error: body })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn validation_error_serializes_with_details() {
        let err = AppError::Validation(ValidationError::InvalidRequest {
            message: "bad input".to_string(),
        });

        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "INVALID_REQUEST");
        assert_eq!(json["error"]["message"], "bad input");
    }

    #[tokio::test]
    async fn internal_error_hides_details() {
        let response = AppError::Internal.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "INTERNAL_ERROR");
        assert_eq!(json["error"]["message"], "Internal server error");
    }
}



