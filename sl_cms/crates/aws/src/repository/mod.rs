//! The DynamoDB side of the AWS adapter: one table, keys laid out as in
//! `doc/aws-dynamodb-design.md`, values stored as JSON strings.
//!
//! Everything here is `async`, which is what the SDK is and what the traits now are.

use std::sync::Arc;

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::error::{ProvideErrorMetadata, SdkError};

use crate::settings::AwsSettings;
use sl_cms_core::models::collection::{CollectionItem, CollectionItemId, CollectionName, CollectionSchema};
use sl_cms_core::models::item_status::ItemMetadata;
use sl_cms_core::repositories::collection_repository::{
    ApplyStatusError, CollectionRepository, Reservation, UniqueValue, canonical_draft,
};
use sl_cms_core::repositories::image_repository::BoxError;

mod composite_fields;
mod images;
mod single_pages;
mod users;

/// How item ids are written into the sort key. DynamoDB sorts keys as bytes, so without padding
/// `10` would come before `2`.
const ID_WIDTH: usize = 20;

pub fn padded(id: u64) -> String {
    format!("{id:0width$}", width = ID_WIDTH)
}

/// One table, one bucket.
pub struct AwsRepository {
    inner: Arc<Inner>,
}

pub struct Inner {
    client: Client,
    table: String,
    /// The image bytes live in S3; the records that describe them live in the table.
    s3: aws_sdk_s3::Client,
    settings: AwsSettings,
}

impl AwsRepository {
    /// Connect to the table and the bucket `settings` names.
    pub async fn connect(settings: &AwsSettings) -> Self {
        AwsRepository {
            inner: Arc::new(Inner {
                client: crate::dynamodb_client(settings).await,
                table: settings.table.clone(),
                s3: crate::s3_client(settings).await,
                settings: settings.clone(),
            }),
        }
    }

    /// Create the table this repository expects.
    pub async fn create_table(&self) -> Result<(), BoxError> {
        self.inner
            .client
            .create_table()
            .set_billing_mode(Some(aws_sdk_dynamodb::types::BillingMode::PayPerRequest))
            .table_name(&self.inner.table)
            .attribute_definitions(
                aws_sdk_dynamodb::types::AttributeDefinition::builder()
                    .attribute_name("pk")
                    .attribute_type(aws_sdk_dynamodb::types::ScalarAttributeType::S)
                    .build()?,
            )
            .attribute_definitions(
                aws_sdk_dynamodb::types::AttributeDefinition::builder()
                    .attribute_name("sk")
                    .attribute_type(aws_sdk_dynamodb::types::ScalarAttributeType::S)
                    .build()?,
            )
            .key_schema(
                aws_sdk_dynamodb::types::KeySchemaElement::builder()
                    .attribute_name("pk")
                    .key_type(aws_sdk_dynamodb::types::KeyType::Hash)
                    .build()?,
            )
            .key_schema(
                aws_sdk_dynamodb::types::KeySchemaElement::builder()
                    .attribute_name("sk")
                    .key_type(aws_sdk_dynamodb::types::KeyType::Range)
                    .build()?,
            )
            .send()
            .await
            .map_err(|e| format!("could not create the table: {}", describe(&e)))?;
        Ok(())
    }

    /// Create the table unless it is already there.
    ///
    /// For a local run, where the alternative is an operator hand-creating a table before the
    /// server will start. A deployment gets its table from Terraform, and this is not called
    /// there: quietly creating one would turn a misconfigured table name into a working server
    /// with an empty database.
    pub async fn ensure_table(&self) -> Result<bool, BoxError> {
        let exists = self
            .inner
            .client
            .describe_table()
            .table_name(&self.inner.table)
            .send()
            .await
            .is_ok();
        if exists {
            return Ok(false);
        }
        self.create_table().await?;
        Ok(true)
    }

    pub async fn delete_table(&self) -> Result<(), BoxError> {
        drop_table(&self.inner).await
    }

    fn encode<T: serde::Serialize>(value: &T) -> Result<String, BoxError> {
        Ok(serde_json::to_string(value)?)
    }

    fn decode<T: serde::de::DeserializeOwned>(data: &str) -> Result<T, BoxError> {
        Ok(serde_json::from_str(data)?)
    }
}

// ---- the async implementation the synchronous traits delegate to -------------------------

/// What DynamoDB actually said.
///
/// `SdkError`'s `Display` collapses to "service error", which is useless in a log: the code and
/// message it sent are the interesting part.
fn describe<E, R>(error: &SdkError<E, R>) -> String
where
    E: ProvideErrorMetadata + std::fmt::Debug,
    R: std::fmt::Debug,
{
    match error.as_service_error() {
        Some(service) => format!(
            "{}: {}",
            service.code().unwrap_or("unknown code"),
            service.message().unwrap_or("no message")
        ),
        None => format!("{error:?}"),
    }
}

/// One record, or `None`.
async fn read(inner: &Inner, pk: &str, sk: &str) -> Result<Option<String>, BoxError> {
    let answer = inner
        .client
        .get_item()
        .table_name(&inner.table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S(sk.to_string()))
        // A CMS saves and then looks at what it saved, so reads are consistent.
        .consistent_read(true)
        .send()
        .await
        .map_err(|e| format!("dynamodb get_item failed: {}", describe(&e)))?;
    match answer.item {
        Some(item) => Ok(item.get("data").and_then(|v| v.as_s().ok()).cloned()),
        None => Ok(None),
    }
}

