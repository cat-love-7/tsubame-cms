use std::fs;
use std::path::PathBuf;

use rkv::{StoreOptions, Value};

use sl_cms_core::models::image::{
    is_safe_file_name, sanitize_ext, Image, ImageID, NewImageInfo, NewImageRequest,
    ReplacementInfo,
    ImageOwner,
};
use crate::repository::Repository;
use sl_cms_core::repositories::image_repository::{BoxError, ImageRepository, Replacement};
use sl_cms_core::repositories::local_image_bytes::LocalImageBytes;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ImageData {
    pub original_filename: String,
    pub file_name: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
    /// Absent in records written before the trash existed, which is why it has a default.
    #[serde(default)]
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The upload this image is waiting for, if a replacement was requested and not applied.
    #[serde(default)]
    pub pending_replacement: Option<String>,
}

/// Where the image reference index lives: which content uses which image.
const IMAGE_REFS_STORE: &str = "image_refs";

/// The entries one owner has: `<owner>|image|<id>`.
fn owner_prefix(owner: &ImageOwner) -> String {
    format!("{}|", owner.storage_key())
}

/// The entries one image has: `image|<id>|<owner>`.
fn image_prefix(id: &ImageID) -> String {
    format!("image|{}|", **id)
}

fn image_owner_key(id: &ImageID, owner: &ImageOwner) -> String {
    format!("{}{}", image_prefix(id), owner.storage_key())
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
            // A fresh upload is in the library, not the trash, and is waiting for nothing.
            deleted_at: None,
            pending_replacement: None,
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
        id: &ImageID,
        ext: &str,
    ) -> Result<ReplacementInfo, BoxError> {
        // Read-modify-write of the image record, and the whole of it is under the one lock: an
        // apply that landed in between would otherwise be written back over from the record read
        // before it, naming bytes that apply had already deleted.
        let _guard = self.begin();
        // A fresh file name rather than an overwrite: the browser (and anything in front of it)
        // caches by URL, so bytes that change under one URL keep showing the old image.
        let file_name = match sanitize_ext(ext) {
            Some(ext) => format!("{}.{}", uuid::Uuid::new_v4(), ext),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let upload_key = uuid::Uuid::new_v4().to_string();
        self.register_upload_key(upload_key.clone(), file_name.clone());

        // Recorded on the image itself: the apply is answered by id, and this is what says which
        // upload is *this* image's. A point read in place of a search of every record for one that
        // happens to name the file. Only the file is changed - the rest of the record is carried
        // over from the read that the lock makes current.
        {
            let env = self.rkv.read().map_err(|e| e.to_string())?;
            let store = env.open_single("image", StoreOptions::create())?;
            let reader = env.read()?;
            let mut data = match store.get(&reader, id.to_le_bytes())? {
                Some(Value::Str(s)) => serde_json::from_str::<ImageData>(&s)?,
                _ => return Err("Image not found".into()),
            };
            data.pending_replacement = Some(file_name.clone());
            let mut writer = env.write()?;
            store.put(
                &mut writer,
                id.to_le_bytes(),
                &Value::Str(&serde_json::to_string(&data)?),
            )?;
            writer.commit()?;
        }

        Ok(ReplacementInfo {
            upload_url: format!("/images/{}?key={}", file_name, upload_key),
            file_name,
        })
    }

    async fn image_bytes_exist(&self, file_name: &str) -> Result<bool, BoxError> {
        Ok(self.image_path(file_name)?.is_file())
    }

    async fn replace_image(&self, id: &ImageID, file_name: &str) -> Result<Replacement, BoxError> {
        // Read, decide and write under the one lock: an apply that decided against a record and
        // then waited to write it could take an upload a newer request had already replaced, or
        // forget one recorded in between - and a request that wrote back a record it read before
        // this apply would point the image at bytes this apply deleted.
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

        // Already showing it: nothing to write, and in particular nothing to forget.
        if previous.file_name == file_name {
            return Ok(Replacement::Applied);
        }
        // Only the upload this image was signed is its to take. Anything else - another image's
        // file, or one no request produced - is not, and saying so is this adapter's answer.
        if previous.pending_replacement.as_deref() != Some(file_name) {
            return Ok(Replacement::NotWaiting);
        }

        let updated = ImageData {
            original_filename: previous.original_filename.clone(),
            file_name: file_name.to_string(),
            // The first upload is when the image entered the library; replacing what it shows
            // does not change that, just as re-publishing does not change `published_at`.
            uploaded_at: previous.uploaded_at,
            // Replacing the bytes does not take an image out of the trash, or put it back in.
            deleted_at: previous.deleted_at,
            // And the upload it came from is no longer waiting for anything.
            pending_replacement: None,
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
        Ok(Replacement::Applied)
    }

    async fn image_file_name(&self, id: &ImageID) -> Result<Option<String>, BoxError> {
        Ok(self.get_image_data(id)?.map(|data| data.file_name))
    }

    async fn set_image_references(
        &self,
        owner: &ImageOwner,
        images: &[ImageID],
    ) -> Result<(), BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(IMAGE_REFS_STORE, StoreOptions::create())?;
        let reader = env.read()?;

        // What this owner used before, so the entries that are gone can be removed. Both
        // directions live in the same store: `<owner>|<image>` answers "what does this content
        // use", and `image|<image>|<owner>` answers "what uses this image".
        let owner_prefix = owner_prefix(owner);
        let mut before = Vec::new();
        for result in store.iter_from(&reader, owner_prefix.as_bytes())? {
            let Ok((key, _)) = result else { continue };
            let key = str::from_utf8(&key)?;
            let Some(rest) = key.strip_prefix(&owner_prefix) else {
                break;
            };
            let Some(raw) = rest.strip_prefix("image|") else {
                continue;
            };
            let Ok(id) = raw.parse::<u64>() else { continue };
            before.push(ImageID::from_u64(id));
        }

        let after: std::collections::BTreeSet<u64> = images.iter().map(|id| **id).collect();
        let before_set: std::collections::BTreeSet<u64> = before.iter().map(|id| **id).collect();
        let mut writer = env.write()?;
        for id in before_set.difference(&after) {
            store.delete(
                &mut writer,
                format!("{owner_prefix}image|{id}").as_bytes(),
            )?;
            store.delete(
                &mut writer,
                image_owner_key(&ImageID::from_u64(*id), owner).as_bytes(),
            )?;
        }
        for id in after.difference(&before_set) {
            store.put(
                &mut writer,
                format!("{owner_prefix}image|{id}").as_bytes(),
                &Value::Str(&owner.storage_key()),
            )?;
            store.put(
                &mut writer,
                image_owner_key(&ImageID::from_u64(*id), owner).as_bytes(),
                &Value::Str(&owner.storage_key()),
            )?;
        }
        writer.commit()?;
        Ok(())
    }

    async fn get_image_references(&self, id: &ImageID) -> Result<Vec<ImageOwner>, BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(IMAGE_REFS_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let prefix = image_prefix(id);
        let mut owners = Vec::new();
        for result in store.iter_from(&reader, prefix.as_bytes())? {
            let Ok((key, _)) = result else { continue };
            let key = str::from_utf8(&key)?;
            let Some(rest) = key.strip_prefix(&prefix) else {
                break;
            };
            if let Some(owner) = ImageOwner::from_storage_key(rest) {
                owners.push(owner);
            }
        }
        owners.sort();
        Ok(owners)
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
