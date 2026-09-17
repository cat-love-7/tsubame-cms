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
    sanitize_ext, Image, ImageId, NewImageInfo, NewImageRequest, ReplacementInfo,
    ImageOwner,
};
use sl_cms_core::repositories::image_repository::{ImageRepository, Replacement};

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
    /// The upload this image is waiting for, if a replacement was requested and not applied.
    #[serde(default)]
    pending_replacement: Option<String>,
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

/// Read an image record, change it with `change`, and write it back only while it is still the
/// record that was read.
///
/// An image record is one blob, so writing one field of it back means writing all of it - and a
/// blob written from a read a replacement has since invalidated would restore the file name of
/// bytes that replacement deleted, leaving the image serving nothing. The write is therefore
/// conditional on the record not having changed, and a refusal is read again and applied to the
/// record as it now is.
async fn change_image(
    inner: &Inner,
    id: u64,
    change: impl Fn(&mut ImageData),
) -> Result<(), BoxError> {
    match change_record(inner, key::IMAGE_INDEX, &key::image(id), change).await? {
        Some(()) => Ok(()),
        None => Err("Image not found".into()),
    }
}

impl ImageRepository for AwsRepository {
    async fn get_image(&self, id: &ImageId) -> Result<Option<Image>, BoxError> {
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

    async fn get_all_images(&self) -> Result<Vec<(ImageId, Image)>, BoxError> {
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
                ImageId::from_u64(id),
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
            // A fresh upload is in the library, not the trash, and is waiting for nothing.
            deleted_at: None,
            pending_replacement: None,
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
            id: ImageId::from_u64(id),
            upload_url: request.uri().to_string(),
            // What the upload will be readable from, which an editor shows straight away: in the
            // signed mode that is a fresh signature, and it is replaced by a new one whenever the
            // record is read again.
            url: serve_url(&inner, &file_name).await?,
        })
    }

    async fn generate_replacement_upload_url(
        &self,
        id: &ImageId,
        ext: &str,
    ) -> Result<ReplacementInfo, BoxError> {
        let inner = self.inner.clone();
        // A fresh object key rather than an overwrite of the existing one: a browser or a CDN
        // caches by URL, so bytes that change under one key keep showing the old image.
        let file_name = match sanitize_ext(ext) {
            Some(ext) => format!("{}.{}", uuid::Uuid::new_v4(), ext),
            None => uuid::Uuid::new_v4().to_string(),
        };
        // The image has to be there before an upload is signed for it, so that a URL never names a
        // record that does not exist. Nothing is signed in vain either: the change below writes only
        // to a record that is still there, and an id that is gone is an error before the URL is
        // handed back.
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

        // Recorded on the image itself: the apply is answered by id, and this is what says which
        // upload is *this* image's. Only the newest request stays waiting.
        change_image(&inner, **id, |data| {
            data.pending_replacement = Some(file_name.clone())
        })
        .await?;
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

    async fn replace_image(&self, id: &ImageId, file_name: &str) -> Result<Replacement, BoxError> {
        let inner = self.inner.clone();
        let raw = **id;
        // Checked and consumed in one conditional write: the record is written back only while it
        // is still the one this attempt read, so an apply cannot take an upload a newer request has
        // replaced, nor forget one recorded after this attempt read the record. A refusal means the
        // record changed under it, which is read again and answered again.
        for _ in 0..RECORD_ATTEMPTS {
            let Some(stored) = read(&inner, key::IMAGE_INDEX, &key::image(raw)).await? else {
                return Err("Image not found".into());
            };
            let previous: ImageData = AwsRepository::decode(&stored)?;

            // Already showing it: nothing to write, and in particular nothing to forget - a
            // replacement waiting for some other upload has to stay waiting.
            if previous.file_name == file_name {
                return Ok(Replacement::Applied);
            }
            // Only the upload this image was signed is its to take.
            if previous.pending_replacement.as_deref() != Some(file_name) {
                return Ok(Replacement::NotWaiting);
            }

            let updated = ImageData {
                original_filename: previous.original_filename.clone(),
                file_name: file_name.to_string(),
                // The first upload is when the image entered the library; replacing what it shows
                // does not change that, just as re-publishing does not change `published_at`.
                uploaded_at: previous.uploaded_at,
                // Replacing the bytes does not take an image out of the trash, or put it back in.
                deleted_at: previous.deleted_at,
                // And the upload it came from is no longer waiting for anything.
                pending_replacement: None,
            };
            if !write_if_unchanged(
                &inner,
                key::IMAGE_INDEX,
                &key::image(raw),
                &stored,
                &AwsRepository::encode(&updated)?,
            )
            .await?
            {
                continue;
            }

            // What it used to name is now unreferenced, so it goes. A missing object is not an
            // error: the previous upload may never have completed.
            inner
                .s3
                .delete_object()
                .bucket(&inner.settings.bucket)
                .key(&previous.file_name)
                .send()
                .await
                .map_err(|e| format!("could not delete the replaced image: {}", describe(&e)))?;
            return Ok(Replacement::Applied);
        }
        Err(format!(
            "the record for image {raw} kept changing under a replacement after {RECORD_ATTEMPTS} attempts"
        )
        .into())
    }

    async fn rename_image(&self, id: &ImageId, original_filename: &str) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        change_image(&inner, **id, |data| {
            data.original_filename = original_filename.to_string()
        })
        .await
    }

    async fn image_file_name(&self, id: &ImageId) -> Result<Option<String>, BoxError> {
        let inner = self.inner.clone();
        let raw = **id;
        match read(&inner, key::IMAGE_INDEX, &key::image(raw)).await? {
            Some(data) => Ok(Some(AwsRepository::decode::<ImageData>(&data)?.file_name)),
            None => Ok(None),
        }
    }

    async fn set_image_references(
        &self,
        owner: &ImageOwner,
        images: &[ImageId],
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

    async fn get_image_references(&self, id: &ImageId) -> Result<Vec<ImageOwner>, BoxError> {
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
        id: &ImageId,
        at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        change_image(&inner, **id, |data| data.deleted_at = at).await
    }

    async fn delete_image(&self, id: &ImageId) -> Result<(), BoxError> {
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
        // Both halves are needed: the records are in DynamoDB and the bytes are in S3 (MinIO), and
        // the bare read below has to reach the *bucket* to be about the bucket at all.
        let dynamo = crate::test_endpoint();
        let s3 = crate::test_s3_endpoint();
        for (what, endpoint) in [("DynamoDB", &dynamo), ("S3", &s3)] {
            if !emulator_reachable(endpoint) {
                eprintln!("skipped: no {what} at {endpoint} (start it with `docker compose up -d`)");
                return;
            }
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
        // The lifetime the repository was built with (300s, above) is in the signature: this is the
        // part a deployment sets with `AWS_IMAGE_URL_TTL_SECONDS`, and the URL is only good for it.
        assert!(
            image.url.contains("X-Amz-Expires=300"),
            "the URL should carry the configured lifetime: {}",
            image.url
        );
        let signed = client
            .get(&image.url)
            .send()
            .await
            .expect("the signed URL should be reachable");
        assert_eq!(signed.status(), 200, "the signature was refused");
        assert_eq!(signed.bytes().await.unwrap().as_ref(), &[1u8, 2, 3]);

        // The same object without the signature is not readable, which is the point. The key comes
        // out of the signed URL, so the query is cut off **first**: keeping it would keep the
        // signature, and the read would then be refused for a reason that has nothing to do with
        // the bucket being private. The address is the S3 endpoint - a bare read aimed at the
        // table's endpoint would be answered by something that never looked at the bucket.
        let key = image
            .url
            .split('?')
            .next()
            .unwrap_or(&image.url)
            .rsplit('/')
            .next()
            .expect("the URL has an object key");
        assert!(
            !key.is_empty() && !key.contains("X-Amz"),
            "not an object key: {key}"
        );
        let bare = format!("{s3}/{bucket}/{key}");
        let refused = client.get(&bare).send().await.expect("the bucket answers");
        // Not "not found", and not a success either: the object is there, and a reader without a
        // signature may not have it. The emulator answers 403 AccessDenied for a request with no
        // authorization at all (S3 answers 403 as well), so what this pins is that the read is
        // refused by the bucket for want of a signature.
        assert_eq!(
            refused.status(),
            403,
            "a private bucket should refuse an unsigned read of an object that is there"
        );

        // And the same mode ties an upload to the image it is for by id, which is what an apply is
        // checked against: a URL that ends in a signature says nothing about whose bytes these are,
        // and a search for whichever record names a file says nothing about an upload no record
        // names yet.
        assert_eq!(
            repository.image_file_name(&info.id).await.unwrap().as_deref(),
            Some(key),
        );
        // Nothing has been signed yet, so the file it serves is not an upload it is waiting for -
        // and naming it anyway changes nothing.
        assert_eq!(
            repository.replace_image(&info.id, key).await.unwrap(),
            Replacement::Applied,
            "the file the image already serves is not a change",
        );
        assert_eq!(
            repository.image_file_name(&info.id).await.unwrap().as_deref(),
            Some(key),
        );

        // A replacement signs a file name and records it on the image, so the apply can be
        // answered by id; only the newest request stays waiting.
        let first = repository
            .generate_replacement_upload_url(&info.id, "png")
            .await
            .expect("a signed replacement");
        let second = repository
            .generate_replacement_upload_url(&info.id, "png")
            .await
            .expect("a second signed replacement");
        assert_ne!(first.file_name, second.file_name, "two uploads share a key");
        assert_eq!(
            repository.replace_image(&info.id, &first.file_name).await.unwrap(),
            Replacement::NotWaiting,
            "an upload a newer request has replaced is not this image's to take",
        );

        // The newest request's upload takes the image, and the bytes it used to serve go with it.
        assert_eq!(
            client
                .put(&second.upload_url)
                .body(vec![9u8])
                .send()
                .await
                .expect("the presigned PUT should be reachable")
                .status(),
            200
        );
        assert_eq!(
            repository
                .replace_image(&info.id, &second.file_name)
                .await
                .unwrap(),
            Replacement::Applied
        );
        assert_eq!(
            repository.image_file_name(&info.id).await.unwrap().as_deref(),
            Some(second.file_name.as_str())
        );
        let old = client
            .get(serve_url(&repository.inner, key).await.unwrap())
            .send()
            .await
            .expect("the bucket answers");
        assert_eq!(old.status(), 404, "the replaced bytes should be gone");

        // Applying the file it now serves again is a no-op rather than a refusal - and it leaves
        // what is waiting waiting, so a re-clicked replacement cannot cancel the upload that was
        // just signed for.
        let third = repository
            .generate_replacement_upload_url(&info.id, "png")
            .await
            .expect("a third signed replacement");
        assert_eq!(
            repository.replace_image(&info.id, &second.file_name).await.unwrap(),
            Replacement::Applied,
            "what it already serves is still not a change",
        );
        assert_eq!(
            client
                .put(&third.upload_url)
                .body(vec![7u8])
                .send()
                .await
                .expect("the presigned PUT should be reachable")
                .status(),
            200
        );
        assert_eq!(
            repository.replace_image(&info.id, &third.file_name).await.unwrap(),
            Replacement::Applied,
            "the upload it was waiting for was still waiting",
        );
    }

    /// What a deployment's S3 permissions actually allow, against the emulator.
    ///
    /// The emulator's root credentials can do anything, so a code path that needs a permission the
    /// deployment does not grant passes here and fails in the cloud: that is how a missing
    /// `s3:GetObject` stayed invisible. Running the same code as a user with the deployment's
    /// policy (`infra/lambda.tf`, mirrored by `deployment_s3_policy`) puts the same wall in front
    /// of it: a key that is there is found, and one that is not is absent.
    ///
    /// The one thing this cannot show is what the policy is *for*: with real S3 a caller that may
    /// not list the bucket is refused for a key that is not there, while MinIO answers 404 either
    /// way. `infra/lambda.tf` carries that reason, and the last part of this test states what the
    /// emulator does instead of pretending to check it.
    #[tokio::test]
    async fn finding_an_upload_needs_the_permissions_the_deployment_grants() {
        use sl_cms_core::repositories::image_repository::ImageRepository;

        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        // The bucket is the part this test is about, and making the restricted user needs the
        // emulator: an absent emulator is a skip, and one that refuses is a failure (see
        // `run_in_emulator`).
        let s3 = crate::test_s3_endpoint();
        if !emulator_reachable(&s3) {
            eprintln!("skipped: no S3 at {s3} (start it with `docker compose up -d`)");
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
        assert!(
            !repository.image_bytes_exist("nothing-here.png").await.unwrap(),
            "a key that is not there is answered *as absent*, which is what the code reads"
        );

        // What the emulator cannot show: with the real service, a caller that may not list the
        // bucket is *refused* (403) for a key that is not there, so "missing" and "not allowed"
        // look the same and this "not there yet" branch is never reached. That is the reason the
        // deployment's policy carries `s3:ListBucket` - and MinIO answers 404 whether or not the
        // policy allows listing, so the emulator pins the policy's shape, not that consequence.
        let Some((stranger, secret)) = crate::restricted_minio_user(&bucket, false) else {
            eprintln!("skipped: no emulator container to make a restricted user in");
            return;
        };
        let (stranger_repository, _table) =
            crate::open_test_repository_as("cms_permissions_denied", &bucket, &stranger, &secret)
                .await
                .expect("a repository as the second restricted user");
        assert!(
            !stranger_repository
                .image_bytes_exist("nothing-here.png")
                .await
                .expect("MinIO answers 404 with or without s3:ListBucket"),
            "a key that is not there is absent, whatever the policy says"
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
