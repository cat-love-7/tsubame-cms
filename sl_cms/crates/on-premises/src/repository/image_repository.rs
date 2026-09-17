use std::fs;
use std::path::PathBuf;

use rkv::{StoreOptions, Value};

use sl_cms_core::models::image::{
    is_safe_file_name, sanitize_ext, Image, ImageID, NewImageInfo, NewImageRequest,
    ReplacementInfo,
};
use crate::repository::Repository;
use sl_cms_core::repositories::image_repository::{BoxError, ImageRepository};
use sl_cms_core::repositories::local_image_bytes::LocalImageBytes;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ImageData {
    pub original_filename: String,
    pub file_name: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
    /// Absent in records written before the trash existed, which is why it has a default.
    #[serde(default)]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Repository {
    /// The lookup itself, without the storage lock: `delete_image` already holds it, and a
    /// `std::sync::Mutex` is not reentrant.
    fn image_data(&self, id: &ImageID) -> Result<Option<ImageData>, BoxError> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, id.to_le_bytes())? {
            Some(Value::Str(s)) => {
                let image_data: ImageData = serde_json::from_str(&s)?;
                Ok(Some(image_data))
            }
            _ => Ok(None),
        }
    }

    fn get_image_data(&self, id: &ImageID) -> Result<Option<ImageData>, BoxError> {
        let _guard = self.begin();
        self.image_data(id)
    }

    /// Resolve `file_name` to a path inside the configured images directory.
    ///
    /// Rejects anything that is not a single plain file name; see
    /// [`sl_cms_core::models::image::is_safe_file_name`] for why this is required.
    fn image_path(&self, file_name: &str) -> Result<PathBuf, BoxError> {
        if !is_safe_file_name(file_name) {
            return Err(format!("invalid image file name: {file_name:?}").into());
        }
        Ok(self.images_dir().join(file_name))
    }
}

impl ImageRepository for Repository {
    async fn get_image(&self, id: &ImageID) -> Result<Option<Image>, BoxError> {
        match self.get_image_data(id)? {
            Some(image) => Ok(Some(Image {
                original_filename: image.original_filename,
                url: format!("/images/{}", image.file_name),
                uploaded_at: image.uploaded_at,
                deleted_at: image.deleted_at,
            })),
            _ => Ok(None),
        }
    }

