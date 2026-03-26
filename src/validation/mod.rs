pub mod amount;
pub mod stellar;

use axum::{
    extract::{rejection::JsonRejection, FromRequest, Request},
    Json,
};
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::errors::{AppError, ValidationError};

/// A drop-in replacement for `axum::Json` that also runs `validator::Validate`
/// on the deserialized body, returning a structured 400 on failure.
pub struct ValidatedJson<T>(pub T);

impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|e: JsonRejection| {
                ValidationError::InvalidJson {
                    reason: e.to_string(),
                }
            })?;

        value.validate().map_err(|errors| {
            // Flatten validator errors into a simple field -> [messages] map.
            let fields: serde_json::Map<String, serde_json::Value> = errors
                .field_errors()
                .iter()
                .map(|(field, errs)| {
                    let messages: Vec<String> = errs
                        .iter()
                        .map(|e| {
                            e.message
                                .as_ref()
                                .map(|m| m.to_string())
                                .unwrap_or_else(|| e.code.to_string())
                        })
                        .collect();
                    (field.to_string(), serde_json::json!(messages))
                })
                .collect();

            AppError::Validation(ValidationError::InvalidFields { fields })
        })?;

        Ok(ValidatedJson(value))
    }
}
