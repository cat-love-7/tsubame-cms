use rkv::backend::{SafeModeDatabase, SafeModeEnvironment};
use rkv::{Rkv, SingleStore, StoreOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

pub mod collections;
pub mod composite_fields;
pub mod credentials;
pub mod images;
pub mod relations;
pub mod single_pages;
pub mod users;

/// Store holding the images: what was uploaded, and the file each record names.
pub(crate) const IMAGE_STORE: &str = "image";

/// Store holding account records (identity and authorisation, not credentials).
pub(crate) const USER_STORE: &str = "user";

/// Store holding a collection's field definitions.
pub(crate) const COLLECTION_SCHEMA_STORE: &str = "collection_schema";

/// Store holding what a collection is told about itself, keyed by collection name.
///
/// Apart from the definition so saving the fields never rewrites the settings, and a collection
/// that has never been given any simply has no record here (see `models::schema::SchemaSettings`).
pub(crate) const COLLECTION_SETTINGS_STORE: &str = "collection_settings";

/// Store holding reusable field groups, keyed by their id.
pub(crate) const COMPOSITE_FIELD_SCHEMA_STORE: &str = "composite_field_schema";

/// Store holding a single page's field definition.
pub(crate) const SINGLE_PAGE_SCHEMA_STORE: &str = "single_page_schema";

/// Store holding what a single page is told about itself, keyed by page name.
pub(crate) const SINGLE_PAGE_SETTINGS_STORE: &str = "single_page_settings";

/// Store holding a single page's published content.
pub(crate) const SINGLE_PAGE_ITEM_STORE: &str = "single_page_item";

/// Store holding the per-partition id counters (`ADD` in DynamoDB, a counter here).
pub(crate) const COUNTER_STORE: &str = "id_counter";

/// Store holding draft/published metadata, keyed per item.
///
/// Kept apart from the item's values so nothing has to be migrated when this grows, and
/// so a schema field may be named `status` without colliding.
pub(crate) const METADATA_STORE: &str = "item_metadata";

/// Store holding an item's working copy, until it is published.
///
/// The item store holds what the delivery API serves; editors save here instead, so a save
/// can never change the live site.
pub(crate) const DRAFT_STORE: &str = "item_draft";

/// Store holding the local password of an account, keyed by user id.
///
/// Deliberately not part of the user record: the record is identity and authorisation, and a
/// credential that is nowhere near it cannot leak through a response, a log line or a debug
/// print (see `crate::credentials`).
pub(crate) const CREDENTIAL_STORE: &str = "credential";

/// Store holding the provider-identifier index: an identity provider's `sub` to our user id.
///
/// A lookup by provider identity happens on every request that arrives with a provider's
/// token, so it is an index rather than a scan.
pub(crate) const IDENTITY_STORE: &str = "identity";

/// Store holding the unique-value index: `collection:field:value` to the item that holds it.
///
/// A field declared unique is checked on every save, so the check has to be a point read. The
/// key holds the value rather than the item, which is what makes two saves of one value settle
/// into one winner under the storage lock this adapter already takes.
pub(crate) const UNIQUE_STORE: &str = "unique";

/// Index key for one unique value: which collection and field it belongs to, and the value.
///
/// `\u{1f}` as the separator: a field name or a value may contain most characters, and this one
/// cannot appear in a field name at all, so two different pairs cannot build the same key.
pub(crate) fn unique_key(
    collection_name: &str,
    unique: &sl_cms_core::repositories::collection_repository::UniqueValue,
) -> String {
    format!(
        "unique:{}\u{1f}{}\u{1f}{}",
        collection_name, unique.field, unique.value
    )
}

/// The side stores (metadata, draft) key an item as `collection:<name>:<id>`, so
/// everything belonging to one collection can be scanned and purged by prefix.
fn collection_prefix(collection_name: &str) -> String {
    format!("collection:{collection_name}:")
}

/// Metadata key for one item of a collection.
pub(crate) fn collection_item_metadata_key(collection_name: &str, item_id: u64) -> String {
    format!("{}{item_id}", collection_prefix(collection_name))
}

/// Prefix shared by every item of one collection, so a range scan can find them.
pub(crate) fn collection_metadata_prefix(collection_name: &str) -> String {
    collection_prefix(collection_name)
}

/// Working-copy key for one item of a collection.
pub(crate) fn collection_item_draft_key(collection_name: &str, item_id: u64) -> String {
    format!("{}{item_id}", collection_prefix(collection_name))
}

/// The same span as [`collection_metadata_prefix`], in the draft store.
pub(crate) fn collection_draft_prefix(collection_name: &str) -> String {
    collection_prefix(collection_name)
}

/// Metadata key for a single page.
pub(crate) fn page_metadata_key(page_name: &str) -> String {
    format!("page:{page_name}")
}

/// Working-copy key for a single page.
pub(crate) fn page_draft_key(page_name: &str) -> String {
    format!("page:{page_name}")
}

pub struct RkvRepository {
    pub rkv: Arc<RwLock<Rkv<SafeModeEnvironment>>>,
    pub counter_store: SingleStore<SafeModeDatabase>,

    /// Held for the whole of every storage operation.
    ///
    /// LMDB refuses `mdb_dbi_open` — which rkv performs on every `open_single` — while a
    /// transaction is active, and this adapter opens its stores per operation rather than
    /// caching handles. Two requests in flight therefore used to make one of them fail with
    /// "attempted to open DB during transaction"; the admin UI does exactly that, loading
    /// the item window, the statuses and the navigation at once.
    ///
    /// Serialising costs nothing at this adapter's scale: the operations last microseconds
    /// and LMDB takes a single writer regardless. The AWS adapter is the one meant to scale
    /// out, and it does not share this constraint.
    ops: Mutex<()>,

    /// One-shot upload tokens: `token -> authorised file name`.
    ///
    /// This deliberately no longer lives in the HTTP layer, and storing the file name
    /// (rather than just presence) stops a token issued for one upload from being
    /// replayed against a different file name.
    upload_keys: Arc<RwLock<HashMap<String, String>>>,

    /// Directory holding uploaded image bytes (derived from `DATA_ROOT`).
    images_dir: PathBuf,
}

impl RkvRepository {
    pub fn new(rkv: Arc<RwLock<Rkv<SafeModeEnvironment>>>, images_dir: PathBuf) -> Self {
        let binding = Arc::clone(&rkv);
        let env = binding.read().unwrap();
        let counter_store = env
            .open_single(COUNTER_STORE, StoreOptions::create())
            .unwrap();
        RkvRepository {
            rkv,
            counter_store,
            ops: Mutex::new(()),
            upload_keys: Arc::new(RwLock::new(HashMap::new())),
            images_dir,
        }
    }

    /// Take the storage lock for the duration of one operation.
    ///
    /// A panic while holding it poisons the mutex; recovering instead of propagating keeps
    /// a single bad request from turning every later request into a 500.
    pub(crate) fn begin(&self) -> MutexGuard<'_, ()> {
        self.ops
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn images_dir(&self) -> &Path {
        &self.images_dir
    }

    pub(crate) fn register_upload_key(&self, key: String, file_name: String) {
        self.upload_keys.write().unwrap().insert(key, file_name);
    }

    pub(crate) fn consume_upload_key(&self, key: &str) -> Option<String> {
        self.upload_keys.write().unwrap().remove(key)
    }
}
