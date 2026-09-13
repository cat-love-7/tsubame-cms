use crate::models::image::{Image, ImageID, NewImageInfo, NewImageRequest};

pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub trait ImageRepository: Send + Sync {
    fn get_image(&self, id: &ImageID) -> Result<Option<Image>, BoxError>;
    fn get_all_images(&self) -> Result<Vec<(ImageID, Image)>, BoxError>;
    fn generate_image_upload_url(&self, upload_info: &NewImageRequest) -> Result<NewImageInfo, BoxError>;
    fn delete_image(&self, id: &ImageID) -> Result<(), BoxError>;

    /// Consume the one-shot upload token previously handed out by
    /// [`ImageRepository::generate_image_upload_url`], returning the file name that the
    /// token authorises.
    ///
    /// Tokens are single-use: a successful call must also invalidate the token, so a
    /// leaked token cannot be replayed. Binding the token to a specific file name means
    /// a token for one upload cannot be used to overwrite a different file.
    fn take_upload_key(&self, key: &str) -> Result<Option<String>, BoxError>;

    /// Read the bytes previously stored under `file_name`.
    ///
    /// Implementations must reject unsafe file names rather than touching storage.
    fn read_image_bytes(&self, file_name: &str) -> Result<Option<Vec<u8>>, BoxError>;

    /// Store `data` under `file_name`.
    ///
    /// Implementations must reject unsafe file names rather than touching storage.
    fn write_image_bytes(&self, file_name: &str, data: &[u8]) -> Result<(), BoxError>;
}
