use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadRequest {
    pub file_name: String,
    pub content_type: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResponse {
    pub id: Uuid,
    pub url: String,
    pub cdn_url: String,
    pub size: u64,
    pub content_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub id: Uuid,
    pub path: PathBuf,
    pub size: u64,
    pub content_type: String,
    pub cache_key: String,
}

pub async fn handle_upload(
    file_name: String,
    content_type: String,
    data: Vec<u8>,
) -> anyhow::Result<UploadResponse> {
    let id = Uuid::new_v4();
    let size = data.len() as u64;
    let cdn_url = format!("https://cdn.example.com/{}/{}", id, file_name);
    let url = format!("/uploads/{}/{}", id, file_name);

    Ok(UploadResponse {
        id,
        url,
        cdn_url,
        size,
        content_type,
        created_at: chrono::Utc::now(),
    })
}
