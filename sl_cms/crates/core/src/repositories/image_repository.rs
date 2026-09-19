use std::future::Future;

use crate::models::image::{
    Image, ImageId, ImageOwner, NewImageInfo, NewImageRequest, ReplacementInfo,
};

pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// What applying a replacement did.
///
/// "The image was not waiting for that upload" is an answer rather than an error, because the
/// caller turns it into a refusal with a message of its own, and the adapter has to be able to say
/// it without failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replacement {
    /// The image now serves the upload it was waiting for.
    Applied,
    /// The image was not waiting for that upload: nothing was changed.
    NotWaiting,
}

/// What every backend has to provide for the image library.
///
/// Handing out a place to upload to is part of the contract, so a local adapter and an
/// object-storage adapter differ in the URL they return rather than in the shape of the
/// answer. Serving and accepting those bytes is *not* here: that belongs to the adapter that
/// stores them itself (see [`crate::repositories::local_image_bytes::LocalImageBytes`]).
pub trait ImageRepository: Send + Sync {
    fn get_image(
        &self,
        id: &ImageId,
    ) -> impl Future<Output = Result<Option<Image>, BoxError>> + Send;
    fn list_images(&self) -> impl Future<Output = Result<Vec<(ImageId, Image)>, BoxError>> + Send;
    fn generate_image_upload_url(
        &self,
        upload_info: &NewImageRequest,
    ) -> impl Future<Output = Result<NewImageInfo, BoxError>> + Send;
    fn delete_image(&self, id: &ImageId) -> impl Future<Output = Result<(), BoxError>> + Send;

    /// The file an image's record currently names, or `None` when there is no such image.
    ///
    /// Asked by id, so it is a point read: the record already knows its own file, and there is
    /// nothing to search for.
    fn image_file_name(
        &self,
        id: &ImageId,
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
        images: &[ImageId],
    ) -> impl Future<Output = Result<(), BoxError>> + Send;

    /// The content that uses an image.
    fn get_image_references(
        &self,
        id: &ImageId,
    ) -> impl Future<Output = Result<Vec<ImageOwner>, BoxError>> + Send;

    /// Move an image in or out of the trash.
    ///
    /// `Some(at)` is the trash and `None` is the library; the bytes and the file name are not
    /// touched either way, so content that references the image keeps resolving until it is
    /// deleted for good.
    fn set_image_deleted_at(
        &self,
        id: &ImageId,
        at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    /// Give an image another display name, keeping its id and its bytes.
    fn rename_image(
        &self,
        id: &ImageId,
        original_filename: &str,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;

    /// Record when the image arrived, for a library that came from somewhere else.
    ///
    /// Nothing else has to move with it: the library is listed by id and the delivery API never
    /// sees this, so it is the one field an import can state freely.
    fn set_image_uploaded_at(
        &self,
        id: &ImageId,
        uploaded_at: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;
    /// Hand out a place to upload replacement bytes, under a file name of the adapter's choosing,
    /// and record that file as this image's pending one.
    ///
    /// The bytes the image serves are untouched: [`ImageRepository::replace_image`] points the
    /// image at the new bytes once they exist, so an upload that fails leaves the current image
    /// serving. What is recorded is only which file the server offered *this* image, and only the
    /// newest request stays waiting.
    ///
    /// Recording it must change nothing else: an adapter that writes the rest of the record back
    /// from what it read can undo an apply that landed in between, and that apply has already
    /// deleted the bytes the stale record still names.
    fn generate_replacement_upload_url(
        &self,
        id: &ImageId,
        request: &crate::models::image::ReplaceImageRequest,
    ) -> impl Future<Output = Result<ReplacementInfo, BoxError>> + Send;
    /// Whether bytes are stored under `file_name`.
    fn image_bytes_exist(
        &self,
        file_name: &str,
    ) -> impl Future<Output = Result<bool, BoxError>> + Send;
    /// Point an image at the upload it was waiting for, and forget that upload.
    ///
    /// The file name is the server's to choose: [`ImageRepository::generate_replacement_upload_url`]
    /// records the one it chose for this image, and only that file applies - see
    /// [`Replacement`]. Checking and consuming are **one step**: an apply that checked the record
    /// and then waited before writing could take an upload a newer request had already replaced, or
    /// forget one recorded in between, and the request in between could write back a record it read
    /// before the apply and point the image at bytes the apply deleted.
    ///
    /// An image that already serves `file_name` is not a change (and nothing is forgotten).
    fn replace_image(
        &self,
        id: &ImageId,
        file_name: &str,
    ) -> impl Future<Output = Result<Replacement, BoxError>> + Send;
}
