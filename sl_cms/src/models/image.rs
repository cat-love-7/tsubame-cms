use crate::models::identity::UintId;


pub type ImageID = UintId<Image>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct ImageResponse {
    pub id: ImageID,
    pub url: String,
}

impl ImageResponse {
    pub fn from_id(id: ImageID) -> Self {
        ImageResponse {
            url: format!("/images/{}", *id),
            id,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Image {
    pub original_filename: String,
    pub url: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct NewImageRequest{
    pub original_filename: String,
    pub ext: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct NewImageInfo{
    pub id: ImageID,
    pub upload_url: String,
}