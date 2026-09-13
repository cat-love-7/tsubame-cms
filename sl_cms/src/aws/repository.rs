//! The DynamoDB side of the AWS adapter: one table, keys laid out as in
//! `doc/aws-dynamodb-design.md`, values stored as JSON strings.
//!
//! Everything meaningful here is `async` (the AWS SDK is). The synchronous repository traits
//! are implemented further down by handing each call to [`BlockingRuntime`] - that is the
//! temporary part; the async methods are the implementation.

use std::sync::Arc;

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::error::{ProvideErrorMetadata, SdkError};

use crate::aws::bridge::BlockingRuntime;
use crate::models::collection::{CollectionItemId, CollectionName, CollectionSchema};
use crate::models::item_status::ItemMetadata;
use crate::repositories::collection_repository::CollectionRepository;
use crate::repositories::image_repository::BoxError;

/// How item ids are written into the sort key. DynamoDB sorts keys as bytes, so without padding
/// `10` would come before `2`.
const ID_WIDTH: usize = 20;

pub fn padded(id: u64) -> String {
    format!("{id:0width$}", width = ID_WIDTH)
}

/// One table, one client.
pub struct AwsRepository {
    inner: Arc<Inner>,
    runtime: BlockingRuntime,
}

pub struct Inner {
    client: Client,
    table: String,
}

