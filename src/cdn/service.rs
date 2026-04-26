use super::upload::{handle_upload, UploadResponse};
use super::transform::{apply_cache_headers, transform_image, TransformOptions, TransformResult};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct CdnRegion {
    pub name: String,
    pub endpoint: String,
}

#[derive(Default)]
pub struct CdnMetrics {
    pub uploads: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub invalidations: AtomicU64,
}

impl CdnMetrics {
    pub fn snapshot(&self) -> HashMap<&'static str, u64> {
        HashMap::from([
            ("uploads", self.uploads.load(Ordering::Relaxed)),
            ("cache_hits", self.cache_hits.load(Ordering::Relaxed)),
            ("cache_misses", self.cache_misses.load(Ordering::Relaxed)),
            ("invalidations", self.invalidations.load(Ordering::Relaxed)),
        ])
    }
}

pub struct CdnService {
    regions: Vec<CdnRegion>,
    cache_ttl: u32,
    /// file_id -> set of CDN URLs that need invalidation tracking
    invalidation_log: Arc<RwLock<Vec<String>>>,
    pub metrics: Arc<CdnMetrics>,
}

impl CdnService {
    pub fn new(cdn_endpoint: String, cache_ttl: u32) -> Self {
        Self {
            regions: vec![CdnRegion {
                name: "default".to_string(),
                endpoint: cdn_endpoint,
            }],
            cache_ttl,
            invalidation_log: Arc::new(RwLock::new(Vec::new())),
            metrics: Arc::new(CdnMetrics::default()),
        }
    }

    pub fn with_regions(mut self, regions: Vec<CdnRegion>) -> Self {
        self.regions = regions;
        self
    }

    pub async fn upload_file(
        &self,
        file_name: String,
        content_type: String,
        data: Vec<u8>,
    ) -> anyhow::Result<UploadResponse> {
        let result = handle_upload(file_name, content_type, data).await?;
        self.metrics.uploads.fetch_add(1, Ordering::Relaxed);
        Ok(result)
    }

    pub async fn transform_and_cache(
        &self,
        url: &str,
        options: TransformOptions,
    ) -> anyhow::Result<TransformResult> {
        let result = transform_image(url, options).await?;
        let cached_url = apply_cache_headers(&result.transformed_url, self.cache_ttl).await;
        self.metrics.cache_misses.fetch_add(1, Ordering::Relaxed);
        Ok(TransformResult {
            transformed_url: cached_url,
            ..result
        })
    }

    /// Returns the CDN URL for a file on the primary (first) region.
    pub fn get_cdn_url(&self, file_id: &str) -> String {
        let endpoint = self
            .regions
            .first()
            .map(|r| r.endpoint.as_str())
            .unwrap_or("https://cdn.example.com");
        format!("{}/{}", endpoint, file_id)
    }

    /// Returns CDN URLs for a file across all configured regions.
    pub fn get_cdn_urls_all_regions(&self, file_id: &str) -> Vec<(String, String)> {
        self.regions
            .iter()
            .map(|r| (r.name.clone(), format!("{}/{}", r.endpoint, file_id)))
            .collect()
    }

    /// Invalidate cache for a file across all regions.
    pub async fn invalidate_cache(&self, file_id: &str) -> anyhow::Result<()> {
        for region in &self.regions {
            let url = format!("{}/{}", region.endpoint, file_id);
            tracing::info!(region = %region.name, url = %url, "Invalidating CDN cache");
            self.invalidation_log.write().await.push(url);
        }
        self.metrics.invalidations.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub async fn invalidation_log(&self) -> Vec<String> {
        self.invalidation_log.read().await.clone()
    }

    pub fn metrics_snapshot(&self) -> HashMap<&'static str, u64> {
        self.metrics.snapshot()
    }
}
