use std::sync::Arc;

use crate::auth::token::TokenIssuer;
use crate::auth::AuthService;
use crate::password_reset::PasswordResetIssuer;
use crate::preview_link::PreviewLinkIssuer;
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
/// Every backend implements this, which is what lets one axum router serve all of them. The
/// local feature adds one more requirement - serving and accepting image bytes itself - so
/// that the routes for it can exist without the shared layers pretending every backend has
/// them.
#[cfg(feature = "on-premises")]
pub trait Storage:
    CollectionRepository
    + CompositeFieldRepository
    + SinglePageRepository
    + ImageRepository
    + UserRepository
    + crate::repositories::local_image_bytes::LocalImageBytes
    + Send
    + Sync
    + 'static
{
}

#[cfg(feature = "on-premises")]
impl<T> Storage for T where
    T: CollectionRepository
        + CompositeFieldRepository
        + SinglePageRepository
        + ImageRepository
        + UserRepository
        + crate::repositories::local_image_bytes::LocalImageBytes
        + Send
        + Sync
        + 'static
{
}

/// See the on-premises definition above: the local byte capability is not part of the
/// contract every backend has to meet.
#[cfg(not(feature = "on-premises"))]
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

#[cfg(not(feature = "on-premises"))]
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
    /// Mints the signed, expiring links that let someone without an account review a draft.
    pub preview_links: PreviewLinkIssuer,
}

impl<R: Storage> AppModule<R> {
    pub fn new(
        repository: Arc<R>,
        token_issuer: TokenIssuer,
        notifier: Arc<dyn Notifier>,
        preview_links: PreviewLinkIssuer,
        password_resets: PasswordResetIssuer,
    ) -> Self {
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
            auth_service: AuthService::new(repository, token_issuer, password_resets.clone()),
            preview_links,
        }
    }
}