impl AwsRepository {
    pub fn new(client: Client, table: String) -> Self {
        AwsRepository {
            inner: Arc::new(Inner { client, table }),
            runtime: BlockingRuntime::new(),
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

    pub async fn delete_table(&self) -> Result<(), BoxError> {
        self.inner
            .client
            .delete_table()
            .table_name(&self.inner.table)
            .send()
            .await
            .map_err(|e| format!("could not delete the table: {}", describe(&e)))?;
        Ok(())
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

/// Every record under `pk` whose sort key starts with `prefix`, in key order.
async fn list(inner: &Inner, pk: &str, prefix: &str) -> Result<Vec<(String, String)>, BoxError> {
    let mut found = Vec::new();
    let mut start_key = None;
    loop {
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

/// Allocate the next id for `pk` with an atomic `ADD`, so concurrent writers cannot share one.
async fn next_id(inner: &Inner, pk: &str) -> Result<u64, BoxError> {
    let answer = inner
        .client
        .update_item()
        .table_name(&inner.table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S("counter".to_string()))
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
    use crate::models::collection::CollectionName;

    pub fn collection(name: &CollectionName) -> String {
        format!("collection#{}", name.as_str())
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
}

impl CollectionRepository for AwsRepository {
    fn get_collection_schema(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Option<CollectionSchema>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        self.runtime.block_on(async move {
            match read(&inner, &key::collection(&name), key::SCHEMA).await? {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        })
    }

    fn list_collection_names(&self) -> Result<Vec<CollectionName>, BoxError> {
        let inner = self.inner.clone();
        self.runtime.block_on(async move {
            let records = list(&inner, key::COLLECTION_INDEX, "").await?;
            records
                .into_iter()
                .map(|(sk, _)| Ok(CollectionName::from(sk.as_str())))
                .collect()
        })
    }

    fn add_collection_schema(
        &self,
        collection_name: &CollectionName,
        schema: &CollectionSchema,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let data = AwsRepository::encode(schema)?;
        self.runtime.block_on(async move {
            write(&inner, &key::collection(&name), key::SCHEMA, &data).await?;
            write(&inner, key::COLLECTION_INDEX, name.as_str(), name.as_str()).await
        })
    }

    fn delete_collection(&self, collection_name: &CollectionName) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        self.runtime.block_on(async move {
            let partition = key::collection(&name);
            for (sk, _) in list(&inner, &partition, "").await? {
                remove(&inner, &partition, &sk).await?;
            }
            remove(&inner, key::COLLECTION_INDEX, name.as_str()).await
        })
    }

    fn list_collection_items(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, crate::models::collection::CollectionItem)>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        self.runtime.block_on(async move {
            let partition = key::collection(&name);
            let mut items = Vec::new();
            for (sk, data) in list(&inner, &partition, "item#").await? {
                let id = sk.trim_start_matches("item#").parse::<u64>()?;
                items.push((CollectionItemId::from_u64(id), AwsRepository::decode(&data)?));
            }
            Ok(items)
        })
    }

    fn get_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<crate::models::collection::CollectionItem>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        self.runtime.block_on(async move {
            match read(&inner, &key::collection(&name), &key::item(id)).await? {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        })
    }

    fn add_collection_item(
        &self,
        collection_name: &CollectionName,
        item_data: &crate::models::collection::CollectionItem,
    ) -> Result<u64, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let data = AwsRepository::encode(item_data)?;
        self.runtime.block_on(async move {
            let partition = key::collection(&name);
            let id = next_id(&inner, &partition).await?;
            write(&inner, &partition, &key::item(id), &data).await?;
            Ok(id)
        })
    }

    fn update_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        item_data: &crate::models::collection::CollectionItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let data = AwsRepository::encode(item_data)?;
        self.runtime.block_on(async move {
            write(&inner, &key::collection(&name), &key::item(id), &data).await
        })
    }

    fn delete_collection_item(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        self.runtime.block_on(async move {
            let partition = key::collection(&name);
            // The published copy, the working copy and the metadata go together: removing one
            // and leaving the others would show up as a half-deleted item.
            remove(&inner, &partition, &key::item(id)).await?;
            remove(&inner, &partition, &key::draft(id)).await?;
            remove(&inner, &partition, &key::metadata(id)).await
        })
    }

    fn get_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<crate::models::collection::CollectionItem>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        self.runtime.block_on(async move {
            match read(&inner, &key::collection(&name), &key::draft(id)).await? {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        })
    }

    fn set_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        item_data: &crate::models::collection::CollectionItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let data = AwsRepository::encode(item_data)?;
        self.runtime.block_on(async move {
            write(&inner, &key::collection(&name), &key::draft(id), &data).await
        })
    }

    fn delete_collection_item_draft(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        self.runtime.block_on(async move {
            remove(&inner, &key::collection(&name), &key::draft(id)).await
        })
    }

    fn list_collection_item_drafts(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, crate::models::collection::CollectionItem)>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        self.runtime.block_on(async move {
            let partition = key::collection(&name);
            let mut drafts = Vec::new();
            for (sk, data) in list(&inner, &partition, "draft#").await? {
                let id = sk.trim_start_matches("draft#").parse::<u64>()?;
                drafts.push((CollectionItemId::from_u64(id), AwsRepository::decode(&data)?));
            }
            Ok(drafts)
        })
    }

    fn get_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
    ) -> Result<Option<ItemMetadata>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        self.runtime.block_on(async move {
            match read(&inner, &key::collection(&name), &key::metadata(id)).await? {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        })
    }

    fn set_item_metadata(
        &self,
        collection_name: &CollectionName,
        item_id: &CollectionItemId,
        metadata: &ItemMetadata,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        let id = **item_id;
        let data = AwsRepository::encode(metadata)?;
        self.runtime.block_on(async move {
            write(&inner, &key::collection(&name), &key::metadata(id), &data).await
        })
    }

    fn list_item_metadata(
        &self,
        collection_name: &CollectionName,
    ) -> Result<Vec<(CollectionItemId, ItemMetadata)>, BoxError> {
        let inner = self.inner.clone();
        let name = collection_name.clone();
        self.runtime.block_on(async move {
            let partition = key::collection(&name);
            let mut metadata = Vec::new();
            for (sk, data) in list(&inner, &partition, "meta#").await? {
                let id = sk.trim_start_matches("meta#").parse::<u64>()?;
                metadata.push((CollectionItemId::from_u64(id), AwsRepository::decode(&data)?));
            }
            Ok(metadata)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::collection::{CollectionItem, CollectionSchema};
    use crate::models::item_status::ItemStatus;

    /// `Some` when something is listening on the emulator's endpoint, so the suite still passes
    /// on a machine (or a CI runner) that has not started `docker compose`.
    fn reachable_emulator() -> Option<String> {
        let endpoint = crate::aws::test_endpoint();
        let authority = endpoint.trim_start_matches("http://").trim_start_matches("https://");
        let host_port = authority.split('/').next()?;
        let socket = if host_port.contains(':') {
            host_port.to_string()
        } else {
            format!("{host_port}:80")
        };
        let address = std::net::ToSocketAddrs::to_socket_addrs(&socket).ok()?.next()?;
        std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(500))
            .ok()
            .map(|_| endpoint)
    }

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

    /// The bridge, the key layout and the DynamoDB calls, against a local emulator.
    ///
    /// `#[tokio::test]` is the point: these are the synchronous trait methods, called from
    /// inside a runtime, which is exactly where a naive `block_on` would panic.
    #[tokio::test]
    async fn a_collection_round_trips_against_dynamodb_local() {
        if reachable_emulator().is_none() {
            eprintln!(
                "skipped: no DynamoDB at {} (start it with `docker compose up -d`)",
                crate::aws::test_endpoint()
            );
            return;
        }
        let (repository, _table) = crate::aws::open_test_repository("cms_test").await;
        let name = CollectionName::from("blog");

        // Schema, and the index that makes listing them a query.
        assert!(repository.get_collection_schema(&name).unwrap().is_none());
        repository.add_collection_schema(&name, &schema()).unwrap();
        assert_eq!(repository.get_collection_schema(&name).unwrap(), Some(schema()));
        assert_eq!(repository.list_collection_names().unwrap(), vec![name.clone()]);

        // Item ids come from the atomic counter: 1, then 2.
        let first = repository.add_collection_item(&name, &values("First")).unwrap();
        let second = repository.add_collection_item(&name, &values("Second")).unwrap();
        assert_eq!((first, second), (1, 2));
        assert_eq!(repository.get_collection_item(&name, &CollectionItemId::from_u64(1)).unwrap(), Some(values("First")));
        assert_eq!(repository.list_collection_items(&name).unwrap().len(), 2);

        // The working copy is separate from the published one.
        repository.set_collection_item_draft(&name, &CollectionItemId::from_u64(1), &values("Draft wording")).unwrap();
        assert_eq!(
            repository.get_collection_item_draft(&name, &CollectionItemId::from_u64(1)).unwrap(),
            Some(values("Draft wording"))
        );
        assert_eq!(repository.list_collection_item_drafts(&name).unwrap().len(), 1);
        // ...and the published copy still says what it said.
        assert_eq!(repository.get_collection_item(&name, &CollectionItemId::from_u64(1)).unwrap(), Some(values("First")));

        // Metadata, including the status the admin list reads.
        let mut metadata = ItemMetadata::default();
        metadata.status = ItemStatus::Published;
        metadata.published_at = Some(chrono::Utc::now());
        repository.set_item_metadata(&name, &CollectionItemId::from_u64(1), &metadata).unwrap();
        assert_eq!(
            repository.get_item_metadata(&name, &CollectionItemId::from_u64(1)).unwrap(),
            Some(metadata)
        );
        assert_eq!(repository.list_item_metadata(&name).unwrap().len(), 1);

        // Deleting an item takes all three records with it.
        repository.delete_collection_item(&name, &CollectionItemId::from_u64(1)).unwrap();
        assert!(repository.get_collection_item(&name, &CollectionItemId::from_u64(1)).unwrap().is_none());
        assert!(repository.get_collection_item_draft(&name, &CollectionItemId::from_u64(1)).unwrap().is_none());
        assert!(repository.get_item_metadata(&name, &CollectionItemId::from_u64(1)).unwrap().is_none());

        // Deleting a collection takes its items and its index entry.
        repository.delete_collection(&name).unwrap();
        assert!(repository.get_collection_schema(&name).unwrap().is_none());
        assert!(repository.list_collection_names().unwrap().is_empty());
        assert!(repository.list_collection_items(&name).unwrap().is_empty());

        repository.delete_table().await.unwrap();
    }
}
