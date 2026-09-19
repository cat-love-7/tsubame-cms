use std::sync::Arc;

use crate::auth::AuthService;
use crate::auth::token::TokenIssuer;
use crate::config::Limits;
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
/// capabilities an adapter may or may not have beyond this — serving image bytes itself — are
/// expressed as separate traits and separate routes, not as a different definition of this
/// one (see [`crate::repositories::local_image_bytes::LocalImageBytes`]).
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

pub struct AppModule<R: Storage> {
    pub collection_service: CollectionService<R, R, R>,
    pub single_page_service: SinglePageService<R, R, R>,
    pub composite_field_service: CompositeFieldService<R>,
    pub image_service: ImageService<R>,
    pub auth_service: AuthService<R>,
    /// Mints the signed, expiring links that let someone without an account review a draft.
    pub preview_links: PreviewLinkIssuer,
    /// How large a request this deployment accepts (`config::Limits`). The router applies them,
    /// and `/auth/capabilities` reports them, so both come from the one place the composition
    /// root read.
    pub limits: Limits,
}

impl<R: Storage> AppModule<R> {
    pub fn new(
        repository: Arc<R>,
        token_issuer: TokenIssuer,
        notifier: Arc<dyn Notifier>,
        preview_links: PreviewLinkIssuer,
        password_resets: PasswordResetIssuer,
        limits: Limits,
    ) -> Self {
        let auth_service =
            AuthService::new(repository.clone(), token_issuer, password_resets.clone());
        AppModule::assemble(repository, notifier, preview_links, auth_service, limits)
    }

    /// The same, for a deployment whose tokens an identity provider issues.
    ///
    /// `verifier` replaces the CMS's own token check, and `bootstrap_admins` are the names an
    /// operator allowed to become the first administrators by signing in.
    pub fn new_with_verifier(
        repository: Arc<R>,
        token_issuer: TokenIssuer,
        notifier: Arc<dyn Notifier>,
        preview_links: PreviewLinkIssuer,
        password_resets: PasswordResetIssuer,
        verifier: Arc<dyn crate::auth::identity::TokenVerifier>,
        bootstrap_admins: Vec<String>,
        limits: Limits,
    ) -> Self {
        let auth_service = AuthService::new(repository.clone(), token_issuer, password_resets)
            .with_external_verifier(verifier)
            .with_bootstrap_admins(bootstrap_admins);
        AppModule::assemble(repository, notifier, preview_links, auth_service, limits)
    }

    /// Let the deployment's identity provider look after the accounts themselves.
    ///
    /// After the module is built rather than among its arguments, because this is the one part
    /// that needs a client for the provider's own API - which is the composition root's business,
    /// not the module's.
    pub fn with_account_provisioner(
        mut self,
        provisioner: Arc<dyn crate::auth::provisioner::AccountProvisioner>,
    ) -> Self {
        self.auth_service
            .set_account_provisioner(provisioner);
        self
    }

    fn assemble(
        repository: Arc<R>,
        notifier: Arc<dyn Notifier>,
        preview_links: PreviewLinkIssuer,
        auth_service: AuthService<R>,
        limits: Limits,
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
            image_service: ImageService::new(repository.clone(), limits.max_image_bytes),
            auth_service,
            preview_links,
            limits,
        }
    }
}
