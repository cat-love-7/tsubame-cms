use rkv::{StoreOptions, Value};

use crate::models::user::{normalize_email, User, UserId};
use crate::on_premises::repository::Repository;
use crate::repositories::user_repository::{BoxError, UserRepository};

impl Repository {
    /// The lookup itself, without the storage lock.
    ///
    /// `delete_user` already holds the lock and a `std::sync::Mutex` is not reentrant, so
    /// the shared part lives here and `get_user_from_id` is the locking wrapper.
    fn user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("user", StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, user_id.as_bytes())? {
            Some(Value::Str(s)) => Ok(Some(serde_json::from_str(&s)?)),
            _ => Ok(None),
        }
    }
}

impl UserRepository for Repository {
    fn get_user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError> {
        let _guard = self.begin();
        self.user_from_id(user_id)
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
        let _guard = self.begin();
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
        let _guard = self.begin();
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
        let _guard = self.begin();
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
        let _guard = self.begin();
        // rkv reports deleting an absent key as an error, so check first.
        if self.user_from_id(user_id)?.is_none() {
            return Ok(());
        }
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single("user", StoreOptions::create())?;
        let mut writer = env.write()?;
        store.delete(&mut writer, user_id.as_bytes())?;
        writer.commit()?;
        Ok(())
    }

}
