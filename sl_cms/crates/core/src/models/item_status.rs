use chrono::{DateTime, Utc};

use crate::models::user::{User, UserId};

/// Who published an item, captured at the moment it was published.
///
/// The id links back to the account while it exists; the username is kept as it was, so the
/// record still reads after the account has been renamed or deleted. That is what makes it
/// usable as an audit trail rather than just a foreign key.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PublishedBy {
    pub id: UserId,
    pub username: String,
}

impl From<&User> for PublishedBy {
    fn from(user: &User) -> Self {
        PublishedBy {
            id: user.id.clone(),
            username: user.username.clone(),
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
    /// When it was **first** published.
    ///
    /// The publication date belongs to the item, so publishing again does not move it and
    /// unpublishing does not erase it. When the live copy last went out is
    /// [`ItemMetadata::last_published_at`], and when the content last changed is `updated_at`.
    pub published_at: Option<DateTime<Utc>>,
    /// When it was last published; cleared when it is unpublished.
    ///
    /// This is the one a build should compare: the published copy only changes when an item is
    /// published, so it answers "could what you built be out of date?" without the unpublished
    /// edits that `updated_at` also tracks.
    #[serde(default)]
    pub last_published_at: Option<DateTime<Utc>>,
    /// When the values were first saved.
    ///
    /// Optional because content that predates this field has no record of it, and because
    /// a single page that was never saved has none either. `#[serde(default)]` is what
    /// lets a metadata record written before timestamps existed still be read.
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    /// When the content last changed: a save, or a publish that released a working copy.
    ///
    /// Publishing an item with nothing waiting does not change it, so a build can still tell an
    /// edit from a publish that had nothing to release.
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
    /// The first publication date is kept: publishing again records nothing new there (see
    /// [`ItemMetadata::released`] for the release time). The publisher is who put the version
    /// that is live on the site, so unpublishing forgets it - unlike the publication date,
    /// which stays a fact about the item.
    pub fn with_status(&self, status: ItemStatus, actor: Option<PublishedBy>) -> Self {
        let (last_published_at, published_by) = match status {
            ItemStatus::Published => (Some(Utc::now()), actor),
            ItemStatus::Draft => (None, None),
        };
        ItemMetadata {
            status,
            published_at: self.published_at.or(last_published_at),
            last_published_at,
            published_by,
            ..self.clone()
        }
    }

    /// Record that a publish released a working copy: the content the site serves just changed.
    ///
    /// Separate from [`ItemMetadata::with_status`] because only a release moves the content
    /// clock; publishing when nothing was waiting changes nothing but the publisher.
    pub fn released(&self, now: DateTime<Utc>) -> Self {
        ItemMetadata {
            updated_at: Some(now),
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
    fn publishing_records_when_and_unpublishing_keeps_the_date() {
        let published = ItemMetadata::default().with_status(ItemStatus::Published, None);
        assert!(published.is_published());
        let first_date = published.published_at.expect("記録されている");
        assert_eq!(published.last_published_at, Some(first_date));

        let unpublished = published.with_status(ItemStatus::Draft, None);
        assert!(!unpublished.is_published());
        // Taking the item down does not change when it was published...
        assert_eq!(unpublished.published_at, Some(first_date));
        // ...but it is no longer out on the site, so nothing was "last published".
        assert!(unpublished.last_published_at.is_none());

        // Publishing it again keeps the original date rather than resetting it, and records the
        // moment this copy went out.
        let again = unpublished.with_status(ItemStatus::Published, None);
        assert_eq!(again.published_at, Some(first_date));
        assert!(again.last_published_at.is_some_and(|at| at >= first_date));
    }

    /// The audit trail: the publisher is stored with the item, and unpublishing - which
    /// takes the item off the site - forgets who put it there (unlike the publication date,
    /// which stays).
    #[test]
    fn publishing_records_who_did_it_and_unpublishing_forgets_that_too() {
        let admin = User::new(
            "Admin",
            true,
            crate::models::user::Permission::admin(),
        );
        let published = ItemMetadata::default()
            .with_status(ItemStatus::Published, Some(PublishedBy::from(&admin)));

        let publisher = published.published_by.clone().expect("記録されている");
        assert_eq!(publisher.username, "admin");
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
        assert_eq!(published.updated_at, Some(written), "publishing is not an edit");

        let draft = published.with_status(ItemStatus::Draft, None);
        assert_eq!(draft.created_at, Some(written));
        assert_eq!(draft.updated_at, Some(written));
        // The publication date stays: the item was published, and taking it down does not
        // change when that was.
        assert_eq!(draft.published_at, published.published_at);
    }

    /// The publication date is the *first* one; later releases are content changes.
    #[test]
    fn publishing_again_keeps_the_first_publication_date() {
        let first = Utc::now();
        let published = ItemMetadata::default()
            .with_status(ItemStatus::Published, None)
            .released(first);

        let later = first + chrono::Duration::days(30);
        let again = published.released(later).with_status(ItemStatus::Published, None);

        assert_eq!(again.published_at, published.published_at);
        assert_eq!(again.updated_at, Some(later), "the release is a content change");
        assert_eq!(again.created_at, published.created_at);
        assert!(
            again.last_published_at.is_some_and(|at| at > first),
            "the release time moves, unlike the publication date"
        );
    }

    /// Publishing with nothing waiting only records who did it.
    #[test]
    fn publishing_without_a_release_does_not_move_the_content_clock() {
        let published = ItemMetadata::default().with_status(ItemStatus::Published, None);
        let unchanged = published.with_status(ItemStatus::Published, None);

        assert_eq!(unchanged.updated_at, published.updated_at);
        assert_eq!(unchanged.published_at, published.published_at);
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
