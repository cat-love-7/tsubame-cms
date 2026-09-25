use std::collections::HashMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};

use crate::app_module::Storage;
use crate::http::{
    AppState, AuthenticatedUser, PreviewTokenQuery, Resource, ensure_preview_allowed,
    preview_link_error, require_admin, require_publish,
};
use crate::models::error::HttpError;
use crate::models::item_status::{ItemDates, ItemMetadata, ItemStatus, PublishedBy};
use crate::models::owner::ItemOwner;
use crate::models::schema::SchemaSettings;
use crate::models::single_page::{SinglePageItemResponse, SinglePageName, SinglePageSchema};
use crate::preview_link::PreviewTarget;

pub fn routes<R: Storage>() -> Router<AppState<R>> {
    Router::new()
        .route("/models/single_pages", get(get_single_pages::<R>))
        // Static segment, registered alongside `{page_name}`: axum prefers it, so the list can
        // ask for every page's state in one request.
        .route(
            "/models/single_pages/items/metadata",
            get(list_single_page_metadata::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/schema",
            get(get_single_page_schema::<R>)
                .post(add_single_page_schema::<R>)
                .put(update_single_page_schema::<R>),
        )
        .route(
            "/models/single_pages/{page_name}",
            delete(delete_single_page::<R>),
        )
        // What the page is told about itself, apart from its fields.
        .route(
            "/models/single_pages/{page_name}/settings",
            get(get_single_page_settings::<R>).put(update_single_page_settings::<R>),
        )
        // The name of every page, for the screens that show a reference.
        .route("/models/single_pages/titles", get(get_page_titles::<R>))
        // Who points at this page, so a delete can say what it would break.
        .route(
            "/models/single_pages/{page_name}/references",
            get(get_page_references::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/item",
            get(get_single_page_item::<R>).put(update_single_page_item::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/item/metadata",
            get(get_single_page_item_metadata::<R>).put(set_single_page_item_metadata::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/preview",
            get(preview_single_page::<R>),
        )
        // What the site serves right now, and throwing the working copy away (see the collection
        // equivalents in `http::collections`).
        .route(
            "/models/single_pages/{page_name}/published",
            get(get_published_single_page_item::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/draft",
            delete(discard_single_page_item_draft::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/preview-link",
            post(create_single_page_preview_link::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/publish",
            post(publish_single_page::<R>),
        )
        .route(
            "/models/single_pages/{page_name}/unpublish",
            post(unpublish_single_page::<R>),
        )
}

/// Draft/published state of every page this account may read, keyed by name.
///
/// The list screen shows a status, when it last changed and who released it, and asking per page
/// would be a request each.
async fn list_single_page_metadata<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
) -> Result<impl IntoResponse, HttpError> {
    // Only what this account may read, judged per page (a grant can name one).
    let readable: Vec<SinglePageName> = module
        .single_page_service
        .list_page_names()
        .await?
        .into_iter()
        .filter(|name| user.can_read(user.permission_for_single_page(name.as_str())))
        .collect();

    let mut statuses = HashMap::new();
    for (name, metadata, has_draft) in module
        .single_page_service
        .list_page_statuses(&readable)
        .await?
    {
        statuses.insert(
            name.to_string(),
            PageStatusResponse {
                metadata,
                has_draft,
            },
        );
    }
    Ok(Json(statuses))
}

async fn get_single_page_item_metadata<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(PageStatusResponse {
        metadata: module.single_page_service.get_page_metadata(&name).await?,
        has_draft: module.single_page_service.page_has_draft(&name).await?,
    }))
}

/// The stored metadata plus whether an unpublished working copy exists (see the collection
/// equivalent).
#[derive(serde::Serialize)]
struct PageStatusResponse {
    #[serde(flatten)]
    metadata: ItemMetadata,
    has_draft: bool,
}

/// State when the page was created, changed or published (see the collection equivalent, including
/// why the publication dates are judged by publish permission).
async fn set_single_page_item_metadata<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
    Json(dates): Json<ItemDates>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    if dates.touches_publication() {
        require_publish(&user, Some(Resource::SinglePage(&name)))?;
    }
    Ok(Json(PageStatusResponse {
        metadata: module
            .single_page_service
            .set_page_dates(&name, &dates)
            .await?,
        has_draft: module.single_page_service.page_has_draft(&name).await?,
    }))
}

/// What the page is told about itself; any signed-in reader, for the same reason as a collection's
/// settings (see `http::collections::get_collection_settings`).
async fn get_single_page_settings<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(
        module
            .single_page_service
            .get_single_page_settings(&name)
            .await?,
    ))
}

/// Turning a setting on changes what content can leave the CMS, so it is an administrator's act -
/// the same rule as a collection's settings.
async fn update_single_page_settings<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
    Json(settings): Json<SchemaSettings>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .update_single_page_settings(&name, &settings)
        .await?;
    Ok(StatusCode::OK)
}

/// What the site serves for this page, as opposed to the working copy the editor saves into.
async fn get_published_single_page_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(
        module
            .single_page_service
            .get_published_single_page_item(&name)
            .await?,
    ))
}

/// Throw the working copy away: the page goes back to what the site is serving.
async fn discard_single_page_item_draft<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .discard_single_page_item_draft(&name)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// What the page would look like if it were published now: the working copy, with the
/// schema. Authenticated, because that copy is unpublished content.
async fn preview_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(SinglePagePreview {
        schema: module
            .single_page_service
            .get_single_page_schema(&name)
            .await?,
        values: module
            .single_page_service
            .get_single_page_item(&name)
            .await?,
    }))
}

