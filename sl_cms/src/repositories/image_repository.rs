use crate::models::image::{Image, ImageID, NewImageInfo, NewImageRequest};

pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// What every backend has to provide for the image library.
///
/// Handing out a place to upload to is part of the contract, so a local adapter and an
/// object-storage adapter differ in the URL they return rather than in the shape of the
/// answer. Serving and accepting those bytes is *not* here: that belongs to the adapter that
/// stores them itself (see [`crate::repositories::local_image_bytes::LocalImageBytes`]).
pub trait ImageRepository: Send + Sync {
    fn get_image(&self, id: &ImageID) -> Result<Option<Image>, BoxError>;
    fn get_all_images(&self) -> Result<Vec<(ImageID, Image)>, BoxError>;
    fn generate_image_upload_url(&self, upload_info: &NewImageRequest) -> Result<NewImageInfo, BoxError>;
    fn delete_image(&self, id: &ImageID) -> Result<(), BoxError>;
}

