use std::fs;
use std::path::PathBuf;

use rkv::{StoreOptions, Value};

use crate::models::image::{is_safe_file_name, Image, ImageID, NewImageInfo, NewImageRequest};
use crate::on_premises::repository::Repository;
use crate::repositories::image_repository::{BoxError, ImageRepository};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ImageData {
    pub original_filename: String,
    pub file_name: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
}

/// Normalise a caller-supplied extension into something safe to embed in a file name.
///
/// The extension arrives from the client, so anything that could introduce a path
/// separator or a traversal (`../../x`) must be stripped. Returns `None` when nothing
/// usable remains, in which case the file name simply has no extension.
fn sanitize_ext(ext: &str) -> Option<String> {
    let cleaned: String = ext
        .trim()
        .trim_start_matches('.')
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(10)
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_ascii_lowercase())
    }
}

impl Repository {
    fn get_image_data(&self, id: &ImageID) -> Result<Option<ImageData>, BoxError> {
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

    /// Resolve `file_name` to a path inside the configured images directory.
    ///
    /// Rejects anything that is not a single plain file name; see
    /// [`crate::models::image::is_safe_file_name`] for why this is required.
    fn image_path(&self, file_name: &str) -> Result<PathBuf, BoxError> {
        if !is_safe_file_name(file_name) {
            return Err(format!("invalid image file name: {file_name:?}").into());
        }
        Ok(self.images_dir().join(file_name))
    }
}

impl ImageRepository for Repository {
    fn get_image(&self, id: &ImageID) -> Result<Option<Image>, BoxError> {
        match self.get_image_data(id)? {
            Some(image) => Ok(Some(Image {
                original_filename: image.original_filename,
                url: format!("/images/{}", image.file_name),
                uploaded_at: image.uploaded_at,
            })),
            _ => Ok(None),
        }
    }

    fn get_all_images(&self) -> Result<Vec<(ImageID, Image)>, BoxError> {
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
                };
                images.push((id, image));
            }
        }
        Ok(images)
    }

    fn generate_image_upload_url(&self, upload_info: &NewImageRequest) -> Result<NewImageInfo, BoxError> {
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
            id: image_id,
        })
    }

    fn delete_image(&self, id: &ImageID) -> Result<(), BoxError> {
        let image = self.get_image_data(id)?.ok_or("Image not found")?;
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

    fn take_upload_key(&self, key: &str) -> Result<Option<String>, BoxError> {
        Ok(self.consume_upload_key(key))
    }

    fn read_image_bytes(&self, file_name: &str) -> Result<Option<Vec<u8>>, BoxError> {
        let path = self.image_path(file_name)?;
        match fs::read(&path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn write_image_bytes(&self, file_name: &str, data: &[u8]) -> Result<(), BoxError> {
        let path = self.image_path(file_name)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_ext;

    #[test]
    fn sanitize_ext_strips_separators_and_traversal() {
        assert_eq!(sanitize_ext("png").as_deref(), Some("png"));
        assert_eq!(sanitize_ext(".PNG").as_deref(), Some("png"));
        assert_eq!(sanitize_ext("jpeg").as_deref(), Some("jpeg"));
        // Path separators and dots cannot survive.
        assert_eq!(sanitize_ext("../../etc/passwd").as_deref(), Some("etcpasswd"));
        assert_eq!(sanitize_ext("a/b").as_deref(), Some("ab"));
        assert_eq!(sanitize_ext("..\\..\\x").as_deref(), Some("x"));
        // Nothing usable left -> no extension.
        assert_eq!(sanitize_ext(""), None);
        assert_eq!(sanitize_ext("..."), None);
        assert_eq!(sanitize_ext("../"), None);
    }
}