#[derive(serde::Serialize)]
struct SinglePagePreview {
    schema: SinglePageSchema,
    values: SinglePageItemResponse,
}

/// Mint a signed, expiring link that shows this working copy to someone without an account
/// (see the collection equivalent in `http::collections`).
async fn create_single_page_preview_link<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    // A page that does not exist is a 404, not a link to nothing.
    module
        .single_page_service
        .get_single_page_schema(&name)
        .await?;
    ensure_preview_allowed(
        &module
            .single_page_service
            .get_single_page_settings(&name)
            .await?,
        &ItemOwner::single_page(name.as_str()),
    )?;

    Ok(Json(module.preview_links.issue(
        &PreviewTarget::SinglePage {
            page: name.as_str().to_string(),
        },
        chrono::Utc::now(),
    )))
}

/// The route a single page's preview link opens. Public: the signature is the credential.
pub fn preview_routes<R: Storage>() -> Router<AppState<R>> {
    Router::new().route(
        "/preview/single_pages/{page_name}",
        get(open_single_page_preview::<R>),
    )
}

async fn open_single_page_preview<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
    Query(query): Query<PreviewTokenQuery>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    let target = PreviewTarget::SinglePage {
        page: name.as_str().to_string(),
    };
    module
        .preview_links
        .verify(&target, &query.token, chrono::Utc::now())
        .map_err(preview_link_error)?;
    // A link that was minted while previews were on stops working the moment they are turned off.
    ensure_preview_allowed(
        &module
            .single_page_service
            .get_single_page_settings(&name)
            .await?,
        &ItemOwner::single_page(name.as_str()),
    )?;

    Ok(Json(SinglePagePreview {
        schema: module
            .single_page_service
            .get_single_page_schema(&name)
            .await?,
        values: module
            .single_page_service
            .get_single_page_item(&name)
            .await?,
    }))
}

async fn publish_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    require_publish(&user, Some(Resource::SinglePage(&page_name)))?;
    let actor = PublishedBy::from(&user);
    set_single_page_status(module, page_name, ItemStatus::Published, actor).await
}

async fn unpublish_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    require_publish(&user, Some(Resource::SinglePage(&page_name)))?;
    let actor = PublishedBy::from(&user);
    set_single_page_status(module, page_name, ItemStatus::Draft, actor).await
}

async fn set_single_page_status<R: Storage>(
    module: AppState<R>,
    page_name: String,
    status: ItemStatus,
    actor: PublishedBy,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    let metadata = module
        .single_page_service
        .set_page_status(&name, status, actor)
        .await?;
    // The same shape the metadata endpoint answers with (see the collection equivalent).
    Ok(Json(PageStatusResponse {
        metadata,
        has_draft: module.single_page_service.page_has_draft(&name).await?,
    }))
}

async fn get_single_pages<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
) -> Result<impl IntoResponse, HttpError> {
    // Only the pages this account may read (see `get_collections`).
    let readable: Vec<SinglePageName> = module
        .single_page_service
        .list_page_names()
        .await?
        .into_iter()
        .filter(|name| user.can_read(user.permission_for_single_page(name.as_str())))
        .collect();
    Ok(Json(readable))
}

async fn get_single_page_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(
        module
            .single_page_service
            .get_single_page_schema(&name)
            .await?,
    ))
}

async fn add_single_page_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
    Json(schema): Json<SinglePageSchema>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .add_single_page_schema(&name, &schema)
        .await?;
    Ok(StatusCode::OK)
}

async fn update_single_page_schema<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
    Json(schema): Json<SinglePageSchema>,
) -> Result<impl IntoResponse, HttpError> {
    require_admin(&user)?;
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .update_single_page_schema(&name, &schema)
        .await?;
    Ok(StatusCode::OK)
}

/// The title of every page, which is what a reference to one shows.
///
/// All of them at once: a site has few pages, and a reference names the page, so the caller does not
/// have to ask one at a time for a list of rows.
async fn get_page_titles<R: Storage>(
    State(module): State<AppState<R>>,
) -> Result<impl IntoResponse, HttpError> {
    Ok(Json(module.single_page_service.page_titles().await?))
}

/// The content that references this page, so a delete can say what it would break.
async fn get_page_references<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(
        module
            .single_page_service
            .get_page_references(&name)
            .await?,
    ))
}

/// `?detach=true`: remove the references to this page and delete it anyway.
#[derive(serde::Deserialize)]
struct DetachQuery {
    #[serde(default)]
    detach: bool,
}

async fn delete_single_page<R: Storage>(
    State(module): State<AppState<R>>,
    Extension(AuthenticatedUser(user)): Extension<AuthenticatedUser>,
    Path(page_name): Path<String>,
    Query(query): Query<DetachQuery>,
) -> Result<impl IntoResponse, HttpError> {
    // The page and its schema go, so this is a structural change.
    require_admin(&user)?;
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .delete_single_page(&name, query.detach)
        .await?;
    Ok(StatusCode::OK)
}

async fn get_single_page_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    Ok(Json(
        module
            .single_page_service
            .get_single_page_item(&name)
            .await?,
    ))
}

async fn update_single_page_item<R: Storage>(
    State(module): State<AppState<R>>,
    Path(page_name): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, HttpError> {
    let name = SinglePageName::from(page_name.as_str());
    module
        .single_page_service
        .update_single_page_item_from_json(&name, &body)
        .await?;
    Ok(StatusCode::OK)
}
