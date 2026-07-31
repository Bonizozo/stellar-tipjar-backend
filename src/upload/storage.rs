use crate::upload::validator::UploadError;
use aws_config::BehaviorVersion;
use aws_sdk_s3::{primitives::ByteStream, Client};
use uuid::Uuid;

pub struct S3Storage {
    client: Client,
    bucket: String,
    cdn_url: Option<String>,
}

impl S3Storage {
    pub async fn new() -> Result<Self, UploadError> {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let client = Client::new(&config);

        let bucket = std::env::var("S3_BUCKET")
            .map_err(|_| UploadError::StorageError("S3_BUCKET not configured".to_string()))?;

        let cdn_url = std::env::var("CDN_URL").ok();

        Ok(Self {
            client,
            bucket,
            cdn_url,
        })
    }

    pub async fn upload(
        &self,
        data: Vec<u8>,
        content_type: &str,
        folder: &str,
    ) -> Result<String, UploadError> {
        let file_id = Uuid::new_v4();
        let extension = Self::get_extension(content_type);
        let key = format!("{}/{}.{}", folder, file_id, extension);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(data))
            .content_type(content_type)
            .acl(aws_sdk_s3::types::ObjectCannedAcl::PublicRead)
            .send()
            .await
            .map_err(|e| UploadError::StorageError(format!("S3 upload failed: {}", e)))?;

        Ok(self.get_public_url(&key))
    }

    pub async fn delete(&self, key: &str) -> Result<(), UploadError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| UploadError::StorageError(format!("S3 delete failed: {}", e)))?;

        Ok(())
    }

    fn get_public_url(&self, key: &str) -> String {
        if let Some(cdn) = &self.cdn_url {
            format!("{}/{}", cdn, key)
        } else {
            format!("https://{}.s3.amazonaws.com/{}", self.bucket, key)
        }
    }

    fn get_extension(content_type: &str) -> &str {
        match content_type {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/webp" => "webp",
            "image/gif" => "gif",
            _ => "bin",
        }
    }

    pub async fn upload_variants(
        &self,
        original: Vec<u8>,
        large: Vec<u8>,
        medium: Vec<u8>,
        thumbnail: Vec<u8>,
        content_type: &str,
        folder: &str,
    ) -> Result<UploadedVariants, UploadError> {
        let base_id = Uuid::new_v4();
        let extension = Self::get_extension(content_type);

        let original_url = self
            .upload_with_name(
                original,
                content_type,
                folder,
                &format!("{}", base_id),
                extension,
            )
            .await?;
        let large_url = self
            .upload_with_name(
                large,
                content_type,
                folder,
                &format!("{}_large", base_id),
                extension,
            )
            .await?;
        let medium_url = self
            .upload_with_name(
                medium,
                content_type,
                folder,
                &format!("{}_medium", base_id),
                extension,
            )
            .await?;
        let thumbnail_url = self
            .upload_with_name(
                thumbnail,
                content_type,
                folder,
                &format!("{}_thumb", base_id),
                extension,
            )
            .await?;

        Ok(UploadedVariants {
            original: original_url,
            large: large_url,
            medium: medium_url,
            thumbnail: thumbnail_url,
        })
    }

    async fn upload_with_name(
        &self,
        data: Vec<u8>,
        content_type: &str,
        folder: &str,
        name: &str,
        extension: &str,
    ) -> Result<String, UploadError> {
        let key = format!("{}/{}.{}", folder, name, extension);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(ByteStream::from(data))
            .content_type(content_type)
            .acl(aws_sdk_s3::types::ObjectCannedAcl::PublicRead)
            .send()
            .await
            .map_err(|e| UploadError::StorageError(format!("S3 upload failed: {}", e)))?;

        Ok(self.get_public_url(&key))
    }
}

#[derive(Debug, Clone)]
pub struct UploadedVariants {
    pub original: String,
    pub large: String,
    pub medium: String,
    pub thumbnail: String,
}
