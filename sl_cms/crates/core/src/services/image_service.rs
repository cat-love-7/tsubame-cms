use std::sync::Arc;

use crate::models::error::{map_internal_error, HttpError};
use crate::models::image::{
    is_safe_display_name, is_safe_file_name, is_safe_image_ext, ImageEntry, ImageID, NewImageInfo,
    NewImageRequest, ReplacementInfo, MAX_IMAGE_NAME_LENGTH,
};
use crate::repositories::image_repository::{ImageRepository, Replacement};

pub struct ImageService<R: ImageRepository> {
    repository: Arc<R>,
}

impl<R: ImageRepository> ImageService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        ImageService { repository }
    }

    /// The image library: what an editor can pick from, newest first.
    ///
    /// Trashed images are not in it (see [`ImageService::list_trash`]), but they still resolve for
    /// content that references them, so an item keeps its picture while the operator decides.
    pub async fn list_images(&self) -> Result<Vec<ImageEntry>, HttpError> {
        Ok(self.entries(false).await?)
    }

    /// The trash: images taken out of the library, most recently trashed first.
    pub async fn list_trash(&self) -> Result<Vec<ImageEntry>, HttpError> {
        let mut trashed = self.entries(true).await?;
        // Most recently trashed first, which is the one an operator wants back.
        trashed.sort_by_key(|entry| std::cmp::Reverse(entry.deleted_at));
        Ok(trashed)
    }

    /// The library or the trash, by whether the record carries a deletion time.
    async fn entries(&self, trashed: bool) -> Result<Vec<ImageEntry>, HttpError> {
        let mut images = self
            .repository
            .get_all_images()
            .await.map_err(map_internal_error)?;
        // Sorted here rather than trusted from the store: the on-premises adapter iterates
        // in key-byte order, which stops being numeric order once ids pass 255.
        images.sort_by_key(|(id, _)| std::cmp::Reverse(**id));

        Ok(images
            .into_iter()
            .filter(|(_, image)| image.deleted_at.is_some() == trashed)
            .map(|(id, image)| ImageEntry::from_image(id, image))
            .collect())
    }

    /// Move an image to the trash, where it stays until it is put back or deleted for good.
    ///
    /// Nothing is rewritten: content that references the image keeps the id it stored and keeps
    /// resolving, which is what makes this undoable. Already being in the trash is not an error -
    /// the caller asked for a state, and that state is what it gets.
    pub async fn trash_image(&self, id: ImageID) -> Result<(), HttpError> {
        self.require_image(&id).await?;
        self.repository
            .set_image_deleted_at(&id, Some(chrono::Utc::now()))
            .await
            .map_err(map_internal_error)
    }

    /// Put an image back in the library.
    pub async fn restore_image(&self, id: ImageID) -> Result<(), HttpError> {
        self.require_image(&id).await?;
        self.repository
            .set_image_deleted_at(&id, None)
            .await
            .map_err(map_internal_error)
    }

    /// Delete an image and the bytes stored under it, for good.
    ///
    /// Content that referenced the image keeps the id it stored, so the reference simply
    /// stops resolving; there is no reference check and nothing is rewritten.
    pub async fn purge_image(&self, id: ImageID) -> Result<(), HttpError> {
        self.require_image(&id).await?;
        self.repository
            .delete_image(&id)
            .await.map_err(map_internal_error)
    }

    /// The content that uses an image.
    ///
    /// What a delete warning is made of: an image already in the trash is only in the library's
    /// way, but deleting it for good takes the picture out of whatever this answers with. The list
    /// is only as complete as the reference index (see [`crate::models::image::referenced_images`]).
    pub async fn references(&self, id: ImageID) -> Result<Vec<crate::models::image::ImageOwner>, HttpError> {
        self.require_image(&id).await?;
        self.repository
            .get_image_references(&id)
            .await
            .map_err(map_internal_error)
    }

    async fn require_image(&self, id: &ImageID) -> Result<(), HttpError> {
        if self
            .repository
            .get_image(id)
            .await
            .map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Image with id '{}' does not exist",
                id
            )));
        }
        Ok(())
    }

    /// Give an image another display name.
    ///
    /// The name is what the library and the picker show. The id, the stored bytes and the URL
    /// are untouched, so content that references the image is unaffected - and a rename can be
    /// undone by renaming it back, which is the point of keeping it separate from replacing.
    pub async fn rename_image(
        &self,
        id: ImageID,
        original_filename: &str,
    ) -> Result<(), HttpError> {
        let name = original_filename.trim();
        if name.is_empty() {
            return Err(HttpError::BadRequest("an image name is required"));
        }
        if name.chars().count() > MAX_IMAGE_NAME_LENGTH {
            return Err(HttpError::BadRequest(&format!(
                "an image name may be at most {MAX_IMAGE_NAME_LENGTH} characters"
            )));
        }
        if !is_safe_display_name(name) {
            return Err(HttpError::BadRequest(
                "an image name cannot contain a path separator or a control character",
            ));
        }
        if self
            .repository
            .get_image(&id)
            .await.map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Image with id '{}' does not exist",
                id
            )));
        }
        self.repository
            .rename_image(&id, name)
            .await.map_err(map_internal_error)
    }

    /// Hand out a place to upload bytes that will replace what an image shows.
    ///
    /// The record is not touched: [`ImageService::replace_image`] applies the replacement once the
    /// bytes exist, so an upload that fails leaves the current image serving. Nothing about the
    /// image's identity changes - not its id, not the name it is shown under - only what it shows.
    pub async fn request_replacement(
        &self,
        id: ImageID,
        ext: &str,
    ) -> Result<ReplacementInfo, HttpError> {
        if !is_safe_image_ext(ext) {
            return Err(HttpError::BadRequest("Invalid image extension"));
        }
        if self
            .repository
            .get_image(&id)
            .await.map_err(map_internal_error)?
            .is_none()
        {
            return Err(HttpError::NotFound(&format!(
                "Image with id '{}' does not exist",
                id
            )));
        }
        self.repository
            .generate_replacement_upload_url(&id, ext)
            .await.map_err(map_internal_error)
    }

    /// Point an image at bytes that have been uploaded for it, and let go of the old ones.
    ///
    /// The file name comes back from [`ImageService::request_replacement`], so this finishes a
    /// replacement rather than starting one. It is checked against storage: a record is never
    /// pointed at bytes that are not there, which is what makes "upload first, then swap" safe.
    ///
    /// The upload it takes has to be the one this image is waiting for, and taking it is what ends
    /// the wait: an apply naming anything else leaves the wait alone, and so does naming the file
    /// the image already serves.
    pub async fn replace_image(&self, id: ImageID, file_name: &str) -> Result<(), HttpError> {
        if !is_safe_file_name(file_name) {
            return Err(HttpError::BadRequest("Invalid image file name"));
        }
        // What the record names now, which is also the answer to "is there such an image". Asked by
        // id: the record knows its own file, and nothing has to be searched for.
        let Some(current) = self
            .repository
            .image_file_name(&id)
            .await
            .map_err(map_internal_error)?
        else {
            return Err(HttpError::NotFound(&format!(
                "Image with id '{}' does not exist",
                id
            )));
        };

        // The bytes have to be there, or the image would serve nothing. Asked of the file the
        // caller named, whether or not it changes anything.
        if !self
            .repository
            .image_bytes_exist(file_name)
            .await.map_err(map_internal_error)?
        {
            return Err(HttpError::NotFound(
                "the uploaded image is not there yet",
            ));
        }

        // Pointing an image at the file it already serves is a no-op rather than a refusal: the
        // screen sends what it was given, and a second click on the same file is not a mistake
        // worth refusing. Answering here (rather than applying it again) is what leaves a
        // replacement that is waiting for some *other* upload waiting.
        if file_name == current {
            return Ok(());
        }

        // The file name is the server's to choose, and the server recorded the one it chose for
        // *this* image when the replacement was requested. An apply naming anything else - another
        // image's file, or one no request produced - is not this image's to take. The repository
        // answers that as it consumes the upload, in one step.
        match self
            .repository
            .replace_image(&id, file_name)
            .await
            .map_err(map_internal_error)?
        {
            Replacement::Applied => Ok(()),
            Replacement::NotWaiting => Err(HttpError::BadRequest(
                "that is not the upload this replacement was for",
            )),
        }
    }

    /// Where an image is served from right now, or `None` when there is no such image.
    ///
    /// Used for the link that keeps working across a replacement (see the `/images/by-id/` route):
    /// the id is the durable name of an image, and where the bytes happen to live is not.
    pub async fn image_url(&self, id: ImageID) -> Result<Option<String>, HttpError> {
        Ok(self
            .repository
            .get_image(&id)
            .await
            .map_err(map_internal_error)?
            .map(|image| image.url))
    }

    pub async fn generate_image_upload_url(
        &self,
        upload_info: NewImageRequest,
    ) -> Result<NewImageInfo, HttpError> {
        self.repository
            .generate_image_upload_url(&upload_info)
            .await.map_err(map_internal_error)
    }

}

/// Serving the bytes is a local adapter's business: this block simply does not apply to an
/// adapter that leaves them to object storage (see [`LocalImageBytes`]), so those adapters
/// have no such methods rather than methods that must never be called.
impl<R: ImageRepository + crate::repositories::local_image_bytes::LocalImageBytes> ImageService<R> {
    /// Consume a one-shot upload token, yielding the file name it authorises.
    pub async fn take_upload_key(&self, key: &str) -> Result<Option<String>, HttpError> {
        self.repository.take_upload_key(key).await.map_err(map_internal_error)
    }

    pub async fn read_image_bytes(&self, file_name: &str) -> Result<Option<Vec<u8>>, HttpError> {
        self.repository
            .read_image_bytes(file_name)
            .await.map_err(map_internal_error)
    }

    pub async fn write_image_bytes(&self, file_name: &str, data: &[u8]) -> Result<(), HttpError> {
        self.repository
            .write_image_bytes(file_name, data)
            .await.map_err(map_internal_error)
    }
}
