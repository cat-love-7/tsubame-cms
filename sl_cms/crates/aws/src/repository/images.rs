//! Images: the record is in the table, the bytes are in S3.
//!
//! Uploading is a two-step dance the browser performs directly against S3: the CMS writes the
//! record, allocates the id and hands back a **presigned PUT**, so image bytes never travel
//! through the API. The URL stored in content is the **stable** one (the bucket, or a CDN in
//! front of it) — a presigned URL written into a page would expire with the link.

use std::time::Duration;

use aws_sdk_s3::presigning::PresigningConfig;

use super::*;
use sl_cms_core::models::image::{
    sanitize_ext, Image, ImageID, NewImageInfo, NewImageRequest, ReplacementInfo,
};
use sl_cms_core::repositories::image_repository::ImageRepository;

/// How long an upload URL is good for. Long enough for a slow phone on a train, short enough
/// that a leaked URL is not a standing invitation.
const UPLOAD_URL_TTL: Duration = Duration::from_secs(15 * 60);

/// What the table remembers about an uploaded image. The bytes are in S3 under `file_name`,
/// which is also the last segment of the URL.
#[derive(serde::Serialize, serde::Deserialize)]
struct ImageData {
    original_filename: String,
    file_name: String,
    uploaded_at: chrono::DateTime<chrono::Utc>,
}

impl AwsRepository {
    /// A stored record as the model the API returns, with the stable URL filled in.
    fn image_from(inner: &Inner, data: &ImageData) -> Image {
        Image {
            original_filename: data.original_filename.clone(),
            url: inner.settings.image_url(&data.file_name),
            uploaded_at: data.uploaded_at,
        }
    }
}

impl ImageRepository for AwsRepository {
    async fn get_image(&self, id: &ImageID) -> Result<Option<Image>, BoxError> {
        let inner = self.inner.clone();
        let id = **id;
        match read(&inner, key::IMAGE_INDEX, &key::image(id)).await? {
            Some(data) => {
                let data: ImageData = AwsRepository::decode(&data)?;
                Ok(Some(AwsRepository::image_from(&inner, &data)))
            }
            None => Ok(None),
        }
    }

    async fn get_all_images(&self) -> Result<Vec<(ImageID, Image)>, BoxError> {
        let inner = self.inner.clone();
        let mut images = Vec::new();
        for (sk, data) in list(&inner, key::IMAGE_INDEX, "image#").await? {
            // The id is in the sort key, so a malformed record is skipped rather than
            // guessed at or panicked over.
            let Ok(id) = sk.trim_start_matches("image#").parse::<u64>() else {
                continue;
            };
            let data: ImageData = AwsRepository::decode(&data)?;
            images.push((ImageID::from_u64(id), AwsRepository::image_from(&inner, &data)));
        }
        Ok(images)
    }

    async fn generate_image_upload_url(&self, upload_info: &NewImageRequest) -> Result<NewImageInfo, BoxError> {
        let inner = self.inner.clone();
        let upload_info = NewImageRequest {
            original_filename: upload_info.original_filename.clone(),
            ext: upload_info.ext.clone(),
        };
        // The id comes from the same atomic counter the collections use, so two uploads at
        // once cannot write the same record.
        let id = next_id(&inner, key::IMAGE_INDEX).await?;
        let file_name = match sanitize_ext(&upload_info.ext) {
            Some(ext) => format!("{}.{}", uuid::Uuid::new_v4(), ext),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let data = ImageData {
            original_filename: upload_info.original_filename.clone(),
            file_name: file_name.clone(),
            uploaded_at: chrono::Utc::now(),
        };
        write(
            &inner,
            key::IMAGE_INDEX,
            &key::image(id),
            &AwsRepository::encode(&data)?,
        )
        .await?;

        let config = PresigningConfig::expires_in(UPLOAD_URL_TTL)
            .map_err(|e| format!("could not build the upload URL: {e}"))?;
        let request = inner
            .s3
            .put_object()
            .bucket(&inner.settings.bucket)
            .key(&file_name)
            .presigned(config)
            .await
            .map_err(|e| format!("could not sign the upload URL: {}", describe(&e)))?;

        Ok(NewImageInfo {
            id: ImageID::from_u64(id),
            upload_url: request.uri().to_string(),
            url: inner.settings.image_url(&file_name),
        })
    }

    async fn generate_replacement_upload_url(
        &self,
        _id: &ImageID,
        ext: &str,
    ) -> Result<ReplacementInfo, BoxError> {
        let inner = self.inner.clone();
        // A fresh object key rather than an overwrite of the existing one: a browser or a CDN
        // caches by URL, so bytes that change under one key keep showing the old image.
        let file_name = match sanitize_ext(ext) {
            Some(ext) => format!("{}.{}", uuid::Uuid::new_v4(), ext),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let config = PresigningConfig::expires_in(UPLOAD_URL_TTL)
            .map_err(|e| format!("could not build the upload URL: {e}"))?;
        let request = inner
            .s3
            .put_object()
            .bucket(&inner.settings.bucket)
            .key(&file_name)
            .presigned(config)
            .await
            .map_err(|e| format!("could not sign the upload URL: {}", describe(&e)))?;

        Ok(ReplacementInfo {
            upload_url: request.uri().to_string(),
            file_name,
        })
    }

    async fn image_bytes_exist(&self, file_name: &str) -> Result<bool, BoxError> {
        let inner = self.inner.clone();
        match inner
            .s3
            .head_object()
            .bucket(&inner.settings.bucket)
            .key(file_name)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            // The object is not there: that is the answer, not a failure.
            Err(e) if e.as_service_error().is_some_and(|e| e.is_not_found()) => Ok(false),
            Err(e) => Err(format!("could not look for the uploaded image: {}", describe(&e)).into()),
        }
    }

    async fn replace_image(&self, id: &ImageID, file_name: &str) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let raw = **id;
        let previous = match read(&inner, key::IMAGE_INDEX, &key::image(raw)).await? {
            Some(data) => AwsRepository::decode::<ImageData>(&data)?,
            None => return Err("Image not found".into()),
        };

        let updated = ImageData {
            original_filename: previous.original_filename.clone(),
            file_name: file_name.to_string(),
            // The first upload is when the image entered the library; replacing what it shows
            // does not change that, just as re-publishing does not change `published_at`.
            uploaded_at: previous.uploaded_at,
        };
        write(
            &inner,
            key::IMAGE_INDEX,
            &key::image(raw),
            &AwsRepository::encode(&updated)?,
        )
        .await?;

        // What it used to name is now unreferenced, so it goes. A missing object is not an
        // error: the previous upload may never have completed.
        if previous.file_name != file_name {
            inner
                .s3
                .delete_object()
                .bucket(&inner.settings.bucket)
                .key(&previous.file_name)
                .send()
                .await
                .map_err(|e| format!("could not delete the replaced image: {}", describe(&e)))?;
        }
        Ok(())
    }

