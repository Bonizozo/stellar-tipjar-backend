use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
use std::io::Cursor;
use crate::upload::validator::UploadError;

pub struct ImageProcessor;

impl ImageProcessor {
    pub fn process(
        data: &[u8],
        max_width: u32,
        max_height: u32,
    ) -> Result<Vec<u8>, UploadError> {
        let img = image::load_from_memory(data)
            .map_err(|e| UploadError::ProcessingError(format!("Failed to load image: {}", e)))?;

        let processed = Self::resize_and_optimize(img, max_width, max_height)?;
        
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        processed
            .write_to(&mut cursor, ImageFormat::Jpeg)
            .map_err(|e| UploadError::ProcessingError(format!("Failed to encode image: {}", e)))?;

        Ok(buffer)
    }

    fn resize_and_optimize(
        img: DynamicImage,
        max_width: u32,
        max_height: u32,
    ) -> Result<DynamicImage, UploadError> {
        let (width, height) = img.dimensions();
        
        if width <= max_width && height <= max_height {
            return Ok(img);
        }

        let ratio = (max_width as f32 / width as f32).min(max_height as f32 / height as f32);
        let new_width = (width as f32 * ratio) as u32;
        let new_height = (height as f32 * ratio) as u32;

        Ok(img.resize(new_width, new_height, FilterType::Lanczos3))
    }

    pub fn create_thumbnail(data: &[u8], size: u32) -> Result<Vec<u8>, UploadError> {
        Self::process(data, size, size)
    }

    pub fn create_variants(data: &[u8]) -> Result<ImageVariants, UploadError> {
        Ok(ImageVariants {
            original: data.to_vec(),
            large: Self::process(data, 1920, 1080)?,
            medium: Self::process(data, 800, 600)?,
            thumbnail: Self::create_thumbnail(data, 200)?,
        })
    }
}

pub struct ImageVariants {
    pub original: Vec<u8>,
    pub large: Vec<u8>,
    pub medium: Vec<u8>,
    pub thumbnail: Vec<u8>,
}
