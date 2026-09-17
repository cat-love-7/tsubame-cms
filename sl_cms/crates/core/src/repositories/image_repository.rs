use std::future::Future;

use crate::models::image::{Image, ImageID, ImageOwner, NewImageInfo, NewImageRequest, ReplacementInfo};

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

    /// The file an image's record currently names, or `None` when there is no such image.
    ///
    /// Asked by id, so it is a point read: the record already knows its own file, and there is
    /// nothing to search for.
    fn image_file_name(
        &self,
        id: &ImageID,
    ) -> impl Future<Output = Result<Option<String>, BoxError>> + Send;

    /// The file this image was signed an upload for, while a replacement is waiting.
    ///
    /// Recorded by [`ImageRepository::generate_replacement_upload_url`] and forgotten by
    /// [`ImageRepository::replace_image`]: the file name is the server's to choose, and recording
    /// which one it chose *for this image* is what makes an apply answerable by id. Without it, an
    /// apply could only be checked against the file names other records happen to use - which says
    /// nothing about an upload that no record names yet.
    fn pending_replacement(
        &self,
        id: &ImageID,
    ) -> impl Future<Output = Result<Option<String>, BoxError>> + Send;

    /// Record which images one piece of content uses, replacing what it used before.
    ///
    /// Called when the content is saved (with the union of what the published and the working
    /// copies reference, because either may be served) and with an empty list when it is deleted.
    /// The index is what answers "what would break if this image went?" - see
    /// [`ImageRepository::get_image_references`].
    fn set_image_references(
        &self,
        owner: &ImageOwner,
        images: &[ImageID],
    ) -> impl Future<Output = Result<(), BoxError>> + Send;

    /// The content that uses an image.
    fn get_image_references(
        &self,
        id: &ImageID,
    ) -> impl Future<Output = Result<Vec<ImageOwner>, BoxError>> + Send;

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
    /// Hand out a place to upload replacement bytes, under a file name of the adapter's choosing,
    /// and record that file as this image's pending one.
    ///
    /// The bytes the image serves are untouched: [`ImageRepository::replace_image`] points the
    /// image at the new bytes once they exist, so an upload that fails leaves the current image
    /// serving. What is recorded is only which file the server offered *this* image.
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
    /// Point an image at the bytes it was given, and forget the pending upload.
    fn replace_image(
        &self,
        id: &ImageID,
        file_name: &str,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
}

