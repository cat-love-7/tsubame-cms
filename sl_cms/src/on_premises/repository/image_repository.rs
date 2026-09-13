use std::fs;

use rkv::{StoreOptions, Value};

use crate::models::image::{Image, ImageID, NewImageInfo, NewImageRequest};
use crate::repositories::image_repository::ImageRepository;
use crate::on_premises::repository::Repository;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ImageData {
    pub original_filename: String,
    pub file_name: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
}
impl Repository {
    fn get_image_data(
        &self,
        id: &ImageID,
    ) -> Result<Option<ImageData>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, id.to_le_bytes())? {
            Some(Value::Str(s)) => {
                let image_data:ImageData = serde_json::from_str(&s)?;
                Ok(Some(image_data))
            },
            _ => Ok(None),
        }
    }
}

impl ImageRepository for Repository {
    fn get_image(
        &self,
        id: &ImageID,
    ) -> Result<Option<crate::models::image::Image>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        match self.get_image_data(id)? {
            Some(image) => {
                Ok(Some(Image {
                    original_filename: image.original_filename,
                    url: format!("/images/{}", image.file_name),
                    uploaded_at: image.uploaded_at,
                }))
            },
            _ => Ok(None),
        }
    }

    fn get_all_images(
        &self,
    ) -> Result<Vec<(ImageID, Image)>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let reader = env.read()?;
        let mut images = Vec::new();
        for result in store.iter_start(&reader)? {
            if let Ok((key, Value::Str(s))) = result {
                let id = ImageID::from_u64(u64::from_le_bytes(key.try_into().unwrap()));
                let image_data:ImageData = serde_json::from_str(&s)?;
                let image = Image {
                    original_filename: image_data.original_filename,
                    url: format!("/images/{}", image_data.file_name),
                    uploaded_at: image_data.uploaded_at,
                };
                images.push((id, image));
            }
        }
        Ok(images)
    }

    fn generate_image_upload_url(
        &self,
        upload_info: &NewImageRequest,
    ) -> Result<NewImageInfo, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let mut writer = env.write()?;
        let save_file_name = format!("{}.{}", uuid::Uuid::new_v4(), upload_info.ext);
        let reader = env.read()?;
        let current_id = match self.counter_store.get(&reader, "image")? {
            Some(Value::U64(id)) => id,
            _ => 0,
        };
        let new_id = current_id + 1;
        let image_id = ImageID::from_u64(new_id);
        let image_data = ImageData {
            original_filename: upload_info.original_filename.clone(),
            file_name: save_file_name.clone(),
            uploaded_at: chrono::Utc::now(),
        };
        store.put(&mut writer, new_id.to_le_bytes(), &Value::Str(&serde_json::to_string(&image_data)?))?;
        self.counter_store.put(&mut writer, "image", &Value::U64(new_id))?;
        writer.commit()?;
        
        Ok(NewImageInfo {
            upload_url: format!("/upload/{}?key={}", save_file_name,""),
            id: image_id,
        })
    }

    fn delete_image(
        &self,
        id: &ImageID,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let mut writer = env.write()?;
        let image = self.get_image_data(id)?.ok_or("Image not found")?;

        fs::remove_file(format!("./data/on_premises/images/{}", image.file_name)).ok();
        store.delete(&mut writer, id.to_le_bytes())?;
        writer.commit()?;
        Ok(())
    }
}