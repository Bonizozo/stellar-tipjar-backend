use super::upload::{handle_upload, UploadResponse};
use super::transform::{apply_cache_headers, transform_image, TransformOptions, TransformResult};
use std::sync::Arc;

pub struct CdnService {
    cdn_endpoint: String,
    cache_ttl: u32,
}

impl CdnService {
    pub fn new(cdn_endpoint: String, cache_ttl: u32) -> Self {
        Self {
            cdn_endpoint,
            cache_ttl,
        }
    }

    pub async fn upload_file(
        &self,
        file_name: String,
        content_type: String,
        data: Vec<u8>,
    ) -> anyhow::Result<UploadResponse> {
        handle_upload(file_name, content_type, data).await
    }

    pub async fn transform_and_cache(
        &self,
        url: &str,
        options: TransformOptions,
    ) -> anyhow::Result<TransformResult> {
        let result = transform_image(url, options).await?;
        let cached_url = apply_cache_headers(&result.transformed_url, self.cache_ttl).await;
        Ok(TransformResult {
            transformed_url: cached_url,
            ..result
        })
    }

    pub fn get_cdn_url(&self, file_id: &str) -> String {
        format!("{}/{}", self.cdn_endpoint, file_id)
    }

    pub fn invalidate_cache(&self, file_id: &str) -> anyhow::Result<()> {
        tracing::info!("Invalidating cache for file: {}", file_id);
        Ok(())
    }
}
