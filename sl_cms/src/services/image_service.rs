use std::sync::Arc;

use crate::models::error::{map_internal_error, HttpError};
use crate::models::image::{NewImageInfo, NewImageRequest};
use crate::repositories::image_repository::ImageRepository;

pub struct ImageService<R: ImageRepository> {
    repository: Arc<R>,
}

impl<R: ImageRepository> ImageService<R> {
    pub fn new(repository: Arc<R>) -> Self {
        ImageService { repository }
    }

    pub fn generate_image_upload_url(
        &self,
        upload_info: NewImageRequest,
    ) -> Result<NewImageInfo, HttpError> {
        self.repository
            .generate_image_upload_url(&upload_info)
            .map_err(map_internal_error)
    }

    /// Consume a one-shot upload token, yielding the file name it authorises.
    pub fn take_upload_key(&self, key: &str) -> Result<Option<String>, HttpError> {
        self.repository.take_upload_key(key).map_err(map_internal_error)
    }

    pub fn read_image_bytes(&self, file_name: &str) -> Result<Option<Vec<u8>>, HttpError> {
        self.repository
            .read_image_bytes(file_name)
            .map_err(map_internal_error)
    }

    pub fn write_image_bytes(&self, file_name: &str, data: &[u8]) -> Result<(), HttpError> {
        self.repository
            .write_image_bytes(file_name, data)
            .map_err(map_internal_error)
    }
}
