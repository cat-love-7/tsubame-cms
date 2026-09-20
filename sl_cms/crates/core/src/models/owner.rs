//! A piece of content, named the way a reference index names it.
//!
//! Both sides of a reference are the same thing - some content: a collection's item, or a single
//! page - and both directions of an index have to say which one. The image index keeps "who uses
//! this image" in this shape; the relation index keeps "who references this item" in it as well,
//! and there the same shape names the *target* too, because a relation points at content.

/// Some content: an item of a collection, or a single page.
///
/// A collection item is named by its collection and its id; a single page is named by the page,
/// because it has exactly one item and an id would say nothing.
#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct ItemOwner {
    pub kind: ItemOwnerKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<u64>,
}

#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ItemOwnerKind {
    CollectionItem,
    SinglePage,
}

impl ItemOwner {
    pub fn collection_item(name: &str, item: u64) -> Self {
        ItemOwner {
            kind: ItemOwnerKind::CollectionItem,
            name: name.to_string(),
            item: Some(item),
        }
    }

    pub fn single_page(name: &str) -> Self {
        ItemOwner {
            kind: ItemOwnerKind::SinglePage,
            name: name.to_string(),
            item: None,
        }
    }

    /// Whether this is a single page, which no collection can be confused with.
    pub fn is_single_page(&self) -> bool {
        self.kind == ItemOwnerKind::SinglePage
    }

    /// What to call this in a sentence somebody reads: "authors item 3", "single page home".
    ///
    /// Refusals name the content they are about, and they have to say it the same way wherever
    /// they come from (see `HttpError::still_referenced` and the relation rules).
    pub fn describe(&self) -> String {
        match self.kind {
            ItemOwnerKind::CollectionItem => {
                format!("{} item {}", self.name, self.item.unwrap_or_default())
            }
            ItemOwnerKind::SinglePage => format!("single page {}", self.name),
        }
    }

    /// The owner as one string, for a key in the index.
    ///
    /// The name is escaped (`:` becomes `%3A`), so a collection named `a:b` cannot produce the
    /// same key as one named `a` with an item called `b`.
    pub fn storage_key(&self) -> String {
        match self.kind {
            ItemOwnerKind::CollectionItem => format!(
                "collection:{}:{}",
                escape(&self.name),
                self.item.unwrap_or_default()
            ),
            ItemOwnerKind::SinglePage => format!("page:{}", escape(&self.name)),
        }
    }

    /// The owner back from [`ItemOwner::storage_key`].
    pub fn from_storage_key(key: &str) -> Option<Self> {
        let mut parts = key.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("collection"), Some(name), Some(item)) => {
                let item = item.parse::<u64>().ok()?;
                Some(ItemOwner::collection_item(&unescape(name), item))
            }
            (Some("page"), Some(name), _) => Some(ItemOwner::single_page(&unescape(name))),
            _ => None,
        }
    }
}

fn escape(name: &str) -> String {
    name.replace('%', "%25").replace(':', "%3A")
}

fn unescape(name: &str) -> String {
    name.replace("%3A", ":").replace("%25", "%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owner_survives_its_storage_key() {
        for owner in [
            ItemOwner::collection_item("blog", 7),
            ItemOwner::single_page("about"),
            // A name that could otherwise be read as two fields of a key.
            ItemOwner::collection_item("a:b", 3),
            ItemOwner::single_page("a:1"),
        ] {
            assert_eq!(
                ItemOwner::from_storage_key(&owner.storage_key()),
                Some(owner.clone()),
                "{}",
                owner.storage_key()
            );
        }
        assert_eq!(ItemOwner::from_storage_key("nonsense"), None);
        // A page is not an item of a collection called `page`.
        assert!(ItemOwner::single_page("home").is_single_page());
        assert!(!ItemOwner::collection_item("home", 1).is_single_page());
    }
}
