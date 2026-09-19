use chrono::{DateTime, Duration, Utc};

use crate::models::collection::CollectionItemId;
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

/// What a batch status change did to one item.
///
/// A batch reports per item rather than as a whole: one item a publisher may not touch, or one
/// whose working copy moved under the operation, does not make the rest of the batch fail.
#[derive(serde::Serialize, Debug, Clone)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ItemStatusOutcome {
    /// The item's status is what was asked for; the metadata is what the screen needs to update.
    Changed {
        id: CollectionItemId,
        metadata: ItemMetadata,
    },
    /// The item was left as it was, and this is why.
    Refused {
        id: CollectionItemId,
        code: String,
        message: String,
    },
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

/// The dates of a record, as a **patch**: a field left out is one this does not touch.
///
/// What a migration from another CMS needs, and what [`ItemMetadata`] is not: the whole record
/// also carries the status, the publisher and the published copy, so writing it back from a read
/// would undo a publish that landed while the caller was deciding - the same reason
/// [`CollectionRepository::touch_item_metadata`](crate::repositories::collection_repository::CollectionRepository::touch_item_metadata)
/// exists. The API therefore takes the dates alone, and storage writes the fields it was given.
///
/// A field is never *cleared* here. Setting one to nothing is a repair nobody has needed, and
/// letting an absent field mean "leave it" and a null mean "forget it" is a distinction every
/// caller would have to know about.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemDates {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_published_at: Option<DateTime<Utc>>,
}

/// How far ahead of this CMS's clock a date may be and still be believed.
///
/// The machine running an import has its own clock, and one that says "now" a few seconds ahead of
/// this one is not wrong about anything that matters. A date further out than this is a mistake -
/// a timezone applied twice, a year typed - and storing it would leave it invisible until somebody
/// sorted by the column.
const CLOCK_SKEW_SECONDS: i64 = 60;

/// Whether `at` is further ahead of this CMS's clock than a client's honestly could be.
///
/// The one rule every date a caller may state is judged by, whether it is a content timestamp or
/// when an image arrived: a timezone applied twice, or a year typed wrong, is otherwise invisible
/// until somebody sorts by the column.
pub fn is_ahead_of_the_clock(at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    at > now + Duration::seconds(CLOCK_SKEW_SECONDS)
}

impl ItemDates {
    /// Whether there is anything to write.
    pub fn is_empty(&self) -> bool {
        self.created_at.is_none()
            && self.updated_at.is_none()
            && self.published_at.is_none()
            && self.last_published_at.is_none()
    }

    /// Whether this says anything about when the content went live.
    ///
    /// That is publish permission rather than edit: the dates are what a build compares to decide
    /// whether its copy is out of date, so they are part of releasing content, not of editing it.
    pub fn touches_publication(&self) -> bool {
        self.published_at.is_some() || self.last_published_at.is_some()
    }

    /// Refuse dates that cannot be true, against `now` and against the record they apply to.
    ///
    /// `existing` is what the record says today, so a patch that sets one field is still judged
    /// with the others in view: setting `last_published_at` before the publication date that is
    /// already stored is as wrong as setting both.
    pub fn check(&self, existing: &ItemMetadata, now: DateTime<Utc>) -> Result<(), String> {
        for (name, at) in [
            ("created_at", self.created_at),
            ("updated_at", self.updated_at),
            ("published_at", self.published_at),
            ("last_published_at", self.last_published_at),
        ] {
            if let Some(at) = at
                && is_ahead_of_the_clock(at, now)
            {
                return Err(format!("{name} is in the future"));
            }
        }

        let merged = existing.with_dates(self);
        if let (Some(created), Some(updated)) = (merged.created_at, merged.updated_at)
            && updated < created
        {
            return Err("updated_at is before created_at".to_string());
        }
        if let (Some(first), Some(last)) = (merged.published_at, merged.last_published_at)
            && last < first
        {
            return Err("last_published_at is before published_at".to_string());
        }
        Ok(())
    }
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