async fn write(inner: &Inner, pk: &str, sk: &str, data: &str) -> Result<(), BoxError> {
    inner
        .client
        .put_item()
        .table_name(&inner.table)
        .item("pk", AttributeValue::S(pk.to_string()))
        .item("sk", AttributeValue::S(sk.to_string()))
        .item("data", AttributeValue::S(data.to_string()))
        .send()
        .await
        .map_err(|e| format!("dynamodb put_item failed: {}", describe(&e)))?;
    Ok(())
}

/// Write a record, but only while it is still the record the caller read.
///
/// `expected` is the stored JSON, so the condition covers the whole record: an image record is one
/// blob, and "change only this one field" is not something the table can express. `false` is not an
/// error - it means somebody wrote in between, and the caller decides what that means.
async fn write_if_unchanged(
    inner: &Inner,
    pk: &str,
    sk: &str,
    expected: &str,
    data: &str,
) -> Result<bool, BoxError> {
    let answer = inner
        .client
        .put_item()
        .table_name(&inner.table)
        .item("pk", AttributeValue::S(pk.to_string()))
        .item("sk", AttributeValue::S(sk.to_string()))
        .item("data", AttributeValue::S(data.to_string()))
        .condition_expression("#data = :expected")
        .expression_attribute_names("#data", "data")
        .expression_attribute_values(":expected", AttributeValue::S(expected.to_string()))
        .send()
        .await;
    match answer {
        Ok(_) => Ok(true),
        Err(e)
            if e.as_service_error().and_then(|e| e.code())
                == Some("ConditionalCheckFailedException") =>
        {
            Ok(false)
        }
        Err(e) => Err(format!("dynamodb put_item failed: {}", describe(&e)).into()),
    }
}

async fn remove(inner: &Inner, pk: &str, sk: &str) -> Result<(), BoxError> {
    inner
        .client
        .delete_item()
        .table_name(&inner.table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S(sk.to_string()))
        .send()
        .await
        .map_err(|e| format!("dynamodb delete_item failed: {}", describe(&e)))?;
    Ok(())
}

/// How many `Query` round trips the adapter has made.
///
/// A DynamoDB query stops at 1MB, so a long list arrives in pages; a test that wants to know it
/// really did cross that boundary has nothing else to look at.
#[cfg(test)]
pub static QUERY_ROUND_TRIPS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Every record under `pk` whose sort key starts with `prefix`, in key order.
async fn list(inner: &Inner, pk: &str, prefix: &str) -> Result<Vec<(String, String)>, BoxError> {
    let mut found = Vec::new();
    let mut start_key = None;
    loop {
        #[cfg(test)]
        QUERY_ROUND_TRIPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut request = inner
            .client
            .query()
            .table_name(&inner.table)
            .expression_attribute_names("#pk", "pk")
            .expression_attribute_values(":pk", AttributeValue::S(pk.to_string()))
            .consistent_read(true);
        // DynamoDB refuses an empty string as a key value, so "everything under this
        // partition" is expressed by leaving the sort key out of the condition entirely.
        if prefix.is_empty() {
            request = request.key_condition_expression("#pk = :pk");
        } else {
            request = request
                .key_condition_expression("#pk = :pk AND begins_with(#sk, :prefix)")
                .expression_attribute_names("#sk", "sk")
                .expression_attribute_values(":prefix", AttributeValue::S(prefix.to_string()));
        }
        if let Some(key) = start_key.take() {
            request = request.set_exclusive_start_key(Some(key));
        }
        let answer = request
            .send()
            .await
            .map_err(|e| format!("dynamodb query failed: {e:?}"))?;
        for item in answer.items() {
            let sk = item.get("sk").and_then(|v| v.as_s().ok()).cloned().unwrap_or_default();
            let data = item.get("data").and_then(|v| v.as_s().ok()).cloned().unwrap_or_default();
            found.push((sk, data));
        }
        // A query stops at 1MB; the contract is "everything", so keep going.
        start_key = answer.last_evaluated_key().cloned();
        if start_key.is_none() {
            return Ok(found);
        }
    }
}

/// Whether something is listening on `endpoint`'s host and port.
///
/// The adapter tests need an emulator, but a machine (or a CI runner) that has not started
/// `docker compose` should still see a green suite, so a test asks this first and skips itself.
/// Deliberately crude: one TCP connect, no protocol.
#[cfg(test)]
pub fn emulator_reachable(endpoint: &str) -> bool {
    let authority = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let Some(host_port) = authority.split('/').next() else {
        return false;
    };
    let socket = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{host_port}:80")
    };
    let Ok(mut addresses) = std::net::ToSocketAddrs::to_socket_addrs(&socket) else {
        return false;
    };
    let Some(address) = addresses.next() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(500)).is_ok()
}

/// One `Put` inside a `TransactWriteItems`, which is how a status change stays all-or-nothing.
fn put_in_transaction(
    inner: &Inner,
    pk: &str,
    sk: &str,
    data: &str,
) -> Result<aws_sdk_dynamodb::types::TransactWriteItem, BoxError> {
    Ok(aws_sdk_dynamodb::types::TransactWriteItem::builder()
        .put(
            aws_sdk_dynamodb::types::Put::builder()
                .table_name(&inner.table)
                .item("pk", AttributeValue::S(pk.to_string()))
                .item("sk", AttributeValue::S(sk.to_string()))
                .item("data", AttributeValue::S(data.to_string()))
                .build()?,
        )
        .build())
}

