use axum::http::StatusCode;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UploadError {
    #[error("File too large: {0} bytes (max: {1} bytes)")]
    FileTooLarge(usize, usize),

    #[error("Invalid file type: {0}")]
    InvalidFileType(String),

    #[error("Invalid image format")]
    InvalidImageFormat,

    #[error("Upload rate limit exceeded")]
    RateLimitExceeded,

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Processing error: {0}")]
    ProcessingError(String),
}

impl axum::response::IntoResponse for UploadError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            UploadError::FileTooLarge(_, _) => (StatusCode::PAYLOAD_TOO_LARGE, self.to_string()),
            UploadError::InvalidFileType(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            UploadError::InvalidImageFormat => (StatusCode::BAD_REQUEST, self.to_string()),
            UploadError::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            UploadError::StorageError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            UploadError::ProcessingError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        (status, message).into_response()
    }
}

pub struct FileValidator {
    max_size: usize,
    allowed_types: Vec<String>,
}

impl FileValidator {
    pub fn new() -> Self {
        Self {
            max_size: 5 * 1024 * 1024, // 5MB default
            allowed_types: vec![
                "image/jpeg".to_string(),
                "image/png".to_string(),
                "image/webp".to_string(),
                "image/gif".to_string(),
            ],
        }
    }

    pub fn with_max_size(mut self, size: usize) -> Self {
        self.max_size = size;
        self
    }

    pub fn with_allowed_types(mut self, types: Vec<String>) -> Self {
        self.allowed_types = types;
        self
    }

    pub fn validate_size(&self, size: usize) -> Result<(), UploadError> {
        if size > self.max_size {
            return Err(UploadError::FileTooLarge(size, self.max_size));
        }
        Ok(())
    }

    pub fn validate_type(&self, content_type: &str) -> Result<(), UploadError> {
        if !self.allowed_types.contains(&content_type.to_string()) {
            return Err(UploadError::InvalidFileType(content_type.to_string()));
        }
        Ok(())
    }

    pub fn validate(&self, size: usize, content_type: &str) -> Result<(), UploadError> {
        self.validate_size(size)?;
        self.validate_type(content_type)?;
        Ok(())
    }
}

impl Default for FileValidator {
    fn default() -> Self {
        Self::new()
    }
}
