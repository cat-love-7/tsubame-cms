//! Images: the record is in the table, the bytes are in S3.
//!
//! Uploading is a two-step dance the browser performs directly against S3: the CMS writes the
//! record, allocates the id and hands back a **presigned PUT**, so image bytes never travel
//! through the API. The URL stored in content is the **stable** one (the bucket, or a CDN in
//! front of it) — a presigned URL written into a page would expire with the link.

use std::time::Duration;

use aws_sdk_s3::presigning::PresigningConfig;

use super::*;
use crate::ImageDelivery;
use sl_cms_core::models::image::{
    sanitize_ext, Image, ImageID, NewImageInfo, NewImageRequest, ReplacementInfo,
    ImageOwner,
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
    /// Absent in records written before the trash existed, which is why it has a default.
    #[serde(default)]
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl AwsRepository {
    /// A stored record as the model the API returns, with its URL filled in.
    async fn image_from(inner: &Inner, data: &ImageData) -> Result<Image, BoxError> {
        Ok(Image {
            original_filename: data.original_filename.clone(),
            url: serve_url(inner, &data.file_name).await?,
            uploaded_at: data.uploaded_at,
            deleted_at: data.deleted_at,
        })
    }
}

/// The URL an image's bytes are served from, for this deployment.
///
/// The object's own address when the bucket is readable (or a CDN in front of it), and a signature
/// over the object when it is not. The mode is the deployment's ([`ImageDelivery`]): the same CMS
/// serves both, and a reader is told which one it got by looking at the URL.
async fn serve_url(inner: &Inner, file_name: &str) -> Result<String, BoxError> {
    match inner.settings.image_delivery {
        ImageDelivery::Stable => Ok(inner.settings.image_url(file_name)),
        ImageDelivery::Presigned { ttl } => {
            let config = PresigningConfig::expires_in(ttl)
                .map_err(|e| format!("could not build the image URL: {e}"))?;
            let request = inner
                .s3
                .get_object()
                .bucket(&inner.settings.bucket)
                .key(file_name)
                .presigned(config)
                .await
                .map_err(|e| format!("could not sign the image URL: {}", describe(&e)))?;
            Ok(request.uri().to_string())
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
                Ok(Some(AwsRepository::image_from(&inner, &data).await?))
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
            images.push((
                ImageID::from_u64(id),
                AwsRepository::image_from(&inner, &data).await?,
            ));
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
            // A fresh upload is in the library, not the trash.
            deleted_at: None,
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
            // What the upload will be readable from, which an editor shows straight away: in the
            // signed mode that is a fresh signature, and it is replaced by a new one whenever the
            // record is read again.
            url: serve_url(&inner, &file_name).await?,
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

    /// Whether the bytes an upload was signed for are actually in the bucket.
    ///
    /// HeadObject needs `s3:GetObject`, and **S3 answers 403 rather than 404 for a key that is not
    /// there unless the caller may also list the bucket** - it will not reveal whether an object
    /// exists otherwise. Both permissions are therefore in the deployment's policy
    /// (`infra/lambda.tf`): without `s3:ListBucket` an upload that never arrived looks like a
    /// failure of the CMS instead of the "not there yet" this reads a 404 as.
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
            // Replacing the bytes does not take an image out of the trash, or put it back in.
            deleted_at: previous.deleted_at,
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

    async fn image_named(&self, file_name: &str) -> Result<Option<ImageID>, BoxError> {
        let inner = self.inner.clone();
        for (sk, data) in list(&inner, key::IMAGE_INDEX, "image#").await? {
            let Ok(id) = sk.trim_start_matches("image#").parse::<u64>() else {
                continue;
            };
            let data: ImageData = AwsRepository::decode(&data)?;
            if data.file_name == file_name {
                return Ok(Some(ImageID::from_u64(id)));
            }
        }
        Ok(None)
    }

    async fn set_image_references(
        &self,
        owner: &ImageOwner,
        images: &[ImageID],
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        // Both directions in the one table: the owner's partition answers "what does this content
        // use", and the image's answers "what uses this image". A save has to know both, because
        // the entries that are gone have to be removed from each.
        let owner_pk = format!("refs#{}", owner.storage_key());
        let before: std::collections::BTreeSet<u64> = list(&inner, &owner_pk, "image#")
            .await?
            .into_iter()
            .filter_map(|(sk, _)| sk.trim_start_matches("image#").parse::<u64>().ok())
            .collect();
        let after: std::collections::BTreeSet<u64> = images.iter().map(|id| **id).collect();

        for id in before.difference(&after) {
            let image_pk = format!("image#{}", id);
            remove(&inner, &owner_pk, &format!("image#{}", id)).await?;
            remove(&inner, &image_pk, &format!("ref#{}", owner.storage_key())).await?;
        }
        for id in after.difference(&before) {
            let image_pk = format!("image#{}", id);
            write(&inner, &owner_pk, &format!("image#{}", id), &owner.storage_key()).await?;
            write(&inner, &image_pk, &format!("ref#{}", owner.storage_key()), &owner.storage_key())
                .await?;
        }
        Ok(())
    }

    async fn get_image_references(&self, id: &ImageID) -> Result<Vec<ImageOwner>, BoxError> {
        let inner = self.inner.clone();
        let image_pk = format!("image#{}", id);
        let mut owners: Vec<ImageOwner> = list(&inner, &image_pk, "ref#")
            .await?
            .into_iter()
            .filter_map(|(sk, _)| ImageOwner::from_storage_key(sk.trim_start_matches("ref#")))
            .collect();
        owners.sort();
        Ok(owners)
    }

    async fn set_image_deleted_at(
        &self,
        id: &ImageID,
        at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let raw = **id;
        let mut data = match read(&inner, key::IMAGE_INDEX, &key::image(raw)).await? {
            Some(data) => AwsRepository::decode::<ImageData>(&data)?,
            None => return Err("Image not found".into()),
        };
        data.deleted_at = at;
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
    /// The other way of serving images: a bucket nobody may read, and a URL that is a signature.
    ///
    /// What a site that fetches its images during a build wants - it reads them while the signature
    /// is fresh, transforms them, and publishes its own copy - and what the deployment chooses with
    /// `AWS_IMAGE_DELIVERY=presigned`. The test is the whole promise of that mode: the URL the API
    /// hands out works, and the same object without a signature does not.
    #[tokio::test]
    async fn a_presigned_deployment_serves_a_url_that_expires() {
        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository_serving(
            "cms_presigned",
            crate::ImageDelivery::Presigned {
                ttl: std::time::Duration::from_secs(300),
            },
        )
        .await
        .expect("a repository");
        let bucket = repository.inner.settings.bucket.clone();
        // Private: no bucket policy at all, which is what the mode is for.
        repository
            .inner
            .s3
            .create_bucket()
            .bucket(&bucket)
            .send()
            .await
            .unwrap_or_else(|e| panic!("could not create {bucket}: {}", describe(&e)));

        let request = NewImageRequest {
            original_filename: "cat.png".to_string(),
            ext: "png".to_string(),
        };
        let info = repository.generate_image_upload_url(&request).await.unwrap();
        sl_cms_core::webhook::install_crypto_provider();
        let client = reqwest::Client::new();
        assert_eq!(
            client
                .put(&info.upload_url)
                .body(vec![1u8, 2, 3])
                .send()
                .await
                .expect("the presigned PUT should be reachable")
                .status(),
            200
        );

        let image = repository.get_image(&info.id).await.unwrap().expect("the record");
        assert!(
            image.url.contains("X-Amz-Signature"),
            "the URL should be a signature: {}",
            image.url
        );
        let signed = client
            .get(&image.url)
            .send()
            .await
            .expect("the signed URL should be reachable");
        assert_eq!(signed.status(), 200, "the signature was refused");
        assert_eq!(signed.bytes().await.unwrap().as_ref(), &[1u8, 2, 3]);

        // The same object without the signature is not readable, which is the point. The key has to
        // be taken from the URL with the query stripped: keeping it would keep the signature too,
        // and the read would be refused for a reason that has nothing to do with the bucket being
        // private.
        let key = info
            .url
            .split('?')
            .next()
            .unwrap_or(&info.url)
            .rsplit('/')
            .next()
            .unwrap();
        let bare = format!("{endpoint}/{bucket}/{key}");
        let refused = client.get(&bare).send().await.expect("the bucket answers");
        // Not "not found": the object is there, and the reader is not allowed to have it. The
        // emulator answers 400 for a request with no authorization at all where S3 answers 403
        // AccessDenied - the refusal is the same, the status is not, so what this pins is that the
        // read does not succeed (a bucket that had become readable would answer 200).
        assert!(
            !refused.status().is_success(),
            "a private bucket should refuse an unsigned read, got {}",
            refused.status()
        );
        assert_ne!(refused.status().as_u16(), 404, "the object is there, the reader is not");

        // And the same mode answers "whose file is this?" from the records: a URL that ends in a
        // signature would never match a file name, so a check written against the URL would let
        // one image take another's bytes here without a word.
        assert_eq!(
            repository.image_named(key).await.unwrap(),
            Some(info.id.clone()),
        );
        assert_eq!(
            repository.image_named("nobody's-file.png").await.unwrap(),
            None
        );
    }

    /// What a deployment's S3 permissions actually allow, against the emulator.
    ///
    /// The emulator's root credentials can do anything, which is how a missing `s3:ListBucket`
    /// stayed invisible: with root, a key that is not there answers 404 and the code's "not there
    /// yet" branch is reached. As a user with the deployment's policy (`infra/lambda.tf`, mirrored
    /// by `deployment_s3_policy`), a key that is there is found, and one that is not is answered
    /// *as absent* - which is what `s3:ListBucket` buys. Without it the emulator refuses to say,
    /// and the CMS cannot tell "missing" from "not allowed": the failure the policy keeps out of
    /// reach.
    #[tokio::test]
    async fn finding_an_upload_needs_the_permissions_the_deployment_grants() {
        use sl_cms_core::repositories::image_repository::ImageRepository;

        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (root, _table) = crate::open_test_repository("cms_permissions")
            .await
            .expect("a table for this test");
        let bucket = root.inner.settings.bucket.clone();
        root.inner
            .s3
            .create_bucket()
            .bucket(&bucket)
            .send()
            .await
            .unwrap_or_else(|e| panic!("could not create {bucket}: {}", describe(&e)));

        // The deployment's own policy: what is there is found, and what is not is absent.
        let Some((user, secret)) = crate::restricted_minio_user(&bucket, true) else {
            eprintln!("skipped: no emulator container to make a restricted user in");
            return;
        };
        let (repository, _table) = crate::open_test_repository_as(
            "cms_permissions_allowed",
            &bucket,
            &user,
            &secret,
        )
        .await
        .expect("a repository as the restricted user");

        // An upload arrives the way the browser sends it: through the presigned PUT, which is
        // signed with these very credentials.
        let request = NewImageRequest {
            original_filename: "there.png".to_string(),
            ext: "png".to_string(),
        };
        let info = repository
            .generate_image_upload_url(&request)
            .await
            .expect("a signed upload");
        sl_cms_core::webhook::install_crypto_provider();
        let put = reqwest::Client::new()
            .put(&info.upload_url)
            .body(vec![1u8, 2, 3])
            .send()
            .await
            .expect("the presigned PUT should be reachable");
        assert_eq!(put.status(), 200, "the restricted user may not upload");

        // The record names the file the bytes went to.
        let stored = repository
            .get_image(&info.id)
            .await
            .unwrap()
            .expect("the record");
        let file_name = stored
            .url
            .split('?')
            .next()
            .unwrap_or(&stored.url)
            .rsplit('/')
            .next()
            .unwrap_or_default();
        assert!(
            repository.image_bytes_exist(file_name).await.unwrap(),
            "an upload that is there has to be found"
        );
        assert_eq!(
            repository.image_bytes_exist("nothing-here.png").await.unwrap(),
            false,
            "with s3:ListBucket, a key that is not there answers 404 - which is what the code reads"
        );

        // And without that permission, the same question has no answer: this is the shape the
        // policy exists to avoid.
        let Some((stranger, secret)) = crate::restricted_minio_user(&bucket, false) else {
            eprintln!("skipped: no emulator container to make a restricted user in");
            return;
        };
        let (repository, _table) =
            crate::open_test_repository_as("cms_permissions_denied", &bucket, &stranger, &secret)
                .await
                .expect("a repository as the second restricted user");
        assert!(
            repository.image_bytes_exist("nothing-here.png").await.is_err(),
            "without s3:ListBucket the emulator refuses to say whether the key exists"
        );
    }

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
