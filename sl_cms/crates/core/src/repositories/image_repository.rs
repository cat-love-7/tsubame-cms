use std::future::Future;

use crate::models::image::{Image, ImageID, NewImageInfo, NewImageRequest, ReplacementInfo};

pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// What every backend has to provide for the image library.
///
/// Handing out a place to upload to is part of the contract, so a local adapter and an
/// object-storage adapter differ in the URL they return rather than in the shape of the
/// answer. Serving and accepting those bytes is *not* here: that belongs to the adapter that
/// stores them itself (see [`crate::repositories::local_image_bytes::LocalImageBytes`]).
pub trait ImageRepository: Send + Sync {
    fn get_image(&self, id: &ImageID) -> impl Future<Output = Result<Option<Image>, BoxError>> + Send;
    fn get_all_images(&self) -> impl Future<Output = Result<Vec<(ImageID, Image)>, BoxError>> + Send;
    fn generate_image_upload_url(&self, upload_info: &NewImageRequest) -> impl Future<Output = Result<NewImageInfo, BoxError>> + Send;
    fn delete_image(&self, id: &ImageID) -> impl Future<Output = Result<(), BoxError>> + Send;

    /// Move an image in or out of the trash.
    ///
    /// `Some(at)` is the trash and `None` is the library; the bytes and the file name are not
    /// touched either way, so content that references the image keeps resolving until it is
    /// deleted for good.
    fn set_image_deleted_at(
        &self,
        id: &ImageID,
        at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    /// Give an image another display name, keeping its id and its bytes.
    fn rename_image(
        &self,
        id: &ImageID,
        original_filename: &str,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    /// Hand out a place to upload replacement bytes, under a file name of the adapter's choosing.
    ///
    /// The record is untouched: [`ImageRepository::replace_image`] points the image at the new
    /// bytes once they exist, so an upload that fails leaves the current image serving.
    fn generate_replacement_upload_url(
        &self,
        id: &ImageID,
        ext: &str,
    ) -> impl Future<Output = Result<ReplacementInfo, BoxError>> + Send;
    /// Whether bytes are stored under `file_name`.
    fn image_bytes_exist(
        &self,
        file_name: &str,
    ) -> impl Future<Output = Result<bool, BoxError>> + Send;
    /// Point an image at `file_name`, deleting the bytes it used to name.
    fn replace_image(
        &self,
        id: &ImageID,
        file_name: &str,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
}