    async fn rename_image(&self, id: &ImageID, original_filename: &str) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let raw = **id;
        let mut data = match read(&inner, key::IMAGE_INDEX, &key::image(raw)).await? {
            Some(data) => AwsRepository::decode::<ImageData>(&data)?,
            None => return Err("Image not found".into()),
        };
        data.original_filename = original_filename.to_string();
        write(
            &inner,
            key::IMAGE_INDEX,
            &key::image(raw),
            &AwsRepository::encode(&data)?,
        )
        .await
    }

    async fn delete_image(&self, id: &ImageID) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let raw = **id;
        let data = match read(&inner, key::IMAGE_INDEX, &key::image(raw)).await? {
            Some(data) => AwsRepository::decode::<ImageData>(&data)?,
            // The on-premises adapter reports a missing image as an error, and the service
            // turns that into a 404; match it so both backends answer the same way.
            None => return Err("Image not found".into()),
        };

        // The bytes first: a record without bytes is a broken thumbnail, while bytes
        // without a record are merely invisible (and cheap).
        inner
            .s3
            .delete_object()
            .bucket(&inner.settings.bucket)
            .key(&data.file_name)
            .send()
            .await
            .map_err(|e| format!("could not delete the image bytes: {}", describe(&e)))?;
        remove(&inner, key::IMAGE_INDEX, &key::image(raw)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Push bytes through the URL the CMS handed out, the way a browser does, and read them
    /// back through the URL that ends up in the content.
    #[tokio::test]
    async fn an_upload_url_puts_bytes_that_the_stable_url_then_serves() {
        let endpoint = crate::test_s3_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no S3 at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository("cms_images").await.expect("a table for this test");
        let bucket = repository.inner.settings.bucket.clone();
        let s3 = repository.inner.s3.clone();
        // The deployment makes the image bucket readable — the URL written into content is
        // public and unsigned — while the emulator starts private.
        s3.create_bucket()
            .bucket(&bucket)
            .send()
            .await
            .unwrap_or_else(|e| panic!("could not create {bucket}: {}", describe(&e)));
        s3.put_bucket_policy()
            .bucket(&bucket)
            .policy(
                serde_json::json!({
                    "Version": "2012-10-17",
                    "Statement": [{
                        "Effect": "Allow",
                        "Principal": { "AWS": ["*"] },
                        "Action": ["s3:GetObject"],
                        "Resource": [format!("arn:aws:s3:::{bucket}/*")],
                    }],
                })
                .to_string(),
            )
            .send()
            .await
            .unwrap_or_else(|e| panic!("could not open {bucket} for reading: {}", describe(&e)));

        let request = NewImageRequest {
            original_filename: "cat.png".to_string(),
            ext: "PNG".to_string(),
        };
        let info = repository.generate_image_upload_url(&request).await.unwrap();
        assert!(info.upload_url.starts_with("http"), "not a URL: {}", info.upload_url);

        // The browser PUTs the bytes straight to S3: image bytes never travel through the API.
        // The presigned URL is exercised the way a browser would, over plain HTTP(S), so the
        // process needs a TLS provider the same way the webhook client does.
        sl_cms_core::webhook::install_crypto_provider();
        let client = reqwest::Client::new();
        let put = client
            .put(&info.upload_url)
            .body(vec![1u8, 2, 3])
            .send()
            .await
            .expect("the presigned PUT should be reachable");
        if put.status() != 200 {
            let status = put.status();
            let body = put.text().await.unwrap_or_default();
            panic!("the signature was refused: {status} {body}");
        }

        // What is stored in content is the stable URL, not the signature: a presigned URL in a
        // page would expire with the link.
        let image = repository.get_image(&info.id).await.unwrap().expect("the record");
        assert_eq!(image.original_filename, "cat.png");
        assert!(
            image.url.starts_with(&format!("{endpoint}/{bucket}/")),
            "unexpected url: {}",
            image.url
        );
        assert!(image.url.ends_with(".png"), "the extension is kept: {}", image.url);
        assert!(!image.url.contains("X-Amz-Signature"), "a signature leaked into content");

        let served = client.get(&image.url).send().await.expect("the object should be readable");
        assert_eq!(served.status(), 200);
        assert_eq!(served.bytes().await.unwrap().as_ref(), &[1u8, 2, 3]);

        // Listing, then deleting: both the record and the bytes go.
        let all = repository.get_all_images().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].0, info.id);
        assert_eq!(all[0].1.url, image.url);

        repository.delete_image(&info.id).await.unwrap();
        assert!(repository.get_image(&info.id).await.unwrap().is_none());
        let gone = client.get(&image.url).send().await.unwrap();
        assert_eq!(gone.status(), 404, "the bytes are still there");
        // Deleting again reports what the on-premises adapter reports, so the service can turn
        // it into the same 404.
        assert!(repository.delete_image(&info.id).await.is_err());

        repository.delete_table().await.unwrap();
    }

    /// Replacing an image's bytes under the same id: the object moves, the record keeps its
    /// identity, and the bytes it used to serve are gone.
    #[tokio::test]
    async fn a_replacement_puts_new_bytes_under_the_same_id() {
        let endpoint = crate::test_s3_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no S3 at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository("cms_replace").await.expect("a table for this test");
        let bucket = repository.inner.settings.bucket.clone();
        let s3 = repository.inner.s3.clone();
        s3.create_bucket().bucket(&bucket).send().await.unwrap_or_else(|e| {
            panic!("could not create {bucket}: {}", describe(&e))
        });
        s3.put_bucket_policy()
            .bucket(&bucket)
            .policy(
                serde_json::json!({
                    "Version": "2012-10-17",
                    "Statement": [{
                        "Effect": "Allow",
                        "Principal": { "AWS": ["*"] },
                        "Action": ["s3:GetObject"],
                        "Resource": [format!("arn:aws:s3:::{bucket}/*")],
                    }],
                })
                .to_string(),
            )
            .send()
            .await
            .unwrap_or_else(|e| panic!("could not open {bucket} for reading: {}", describe(&e)));

        sl_cms_core::webhook::install_crypto_provider();
        let client = reqwest::Client::new();

        let info = repository
            .generate_image_upload_url(&NewImageRequest {
                original_filename: "logo.png".to_string(),
                ext: "png".to_string(),
            })
            .await
            .unwrap();
        client
            .put(&info.upload_url)
            .body(vec![1u8, 2, 3])
            .send()
            .await
            .expect("the presigned PUT should be reachable");
        let before = repository.get_image(&info.id).await.unwrap().expect("the record");

        // Where to put the replacement, and what it will be called.
        let replacement = repository
            .generate_replacement_upload_url(&info.id, "PNG")
            .await
            .unwrap();
        assert!(replacement.file_name.ends_with(".png"), "{}", replacement.file_name);
        assert_ne!(replacement.file_name, "");
        // The record is untouched until the bytes are there.
        assert_eq!(
            repository.get_image(&info.id).await.unwrap().unwrap().url,
            before.url
        );

        client
            .put(&replacement.upload_url)
            .body(vec![4u8, 5, 6])
            .send()
            .await
            .expect("the presigned PUT should be reachable");
        assert!(
            repository.image_bytes_exist(&replacement.file_name).await.unwrap(),
            "the uploaded bytes should be found before the swap"
        );

        repository
            .replace_image(&info.id, &replacement.file_name)
            .await
            .unwrap();

        let after = repository.get_image(&info.id).await.unwrap().expect("the record");
        assert_eq!(after.original_filename, "logo.png", "the name is not part of the bytes");
        assert_eq!(after.uploaded_at, before.uploaded_at, "neither is when it arrived");
        assert_ne!(after.url, before.url, "new bytes, new URL");
        assert_eq!(
            client.get(&after.url).send().await.unwrap().bytes().await.unwrap().as_ref(),
            &[4u8, 5, 6]
        );
        assert_eq!(
            client.get(&before.url).send().await.unwrap().status(),
            404,
            "the replaced bytes should be gone, not left to be served"
        );

        // Asking about bytes that were never uploaded answers honestly.
        assert!(!repository.image_bytes_exist("nothing-here.png").await.unwrap());

        repository.delete_table().await.unwrap();
    }
}
