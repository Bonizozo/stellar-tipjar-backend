use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub quality: Option<u8>,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformResult {
    pub original_url: String,
    pub transformed_url: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
}

pub async fn transform_image(
    url: &str,
    options: TransformOptions,
) -> anyhow::Result<TransformResult> {
    let width = options.width.unwrap_or(800);
    let height = options.height.unwrap_or(600);
    let quality = options.quality.unwrap_or(85);
    let format = options.format.unwrap_or_else(|| "webp".to_string());

    let transformed_url = format!(
        "{}?w={}&h={}&q={}&fmt={}",
        url, width, height, quality, format
    );

    Ok(TransformResult {
        original_url: url.to_string(),
        transformed_url,
        width,
        height,
        size: 0,
    })
}

pub async fn apply_cache_headers(url: &str, max_age: u32) -> String {
    format!("{};cache-control=public,max-age={}", url, max_age)
}
