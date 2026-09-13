use chrono::{DateTime, Utc};

use crate::models::user::{User, UserId};

/// Who published an item, captured at the moment it was published.
///
/// The id links back to the account while it exists; the email is kept as it was, so the
/// record still reads after the account has been renamed or deleted. That is what makes it
/// usable as an audit trail rather than just a foreign key.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PublishedBy {
    pub id: UserId,
    pub email: String,
}

impl From<&User> for PublishedBy {
    fn from(user: &User) -> Self {
        PublishedBy {
            id: user.id.clone(),
            email: user.email.clone(),
        }
    }
}

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
    /// When the values were first saved.
    ///
    /// Optional because content that predates this field has no record of it, and because
    /// a single page that was never saved has none either. `#[serde(default)]` is what
    /// lets a metadata record written before timestamps existed still be read.
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    /// When the values were last saved.
    ///
    /// Tracks **content edits only**: publishing does not change it (that is what
    /// `published_at` is for), so a build can tell whether the content itself moved on
    /// since the last one.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// Who published it last; cleared when it is unpublished, like `published_at`.
    ///
    /// Optional for the same reason as the timestamps: a record written before this field
    /// existed still has to be readable.
    #[serde(default)]
    pub published_by: Option<PublishedBy>,
}

impl ItemMetadata {
    /// Change the published state, leaving the content timestamps alone.
    ///
    /// Publishing is not an edit, and it must not erase when the content was written: a
    /// record that was never published has no timestamps to lose, but an existing one
    /// does. The publisher is only meaningful while the item is published, so
    /// unpublishing forgets it just as it forgets `published_at`.
    pub fn with_status(&self, status: ItemStatus, actor: Option<PublishedBy>) -> Self {
        let (published_at, published_by) = match status {
            ItemStatus::Published => (Some(Utc::now()), actor),
            ItemStatus::Draft => (None, None),
        };
        ItemMetadata {
            status,
            published_at,
            published_by,
            ..self.clone()
        }
    }

    /// Record that the values were just saved, leaving the status untouched.
    ///
    /// Saving a published item must not unpublish it, and re-saving must not move
    /// `created_at`.
    pub fn touched(&self, now: DateTime<Utc>) -> Self {
        ItemMetadata {
            created_at: self.created_at.or(Some(now)),
            updated_at: Some(now),
            ..self.clone()
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
        let published = ItemMetadata::default().with_status(ItemStatus::Published, None);
        assert!(published.is_published());
        assert!(published.published_at.is_some());

        let unpublished = published.with_status(ItemStatus::Draft, None);
        assert!(!unpublished.is_published());
        assert!(unpublished.published_at.is_none());
    }

    /// The audit trail: the publisher is stored with the item, and unpublishing - which
    /// takes the item off the site - forgets it along with the publication time.
    #[test]
    fn publishing_records_who_did_it_and_unpublishing_forgets_that_too() {
        let admin = User::new(
            "Admin@Example.com",
            "hash".to_string(),
            true,
            crate::models::user::Permission::admin(),
        );
        let published = ItemMetadata::default()
            .with_status(ItemStatus::Published, Some(PublishedBy::from(&admin)));

        let publisher = published.published_by.clone().expect("記録されている");
        assert_eq!(publisher.email, "admin@example.com");
        assert_eq!(publisher.id, admin.id);

        let unpublished = published.with_status(ItemStatus::Draft, None);
        assert!(unpublished.published_by.is_none());
    }

    /// A metadata record written before `published_by` existed must still deserialize.
    #[test]
    fn a_record_from_before_the_publisher_was_recorded_is_still_readable() {
        let legacy: ItemMetadata =
            serde_json::from_str(r#"{"status":"published","published_at":null}"#).unwrap();

        assert!(legacy.published_by.is_none());
    }

    /// Publishing must not look like an edit, and must not throw away when the content was
    /// written even though only the status is changing.
    #[test]
    fn changing_the_status_keeps_the_content_timestamps() {
        let written = Utc::now();
        let metadata = ItemMetadata::default().touched(written);
        let published = metadata.with_status(ItemStatus::Published, None);

        assert!(published.is_published());
        assert_eq!(published.created_at, Some(written));
        assert_eq!(published.updated_at, Some(written));

        let draft = published.with_status(ItemStatus::Draft, None);
        assert!(draft.published_at.is_none());
        assert_eq!(draft.created_at, Some(written));
        assert_eq!(draft.updated_at, Some(written));
    }

    #[test]
    fn status_serialises_in_lowercase_for_the_api() {
        assert_eq!(serde_json::to_string(&ItemStatus::Published).unwrap(), "\"published\"");
        assert_eq!(serde_json::to_string(&ItemStatus::Draft).unwrap(), "\"draft\"");
    }

    #[test]
    fn saving_records_when_the_values_changed_and_keeps_the_first_time() {
        let created = Utc::now();
        let first = ItemMetadata::default().touched(created);
        assert_eq!(first.created_at, Some(created));
        assert_eq!(first.updated_at, Some(created));
        // Saving is not publishing.
        assert!(!first.is_published());

        let later = created + chrono::Duration::seconds(5);
        let second = first.touched(later);
        assert_eq!(second.created_at, Some(created), "created_at must not move");
        assert_eq!(second.updated_at, Some(later));
    }

    #[test]
    fn saving_a_published_item_leaves_its_status_alone() {
        let published = ItemMetadata::default().with_status(ItemStatus::Published, None);
        let saved = published.touched(Utc::now());

        assert!(saved.is_published());
        assert_eq!(saved.published_at, published.published_at);
    }

    /// A metadata record written before `created_at` / `updated_at` existed is still
    /// readable, so existing content needs no migration.
    #[test]
    fn a_record_from_before_timestamps_existed_is_still_readable() {
        let legacy: ItemMetadata =
            serde_json::from_str(r#"{"status":"published","published_at":null}"#).unwrap();

        assert!(legacy.is_published());
        assert!(legacy.created_at.is_none());
        assert!(legacy.updated_at.is_none());
    }
}
