use std::sync::Arc;

use crate::auth::token::TokenIssuer;
use crate::auth::AuthService;
use crate::repositories::collection_repository::CollectionRepository;
use crate::repositories::composite_field_repository::CompositeFieldRepository;
use crate::repositories::image_repository::ImageRepository;
use crate::repositories::single_page_repository::SinglePageRepository;
use crate::repositories::user_repository::UserRepository;
use crate::services::collection_service::CollectionService;
use crate::services::composite_field_service::CompositeFieldService;
use crate::services::image_service::ImageService;
use crate::services::single_page_service::SinglePageService;
use crate::webhook::Notifier;

/// The full set of storage capabilities the HTTP layer requires.
///
/// Both the on-premises (rkv) and the AWS (DynamoDB + S3) adapters implement this, which
/// is what lets one axum router serve every backend.
pub trait Storage:
    CollectionRepository
    + CompositeFieldRepository
    + SinglePageRepository
    + ImageRepository
    + UserRepository
    + Send
    + Sync
    + 'static
{
}

impl<T> Storage for T where
    T: CollectionRepository
        + CompositeFieldRepository
        + SinglePageRepository
        + ImageRepository
        + UserRepository
        + Send
        + Sync
        + 'static
{
}

/// Composition root: wires the domain services onto a single storage adapter.
pub struct AppModule<R: Storage> {
    pub collection_service: CollectionService<R, R, R>,
    pub single_page_service: SinglePageService<R, R, R>,
    pub composite_field_service: CompositeFieldService<R>,
    pub image_service: ImageService<R>,
    pub auth_service: AuthService<R>,
}

impl<R: Storage> AppModule<R> {
    pub fn new(repository: Arc<R>, token_issuer: TokenIssuer, notifier: Arc<dyn Notifier>) -> Self {
        AppModule {
            collection_service: CollectionService::new(
                repository.clone(),
                repository.clone(),
                repository.clone(),
                notifier.clone(),
            ),
            single_page_service: SinglePageService::new(
                repository.clone(),
                repository.clone(),
                repository.clone(),
                notifier,
            ),
            composite_field_service: CompositeFieldService::new(repository.clone()),
            image_service: ImageService::new(repository.clone()),
            auth_service: AuthService::new(repository, token_issuer),
        }
    }
}
