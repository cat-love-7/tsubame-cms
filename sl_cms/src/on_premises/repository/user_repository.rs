use rkv::{StoreOptions, Value};

use crate::models::collection::CollectionName;
use crate::models::single_page::SinglePageName;
use crate::models::user::{normalize_email, Permission, User, UserId};
use crate::on_premises::repository::Repository;
use crate::repositories::user_repository::{BoxError, UserRepository};

impl UserRepository for Repository {
    fn get_user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("user", StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, user_id.as_bytes())? {
            Some(Value::Str(s)) => Ok(Some(serde_json::from_str(&s)?)),
            _ => Ok(None),
        }
    }

    fn get_user_from_email(&self, email: &str) -> Result<Option<User>, BoxError> {
        // A linear scan is fine at the scale of a CMS's accounts, and it avoids a second
        // record that would have to be kept in sync with the primary one.
        let wanted = normalize_email(email);
        Ok(self
            .get_all_users()?
            .into_iter()
            .map(|(_, user)| user)
            .find(|user| user.email == wanted))
    }

    fn add_user(&self, user: &User) -> Result<UserId, BoxError> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("user", StoreOptions::create())?;
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            user.id.as_bytes(),
            &Value::Str(&serde_json::to_string(user)?),
        )?;
        writer.commit()?;
        Ok(user.id.clone())
    }

    fn update_user(&self, user_id: &UserId, user: &User) -> Result<(), BoxError> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("user", StoreOptions::create())?;
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            user_id.as_bytes(),
            &Value::Str(&serde_json::to_string(user)?),
        )?;
        writer.commit()?;
        Ok(())
    }

    fn get_all_users(&self) -> Result<Vec<(UserId, User)>, BoxError> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("user", StoreOptions::create())?;
        let reader = env.read()?;
        let mut users = Vec::new();
        for result in store.iter_start(&reader)? {
            if let Ok((key, Value::Str(s))) = result {
                let id = UserId::from(str::from_utf8(&key)?);
                let user: User = serde_json::from_str(&s)?;
                users.push((id, user));
            }
        }
        Ok(users)
    }

    fn delete_user(&self, user_id: &UserId) -> Result<(), BoxError> {
        // rkv reports deleting an absent key as an error, so check first.
        if self.get_user_from_id(user_id)?.is_none() {
            return Ok(());
        }
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("user", StoreOptions::create())?;
        let mut writer = env.write()?;
        store.delete(&mut writer, user_id.as_bytes())?;
        writer.commit()?;
        Ok(())
    }

    fn get_user_permissions(&self, user_id: &UserId) -> Result<Option<Permission>, BoxError> {
        Ok(self.get_user_from_id(user_id)?.map(|user| user.permission))
    }

    // Per-resource permissions are not stored yet. Rather than inventing a half-model,
    // these report "no specific grant", which lets callers fall back to the account-wide
    // permissions returned by `get_user_permissions`.
    fn get_collection_permissions(
        &self,
        _user_id: &UserId,
        _collection_name: &CollectionName,
    ) -> Result<Option<Permission>, BoxError> {
        Ok(None)
    }

    fn get_single_page_permissions(
        &self,
        _user_id: &UserId,
        _page_name: &SinglePageName,
    ) -> Result<Option<Permission>, BoxError> {
        Ok(None)
    }

    fn get_image_permissions(&self, _user_id: &UserId) -> Result<Option<Permission>, BoxError> {
        Ok(None)
    }
}