/// One `Delete` inside a `TransactWriteItems`, refused unless the record is still what the caller
/// read.
///
/// Used to promote a working copy: the delete is the part that throws work away, so it is the part
/// that has to be conditional. A record written before the canonical rendering existed (in hash
/// order) does not match, which refuses the publish once - the next save stores it canonically.
fn delete_draft_in_transaction(
    inner: &Inner,
    pk: &str,
    sk: &str,
    expected: &str,
) -> Result<aws_sdk_dynamodb::types::TransactWriteItem, BoxError> {
    Ok(aws_sdk_dynamodb::types::TransactWriteItem::builder()
        .delete(
            aws_sdk_dynamodb::types::Delete::builder()
                .table_name(&inner.table)
                .key("pk", AttributeValue::S(pk.to_string()))
                .key("sk", AttributeValue::S(sk.to_string()))
                .condition_expression("#data = :expected")
                .expression_attribute_names("#data", "data")
                .expression_attribute_values(
                    ":expected",
                    AttributeValue::S(expected.to_string()),
                )
                .build()?,
        )
        .build())
}

/// What applying a transaction did.
enum TransactOutcome {
    /// Every write landed.
    Applied,
    /// A write was refused by its condition, and nothing landed. The index is the position in the
    /// list of writes, which is how DynamoDB reports it (`CancellationReasons` lines up with the
    /// request).
    Refused { write: usize },
}

/// Apply `writes` as one transaction: all of them, or none.
async fn transact(
    inner: &Inner,
    writes: Vec<aws_sdk_dynamodb::types::TransactWriteItem>,
) -> Result<TransactOutcome, BoxError> {
    match inner
        .client
        .transact_write_items()
        .set_transact_items(Some(writes))
        .send()
        .await
    {
        Ok(_) => Ok(TransactOutcome::Applied),
        Err(e) => {
            // A condition that does not hold cancels the transaction, and the caller usually wants
            // to know *which* condition rather than only that something was refused.
            let refused = match e.as_service_error() {
                Some(
                    aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError::TransactionCanceledException(
                        cancelled,
                    ),
                ) => cancelled
                    .cancellation_reasons()
                    .iter()
                    .position(|reason| reason.code() == Some("ConditionalCheckFailed")),
                _ => None,
            };
            match refused {
                Some(write) => Ok(TransactOutcome::Refused { write }),
                None => Err(format!("dynamodb transact_write_items failed: {}", describe(&e)).into()),
            }
        }
    }
}

/// Drop the table behind a repository.
async fn drop_table(inner: &Inner) -> Result<(), BoxError> {
    inner
        .client
        .delete_table()
        .table_name(&inner.table)
        .send()
        .await
        .map_err(|e| format!("could not delete the table: {}", describe(&e)))?;
    Ok(())
}

/// Allocate the next id for `pk` with an atomic `ADD`, so concurrent writers cannot share one.
async fn next_id(inner: &Inner, pk: &str) -> Result<u64, BoxError> {
    let answer = inner
        .client
        .update_item()
        .table_name(&inner.table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S(key::COUNTER.to_string()))
        .update_expression("ADD #value :one")
        .expression_attribute_names("#value", "value")
        .expression_attribute_values(":one", AttributeValue::N("1".to_string()))
        .return_values(aws_sdk_dynamodb::types::ReturnValue::UpdatedNew)
        .send()
        .await
        .map_err(|e| format!("dynamodb update_item failed: {}", describe(&e)))?;
    let value = answer
        .attributes()
        .and_then(|attributes| attributes.get("value"))
        .and_then(|value| value.as_n().ok())
        .ok_or("dynamodb did not return the new counter value")?;
    value
        .parse::<u64>()
        .map_err(|e| format!("dynamodb returned a counter that is not a number: {e}").into())
}

/// Keys, in one place so a typo cannot make two features disagree.
pub mod key {
    use super::padded;
    use sl_cms_core::models::collection::CollectionName;
    use sl_cms_core::models::schema::CompositeFieldId;
    use sl_cms_core::models::single_page::SinglePageName;
    use sl_cms_core::models::user::UserId;

    pub fn collection(name: &CollectionName) -> String {
        format!("collection#{}", name.as_str())
    }
    /// Where the unique values of one field live: the value is the sort key, so claiming one is
    /// a point read and a conditional write, and no scan is involved.
    pub fn unique(collection_name: &str, field: &str) -> String {
        format!("unique#{collection_name}#{field}")
    }
    pub fn item(id: u64) -> String {
        format!("item#{}", padded(id))
    }
    pub fn draft(id: u64) -> String {
        format!("draft#{}", padded(id))
    }
    pub fn metadata(id: u64) -> String {
        format!("meta#{}", padded(id))
    }
    pub const COUNTER: &str = "counter";
    pub const SCHEMA: &str = "schema";
    /// Where the names of every collection are listed, so listing them is a query.
    pub const COLLECTION_INDEX: &str = "collections";

    // Single pages: one partition per page, because a page is schema + published copy +
    // working copy + metadata, and they are always read and deleted together.
    pub fn page(name: &SinglePageName) -> String {
        format!("page#{}", name.as_str())
    }
    /// Where the page names are listed, so listing them is a query.
    pub const PAGE_INDEX: &str = "pages";

    // Accounts. The record is keyed by id; the username lives in a reservation record, so a
    // name is looked up with a point read instead of a scan, and two accounts cannot share one.
    pub fn user(id: &UserId) -> String {
        format!("user#{}", id.as_str())
    }
    pub const USER_INDEX: &str = "users";
    pub fn username(username: &str) -> String {
        format!("username#{}", username)
    }
    /// The same reservation, for the identifier an identity provider knows an account by.
    pub fn external_id(external_id: &str) -> String {
        format!("external_id#{external_id}")
    }