    async fn get_all_images(&self) -> Result<Vec<(ImageID, Image)>, BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let reader = env.read()?;
        let mut images = Vec::new();
        for result in store.iter_start(&reader)? {
            if let Ok((key, Value::Str(s))) = result {
                // Keys are written as 8-byte little-endian ids; skip anything else
                // instead of panicking on a malformed record.
                let Ok(raw_id) = <[u8; 8]>::try_from(key) else {
                    continue;
                };
                let id = ImageID::from_u64(u64::from_le_bytes(raw_id));
                let image_data: ImageData = serde_json::from_str(&s)?;
                let image = Image {
                    original_filename: image_data.original_filename,
                    url: format!("/images/{}", image_data.file_name),
                    uploaded_at: image_data.uploaded_at,
                    deleted_at: image_data.deleted_at,
                };
                images.push((id, image));
            }
        }
        Ok(images)
    }

    async fn generate_image_upload_url(&self, upload_info: &NewImageRequest) -> Result<NewImageInfo, BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        // Write transaction first, then the read snapshot that reads the id counter,
        // so concurrent uploaders cannot allocate the same id.
        let mut writer = env.write()?;
        let reader = env.read()?;

        let save_file_name = match sanitize_ext(&upload_info.ext) {
            Some(ext) => format!("{}.{}", uuid::Uuid::new_v4(), ext),
            None => uuid::Uuid::new_v4().to_string(),
        };
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
            // A fresh upload is in the library, not the trash.
            deleted_at: None,
        };
        store.put(&mut writer, new_id.to_le_bytes(), &Value::Str(&serde_json::to_string(&image_data)?))?;
        self.counter_store.put(&mut writer, "image", &Value::U64(new_id))?;
        writer.commit()?;

        // The capability token must be registered, otherwise the upload endpoint can
        // never authorise the subsequent PUT (previously it always returned 401).
        let upload_key = uuid::Uuid::new_v4().to_string();
        self.register_upload_key(upload_key.clone(), save_file_name.clone());

        Ok(NewImageInfo {
            upload_url: format!("/images/{}?key={}", save_file_name, upload_key),
            url: format!("/images/{}", save_file_name),
            id: image_id,
        })
    }

    async fn rename_image(&self, id: &ImageID, original_filename: &str) -> Result<(), BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let reader = env.read()?;
        let mut data = match store.get(&reader, id.to_le_bytes())? {
            Some(Value::Str(s)) => serde_json::from_str::<ImageData>(&s)?,
            _ => return Err("Image not found".into()),
        };
        data.original_filename = original_filename.to_string();
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            id.to_le_bytes(),
            &Value::Str(&serde_json::to_string(&data)?),
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn generate_replacement_upload_url(
        &self,
        _id: &ImageID,
        ext: &str,
    ) -> Result<ReplacementInfo, BoxError> {
        // A fresh file name rather than an overwrite: the browser (and anything in front of it)
        // caches by URL, so bytes that change under one URL keep showing the old image.
        let file_name = match sanitize_ext(ext) {
            Some(ext) => format!("{}.{}", uuid::Uuid::new_v4(), ext),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let upload_key = uuid::Uuid::new_v4().to_string();
        self.register_upload_key(upload_key.clone(), file_name.clone());

        Ok(ReplacementInfo {
            upload_url: format!("/images/{}?key={}", file_name, upload_key),
            file_name,
        })
    }

    async fn image_bytes_exist(&self, file_name: &str) -> Result<bool, BoxError> {
        Ok(self.image_path(file_name)?.is_file())
    }

    async fn replace_image(&self, id: &ImageID, file_name: &str) -> Result<(), BoxError> {
        let _guard = self.begin();
        // The path check happens before anything is written: the name comes from the server's own
        // upload URL, but a record must never be pointed outside the images directory.
        self.image_path(file_name)?;

        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let reader = env.read()?;
        let previous = match store.get(&reader, id.to_le_bytes())? {
            Some(Value::Str(s)) => serde_json::from_str::<ImageData>(&s)?,
            _ => return Err("Image not found".into()),
        };

        let updated = ImageData {
            original_filename: previous.original_filename.clone(),
            file_name: file_name.to_string(),
            // The first upload is when the image entered the library; replacing what it shows
            // does not change that, just as re-publishing does not change `published_at`.
            uploaded_at: previous.uploaded_at,
            // Replacing the bytes does not take an image out of the trash, or put it back in.
            deleted_at: previous.deleted_at,
        };
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            id.to_le_bytes(),
            &Value::Str(&serde_json::to_string(&updated)?),
        )?;
        writer.commit()?;

        // What it used to name is now unreferenced. A missing file is not an error: the previous
        // upload may never have completed.
        if previous.file_name != file_name {
            if let Ok(path) = self.image_path(&previous.file_name) {
                fs::remove_file(path).ok();
            }
        }
        Ok(())
    }

    async fn set_image_deleted_at(
        &self,
        id: &ImageID,
        at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let reader = env.read()?;
        let mut data = match store.get(&reader, id.to_le_bytes())? {
            Some(Value::Str(s)) => serde_json::from_str::<ImageData>(&s)?,
            _ => return Err("Image not found".into()),
        };
        data.deleted_at = at;
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            id.to_le_bytes(),
            &Value::Str(&serde_json::to_string(&data)?),
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn delete_image(&self, id: &ImageID) -> Result<(), BoxError> {
        let _guard = self.begin();
        let image = self.image_data(id)?.ok_or("Image not found")?;
        // Remove the stored bytes first; a missing file is not an error (the metadata
        // may exist without bytes if an upload never completed).
        if let Ok(path) = self.image_path(&image.file_name) {
            fs::remove_file(path).ok();
        }

        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("image", StoreOptions::create())?;
        let mut writer = env.write()?;
        store.delete(&mut writer, id.to_le_bytes())?;
        writer.commit()?;
        Ok(())
    }

}

/// The bytes themselves: this adapter stores them on disk and hands out its own upload URL,
/// so it is the one that authorises and serves the transfers (see [`LocalImageBytes`]).
impl LocalImageBytes for Repository {
    async fn take_upload_key(&self, key: &str) -> Result<Option<String>, BoxError> {
        Ok(self.consume_upload_key(key))
    }

    async fn read_image_bytes(&self, file_name: &str) -> Result<Option<Vec<u8>>, BoxError> {
        let path = self.image_path(file_name)?;
        match fs::read(&path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn write_image_bytes(&self, file_name: &str, data: &[u8]) -> Result<(), BoxError> {
        let path = self.image_path(file_name)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data)?;
        Ok(())
    }
}
