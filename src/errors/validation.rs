#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("invalid JSON payload")]
    InvalidJson { reason: String },
    #[error("validation failed")]
    InvalidFields { fields: serde_json::Map<String, serde_json::Value> },
    #[error("invalid request")]
    InvalidRequest { message: String },
}

impl ValidationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidJson { .. } => "INVALID_JSON",
            Self::InvalidFields { .. } => "VALIDATION_ERROR",
            Self::InvalidRequest { .. } => "INVALID_REQUEST",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidJson { reason } => format!("Invalid JSON payload: {}", reason),
            Self::InvalidFields { .. } => "One or more fields failed validation".to_string(),
            Self::InvalidRequest { message } => message.clone(),
        }
    }

    pub fn details(&self) -> serde_json::Value {
        match self {
            Self::InvalidJson { reason } => serde_json::json!({ "reason": reason }),
            Self::InvalidFields { fields } => serde_json::json!({ "fields": fields }),
            Self::InvalidRequest { .. } => serde_json::json!({}),
        }
    }
}

