use rkv::{StoreOptions, Value};

use crate::repository::{IDENTITY_STORE, RkvRepository, USER_STORE};
use sl_cms_core::models::user::{User, UserId, normalize_username};
use sl_cms_core::repositories::user_repository::{BoxError, UserRepository};

impl RkvRepository {
    /// The lookup itself, without the storage lock.
    ///
    /// `delete_user` already holds the lock and a `std::sync::Mutex` is not reentrant, so
    /// the shared part lives here and `get_user_from_id` is the locking wrapper.
    fn user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError> {
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(USER_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        match store.get(&reader, user_id.as_bytes())? {
            Some(Value::Str(s)) => {
                let mut user: User = serde_json::from_str(&s)?;
                // A record written when the identifier was the email address still signs in.
                user.adopt_legacy_identifier();
                Ok(Some(user))
            }
            _ => Ok(None),
        }
    }
}

impl UserRepository for RkvRepository {
    async fn get_user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError> {
        let _guard = self.begin();
        self.user_from_id(user_id)
    }

    async fn get_user_from_external_id(&self, external_id: &str) -> Result<Option<User>, BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(IDENTITY_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let id = match store.get(&reader, external_id.as_bytes())? {
            Some(Value::Str(id)) => id.to_string(),
            _ => return Ok(None),
        };
        self.user_from_id(&UserId::from(id.as_str()))
    }

    async fn get_user_from_username(&self, username: &str) -> Result<Option<User>, BoxError> {
        // A linear scan is fine at the scale of a CMS's accounts, and it avoids a second
        // record that would have to be kept in sync with the primary one.
        let wanted = normalize_username(username);
        Ok(self
            .list_users()
            .await?
            .into_iter()
            .map(|(_, user)| user)
            .find(|user| user.username == wanted))
    }

    async fn add_user(&self, user: &User) -> Result<UserId, BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(USER_STORE, StoreOptions::create())?;
        // Opened before the transaction, for the reason given in `delete_collection`.
        let identities = env.open_single(IDENTITY_STORE, StoreOptions::create())?;
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            user.id.as_bytes(),
            &Value::Str(&serde_json::to_string(user)?),
        )?;
        // One transaction with the record: an index that disagreed with it would be a lookup
        // that resolves to the wrong account, or to none.
        if let Some(external_id) = &user.external_id {
            identities.put(
                &mut writer,
                external_id.as_bytes(),
                &Value::Str(user.id.as_str()),
            )?;
        }
        writer.commit()?;
        Ok(user.id.clone())
    }

    async fn update_user(&self, user_id: &UserId, user: &User) -> Result<(), BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(USER_STORE, StoreOptions::create())?;
        let identities = env.open_single(IDENTITY_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let previous = match store.get(&reader, user_id.as_bytes())? {
            Some(Value::Str(s)) => serde_json::from_str::<User>(&s)?.external_id,
            _ => None,
        };
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            user_id.as_bytes(),
            &Value::Str(&serde_json::to_string(user)?),
        )?;
        if let Some(external_id) = &user.external_id {
            identities.put(
                &mut writer,
                external_id.as_bytes(),
                &Value::Str(user.id.as_str()),
            )?;
        }
        // A provider identity the account no longer answers to must not keep resolving to it.
        if let Some(previous) = previous.filter(|id| Some(id) != user.external_id.as_ref()) {
            identities.delete(&mut writer, previous.as_bytes())?;
        }
        writer.commit()?;
        Ok(())
    }

    async fn record_login(
        &self,
        user_id: &UserId,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), BoxError> {
        // Read and write under the one lock: a sign-in must not write back an account an
        // administrator has changed in the meantime (see `UserRepository::record_login`).
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(USER_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let mut user = match store.get(&reader, user_id.as_bytes())? {
            Some(Value::Str(s)) => serde_json::from_str::<User>(&s)?,
            _ => return Err("user not found".into()),
        };
        user.last_login = Some(at);
        let mut writer = env.write()?;
        store.put(
            &mut writer,
            user_id.as_bytes(),
            &Value::Str(&serde_json::to_string(&user)?),
        )?;
        writer.commit()?;
        Ok(())
    }

    async fn list_users(&self) -> Result<Vec<(UserId, User)>, BoxError> {
        let _guard = self.begin();
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(USER_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let mut users = Vec::new();
        for result in store.iter_start(&reader)? {
            if let Ok((key, Value::Str(s))) = result {
                let id = UserId::from(str::from_utf8(&key)?);
                let mut user: User = serde_json::from_str(&s)?;
                user.adopt_legacy_identifier();
                users.push((id, user));
            }
        }
        Ok(users)
    }

    async fn delete_user(&self, user_id: &UserId) -> Result<(), BoxError> {
        let _guard = self.begin();
        // rkv reports deleting an absent key as an error, so check first.
        if self.user_from_id(user_id)?.is_none() {
            return Ok(());
        }
        let env = self.rkv.read().map_err(|e| e.to_string())?;
        let store = env.open_single(USER_STORE, StoreOptions::create())?;
        let identities = env.open_single(IDENTITY_STORE, StoreOptions::create())?;
        let reader = env.read()?;
        let external_id = match store.get(&reader, user_id.as_bytes())? {
            Some(Value::Str(s)) => serde_json::from_str::<User>(&s)?.external_id,
            _ => None,
        };
        let mut writer = env.write()?;
        store.delete(&mut writer, user_id.as_bytes())?;
        if let Some(external_id) = external_id {
            identities.delete(&mut writer, external_id.as_bytes())?;
        }
        writer.commit()?;
        Ok(())
    }
}