    /// The record with the dates a migration states applied, leaving everything else - the status,
    /// the publisher, the published copy - exactly as it is.
    pub fn with_dates(&self, dates: &ItemDates) -> Self {
        ItemMetadata {
            created_at: dates.created_at.or(self.created_at),
            updated_at: dates.updated_at.or(self.updated_at),
            published_at: dates.published_at.or(self.published_at),
            last_published_at: dates.last_published_at.or(self.last_published_at),
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
        let admin = User::new("Admin", true, crate::models::user::Permission::admin());
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
        assert_eq!(
            published.updated_at,
            Some(written),
            "publishing is not an edit"
        );

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
        let again = published
            .released(later)
            .with_status(ItemStatus::Published, None);

        assert_eq!(again.published_at, published.published_at);
        assert_eq!(
            again.updated_at,
            Some(later),
            "the release is a content change"
        );
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
        assert_eq!(
            serde_json::to_string(&ItemStatus::Published).unwrap(),
            "\"published\""
        );
        assert_eq!(
            serde_json::to_string(&ItemStatus::Draft).unwrap(),
            "\"draft\""
        );
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

    /// A patch states some dates and leaves the rest of the record - status, publisher, the dates
    /// it did not mention - exactly as they were.
    #[test]
    fn a_patch_moves_only_the_dates_it_names() {
        let imported = Utc::now() - chrono::Duration::days(400);
        let published = ItemMetadata::default()
            .with_status(ItemStatus::Published, None)
            .released(Utc::now());
        let patched = published.with_dates(&ItemDates {
            created_at: Some(imported),
            ..ItemDates::default()
        });

        assert_eq!(patched.created_at, Some(imported));
        assert_eq!(
            patched.updated_at, published.updated_at,
            "a date that was not named stays as it was"
        );
        assert_eq!(patched.published_at, published.published_at);
        assert_eq!(patched.status, ItemStatus::Published);
    }

    /// Dates that cannot be true are refused rather than stored, because a wrong one is invisible
    /// until somebody sorts by it.
    #[test]
    fn impossible_dates_are_refused() {
        let now = Utc::now();
        let existing = ItemMetadata::default().touched(now);

        let future = ItemDates {
            created_at: Some(now + chrono::Duration::days(1)),
            ..ItemDates::default()
        };
        assert_eq!(
            future.check(&existing, now).unwrap_err(),
            "created_at is in the future"
        );

        // A clock a little ahead of this one is not a mistake: the machine running an import has
        // its own, and five seconds of skew is not wrong about anything that matters.
        let now_ish = ItemDates {
            updated_at: Some(now + chrono::Duration::seconds(5)),
            ..ItemDates::default()
        };
        assert!(now_ish.check(&existing, now).is_ok());

        let backwards = ItemDates {
            updated_at: Some(now - chrono::Duration::days(1)),
            ..ItemDates::default()
        };
        assert_eq!(
            backwards.check(&existing, now).unwrap_err(),
            "updated_at is before created_at"
        );

        // Judged against the record, not only against the patch: this states one date and is
        // still wrong because of the one already stored.
        let after_the_publication = ItemMetadata {
            published_at: Some(now - chrono::Duration::days(1)),
            ..ItemMetadata::default()
        };
        let earlier_release = ItemDates {
            last_published_at: Some(now - chrono::Duration::days(2)),
            ..ItemDates::default()
        };
        assert_eq!(
            earlier_release
                .check(&after_the_publication, now)
                .unwrap_err(),
            "last_published_at is before published_at"
        );
    }

    /// The publication dates are what a build compares, so stating them is publish permission.
    #[test]
    fn a_patch_knows_whether_it_touches_publication() {
        assert!(!ItemDates {
            created_at: Some(Utc::now()),
            ..ItemDates::default()
        }
        .touches_publication());
        assert!(
            ItemDates {
                published_at: Some(Utc::now()),
                ..ItemDates::default()
            }
            .touches_publication()
        );
        assert!(
            ItemDates {
                last_published_at: Some(Utc::now()),
                ..ItemDates::default()
            }
            .touches_publication()
        );
    }
}
