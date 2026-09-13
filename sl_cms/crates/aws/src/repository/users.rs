//! Accounts: the record itself, plus a reservation per username.
//!
//! The reservation is what makes a username unique. Without it, `get_user_from_username` would
//! have to scan every account, and two administrators creating "ops" at the same moment would
//! both succeed. The reservation is a separate record (`pk = users`, `sk = username#<name>`)
//! holding the id that owns the name, so a sign-in is one point read, and a conditional write
//! settles the race.

use super::*;
use sl_cms_core::models::user::{normalize_username, User, UserId};
use sl_cms_core::repositories::user_repository::UserRepository;

impl AwsRepository {
    /// A stored account, with the legacy-identifier repair applied, so a record written when
    /// the identifier *was* the email address still signs in.
    fn decode_user(data: &str) -> Result<User, BoxError> {
        let mut user: User = AwsRepository::decode(data)?;
        user.adopt_legacy_identifier();
        Ok(user)
    }
}

/// Claim `username` for `id`, or fail.
///
/// The condition is what makes this safe: `attribute_not_exists(pk)` turns a second claim on
/// the same name into a `ConditionalCheckFailedException` rather than a silent takeover.
async fn reserve_username(inner: &Inner, username: &str, id: &UserId) -> Result<(), BoxError> {
    let username = normalize_username(username);
    inner
        .client
        .put_item()
        .table_name(&inner.table)
        .item("pk", AttributeValue::S(key::USER_INDEX.to_string()))
        .item("sk", AttributeValue::S(key::username(&username)))
        .item("data", AttributeValue::S(id.as_str().to_string()))
        .condition_expression("attribute_not_exists(pk)")
        .send()
        .await
        .map_err(|e| -> BoxError {
            if e.as_service_error().and_then(|e| e.code()) == Some("ConditionalCheckFailedException")
            {
                format!("the username {username:?} is already taken").into()
            } else {
                format!("dynamodb put_item failed: {}", describe(&e)).into()
            }
        })?;
    Ok(())
}

/// Give up a name, but only if it is still ours: another account may have taken it in the
/// meantime, and deleting that account's reservation would let a third account take a name
/// that is in use.
async fn release_username(inner: &Inner, username: &str, id: &UserId) -> Result<(), BoxError> {
    let username = normalize_username(username);
    let answer = inner
        .client
        .delete_item()
        .table_name(&inner.table)
        .key("pk", AttributeValue::S(key::USER_INDEX.to_string()))
        .key("sk", AttributeValue::S(key::username(&username)))
        .condition_expression("attribute_not_exists(pk) OR #data = :id")
        .expression_attribute_names("#data", "data")
        .expression_attribute_values(":id", AttributeValue::S(id.as_str().to_string()))
        .send()
        .await;
    match answer {
        Ok(_) => Ok(()),
        // Someone else holds the name now, so there is nothing of ours to release.
        Err(e)
            if e.as_service_error().and_then(|e| e.code())
                == Some("ConditionalCheckFailedException") =>
        {
            Ok(())
        }
        Err(e) => Err(format!("dynamodb delete_item failed: {}", describe(&e)).into()),
    }
}

impl UserRepository for AwsRepository {
    async fn get_user_from_id(&self, user_id: &UserId) -> Result<Option<User>, BoxError> {
        let inner = self.inner.clone();
        let id = user_id.clone();
        match read(&inner, &key::user(&id), key::RECORD).await? {
            Some(data) => Ok(Some(AwsRepository::decode_user(&data)?)),
            None => Ok(None),
        }
    }

    async fn get_user_from_username(&self, username: &str) -> Result<Option<User>, BoxError> {
        let inner = self.inner.clone();
        let username = normalize_username(username);
        let Some(id) = read(&inner, key::USER_INDEX, &key::username(&username)).await? else {
            return Ok(None);
        };
        let id = UserId::from(id.as_str());
        match read(&inner, &key::user(&id), key::RECORD).await? {
            Some(data) => Ok(Some(AwsRepository::decode_user(&data)?)),
            // A reservation with no record behind it: the account was deleted while this
            // read was in flight. The name is not usable by anyone else either, so report
            // it as free rather than as an account with nothing in it.
            None => Ok(None),
        }
    }

    async fn add_user(&self, user: &User) -> Result<UserId, BoxError> {
        let inner = self.inner.clone();
        let user = user.clone();
        reserve_username(&inner, &user.username, &user.id).await?;
        write(
            &inner,
            &key::user(&user.id),
            key::RECORD,
            &AwsRepository::encode(&user)?,
        )
        .await?;
        Ok(user.id)
    }

    async fn update_user(&self, user_id: &UserId, user: &User) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let id = user_id.clone();
        let user = user.clone();
        let previous = match read(&inner, &key::user(&id), key::RECORD).await? {
            Some(data) => Some(AwsRepository::decode_user(&data)?.username),
            None => None,
        };
        if previous.as_deref() != Some(user.username.as_str()) {
            reserve_username(&inner, &user.username, &id).await?;
            if let Some(previous) = previous {
                release_username(&inner, &previous, &id).await?;
            }
        }
        write(
            &inner,
            &key::user(&id),
            key::RECORD,
            &AwsRepository::encode(&user)?,
        )
        .await
    }

    async fn get_all_users(&self) -> Result<Vec<(UserId, User)>, BoxError> {
        let inner = self.inner.clone();
        // Accounts are few and listed rarely, and the reservation list holds every one of
        // them (it is written in the same breath as the record), so a point read each is
        // honest and keeps a single copy of the authoritative record.
        let mut users = Vec::new();
        for (_sk, id) in list(&inner, key::USER_INDEX, "username#").await? {
            let id = UserId::from(id.as_str());
            if let Some(data) = read(&inner, &key::user(&id), key::RECORD).await? {
                users.push((id, AwsRepository::decode_user(&data)?));
            }
        }
        Ok(users)
    }

    async fn delete_user(&self, user_id: &UserId) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let id = user_id.clone();
        if let Some(data) = read(&inner, &key::user(&id), key::RECORD).await? {
            let user = AwsRepository::decode_user(&data)?;
            release_username(&inner, &user.username, &id).await?;
        }
        // Deleting an absent key is not an error in DynamoDB, which is the behaviour the
        // on-premises adapter goes out of its way to reproduce.
        remove(&inner, &key::user(&id), key::RECORD).await
    }
}