    // Composite field schemas, listed in one query.
    pub fn composite_field(id: &CompositeFieldId) -> String {
        format!("composite#{}", id.as_str())
    }
    pub const COMPOSITE_FIELD_INDEX: &str = "composite_fields";

    // Images: the record under the index partition, the bytes in S3 under `file_name`.
    pub fn image(id: u64) -> String {
        format!("image#{}", padded(id))
    }
    pub const IMAGE_INDEX: &str = "images";

    /// The sort key of a record that is alone in its partition (an account, a composite field).
    pub const RECORD: &str = "record";
    /// The sort keys inside a page partition.
    pub const ITEM: &str = "item";
    pub const DRAFT: &str = "draft";
    pub const META: &str = "meta";
}

impl CollectionRepository for AwsRepository {
    async fn get_collection_schema(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Option<CollectionSchema>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        match read(&inner, &key::collection(&name), key::SCHEMA).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn list_collection_names(&self) -> Result<Vec<CollectionName>, BoxError> {
        let inner = self.inner.clone();
        let records = list(&inner, key::COLLECTION_INDEX, "").await?;
        records
            .into_iter()
            .map(|(sk, _)| Ok(CollectionName::from(sk.as_str())))
            .collect()
    }

    async fn add_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let data = AwsRepository::encode(schema)?;
        write(&inner, &key::collection(&name), key::SCHEMA, &data).await?;
        write(&inner, key::COLLECTION_INDEX, name.as_str(), name.as_str()).await
    }

