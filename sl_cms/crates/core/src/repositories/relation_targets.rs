use std::future::Future;
use std::pin::Pin;

use crate::models::schema::RelationTarget;
use crate::repositories::collection_repository::{BoxError, CollectionRepository};
use crate::repositories::single_page_repository::SinglePageRepository;

/// Boxed rather than a bare `async fn` so the trait stays object safe (`Arc<dyn
/// RelationTargetSource>` is what the services hold) and so the future can be required to be
/// `Send`; the same shape as `webhook::NotifyFuture`.
pub type RelationTargetsFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<RelationTarget>, BoxError>> + Send + 'a>>;

/// What a relation field is allowed to point at, which is what a schema save is checked against.
///
/// Narrower than either repository on purpose: validating a schema needs to know which names
/// exist, not to read any of them, and the schema save of a collection should not have to depend
/// on the whole repository of the single pages - nor the other way around. Both services are
/// handed the same storage, which answers both lists (see the blanket implementation).
pub trait RelationTargetSource: Send + Sync + 'static {
    /// Every collection and single page of this site, so a target can be matched against them.
    fn relation_targets(&self) -> RelationTargetsFuture<'_>;
}

/// Storage answers both halves, because it is both repositories (see `Storage`).
impl<T: CollectionRepository + SinglePageRepository> RelationTargetSource for T {
    fn relation_targets(&self) -> RelationTargetsFuture<'_> {
        Box::pin(async move {
            let mut targets = Vec::new();
            for name in self.list_collection_names().await? {
                targets.push(RelationTarget::Collection {
                    name: name.as_str().to_string(),
                });
            }
            for name in self.list_all_page_names().await? {
                targets.push(RelationTarget::SinglePage {
                    name: name.as_str().to_string(),
                });
            }
            Ok(targets)
        })
    }
}

/// A fixed answer, for tests that do not want a repository behind it.
pub struct StaticRelationTargets {
    targets: Vec<RelationTarget>,
}

impl StaticRelationTargets {
    pub fn new(targets: Vec<RelationTarget>) -> Self {
        StaticRelationTargets { targets }
    }

    /// A site with nothing a relation could point at.
    pub fn none() -> Self {
        StaticRelationTargets {
            targets: Vec::new(),
        }
    }
}

impl RelationTargetSource for StaticRelationTargets {
    fn relation_targets(&self) -> RelationTargetsFuture<'_> {
        Box::pin(async move { Ok(self.targets.clone()) })
    }
}
