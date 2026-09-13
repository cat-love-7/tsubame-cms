use crate::models::image::{Image, ImageID, NewImageInfo, NewImageRequest};

pub trait ImageRepository: Send + Sync {
    fn get_image(
        &self,
        id: &ImageID,
    ) -> Result<Option<Image>, Box<dyn std::error::Error + Send + Sync + 'static>>;
    fn get_all_images(
        &self,
    ) -> Result<Vec<(ImageID, Image)>, Box<dyn std::error::Error + Send + Sync + 'static>>;
    fn generate_image_upload_url(
        &self,
        upload_info: &NewImageRequest,
    ) -> Result<NewImageInfo, Box<dyn std::error::Error + Send + Sync + 'static>>;
    fn delete_image(
        &self,
        id: &ImageID,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>>;
}