    async fn delete_collection(&self, collection_name: &CollectionName) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        for (sk, _) in list(&inner, &partition, "").await? {
            remove(&inner, &partition, &sk).await?;
        }
        remove(&inner, key::COLLECTION_INDEX, name.as_str()).await
    }

    async fn list_collection_items(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, sl_cms_core::models::collection::CollectionItem)>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        let mut items = Vec::new();
        for (sk, data) in list(&inner, &partition, "item#").await? {
            let id = sk.trim_start_matches("item#").parse::<u64>()?;
            items.push((CollectionItemId::from_u64(id), AwsRepository::decode(&data)?));
        }
        Ok(items)
    }

    async fn get_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<sl_cms_core::models::collection::CollectionItem>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        match read(&inner, &key::collection(&name), &key::item(id)).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn add_collection_item(
        &self,
        collection_name: &CollectionName,
        item_data: &sl_cms_core::models::collection::CollectionItem,
    ) -> Result<u64, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let data = AwsRepository::encode(item_data)?;
        let partition = key::collection(&name);
        let id = next_id(&inner, &partition).await?;
        write(&inner, &partition, &key::item(id), &data).await?;
        Ok(id)
    }

    async fn update_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        item_data: &sl_cms_core::models::collection::CollectionItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let data = AwsRepository::encode(item_data)?;
        write(&inner, &key::collection(&name), &key::item(id), &data).await
    }

    async fn delete_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let partition = key::collection(&name);
        // The published copy, the working copy and the metadata go together: removing one
        // and leaving the others would show up as a half-deleted item.
        remove(&inner, &partition, &key::item(id)).await?;
        remove(&inner, &partition, &key::draft(id)).await?;
        remove(&inner, &partition, &key::metadata(id)).await
    }

    async fn get_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<sl_cms_core::models::collection::CollectionItem>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        match read(&inner, &key::collection(&name), &key::draft(id)).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn set_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        item_data: &sl_cms_core::models::collection::CollectionItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        // The canonical rendering, so a promotion can compare the record with what the publisher
        // read (see `apply_item_status`).
        let data = canonical_draft(item_data);
        write(&inner, &key::collection(&name), &key::draft(id), &data).await
    }

    async fn delete_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        remove(&inner, &key::collection(&name), &key::draft(id)).await
    }

    async fn list_collection_item_drafts(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, sl_cms_core::models::collection::CollectionItem)>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        let mut drafts = Vec::new();
        for (sk, data) in list(&inner, &partition, "draft#").await? {
            let id = sk.trim_start_matches("draft#").parse::<u64>()?;
            drafts.push((CollectionItemId::from_u64(id), AwsRepository::decode(&data)?));
        }
        Ok(drafts)
    }

    async fn get_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<ItemMetadata>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        match read(&inner, &key::collection(&name), &key::metadata(id)).await? {
            Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
            None => Ok(None),
        }
    }

    async fn set_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        metadata: &ItemMetadata,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let data = AwsRepository::encode(metadata)?;
        write(&inner, &key::collection(&name), &key::metadata(id), &data).await
    }

    async fn list_published_items_page(
        &self,
        collection_name: &CollectionName,
        offset: usize,
        limit: Option<usize>,
    ) -> Result<
        (Vec<(CollectionItemId, CollectionItem, ItemMetadata)>, usize),
        BoxError,
    > {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        // Walk the statuses, not the content. A metadata record is a few hundred bytes and
        // the item behind it is not, so a page of a 10k-item collection reads 10k small
        // records and the content of one page — instead of everything.
        let mut total = 0usize;
        let mut window = Vec::new();
        let mut start_key = None;
        loop {
            let mut request = inner
                .client
                .query()
                .table_name(&inner.table)
                .key_condition_expression("#pk = :pk AND begins_with(#sk, :prefix)")
                .expression_attribute_names("#pk", "pk")
                .expression_attribute_names("#sk", "sk")
                .expression_attribute_values(":pk", AttributeValue::S(partition.clone()))
                .expression_attribute_values(":prefix", AttributeValue::S("meta#".to_string()))
                .consistent_read(true);
            if let Some(key) = start_key.take() {
                request = request.set_exclusive_start_key(Some(key));
            }
            let answer = request
                .send()
                .await
                .map_err(|e| format!("dynamodb query failed: {}", describe(&e)))?;
            for record in answer.items() {
                let sk = record
                    .get("sk")
                    .and_then(|value| value.as_s().ok())
                    .cloned()
                    .unwrap_or_default();
                let data = record
                    .get("data")
                    .and_then(|value| value.as_s().ok())
                    .cloned()
                    .unwrap_or_default();
                let metadata: ItemMetadata = AwsRepository::decode(&data)?;
                if !metadata.is_published() {
                    continue;
                }
                total += 1;
                if total <= offset || window.len() >= limit.unwrap_or(usize::MAX) {
                    // Past the window: still counted, but its content is not read.
                    continue;
                }
                let id = sk.trim_start_matches("meta#").parse::<u64>()?;
                match read(&inner, &partition, &key::item(id)).await? {
                    Some(item) => window.push((
                        CollectionItemId::from_u64(id),
                        AwsRepository::decode(&item)?,
                        metadata,
                    )),
                    // A status with no content behind it: counting it would make `total` a
                    // number the pages cannot add up to.
                    None => total -= 1,
                }
            }
            start_key = answer.last_evaluated_key().cloned();
            if start_key.is_none() {
                break;
            }
        }
        Ok((window, total))
    }

    async fn reserve_unique_value(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        unique: &UniqueValue,
    ) -> Result<Reservation, BoxError> {
        let inner = self.inner.clone();
        let collection = collection_name.clone();
        let unique = unique.clone();
        let id = item_id.clone();
        let partition = key::unique(collection.as_str(), &unique.field);
        // One write, and what it means depends on what was there: creating the entry is a claim
        // this caller can give back, while finding it already ours is not. The write is conditional
        // on the entry being *absent*, so a success is unambiguous, and a refusal is followed by a
        // read that says whether it is ours, somebody else's, or gone.
        //
        // Gone is possible - the holder released it between the write and the read - and is not a
        // refusal at all, so the write is tried again rather than reporting a claim that was never
        // made. (A loop rather than a recursion: the window is a round trip wide, and after a
        // handful of attempts something is wrong a caller should hear about.)
        for _ in 0..RESERVE_ATTEMPTS {
            let answer = inner
                .client
                .put_item()
                .table_name(&inner.table)
                .item("pk", AttributeValue::S(partition.clone()))
                .item("sk", AttributeValue::S(unique.value.clone()))
                .item("data", AttributeValue::S(id.to_string()))
                .condition_expression("attribute_not_exists(pk)")
                .send()
                .await;
            match answer {
                Ok(_) => return Ok(Reservation::Claimed),
                Err(e)
                    if e.as_service_error().and_then(|e| e.code())
                        == Some("ConditionalCheckFailedException") =>
                {
                    match read(&inner, &partition, &unique.value).await? {
                        // Ours already: not a conflict with itself, and nothing to give back.
                        Some(owner) if owner == id.to_string() => {
                            return Ok(Reservation::AlreadyHeld)
                        }
                        // Say who holds it, so the refusal can name the item rather than only the
                        // value.
                        Some(owner) => {
                            return Ok(Reservation::Taken {
                                owner: CollectionItemId::from_u64(owner.parse()?),
                            })
                        }
                        None => continue,
                    }
                }
                Err(e) => return Err(format!("dynamodb put_item failed: {}", describe(&e)).into()),
            }
        }
        Err(format!(
            "the unique index for {} kept changing under {} after {RESERVE_ATTEMPTS} attempts",
            unique.field, unique.value
        )
        .into())
    }

    async fn find_unique_value(
        &self,
        collection_name: &CollectionName,
        unique: &UniqueValue,
    ) -> Result<Option<CollectionItemId>, BoxError> {
        let inner = self.inner.clone();
        let partition = key::unique(collection_name.as_str(), &unique.field);
        match read(&inner, &partition, &unique.value).await? {
            Some(owner) => Ok(Some(CollectionItemId::from_u64(owner.parse()?))),
            None => Ok(None),
        }
    }

    async fn release_unique_value(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        unique: &UniqueValue,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let partition = key::unique(collection_name.as_str(), &unique.field);
        // Only while it is still ours: the value may have been claimed by another item since
        // this one stopped holding it, and deleting that claim would hand it to a third.
        let answer = inner
            .client
            .delete_item()
            .table_name(&inner.table)
            .key("pk", AttributeValue::S(partition))
            .key("sk", AttributeValue::S(unique.value.clone()))
            .condition_expression("#data = :id")
            .expression_attribute_names("#data", "data")
            .expression_attribute_values(":id", AttributeValue::S(item_id.to_string()))
            .send()
            .await;
        match answer {
            Ok(_) => Ok(()),
            // Not ours any more, or already gone: nothing to release.
            Err(e)
                if e.as_service_error().and_then(|e| e.code())
                    == Some("ConditionalCheckFailedException") =>
            {
                Ok(())
            }
            Err(e) => Err(format!("dynamodb delete_item failed: {}", describe(&e)).into()),
        }
    }

    async fn apply_item_status(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        draft: Option<&CollectionItem>,
        metadata: &ItemMetadata,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = item_id.clone();
        let draft = draft.cloned();
        let data = AwsRepository::encode(metadata)?;
        let partition = key::collection(&name);
        let metadata_key = key::metadata(*id);
        let Some(draft) = draft else {
            // Nothing pending: only the status changes, and one write is already atomic.
            return write(&inner, &partition, &metadata_key, &data).await;
        };
        let draft_data = AwsRepository::encode(&draft)?;
        // One transaction. A published copy that exists while its working copy is still
        // there, or a status naming content that has not landed, is a CMS that disagrees
        // with itself — the delivery API reads the copy and the admin list reads the
        // status. DynamoDB's transaction costs two writes per item, which is the price of
        // never showing that state.
        // The draft is stored in its canonical rendering, which is what makes this condition a
        // content comparison rather than a byte comparison: a save that landed after the caller
        // read the working copy changed that rendering, and publishing the older content would
        // delete the newer save.
        let expected = canonical_draft(&draft);
        let outcome = transact(
            &inner,
            vec![
                put_in_transaction(&inner, &partition, &key::item(*id), &draft_data)?,
                delete_draft_in_transaction(&inner, &partition, &key::draft(*id), &expected)?,
                put_in_transaction(&inner, &partition, &metadata_key, &data)?,
            ],
        )
        .await?;
        match outcome {
            TransactOutcome::Applied => Ok(()),
            // Position 1 is the draft delete above.
            TransactOutcome::Refused { write: 1 } => {
                Err(Box::new(ApplyStatusError::DraftChanged))
            }
            TransactOutcome::Refused { write } => Err(format!(
                "publishing item {id} was refused by write {write}, which has no condition"
            )
            .into()),
        }
    }

    async fn list_item_metadata(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, ItemMetadata)>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let partition = key::collection(&name);
        let mut metadata = Vec::new();
        for (sk, data) in list(&inner, &partition, "meta#").await? {
            let id = sk.trim_start_matches("meta#").parse::<u64>()?;
            metadata.push((CollectionItemId::from_u64(id), AwsRepository::decode(&data)?));
        }
        Ok(metadata)
    }
}

