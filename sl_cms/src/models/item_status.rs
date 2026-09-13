use chrono::{DateTime, Utc};

/// Whether content is visible through the public delivery API.
///
/// Content starts as a draft: publishing is an explicit act, so nothing reaches a public
/// site merely because an editor saved it.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus {
    #[default]
    Draft,
    Published,
}

/// Metadata about an item that is *not* part of the user-defined schema.
///
/// Kept separate from the item's values so a schema field can be named `status` or
/// `published_at` without colliding, and so nothing has to be migrated when this grows.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Default)]
pub struct ItemMetadata {
    pub status: ItemStatus,
    /// When it was last published; cleared when it is unpublished.
    pub published_at: Option<DateTime<Utc>>,
}

impl ItemMetadata {
    pub fn with_status(status: ItemStatus) -> Self {
        match status {
            ItemStatus::Draft => ItemMetadata::default(),
            ItemStatus::Published => ItemMetadata {
                status: ItemStatus::Published,
                published_at: Some(Utc::now()),
            },
        }
    }

    pub fn is_published(&self) -> bool {
        self.status == ItemStatus::Published
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_starts_as_a_draft() {
        // The default deliberately is not Published: an unset status must never expose
        // content on a public site.
        assert_eq!(ItemStatus::default(), ItemStatus::Draft);
        let metadata = ItemMetadata::default();
        assert!(!metadata.is_published());
        assert!(metadata.published_at.is_none());
    }

    #[test]
    fn publishing_records_when_and_unpublishing_forgets_it() {
        let published = ItemMetadata::with_status(ItemStatus::Published);
        assert!(published.is_published());
        assert!(published.published_at.is_some());

        let unpublished = ItemMetadata::with_status(ItemStatus::Draft);
        assert!(!unpublished.is_published());
        assert!(unpublished.published_at.is_none());
    }

    #[test]
    fn status_serialises_in_lowercase_for_the_api() {
        assert_eq!(serde_json::to_string(&ItemStatus::Published).unwrap(), "\"published\"");
        assert_eq!(serde_json::to_string(&ItemStatus::Draft).unwrap(), "\"draft\"");
    }
}
