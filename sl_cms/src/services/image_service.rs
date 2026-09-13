use std::sync::Arc;

use crate::{models::{error::{HttpError, map_internal_error}, image::{NewImageInfo, NewImageRequest}}, repositories::image_repository::ImageRepository};


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
        return self.repository.generate_image_upload_url(&upload_info).map_err(map_internal_error)
    }
}