/// How many times a unique reservation retries the conditional write before giving up.
///
/// Each retry means the value was released between a refused write and the read that followed it,
/// which is a round trip wide; more than a handful in a row is a sign of something else.
const RESERVE_ATTEMPTS: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;
    use sl_cms_core::models::collection::{CollectionItem, CollectionSchema};
    use sl_cms_core::models::item_status::ItemStatus;

    fn schema() -> CollectionSchema {
        serde_json::from_value(serde_json::json!([
            { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 }
        ]))
        .expect("a schema")
    }

    fn values(title: &str) -> CollectionItem {
        serde_json::from_value::<CollectionItem>(serde_json::json!({ "title": { "Text": title } }))
            .expect("values")
    }

    /// The key layout and the DynamoDB calls, against a local emulator.
    ///
    /// `#[tokio::test]` is the point: these are the synchronous trait methods, called from
    /// inside a runtime, which is exactly where a naive `block_on` would panic.
    #[tokio::test]
    async fn a_collection_round_trips_against_dynamodb_local() {
        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository("cms_test").await.expect("a table for this test");
        let name = CollectionName::from("blog");

        // Schema, and the index that makes listing them a query.
        assert!(repository.get_collection_schema(&name).await.unwrap().is_none());
        repository.add_collection_schema(&name, &schema()).await.unwrap();
        assert_eq!(repository.get_collection_schema(&name).await.unwrap(), Some(schema()));
        assert_eq!(repository.list_collection_names().await.unwrap(), vec![name.clone()]);

        // Item ids come from the atomic counter: 1, then 2.
        let first = repository.add_collection_item(&name, &values("First")).await.unwrap();
        let second = repository.add_collection_item(&name, &values("Second")).await.unwrap();
        assert_eq!((first, second), (1, 2));
        assert_eq!(repository.get_collection_item(&name, &CollectionItemId::from_u64(1)).await.unwrap(), Some(values("First")));
        assert_eq!(repository.list_collection_items(&name).await.unwrap().len(), 2);

        // The working copy is separate from the published one.
        repository.set_collection_item_draft(&name, &CollectionItemId::from_u64(1), &values("Draft wording")).await.unwrap();
        assert_eq!(
            repository.get_collection_item_draft(&name, &CollectionItemId::from_u64(1)).await.unwrap(),
            Some(values("Draft wording"))
        );
        assert_eq!(repository.list_collection_item_drafts(&name).await.unwrap().len(), 1);
        // ...and the published copy still says what it said.
        assert_eq!(repository.get_collection_item(&name, &CollectionItemId::from_u64(1)).await.unwrap(), Some(values("First")));

        // Metadata, including the status the admin list reads.
        let metadata = ItemMetadata {
            status: ItemStatus::Published,
            published_at: Some(chrono::Utc::now()),
            ..ItemMetadata::default()
        };
        repository.set_item_metadata(&name, &CollectionItemId::from_u64(1), &metadata).await.unwrap();
        assert_eq!(
            repository.get_item_metadata(&name, &CollectionItemId::from_u64(1)).await.unwrap(),
            Some(metadata)
        );
        assert_eq!(repository.list_item_metadata(&name).await.unwrap().len(), 1);

        // Deleting an item takes all three records with it.
        repository.delete_collection_item(&name, &CollectionItemId::from_u64(1)).await.unwrap();
        assert!(repository.get_collection_item(&name, &CollectionItemId::from_u64(1)).await.unwrap().is_none());
        assert!(repository.get_collection_item_draft(&name, &CollectionItemId::from_u64(1)).await.unwrap().is_none());
        assert!(repository.get_item_metadata(&name, &CollectionItemId::from_u64(1)).await.unwrap().is_none());

        // Deleting a collection takes its items and its index entry.
        repository.delete_collection(&name).await.unwrap();
        assert!(repository.get_collection_schema(&name).await.unwrap().is_none());
        assert!(repository.list_collection_names().await.unwrap().is_empty());
        assert!(repository.list_collection_items(&name).await.unwrap().is_empty());

        repository.delete_table().await.unwrap();
    }

    /// A write is refused when the record changed under the caller that is writing it.
    ///
    /// This is what keeps a replacement request from writing the record it read back over an apply
    /// that landed in between: the apply has already deleted the bytes the stale record names, so
    /// restoring it would leave the image serving nothing.
    #[tokio::test]
    async fn a_write_from_a_stale_read_is_refused() {
        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository("cms_stale").await.expect("a table for this test");
        let inner = repository.inner.clone();
        let (pk, sk) = (key::IMAGE_INDEX, key::image(1));

        write(&inner, pk, &sk, "{\"file_name\":\"old.png\"}").await.unwrap();
        let stale = read(&inner, pk, &sk).await.unwrap().expect("the record");

        // Somebody else moves the record on - an apply, say - and this writer then writes back what
        // it read.
        write(&inner, pk, &sk, "{\"file_name\":\"new.png\"}").await.unwrap();
        assert!(
            !write_if_unchanged(&inner, pk, &sk, &stale, "{\"file_name\":\"old.png\"}")
                .await
                .unwrap(),
            "a write from a stale read should be refused"
        );
        assert_eq!(
            read(&inner, pk, &sk).await.unwrap().as_deref(),
            Some("{\"file_name\":\"new.png\"}"),
            "the refusal should leave the newer record alone"
        );

        // The same write lands while the record is still the one that was read.
        let current = read(&inner, pk, &sk).await.unwrap().expect("the record");
        assert!(
            write_if_unchanged(&inner, pk, &sk, &current, "{\"file_name\":\"new.png\",\"pending\":\"next\"}")
                .await
                .unwrap(),
            "a write from the current record should land"
        );
        assert_eq!(
            read(&inner, pk, &sk).await.unwrap().as_deref(),
            Some("{\"file_name\":\"new.png\",\"pending\":\"next\"}")
        );

        repository.delete_table().await.unwrap();
    }

    /// The unique index against the emulator: what is free is claimed, what is taken names its
    /// holder, a value the item already holds is not a conflict with itself - and an answer of
    /// "held" means the index really holds it, which is what a release between two calls used to
    /// break (the value came back as held without ever being written).
    #[tokio::test]
    async fn a_unique_value_is_claimed_by_one_item_at_a_time() {
        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository("cms_test")
            .await
            .expect("a table for this test");
        let name = CollectionName::from("unique_claims");
        let value = |slug: &str| UniqueValue {
            field: "slug".to_string(),
            value: slug.to_string(),
        };
        let one = CollectionItemId::from_u64(1);
        let two = CollectionItemId::from_u64(2);

        // Free: claimed, and the index says so.
        assert_eq!(
            repository
                .reserve_unique_value(&name, &one, &value("intro"))
                .await
                .unwrap(),
            Reservation::Claimed
        );
        assert_eq!(
            repository.find_unique_value(&name, &value("intro")).await.unwrap(),
            Some(one)
        );

        // Ours already: not a conflict with itself, and nothing for a caller to give back.
        assert_eq!(
            repository
                .reserve_unique_value(&name, &one, &value("intro"))
                .await
                .unwrap(),
            Reservation::AlreadyHeld
        );

        // Taken: the answer names the item, so a refusal can too.
        assert_eq!(
            repository
                .reserve_unique_value(&name, &two, &value("intro"))
                .await
                .unwrap(),
            Reservation::Taken { owner: one }
        );

        // Released: free again, and the next claim really lands.
        repository
            .release_unique_value(&name, &one, &value("intro"))
            .await
            .unwrap();
        assert_eq!(repository.find_unique_value(&name, &value("intro")).await.unwrap(), None);
        assert_eq!(
            repository
                .reserve_unique_value(&name, &two, &value("intro"))
                .await
                .unwrap(),
            Reservation::Claimed
        );
        assert_eq!(
            repository.find_unique_value(&name, &value("intro")).await.unwrap(),
            Some(two)
        );

        // Releasing a value that is no longer ours leaves the holder alone.
        repository
            .release_unique_value(&name, &one, &value("intro"))
            .await
            .unwrap();
        assert_eq!(
            repository.find_unique_value(&name, &value("intro")).await.unwrap(),
            Some(CollectionItemId::from_u64(2))
        );
    }

    /// The id counter is an atomic `ADD`, which is the whole reason it is not a read-modify-write
    /// against the item records: two items can be created at the same moment in different
    /// requests, and neither may lose.
    // Several worker threads, because the point of the test is that two *concurrent* creates
    // cannot be handed the same id: on a single-threaded runtime the tasks would take turns.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_creates_do_not_share_an_id() {
        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository("cms_concurrent").await.expect("a table for this test");
        let name = CollectionName::from("blog");
        repository.add_collection_schema(&name, &schema()).await.unwrap();

        let writers = 8;
        let each = 5;
        let mut handles = Vec::new();
        for _ in 0..writers {
            let repository = repository.clone();
            let name = name.clone();
            handles.push(tokio::spawn(async move {
                let mut ids = Vec::new();
                for _ in 0..each {
                    ids.push(repository.add_collection_item(&name, &values("x")).await.unwrap());
                }
                ids
            }));
        }
        let mut ids: Vec<u64> = Vec::new();
        for handle in handles {
            ids.extend(handle.await.expect("a writer task panicked"));
        }
        ids.sort_unstable();

        assert_eq!(
            ids,
            (1..=(writers * each)).collect::<Vec<u64>>(),
            "an id was handed out twice, or one was skipped"
        );
        assert_eq!(
            repository.list_collection_items(&name).await.unwrap().len(),
            (writers * each) as usize
        );

        repository.delete_table().await.unwrap();
    }

    /// A record is one DynamoDB item, and DynamoDB refuses anything over 400KB, so the CMS has to
    /// stay under it. Image bytes are in S3, which is what keeps that reachable: a value that is
    /// merely large is fine, one that is over the limit is refused rather than silently truncated.
    #[tokio::test]
    async fn a_large_value_round_trips_and_an_oversized_one_is_refused() {
        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository("cms_size").await.expect("a table for this test");
        let name = CollectionName::from("blog");
        repository.add_collection_schema(&name, &schema()).await.unwrap();

        let large = "a".repeat(300_000);
        let id = repository.add_collection_item(&name, &values(&large)).await.unwrap();
        assert_eq!(
            repository
                .get_collection_item(&name, &CollectionItemId::from_u64(id))
                .await.unwrap(),
            Some(values(&large)),
            "a 300KB value should survive the round trip"
        );

        let oversized = "a".repeat(500_000);
        assert!(
            repository.add_collection_item(&name, &values(&oversized)).await.is_err(),
            "a record over DynamoDB's 400KB limit has to be refused"
        );

        repository.delete_table().await.unwrap();
    }

    /// Publishing touches three records, and a failure has to leave all three as they were.
    ///
    /// DynamoDB refuses an item over 400KB, so an oversized working copy is a transaction that
    /// cannot commit — and a version of this that wrote the records one by one would leave the
    /// working copy deleted and the old content published behind it, which is exactly the
    /// half-applied state the transaction is there to prevent.
    #[tokio::test]
    async fn a_failed_publish_leaves_every_record_alone() {
        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository("cms_publish").await.expect("a table for this test");
        let name = CollectionName::from("blog");
        repository.add_collection_schema(&name, &schema()).await.unwrap();
        let id = CollectionItemId::from_u64(
            repository.add_collection_item(&name, &values("Published text")).await.unwrap(),
        );
        repository
            .set_collection_item_draft(&name, &id, &values("Draft text"))
            .await.unwrap();
        assert!(
            !repository.get_item_metadata(&name, &id).await.unwrap().unwrap_or_default().is_published(),
            "the item starts unpublished, so publishing has something to change"
        );

        let published = ItemMetadata {
            status: ItemStatus::Published,
            published_at: Some(chrono::Utc::now()),
            ..ItemMetadata::default()
        };
        let oversized = "a".repeat(500_000);
        let refused = repository
            .apply_item_status(&name, &id, Some(&values(&oversized)), &published)
            .await;
        assert!(refused.is_err(), "a working copy over 400KB cannot be published");

        // Nothing moved: not the published copy, not the working copy, not the status.
        assert_eq!(
            repository.get_collection_item(&name, &id).await.unwrap(),
            Some(values("Published text")),
            "the published copy changed even though the publish failed"
        );
        assert_eq!(
            repository.get_collection_item_draft(&name, &id).await.unwrap(),
            Some(values("Draft text")),
            "the working copy was consumed by a publish that failed"
        );
        assert!(
            !repository.get_item_metadata(&name, &id).await.unwrap().unwrap_or_default().is_published(),
            "the status says published although nothing was"
        );

        // And the same call with a working copy that fits does all three at once.
        repository
            .apply_item_status(&name, &id, Some(&values("Draft text")), &published)
            .await.expect("a publish that fits");
        assert_eq!(
            repository.get_collection_item(&name, &id).await.unwrap(),
            Some(values("Draft text")),
            "the working copy should have replaced the published one"
        );
        assert!(repository.get_collection_item_draft(&name, &id).await.unwrap().is_none());
        assert!(repository.get_item_metadata(&name, &id).await.unwrap().unwrap().is_published());

        repository.delete_table().await.unwrap();
    }

    /// A DynamoDB query stops at 1MB, so a long list arrives in pages and the adapter has to
    /// follow them to the end. 120 items of 10KB are comfortably past that; every id has to come
    /// back exactly once, and the round-trip count proves the boundary was actually crossed
    /// (otherwise the test would pass without testing anything).
    #[tokio::test]
    async fn a_list_longer_than_one_query_page_is_read_to_the_end() {
        let endpoint = crate::test_endpoint();
        if !emulator_reachable(&endpoint) {
            eprintln!("skipped: no DynamoDB at {endpoint} (start it with `docker compose up -d`)");
            return;
        }
        let (repository, _table) = crate::open_test_repository("cms_onemb").await.expect("a table for this test");
        let name = CollectionName::from("blog");
        repository.add_collection_schema(&name, &schema()).await.unwrap();

        let items = 120;
        let value = "a".repeat(10_000);
        for _ in 0..items {
            repository.add_collection_item(&name, &values(&value)).await.unwrap();
        }

        let before = QUERY_ROUND_TRIPS.load(std::sync::atomic::Ordering::Relaxed);
        let read = repository.list_collection_items(&name).await.unwrap();
        let round_trips = QUERY_ROUND_TRIPS.load(std::sync::atomic::Ordering::Relaxed) - before;

        assert_eq!(read.len(), items, "a page boundary dropped items");
        assert!(
            round_trips > 1,
            "the list fitted in one query page, so the boundary was never crossed"
        );

        repository.delete_table().await.unwrap();
    }
}

