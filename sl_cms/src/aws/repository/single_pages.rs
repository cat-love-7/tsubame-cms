//! Single pages: a schema, a published copy, an unpublished working copy and metadata.
//!
//! A page has no items and no ids, so unlike a collection it needs no counter and no secondary
//! index of item ids: the page name *is* the key, and everything about the page lives in one
//! partition (`pk = page#<name>`), which is also what makes deleting a page one query and a
//! handful of deletes.

use super::*;
use crate::models::item_status::ItemMetadata;
use crate::models::single_page::{SinglePageItem, SinglePageName, SinglePageSchema};
use crate::repositories::single_page_repository::SinglePageRepository;

impl SinglePageRepository for AwsRepository {
    fn get_single_page_schema(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageSchema>, BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        self.runtime.block_on(async move {
            match read(&inner, &key::page(&name), key::SCHEMA).await? {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        })
    }

    fn list_all_page_names(&self) -> Result<Vec<SinglePageName>, BoxError> {
        let inner = self.inner.clone();
        self.runtime.block_on(async move {
            let records = list(&inner, key::PAGE_INDEX, "").await?;
            records
                .into_iter()
                .map(|(sk, _)| Ok(SinglePageName::from(sk.as_str())))
                .collect()
        })
    }

    fn add_single_page_schema(
        &self,
        page_name: &SinglePageName,
        schema: &SinglePageSchema,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let data = AwsRepository::encode(schema)?;
        self.runtime.block_on(async move {
            write(&inner, &key::page(&name), key::SCHEMA, &data).await?;
            // The index entry is what `list_all_page_names` queries; it is written after the
            // schema so a name is never listed before the page exists.
            write(&inner, key::PAGE_INDEX, name.as_str(), name.as_str()).await
        })
    }

    fn delete_single_page(&self, page_name: &SinglePageName) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        self.runtime.block_on(async move {
            let partition = key::page(&name);
            // Schema, published copy, working copy and metadata, in one pass. Any of them may
            // be absent (a page that was never published, or never edited), and deleting an
            // absent key is not an error.
            for (sk, _) in list(&inner, &partition, "").await? {
                remove(&inner, &partition, &sk).await?;
            }
            remove(&inner, key::PAGE_INDEX, name.as_str()).await
        })
    }

    fn get_single_page_item(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageItem>, BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        self.runtime.block_on(async move {
            match read(&inner, &key::page(&name), key::ITEM).await? {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        })
    }

    fn update_single_page_item(
        &self,
        page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let data = AwsRepository::encode(item_data)?;
        self.runtime
            .block_on(async move { write(&inner, &key::page(&name), key::ITEM, &data).await })
    }

    fn get_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<SinglePageItem>, BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        self.runtime.block_on(async move {
            match read(&inner, &key::page(&name), key::DRAFT).await? {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        })
    }

    fn set_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
        item_data: &SinglePageItem,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let data = AwsRepository::encode(item_data)?;
        self.runtime
            .block_on(async move { write(&inner, &key::page(&name), key::DRAFT, &data).await })
    }

    fn delete_single_page_item_draft(
        &self,
        page_name: &SinglePageName,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        self.runtime
            .block_on(async move { remove(&inner, &key::page(&name), key::DRAFT).await })
    }

    fn get_page_metadata(
        &self,
        page_name: &SinglePageName,
    ) -> Result<Option<ItemMetadata>, BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        self.runtime.block_on(async move {
            match read(&inner, &key::page(&name), key::META).await? {
                Some(data) => Ok(Some(AwsRepository::decode(&data)?)),
                None => Ok(None),
            }
        })
    }


    fn set_page_metadata(
        &self,
        page_name: &SinglePageName,
        metadata: &ItemMetadata,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let data = AwsRepository::encode(metadata)?;
        self.runtime
            .block_on(async move { write(&inner, &key::page(&name), key::META, &data).await })
    }

    fn apply_page_status(
        &self,
        page_name: &SinglePageName,
        draft: Option<&SinglePageItem>,
        metadata: &ItemMetadata,
    ) -> Result<(), BoxError> {
        let inner = self.inner.clone();
        let name = page_name.clone();
        let draft = draft.cloned();
        let data = AwsRepository::encode(metadata)?;
        self.runtime.block_on(async move {
            let partition = key::page(&name);
            let Some(draft) = draft else {
                // Nothing pending: only the status changes.
                return write(&inner, &partition, key::META, &data).await;
            };
            let draft_data = AwsRepository::encode(&draft)?;
            // One transaction, for the reason in `CollectionRepository::apply_item_status`.
            transact(
                &inner,
                vec![
                    put_in_transaction(&inner, &partition, key::ITEM, &draft_data)?,
                    delete_in_transaction(&inner, &partition, key::DRAFT)?,
                    put_in_transaction(&inner, &partition, key::META, &data)?,
                ],
            )
            .await
        })
    }
}
