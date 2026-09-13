use std::sync::Arc;

use crate::models::error::{map_internal_error, HttpError};
use crate::models::image::{ImageEntry, ImageID, NewImageInfo, NewImageRequest};
use crate::repositories::image_repository::ImageRepository;

pub struct ImageService<R: ImageRepository> {
    repository: Arc<R>,
}

impl<R: ImageRepository> ImageService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        ImageService { repository }
    }

    /// The image library, newest first.
    pub async fn list_images(&self) -> Result<Vec<ImageEntry>, HttpError> {
        let mut images = self
            .repository
            .get_all_images()
            .await.map_err(map_internal_error)?;
        // Sorted here rather than trusted from the store: the on-premises adapter iterates
        // in key-byte order, which stops being numeric order once ids pass 255.
        images.sort_by_key(|(id, _)| std::cmp::Reverse(**id));

        Ok(images
            .into_iter()
            .map(|(id, image)| ImageEntry::from_image(id, image))
            .collect())
    }

    /// Delete an image and the bytes stored under it.
    ///
    /// Content that referenced the image keeps the id it stored, so the reference simply
    /// stops resolving; there is no reference check and nothing is rewritten.
    pub async fn delete_image(&self, id: ImageID) -> Result<(), HttpError> {
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
            .delete_image(&id)
            .await.map_err(map_internal_error)
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
