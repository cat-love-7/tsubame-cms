// HTTP-level integration tests (T1.8), written once and run against every storage backend.
//
// These drive the real router with `tower::ServiceExt::oneshot`, so routing, extractors, the
// auth middleware, status codes and JSON shapes are all exercised — not just the services
// underneath. Storage is a real adapter, built by the harness (`sl_cms_tests::TestBackend`),
// so the adapter is covered too: a backend that passes this file is interchangeable with one
// that does.
//
// The tests are the contract, so they may only use what every backend promises. The one
// exception is image bytes, which an object-storage backend leaves to the object store; those
// tests ask `Backend::SERVES_IMAGE_BYTES` first.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use sl_cms_core::models::pagination::DEFAULT_PAGE_LIMIT;
use sl_cms_core::models::user::Permission;
use sl_cms_core::preview_link::PreviewTarget;
use sl_cms_core::webhook::{Notifier, WebhookNotifier};
use sl_cms_core::auth::token::TokenIssuer;
use sl_cms_tests::{
    send, send_raw, send_with_headers, TestBackend, ADMIN_EMAIL, ADMIN_PASSWORD, TEST_SECRET,
    TEST_TOKEN_TTL_HOURS, VIEWER_EMAIL, VIEWER_PASSWORD,
};

/// Which backend this copy drives; the runner file defines it just before including this file.
use crate::Backend;


/// Read a timestamp out of a JSON body, failing loudly when it is missing.
fn timestamp(value: &Value) -> chrono::DateTime<chrono::FixedOffset> {
    let raw = value
        .as_str()
        .unwrap_or_else(|| panic!("expected a timestamp, got {value}"));
    chrono::DateTime::parse_from_rfc3339(raw).expect("an RFC3339 timestamp")
}

/// A schema with the shape the Angular client sends.
fn sample_schema() -> Value {
    json!([
        { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 },
        { "name": "tags", "field_type": { "TextEnum": ["news", "blog"] }, "required": false, "width": 12, "height": 1 }
    ])
}

/// The harness, specialised to the backend this copy of the suite runs against.
///
/// Every test names `TestApp`, so which storage it drives is decided by this one line.
type TestApp = sl_cms_tests::TestApp<Backend>;

async fn test_app() -> TestApp {
    TestApp::new().await
}

/// Same app, but with webhooks wired to `notifier`.
async fn test_app_with_notifier(notifier: Arc<dyn Notifier>) -> TestApp {
    TestApp::with_notifier(notifier).await
}

async fn create_user(app: &TestApp, email: &str, password: &str, is_admin: bool) -> StatusCode {
    create_account(
        app,
        json!({ "username": email, "password": password, "is_admin": is_admin }),
    )
    .await
    .0
}

/// Create an account, the way this deployment creates one.
///
/// Where the CMS stores the credential this is `POST /auth/users`, so the endpoint and its 201
/// and 409 are exercised. Where an identity provider owns the credential there is no such
/// endpoint yet — provisioning a Cognito user is `doc/aws-plan.md` P4 — so the account record is
/// written directly: the tests *about* permissions run against both deployments, and the ones
/// about passwords are gated on `Backend::PASSWORD_LOGIN`.
async fn create_account(app: &TestApp, payload: Value) -> (StatusCode, Value) {
    if app.password_login() {
        return send(
            &app.router,
            Method::POST,
            "/auth/users",
            Some(&app.admin_token),
            Some(payload),
        )
        .await;
    }
    let username = payload["username"].as_str().expect("a username");
    let is_admin = payload["is_admin"].as_bool().unwrap_or(false);
    let permission: Permission = serde_json::from_value(payload["permission"].clone())
        .unwrap_or_else(|_| Permission::viewer());
    match app
        .backend()
        .create_account(username, is_admin, permission)
        .await
    {
        Ok(user) => (
            StatusCode::CREATED,
            serde_json::to_value(user.to_response()).expect("a serialisable account"),
        ),
        Err(_) => (StatusCode::CONFLICT, Value::Null),
    }
}

/// Sign in, the way this deployment signs in.
///
/// Where the CMS checks passwords this posts to `/auth/login`, so a test that needs a token
/// also exercises signing in. Where an identity provider does it there is no endpoint to post
/// to, and the harness mints the token the way a deployment does once the provider has
/// authenticated someone. The tests *about* passwords are gated on `Backend::PASSWORD_LOGIN`;
/// this is for the ones that only need to be someone.
async fn login(app: &TestApp, username: &str, password: &str) -> (StatusCode, Value) {
    if app.password_login() {
        return sl_cms_tests::login(&app.router, username, password).await;
    }
    // The same answer the real endpoint gives, assembled from the account record: the suite's
    // password tests are gated, and this keeps the rest reading like a sign-in.
    let user = app.backend().user_for(username).await;
    let (token, expires_at) = TokenIssuer::new(TEST_SECRET, TEST_TOKEN_TTL_HOURS)
        .issue(&user)
        .expect("a token");
    (
        StatusCode::OK,
        json!({ "token": token, "expires_at": expires_at, "user": user.to_response() }),
    )
}

/// What the deployment can do, which a client asks before it draws anything.
#[tokio::test]
async fn capabilities_say_how_this_deployment_signs_users_in() {
    let app = test_app().await;

    let (status, body) = send(&app.router, Method::GET, "/auth/capabilities", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["password_login"], Backend::PASSWORD_LOGIN);
    assert_eq!(
        body["image_upload"],
        if Backend::SERVES_IMAGE_BYTES {
            "proxied"
        } else {
            "presigned"
        }
    );
}

/// Where there are no local passwords, the endpoints that would use one say so rather than
/// disappearing: a client that guessed the path learns where to sign in instead of reading a
/// 404 as "wrong URL".
#[tokio::test]
async fn a_deployment_without_local_passwords_explains_itself() {
    if Backend::PASSWORD_LOGIN {
        return;
    }
    let app = test_app().await;

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/auth/login",
        None,
        Some(json!({ "username": ADMIN_EMAIL, "password": ADMIN_PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    let message = String::from_utf8_lossy(&body);
    assert!(
        message.contains("Cognito"),
        "the answer should say where to sign in: {message}"
    );

    // The same for an account route that would touch a credential, with a real token.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/me/password",
        Some(&app.admin_token),
        Some(json!({ "current_password": "x", "new_password": "y" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

// ---------------------------------------------------------------- liveness / routing

#[tokio::test]
async fn root_is_public_and_unknown_routes_404_rather_than_401() {
    let app = test_app().await;

    let (status, _) = send_raw(&app.router, Method::GET, "/", None, None).await;
    assert_eq!(status, StatusCode::OK);

    // The auth middleware is applied per-route, so an unknown path must not be reported
    // as an authentication failure.
    let (status, _) = send_raw(&app.router, Method::GET, "/not-a-route", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ------------------------------------------------------------------------ authentication

#[tokio::test]
async fn model_routes_require_a_bearer_token() {
    let app = test_app().await;

    let (status, _) = send_raw(&app.router, Method::GET, "/models/collections", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send_raw(&app.router, Method::GET, "/models/collections", Some("garbage"), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A token without the `Bearer ` prefix is not accepted either.
    let request = Request::builder()
        .method(Method::GET)
        .uri("/models/collections")
        .header(header::AUTHORIZATION, &app.admin_token)
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn login_rejects_bad_credentials_without_revealing_which_part_was_wrong() {
    // This deployment leaves sign-in to an identity provider: there is no password endpoint to
    // drive, which is what `GET /auth/capabilities` reports to a client.
    if !Backend::PASSWORD_LOGIN {
        eprintln!("skipped: this deployment signs users in through an identity provider");
        return;
    }
    let app = test_app().await;

    let (unknown_status, unknown_body) = login(&app, "nobody@example.com", ADMIN_PASSWORD).await;
    let (wrong_status, wrong_body) = login(&app, ADMIN_EMAIL, "not-the-password").await;

    assert_eq!(unknown_status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown_body, wrong_body);
}

#[tokio::test]
async fn me_returns_the_current_user_without_the_password_hash() {
    let app = test_app().await;

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/auth/me",
        Some(&app.admin_token),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["username"], ADMIN_EMAIL);
    assert_eq!(body["is_admin"], true);
    assert!(body.get("password_hash").is_none(), "leaked hash: {body}");
}

// ------------------------------------------------------------------------ authorization

#[tokio::test]
async fn viewer_can_read_but_not_write_and_cannot_manage_users() {
    let app = test_app().await;
    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );

    let (status, body) = login(&app, VIEWER_EMAIL, VIEWER_PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["is_admin"], false);
    let viewer_token = body["token"].as_str().unwrap().to_string();

    // Reads are allowed.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections",
        Some(&viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Writes are not.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&viewer_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Account management is administrator-only even for editors.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/users",
        Some(&viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/users",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn rejects_duplicate_and_weak_user_registrations() {
    // The validation lives in the account-creation endpoint, which this deployment does not
    // have: it is Cognito that would reject a weak or duplicate registration.
    if !Backend::PASSWORD_LOGIN {
        eprintln!("skipped: this deployment signs users in through an identity provider");
        return;
    }
    let app = test_app().await;
    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );
    // Duplicate (different case) -> 409.
    assert_eq!(
        create_user(&app, "VIEWER@example.com", VIEWER_PASSWORD, false).await,
        StatusCode::CONFLICT
    );
    // Too short -> 400.
    assert_eq!(create_user(&app, "other@example.com", "short", false).await, StatusCode::BAD_REQUEST);
}

// ------------------------------------------------------------------------------- content

#[tokio::test]
async fn collection_item_crud_round_trip() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/schema",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["name"], "title");
    // width/height must survive the round trip; their absence used to make saves fail.
    assert_eq!(body[0]["width"], 12);
    assert_eq!(body[1]["field_type"]["TextEnum"][1], "blog");

    // Create.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Hello", "tags": ["news"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(1));

    // List (the id/response tuple shape is preserved).
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0][0], 1);

    // Read one.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Validation is enforced through the HTTP layer too. Error bodies are plain text,
    // so read them raw rather than as JSON.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "tags": ["news"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("title"), "unexpected body: {text}");

    // Update + delete.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/collections/blog/items/1",
        Some(&token),
        Some(json!({ "title": "Updated", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/collections/blog/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Deleting the collection itself used to fail with 500.
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/collections/blog",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Values are untyped on the wire because the schema already states each field's type.
#[tokio::test]
async fn item_values_are_untyped_and_mismatches_are_rejected() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;

    // Untagged values are accepted.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Hello", "tags": ["blog"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ...and come back untagged, so request and response agree on the shape.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Hello");
    assert_eq!(body["tags"], json!(["blog"]));

    // A value of the wrong type is rejected rather than coerced.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": 42 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("expected a string"), "unexpected body: {text}");

    // An undeclared field is rejected so a client typo cannot silently drop content.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "ok", "titel": "typo" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("Unknown field"), "unexpected body: {text}");

    // An enum value outside the declared options is rejected.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "ok", "tags": ["not-an-option"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// What a schema says about a value comes back as something a form can act on: the status says
/// the input was wrong, the code says how, and the field says which input - a path, so a refusal
/// inside a composite still points at something the screen can mark.
#[tokio::test]
async fn a_value_outside_its_schema_is_refused_with_a_code_and_the_field() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let composite = json!([
        { "name": "description", "field_type": { "Text": { "max_length": 5 } }, "required": false, "width": 12, "height": 1 }
    ]);
    send(&app.router, Method::POST, "/models/composite_fields/seo", Some(&token), Some(composite)).await;

    let deep = json!([
        { "name": "label", "field_type": { "Text": { "max_length": 3 } }, "required": false, "width": 12, "height": 1 }
    ]);
    send(&app.router, Method::POST, "/models/composite_fields/deep", Some(&token), Some(deep)).await;

    let schema = json!([
        { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 },
        { "name": "code", "field_type": { "Text": { "max_length": 5 } }, "required": false, "width": 12, "height": 1 },
        { "name": "tags", "field_type": { "Array": [{ "Text": { "max_length": 3 } }] }, "required": false, "width": 12, "height": 1 },
        { "name": "seo", "field_type": { "CompositeField": { "id": "seo" } }, "required": false, "width": 12, "height": 1 },
        { "name": "parts", "field_type": { "Array": [{ "CompositeField": { "id": "deep" } }] }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/limited/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A required field left empty.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_required");
    assert_eq!(body["field"], "title");

    // Longer than the field allows. The limit counts characters: six Japanese characters are
    // over a limit of five, and five are not, whatever they take in bytes.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "ok", "code": "あいうえおか" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_too_long");
    assert_eq!(body["field"], "code");

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "ok", "code": "あいうえお" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // An array item is named by its index, so the reader knows which one to look at.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "ok", "tags": ["one", "toolong"] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_too_long");
    assert_eq!(body["field"], "tags[1]");

    // A refusal inside a composite names the path down to it.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "ok", "seo": { "description": "toolong" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_too_long");
    assert_eq!(body["field"], "seo.description");

    // And one inside an element of an array of composites names the whole way there.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/limited/item",
        Some(&token),
        Some(json!({ "title": "ok", "parts": [{ "id": "deep", "values": { "label": "toolong" } }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "field_too_long");
    assert_eq!(body["field"], "parts[0].label");
}

/// Image arrays are allowed on their own; `Number` + `Image` together is not, because
/// array items carry no type tag and an image id is a JSON number.
#[tokio::test]
async fn image_arrays_work_but_number_and_image_together_are_rejected() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let ambiguous = json!([{
        "name": "mixed",
        "field_type": { "Array": ["Number", "Image"] },
        "required": false,
        "width": 12,
        "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/mixed/schema",
        Some(&token),
        Some(ambiguous),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("both Number and Image"), "unexpected body: {text}");

    // Record an image so the id resolves when it is read back.
    let (_, upload) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "a.png", "ext": "png" })),
    )
    .await;
    let image_id = upload["id"].as_u64().expect("image id");

    let schema = json!([{
        "name": "covers",
        "field_type": { "Array": ["Image"] },
        "required": false,
        "width": 12,
        "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/gallery/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A bare id is accepted on write.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/gallery/item",
        Some(&token),
        Some(json!({ "covers": [image_id] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // It comes back as the `{id, url}` shape, and re-submitting that is accepted too.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/gallery/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["covers"][0]["id"], image_id);

    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/gallery/items/1",
        Some(&token),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
}

/// Structural schema rules are enforced at save time, not when someone later writes
/// content against the schema.
#[tokio::test]
async fn invalid_schemas_are_rejected_when_saved() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let cases = [
        (
            json!([{ "name": "", "field_type": "Number", "required": false, "width": 12, "height": 1 }]),
            "must not be empty",
        ),
        (
            json!([
                { "name": "dup", "field_type": "Number", "required": false, "width": 12, "height": 1 },
                { "name": "dup", "field_type": "Boolean", "required": false, "width": 12, "height": 1 }
            ]),
            "duplicate field name",
        ),
        (
            json!([{ "name": "wide", "field_type": "Number", "required": false, "width": 20, "height": 1 }]),
            "width must be between 1 and 12",
        ),
        (
            json!([{ "name": "empty", "field_type": { "Array": [] }, "required": false, "width": 12, "height": 1 }]),
            "at least one item type",
        ),
    ];

    for (index, (schema, expected)) in cases.iter().enumerate() {
        let (status, body) = send_raw(
            &app.router,
            Method::POST,
            &format!("/models/collections/bad{index}/schema"),
            Some(&token),
            Some(schema.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "case {index} was accepted");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains(expected), "case {index}: unexpected body: {text}");
    }
}

/// A field declared unique is checked by an index, not by a scan: the same value cannot be
/// stored twice, and the refusal names the field so a form can mark it.
#[tokio::test]
async fn a_unique_field_refuses_a_value_another_item_holds() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let schema = json!([{
        "name": "slug",
        "field_type": { "Text": {} },
        "required": false,
        "width": 12,
        "height": 1,
        "unique": true
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/schema",
        Some(&token),
        Some(schema.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let create = |slug: serde_json::Value| {
        let token = token.clone();
        let router = app.router.clone();
        async move {
            send_raw(
                &router,
                Method::POST,
                "/models/collections/pages/item",
                Some(&token),
                Some(json!({ "slug": slug })),
            )
            .await
        }
    };

    let (status, body) = create(json!("intro")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = create(json!("guide")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // The value is taken, and the refusal says which field and which item.
    let (status, body) = create(json!("intro")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let refusal: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(refusal["code"], "value_taken");
    assert_eq!(refusal["field"], "slug");
    assert!(
        refusal["message"].as_str().unwrap_or_default().contains("item 1"),
        "who holds it belongs in the message: {refusal}"
    );

    // An empty value is "not set": two items may leave an optional unique field blank.
    let (status, body) = create(json!("")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = create(json!("")).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Saving an item with the value it already has is not a conflict with itself.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "intro" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Changing it gives the old value up...
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "welcome" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = create(json!("intro")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the value it no longer holds is free: {}",
        String::from_utf8_lossy(&body)
    );

    // ...and so does deleting the item.
    let (status, _) = send_raw(
        &app.router,
        Method::DELETE,
        "/models/collections/pages/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = create(json!("welcome")).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "deleting frees what the item held: {}",
        String::from_utf8_lossy(&body)
    );
}

/// A published item keeps its published value reserved while a draft edits it: the live copy is
/// still what the delivery API serves, so another item must not be able to take its value.
#[tokio::test]
async fn a_published_value_stays_reserved_while_a_draft_changes_it() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let schema = json!([{
        "name": "slug",
        "field_type": { "Text": {} },
        "required": false,
        "width": 12,
        "height": 1,
        "unique": true
    }]);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/item",
        Some(&token),
        Some(json!({ "slug": "intro" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The draft moves the item to another value; both are held.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "welcome" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    for taken in ["intro", "welcome"] {
        let (status, _) = send_raw(
            &app.router,
            Method::POST,
            "/models/collections/pages/item",
            Some(&token),
            Some(json!({ "slug": taken })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "{taken} is still in use by the published copy or the working one"
        );
    }

    // Releasing the draft gives up the old published value and keeps the new one.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/item",
        Some(&token),
        Some(json!({ "slug": "intro" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the released value is free again: {}",
        String::from_utf8_lossy(&body)
    );
}

/// Taking a published item down frees the value its published copy held, while the working copy
/// keeps its own: nothing serves the published copy any more, but that draft is still an item's
/// content.
#[tokio::test]
async fn unpublishing_frees_the_published_value_but_not_the_working_one() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let unique = json!([{
        "name": "slug", "field_type": { "Text": {} },
        "required": false, "width": 12, "height": 1, "unique": true
    }]);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/schema",
        Some(&token),
        Some(unique),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let create = |slug: &str| {
        let token = token.clone();
        let router = app.router.clone();
        let slug = slug.to_string();
        async move {
            send_raw(
                &router,
                Method::POST,
                "/models/collections/pages/item",
                Some(&token),
                Some(json!({ "slug": slug })),
            )
            .await
        }
    };
    assert_eq!(create("intro").await.0, StatusCode::OK);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "welcome" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/items/1/unpublish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = create("intro").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "nothing serves the published copy any more: {}",
        String::from_utf8_lossy(&body)
    );
    let (status, _) = create("welcome").await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the working copy still holds its value"
    );
}

/// A unique value names one item, so both APIs can resolve one with a point read rather than a
/// scan - which is what a site needs to turn a slug into an item.
#[tokio::test]
async fn a_unique_value_resolves_to_its_item() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let schema = json!([
        {
            "name": "slug", "field_type": { "Text": {} },
            "required": false, "width": 12, "height": 1, "unique": true
        },
        { "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/item",
        Some(&token),
        Some(json!({ "slug": "intro", "title": "Hello" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The management API finds it by value, and says which item it is.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/pages/items/by/slug/intro",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], 1);
    assert_eq!(body["values"]["title"], "Hello");

    // A value nobody holds is a 404, and a field that is not unique is a 400: the value would
    // not name one item.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections/pages/items/by/slug/missing",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections/pages/items/by/title/Hello",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Nothing is published yet, so the delivery API answers "not found" rather than revealing
    // that a draft exists.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/content/collections/pages/items/by/slug/intro",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/collections/pages/items/by/slug/intro",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], 1);
    assert_eq!(body["values"]["title"], "Hello");
    assert!(body["last_published_at"].is_string());

    // A draft moving the value does not move the published one: the live value still resolves,
    // and the value waiting to be released does not (yet).
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/pages/items/1",
        Some(&token),
        Some(json!({ "slug": "welcome", "title": "Hello" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/collections/pages/items/by/slug/intro",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the served copy still uses it: {body}");
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/content/collections/pages/items/by/slug/welcome",
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a draft does not publish a value early"
    );
    // The management side still finds the item by either value: the index holds both.
    for value in ["intro", "welcome"] {
        let (status, body) = send(
            &app.router,
            Method::GET,
            &format!("/models/collections/pages/items/by/slug/{value}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}: {body}");
        assert_eq!(body["id"], 1);
    }

    // Releasing the change moves the delivery API over.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/pages/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/content/collections/pages/items/by/slug/intro",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/content/collections/pages/items/by/slug/welcome",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Making a field unique indexes what is already stored, and refuses while items would break it.
#[tokio::test]
async fn making_a_field_unique_indexes_the_items_already_stored() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let plain = json!([{
        "name": "slug", "field_type": { "Text": {} },
        "required": false, "width": 12, "height": 1
    }]);
    let unique = json!([{
        "name": "slug", "field_type": { "Text": {} },
        "required": false, "width": 12, "height": 1, "unique": true
    }]);
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/pages/schema",
        Some(&token),
        Some(plain.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let item = |slug: &str| {
        let token = token.clone();
        let router = app.router.clone();
        let slug = slug.to_string();
        async move {
            send_raw(
                &router,
                Method::POST,
                "/models/collections/pages/item",
                Some(&token),
                Some(json!({ "slug": slug })),
            )
            .await
        }
    };
    assert_eq!(item("intro").await.0, StatusCode::OK);
    assert_eq!(item("guide").await.0, StatusCode::OK);

    // Nothing is duplicated, so the constraint can be switched on...
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/pages/schema",
        Some(&token),
        Some(unique.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // ...and the items stored before it are in the index.
    let (status, body) = item("intro").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["field"],
        "slug"
    );

    // A duplicate that already exists is refused rather than silently indexed half-way.
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/pages/schema",
        Some(&token),
        Some(plain),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(item("intro").await.0, StatusCode::OK);
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/pages/schema",
        Some(&token),
        Some(unique),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{}", String::from_utf8_lossy(&body));
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["code"],
        "value_taken"
    );
}

/// A missing resource is a 404 and a duplicate is a 409; both used to be reported as 400,
/// which made "you asked for something that is not there" indistinguishable from "your
/// request was malformed".
#[tokio::test]
async fn missing_resources_are_404_and_duplicates_are_409() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    // Nothing exists yet, so every one of these is "not found".
    let not_found = [
        (Method::DELETE, "/models/collections/nope", None),
        (
            Method::PUT,
            "/models/collections/nope/schema",
            Some(sample_schema()),
        ),
        (Method::DELETE, "/models/single_pages/nope", None),
        (Method::PUT, "/models/single_pages/nope/item", Some(json!({}))),
        (Method::PUT, "/models/composite_fields/nope", Some(json!([]))),
        (Method::DELETE, "/models/composite_fields/nope", None),
    ];
    for (method, uri, body) in not_found {
        let (status, _) = send_raw(&app.router, method.clone(), uri, Some(&token), body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
    }

    // Creating the same thing twice conflicts with the current state.
    let duplicates = [
        ("/models/collections/dup/schema", sample_schema()),
        ("/models/single_pages/dup/schema", sample_schema()),
        ("/models/composite_fields/dup", sample_schema()),
    ];
    for (uri, body) in duplicates {
        let (status, _) = send_raw(
            &app.router,
            Method::POST,
            uri,
            Some(&token),
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "first POST {uri} should succeed");

        let (status, _) = send_raw(&app.router, Method::POST, uri, Some(&token), Some(body)).await;
        assert_eq!(status, StatusCode::CONFLICT, "second POST {uri}");
    }

    // A genuinely invalid payload is still a 400.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/bad/schema",
        Some(&token),
        Some(json!([{ "name": "", "field_type": "Number", "required": false, "width": 12, "height": 1 }])),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Composite values are read wrapped (`{id, values}`) and written bare, which used to make
/// a load-then-save round trip fail with "Unknown field 'id'".
#[tokio::test]
async fn composite_values_round_trip_and_references_are_validated() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let composite = json!([
        { "name": "description", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 },
        { "name": "score", "field_type": "Number", "required": false, "width": 6, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/composite_fields/seo",
        Some(&token),
        Some(composite),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let schema = json!([
        { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 },
        { "name": "seo", "field_type": { "CompositeField": { "id": "seo" } }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/posts/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A write sends the bare object of sub-values.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/posts/item",
        Some(&token),
        Some(json!({ "title": "Hello", "seo": { "description": "meta", "score": 7 } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A read wraps it, and re-submitting that unchanged has to be accepted.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/posts/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["seo"]["id"], "seo");
    assert_eq!(body["seo"]["values"]["description"], "meta");

    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/posts/items/1",
        Some(&token),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A dangling composite reference is refused when the schema is saved.
    let dangling = json!([{
        "name": "bad",
        "field_type": { "CompositeField": { "id": "missing" } },
        "required": false, "width": 12, "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/bad/schema",
        Some(&token),
        Some(dangling),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("does not exist"));

    // And a composite cannot reference itself.
    let cyclic = json!([{
        "name": "self",
        "field_type": { "CompositeField": { "id": "loop" } },
        "required": false, "width": 12, "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/composite_fields/loop",
        Some(&token),
        Some(cyclic),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("cannot reference itself"));
}

/// An array whose items are composites.
///
/// The element types are tried in the declared order and the first that accepts the JSON wins,
/// so a composite element is written either bare or with the `{id, values}` wrapper a read
/// produces. The wrapper is what keeps a value with two plausible definitions unambiguous.
#[tokio::test]
async fn composite_arrays_round_trip() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let definition = |field: &str| {
        json!([
            { "name": field, "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
        ])
    };
    for (id, field) in [("block_a", "heading"), ("block_b", "body")] {
        let (status, body) = send_raw(
            &app.router,
            Method::POST,
            &format!("/models/composite_fields/{id}"),
            Some(&token),
            Some(definition(field)),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    }

    let schema = json!([{
        "name": "blocks",
        "field_type": { "Array": [
            { "CompositeField": { "id": "block_a" } },
            { "CompositeField": { "id": "block_b" } }
        ] },
        "required": false,
        "width": 12,
        "height": 1
    }]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/page/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // One element names its definition, one leaves it off (the first declared type then wins).
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/page/item",
        Some(&token),
        Some(json!({ "blocks": [
            { "id": "block_b", "values": { "body": "second" } },
            { "heading": "first" }
        ] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/page/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["blocks"][0]["id"], "block_b");
    assert_eq!(body["blocks"][0]["values"]["body"], "second");
    assert_eq!(body["blocks"][1]["id"], "block_a");
    assert_eq!(body["blocks"][1]["values"]["heading"], "first");

    // What a read produced is accepted back unchanged, which is what lets an editor load an
    // item, change one field and save it.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/collections/page/items/1",
        Some(&token),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // A definition the array does not declare is refused rather than read as another one.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/page/item",
        Some(&token),
        Some(json!({ "blocks": [{ "id": "block_c", "values": { "heading": "x" } }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("does not match any declared array item type"), "unexpected body: {text}");

    // An element that is not an object at all is refused the same way.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/page/item",
        Some(&token),
        Some(json!({ "blocks": [7] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A block that holds blocks: the definition references itself through an array. That is a
    // loop in the reference graph and it is allowed, because the elements come from the value -
    // an empty array is where it stops.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/composite_fields/tree",
        Some(&token),
        Some(json!([
            { "name": "line", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 },
            { "name": "children", "field_type": { "Array": [{ "CompositeField": { "id": "tree" } }] },
              "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/nested/schema",
        Some(&token),
        Some(json!([{
            "name": "blocks",
            "field_type": { "Array": [{ "CompositeField": { "id": "tree" } }] },
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let tree = json!({ "blocks": [
        { "id": "tree", "values": { "line": "root", "children": [
            { "line": "leaf", "children": [] }
        ] } }
    ] });
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/nested/item",
        Some(&token),
        Some(tree),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/nested/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["blocks"][0]["values"]["line"], "root");
    assert_eq!(
        body["blocks"][0]["values"]["children"][0]["values"]["line"],
        "leaf"
    );
    assert_eq!(
        body["blocks"][0]["values"]["children"][0]["values"]["children"],
        json!([])
    );

    // A loop that never enters an array is still refused: the editor draws a composite's
    // sub-fields from the schema, so that one would never finish. Two definitions are needed
    // because the first has to exist before the second may point at it.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/composite_fields/loop_b",
        Some(&token),
        Some(json!([{
            "name": "value", "field_type": "Number",
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/composite_fields/loop_a",
        Some(&token),
        Some(json!([{
            "name": "to_b",
            "field_type": { "CompositeField": { "id": "loop_b" } },
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // Now closing the loop is refused, and it is the cycle that is reported rather than the
    // reference being missing.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/composite_fields/loop_b",
        Some(&token),
        Some(json!([{
            "name": "back",
            "field_type": { "CompositeField": { "id": "loop_a" } },
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("cannot reference itself"),
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );

    // And a composite array item that does not exist is refused when the schema is saved.
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/models/collections/unknown/schema",
        Some(&token),
        Some(json!([{
            "name": "blocks",
            "field_type": { "Array": [{ "CompositeField": { "id": "missing" } }] },
            "required": false, "width": 12, "height": 1
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("does not exist"),
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn deleting_a_collection_without_items_succeeds() {    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/empty/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/collections/empty",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// The page list is one request: every page's state, so a screen can show a status without asking
/// per page. Readable pages only, judged as elsewhere.
#[tokio::test]
async fn the_single_page_list_reports_every_page_state() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    for name in ["about", "contact"] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            &format!("/models/single_pages/{name}/schema"),
            Some(&token),
            Some(sample_schema()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(
            &app.router,
            Method::PUT,
            &format!("/models/single_pages/{name}/item"),
            Some(&token),
            Some(json!({ "title": name, "tags": [] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // One is published, the other is left as a draft.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/about/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/items/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["about"]["status"], "published");
    assert_eq!(body["about"]["has_draft"], false);
    assert_eq!(body["contact"]["status"], "draft");
    assert!(body["contact"]["updated_at"].is_string(), "{body}");

    // Saving after publishing leaves a working copy behind, which the list has to show.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/about/item",
        Some(&token),
        Some(json!({ "title": "About us", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/items/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(body["about"]["status"], "published");
    assert_eq!(body["about"]["has_draft"], true, "the working copy is not live yet");

    // A read-only account sees the pages it may read, and nothing else.
    let (status, _) = create_account(
        &app,
        json!({
            "username": "page-reader",
            "password": "reader-password",
            "is_admin": false,
            "permission": { "can_view": true, "can_edit": false, "can_publish": false }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, login_body) = login(&app, "page-reader", "reader-password").await;
    let reader = login_body["token"].as_str().unwrap().to_string();
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/items/metadata",
        Some(&reader),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["about"]["status"], "published", "{body}");
}

#[tokio::test]
async fn single_page_crud_round_trip() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/home/item",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // FieldValueResponse is untagged, so the text value comes back bare.
    assert_eq!(body["title"], "Home");

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/single_pages/home",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// -------------------------------------------------------------------------------- images

/// The upload endpoint exists only where the CMS itself stores the bytes. On AWS the browser
/// PUTs straight to S3 with a presigned URL, and that path is covered by the adapter's own
/// tests (`aws::repository::images`), because there is no route in this router to drive.
#[tokio::test]
async fn image_upload_requires_auth_but_downloads_are_public() {
    if !Backend::SERVES_IMAGE_BYTES {
        eprintln!("skipped: this backend hands the browser a signed URL instead of serving bytes");
        return;
    }
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "logo.png", "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let upload_url = body["upload_url"].as_str().unwrap().to_string();
    assert!(upload_url.starts_with("/images/"), "unexpected url: {upload_url}");

    // Uploading without a token is refused even with a valid capability token.
    let (status, _) = send_raw(&app.router, Method::PUT, &upload_url, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // With the token it succeeds.
    let request = Request::builder()
        .method(Method::PUT)
        .uri(&upload_url)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from("PNG-BYTES"))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // Downloading needs no token (an <img> tag cannot send one).
    let path = upload_url.split('?').next().unwrap();
    let (status, bytes) = send_raw(&app.router, Method::GET, path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"PNG-BYTES");

    // The capability token was consumed.
    let request = Request::builder()
        .method(Method::PUT)
        .uri(&upload_url)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from("PNG-BYTES"))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// An image can be renamed without touching what it is: the id, the bytes and the URL stay, so
/// content that references it is unaffected and the rename is undoable.
#[tokio::test]
async fn an_image_can_be_renamed_without_touching_its_bytes() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, info) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "photo.png", "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = info["id"].as_u64().expect("the image id");
    let url = info["url"].as_str().unwrap().to_string();

    // The name can be anything a reader recognises, including non-ASCII.
    let (status, bytes) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/models/images/{id}"),
        Some(&token),
        Some(json!({ "original_filename": "  表紙の写真.png  " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    assert!(bytes.is_empty(), "a mutation answers with an empty body");

    let (status, body) = send(&app.router, Method::GET, "/models/images", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["id"], id, "the id does not change");
    assert_eq!(body[0]["url"], url, "nor does where the bytes live");
    assert_eq!(body[0]["original_filename"], "表紙の写真.png", "trimmed, and otherwise as given");

    // A name the library could not show, or one that is a path, is refused.
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/models/images/{id}"),
        Some(&token),
        Some(json!({ "original_filename": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/models/images/{id}"),
        Some(&token),
        Some(json!({ "original_filename": "../../etc/passwd" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Renaming something that is not there reports it instead of quietly succeeding.
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        "/models/images/9999",
        Some(&token),
        Some(json!({ "original_filename": "other.png" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A read-only account may not rename; the middleware decides, not the handler.
    let (status, _) = create_account(
        &app,
        json!({
            "username": "viewer-for-images",
            "password": "viewer-password",
            "is_admin": false,
            "permission": { "can_view": true, "can_edit": false, "can_publish": false }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = login(&app, "viewer-for-images", "viewer-password").await;
    assert_eq!(status, StatusCode::OK);
    let viewer = body["token"].as_str().unwrap().to_string();
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/models/images/{id}"),
        Some(&viewer),
        Some(json!({ "original_filename": "theirs.png" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Uploading an image is shared work: an editor with a grant for one collection needs the images
/// that collection uses, so uploading does not need the account-wide permission. Changing or
/// deleting what is already there does, because other content may be using it.
#[tokio::test]
async fn uploading_an_image_needs_edit_somewhere_but_changing_one_needs_it_everywhere() {
    let app = test_app().await;
    let admin = app.admin_token.clone();

    // A collection exists for the grant to name.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&admin),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // An editor of that one collection: no account-wide permission at all.
    let (status, created) = create_account(
        &app,
        json!({
            "username": "collection-editor",
            "password": "editor-password",
            "is_admin": false,
            "permission": { "can_view": true, "can_edit": false, "can_publish": false },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let editor_id = created["id"].as_str().unwrap().to_string();
    // The grant is set on the account, which is where the server keeps overrides.
    let (status, body) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{editor_id}"),
        Some(&admin),
        Some(json!({
            "collection_permissions": {
                "blog": { "can_view": true, "can_edit": true, "can_publish": false }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = login(&app, "collection-editor", "editor-password").await;
    assert_eq!(status, StatusCode::OK);
    let editor = body["token"].as_str().unwrap().to_string();

    // Uploading: allowed, because they may edit a collection that uses images.
    let (status, info) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&editor),
        Some(json!({ "original_filename": "from-editor.png", "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{info}");
    let id = info["id"].as_u64().unwrap();

    // Renaming and deleting: refused, because the library is shared with everyone.
    for (method, path, body) in [
        (
            Method::PUT,
            format!("/models/images/{id}"),
            Some(json!({ "original_filename": "theirs.png" })),
        ),
        (Method::DELETE, format!("/models/images/{id}"), None),
        (
            Method::POST,
            format!("/models/images/{id}/replace"),
            Some(json!({ "ext": "png" })),
        ),
    ] {
        let (status, _) = send_raw(&app.router, method.clone(), &path, Some(&editor), body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
    }

    // A viewer may not even ask for an upload URL.
    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );
    let (status, body) = login(&app, VIEWER_EMAIL, VIEWER_PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let viewer = body["token"].as_str().unwrap().to_string();
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&viewer),
        Some(json!({ "original_filename": "nope.png", "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The account-wide editor may of course still upload.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&admin),
        Some(json!({ "original_filename": "from-admin.png", "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// Replacing an image keeps everything about its identity: the id, the name it is shown under, when
/// it entered the library, and every reference to it. Only what it shows changes - and the bytes it
/// used to show are gone, which is what stops a cached URL serving the old picture.
#[tokio::test]
async fn an_image_can_be_replaced_keeping_its_id() {
    if !Backend::SERVES_IMAGE_BYTES {
        eprintln!("skipped: this backend hands the browser a signed URL instead of serving bytes");
        return;
    }
    let app = test_app().await;
    let token = app.admin_token.clone();

    /// Upload `bytes` the way the UI does, answering the info the server recorded.
    async fn upload<B: sl_cms_tests::TestBackend>(
        app: &sl_cms_tests::TestApp<B>,
        token: &str,
        name: &str,
        bytes: &str,
    ) -> Value {
        let (status, info) = send(
            &app.router,
            Method::POST,
            "/models/images/get_upload_url",
            Some(token),
            Some(json!({ "original_filename": name, "ext": "png" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let upload_url = info["upload_url"].as_str().unwrap().to_string();
        let request = Request::builder()
            .method(Method::PUT)
            .uri(&upload_url)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(bytes.to_string()))
            .unwrap();
        let response = app.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        info
    }

    let first = upload(&app, &token, "logo.png", "OLD-BYTES").await;
    let id = first["id"].as_u64().unwrap();
    let old_url = first["url"].as_str().unwrap().to_string();
    let old_path = old_url.clone();

    let (_, before) = send(&app.router, Method::GET, "/models/images", Some(&token), None).await;
    let uploaded_at = before[0]["uploaded_at"].clone();

    // Ask where to put replacement bytes: the record is untouched at this point.
    let (status, replacement) = send(
        &app.router,
        Method::POST,
        &format!("/models/images/{id}/replace"),
        Some(&token),
        Some(json!({ "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replacement}");
    let new_file = replacement["file_name"].as_str().unwrap().to_string();
    let upload_url = replacement["upload_url"].as_str().unwrap().to_string();
    assert_ne!(new_file, "", "a replacement gets its own file name");

    // Until the bytes arrive, the image still serves what it did before.
    let (status, bytes) = send_raw(&app.router, Method::GET, &old_path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"OLD-BYTES");

    // Applying a replacement before the upload is refused: nothing points at missing bytes.
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/models/images/{id}"),
        Some(&token),
        Some(json!({ "file_name": new_file })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let request = Request::builder()
        .method(Method::PUT)
        .uri(&upload_url)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from("NEW-BYTES"))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/models/images/{id}"),
        Some(&token),
        Some(json!({ "file_name": new_file })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The same image: same id, same name, same place in the library's history.
    let (status, body) = send(&app.router, Method::GET, "/models/images", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["id"], id);
    assert_eq!(body[0]["original_filename"], "logo.png");
    assert_eq!(body[0]["uploaded_at"], uploaded_at);
    let new_url = body[0]["url"].as_str().unwrap().to_string();
    assert_ne!(new_url, old_url, "the bytes moved, so the URL did");

    // The new bytes are served, and the old URL is gone rather than serving what it did.
    let new_path = new_url.clone();
    let (status, bytes) = send_raw(&app.router, Method::GET, &new_path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"NEW-BYTES");
    let (status, _) = send_raw(&app.router, Method::GET, &old_path, None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The durable link follows the image: it is the id that names it, not the file. This backend
    // serves the bytes itself, so the link answers with them rather than pointing elsewhere.
    let (status, bytes) = send_raw(&app.router, Method::GET, &format!("/images/by-id/{id}"), None, None).await;
    assert_eq!(status, StatusCode::OK, "the id link should serve what the image shows now");
    assert_eq!(bytes, b"NEW-BYTES");

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/images/by-id/{id}"))
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-cache",
        "the bytes behind an id can change, so this answer must not be cached"
    );

    // A link to an image that is not there is a 404 rather than a redirect to nowhere.
    let request = Request::builder()
        .method(Method::GET)
        .uri("/images/by-id/9999")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Two images cannot end up serving the same bytes: the second image's file is refused.
    let other = upload(&app, &token, "other.png", "OTHER-BYTES").await;
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/models/images/{id}"),
        Some(&token),
        Some(json!({ "file_name": new_file })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "its own bytes are still a no-op");

    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/models/images/{id}"),
        Some(&token),
        Some(json!({ "file_name": other["url"].as_str().unwrap().rsplit('/').next().unwrap() })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The image library: what the admin screen lists, and what deleting an image does.
#[tokio::test]
async fn images_can_be_listed_and_deleted() {
    if !Backend::SERVES_IMAGE_BYTES {
        eprintln!("skipped: this backend hands the browser a signed URL instead of serving bytes");
        return;
    }
    let app = test_app().await;
    let token = app.admin_token.clone();

    // The library is not public.
    let (status, _) = send_raw(&app.router, Method::GET, "/models/images", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = send(&app.router, Method::GET, "/models/images", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]), "nothing has been uploaded yet");

    // Upload one, through the same two steps the UI takes.
    let (status, info) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "photo.png", "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = info["id"].as_u64().expect("the image id");
    let upload_url = info["upload_url"].as_str().unwrap().to_string();

    let request = Request::builder()
        .method(Method::PUT)
        .uri(&upload_url)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::from("PNG-BYTES"))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // It is listed with the metadata the screen shows.
    let (status, body) = send(&app.router, Method::GET, "/models/images", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["id"], id);
    assert_eq!(body[0]["original_filename"], "photo.png");
    assert!(body[0]["url"].as_str().unwrap().starts_with("/images/"));
    assert!(body[0]["uploaded_at"].is_string());

    let path = body[0]["url"].as_str().unwrap().to_string();
    let (status, bytes) = send_raw(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"PNG-BYTES");

    // Deleting answers with an empty body, like every other mutation.
    let (status, bytes) = send_raw(
        &app.router,
        Method::DELETE,
        &format!("/models/images/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.is_empty());

    // Gone from the list, and the bytes went with it.
    let (status, body) = send(&app.router, Method::GET, "/models/images", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
    let (status, _) = send_raw(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Deleting it twice reports the missing image instead of succeeding quietly.
    let (status, _) = send_raw(
        &app.router,
        Method::DELETE,
        &format!("/models/images/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn rejects_unsafe_image_file_names_and_extensions() {
    if !Backend::SERVES_IMAGE_BYTES {
        eprintln!("skipped: this backend hands the browser a signed URL instead of serving bytes");
        return;
    }
    let app = test_app().await;
    let token = app.admin_token.clone();

    // Percent-encoded traversal: axum decodes path segments after routing, so this must be
    // rejected by the handler rather than escaping the images directory.
    let (status, _) = send_raw(
        &app.router,
        Method::GET,
        "/images/..%2F..%2F..%2FCargo.toml",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "x", "ext": "../../etc/passwd" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------------- CORS

#[tokio::test]
async fn cors_allows_only_configured_origins() {
    let app = test_app().await;

    let request = Request::builder()
        .method(Method::GET)
        .uri("/")
        .header(header::ORIGIN, "http://localhost:4200")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(),
        "http://localhost:4200"
    );

    let request = Request::builder()
        .method(Method::GET)
        .uri("/")
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert!(response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}

// ------------------------------------------------------------------- draft / published

/// Nothing is public until it is published, and unpublishing hides it again.
#[tokio::test]
async fn only_published_collection_items_reach_the_content_api() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for title in ["First", "Second"] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/models/collections/blog/item",
            Some(&token),
            Some(json!({ "title": title, "tags": [] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // Both items are drafts: the collection is served, but empty, and it is not advertised
    // by the index route at all.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["schema"][0]["name"], "title");
    assert_eq!(body["items"], json!([]));

    let (status, body) = send(&app.router, Method::GET, "/content/collections", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    // A draft answers 404 rather than 403, so the delivery API does not confirm that
    // unpublished content exists.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog/items/1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The admin side still reports a status for every item, defaults included.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items/1/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "draft");
    assert_eq!(body["published_at"], Value::Null);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["1"]["status"], "draft");
    assert_eq!(body["2"]["status"], "draft");

    // Publish the first item.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "published");
    assert!(
        body["published_at"].is_string(),
        "publishing must record a timestamp: {body}"
    );

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog/items/1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], 1);
    assert_eq!(body["values"]["title"], "First");
    assert!(body["published_at"].is_string());
    // Values are untyped, but a single item is fetched by a client that already knows the
    // schema, so only the list route carries it.
    assert!(body.get("schema").is_none());

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["values"]["title"], "First");
    // The second item stays a draft inside the same collection.
    assert_eq!(body["items"][0]["id"], 1);

    let (status, body) = send(&app.router, Method::GET, "/content/collections", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(["blog"]));

    // Publishing something that does not exist is a 404, not a silent success.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/items/99/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Unpublishing removes it from the public API. The publication date stays: the item was
    // published, and that does not stop being true.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/items/1/unpublish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "draft");
    assert!(body["published_at"].is_string());

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog/items/1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send(&app.router, Method::GET, "/content/collections", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    // Deleting a published item must not leave its status behind either: the collection
    // index goes empty with it.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/collections/blog/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::GET, "/content/collections", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn only_published_single_pages_reach_the_content_api() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(&app.router, Method::GET, "/content/single-pages", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/content/single-pages/home",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/home/item/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "draft");

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "published");

    let (status, body) = send(&app.router, Method::GET, "/content/single-pages", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(["home"]));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/single-pages/home",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Home");
    assert_eq!(body["schema"][0]["name"], "title");
    assert!(body["published_at"].is_string());

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/unpublish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/content/single-pages/home",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Deleting a published page removes its metadata, so recreating the same name starts
    // out hidden again.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/single_pages/home",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home again", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::GET, "/content/single-pages", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

/// Publishing is a write, so a read-only account cannot do it.
#[tokio::test]
async fn publishing_requires_edit_permission() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Hello", "tags": [] })),
    )
    .await;

    assert_eq!(
        create_user(&app, VIEWER_EMAIL, VIEWER_PASSWORD, false).await,
        StatusCode::CREATED
    );
    let (status, body) = login(&app, VIEWER_EMAIL, VIEWER_PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
    let viewer_token = body["token"].as_str().unwrap().to_string();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/items/1/publish",
        Some(&viewer_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The public route stays reachable without a token.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// ------------------------------------------------------------------------------- webhooks

/// One delivery the CMS posted to the test receiver.
#[derive(Debug, Clone)]
struct ReceivedWebhook {
    event: Option<String>,
    delivery: Option<String>,
    signature: Option<String>,
    body: Vec<u8>,
}

/// A local receiver for webhook deliveries, answering `status` to everything.
async fn start_webhook_receiver(
    status: StatusCode,
) -> (String, Arc<tokio::sync::Mutex<Vec<ReceivedWebhook>>>) {
    let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let sink = received.clone();

    let app = Router::new().route(
        "/hook",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let sink = sink.clone();
                async move {
                    let header = |name: &str| {
                        headers
                            .get(name)
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string)
                    };
                    sink.lock().await.push(ReceivedWebhook {
                        event: header("x-cms-event"),
                        delivery: header("x-cms-delivery"),
                        signature: header("x-cms-signature"),
                        body: body.to_vec(),
                    });
                    status
                }
            },
        ),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{addr}/hook"), received)
}

/// Wait for `count` deliveries. Delivery is asynchronous by design, so the test polls.
async fn wait_for_webhooks(
    received: &Arc<tokio::sync::Mutex<Vec<ReceivedWebhook>>>,
    count: usize,
) -> Vec<ReceivedWebhook> {
    for _ in 0..300 {
        {
            let received = received.lock().await;
            if received.len() >= count {
                return received.clone();
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let received = received.lock().await;
    panic!(
        "expected {count} webhook deliveries, got {}",
        received.len()
    );
}

/// Assert that a mutating call succeeded *and* answered with no body.
///
/// The browser client parses every non-empty body as JSON, so a plain-text acknowledgement
/// is indistinguishable from a failure for it.
async fn expect_empty_body(
    app: &TestApp,
    token: &str,
    method: Method,
    uri: &str,
    body: Option<Value>,
) {
    let (status, bytes) = send_raw(&app.router, method.clone(), uri, Some(token), body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}");
    assert!(
        bytes.is_empty(),
        "{method} {uri} answered with a body: {:?}",
        String::from_utf8_lossy(&bytes)
    );
}

/// Mutations answer with an empty body on purpose; see [`expect_empty_body`].
#[tokio::test]
async fn mutations_answer_with_an_empty_body() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let schema = sample_schema();

    expect_empty_body(&app, &token, Method::POST, "/models/collections/blog/schema", Some(schema.clone())).await;
    expect_empty_body(&app, &token, Method::PUT, "/models/collections/blog/schema", Some(schema.clone())).await;

    // Creating an item answers with its id, so it is checked separately from the mutations.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&token),
        Some(json!({ "title": "Hello", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = body.as_u64().expect("the item id");

    expect_empty_body(
        &app,
        &token,
        Method::PUT,
        &format!("/models/collections/blog/items/{id}"),
        Some(json!({ "title": "Edited", "tags": [] })),
    )
    .await;
    expect_empty_body(&app, &token, Method::DELETE, &format!("/models/collections/blog/items/{id}"), None).await;
    expect_empty_body(&app, &token, Method::DELETE, "/models/collections/blog", None).await;

    expect_empty_body(&app, &token, Method::POST, "/models/single_pages/home/schema", Some(schema.clone())).await;
    expect_empty_body(&app, &token, Method::PUT, "/models/single_pages/home/schema", Some(schema.clone())).await;
    expect_empty_body(
        &app,
        &token,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    expect_empty_body(&app, &token, Method::DELETE, "/models/single_pages/home", None).await;

    expect_empty_body(&app, &token, Method::POST, "/models/composite_fields/seo", Some(schema.clone())).await;
    expect_empty_body(&app, &token, Method::PUT, "/models/composite_fields/seo", Some(schema.clone())).await;
    expect_empty_body(&app, &token, Method::DELETE, "/models/composite_fields/seo", None).await;
}

/// Create `collection` with one item and return the new item's id.
async fn create_sample_item(app: &TestApp, collection: &str) -> u64 {
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/{collection}/schema"),
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/{collection}/item"),
        Some(&app.admin_token),
        Some(json!({ "title": "Hello", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body.as_u64().expect("the item id")
}

#[tokio::test]
async fn publishing_notifies_the_configured_webhook() {
    let (url, received) = start_webhook_receiver(StatusCode::OK).await;
    let secret = b"webhook-secret".to_vec();
    let app = test_app_with_notifier(Arc::new(
        WebhookNotifier::new(vec![url], Some(secret.clone())).unwrap(),
    ))
    .await;

    let item_id = create_sample_item(&app, "blog").await;

    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/publish"),
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let deliveries = wait_for_webhooks(&received, 1).await;
    let first = &deliveries[0];
    assert_eq!(first.event.as_deref(), Some("collection_item.published"));
    assert!(
        first.delivery.is_some(),
        "a delivery id lets a receiver ignore a duplicate"
    );

    let body: Value = serde_json::from_slice(&first.body).unwrap();
    assert_eq!(body["event"], "collection_item.published");
    assert_eq!(body["collection"], "blog");
    assert_eq!(body["id"], item_id);
    assert_eq!(body["status"], "published");
    assert!(body["published_at"].is_string());
    // A collection event says nothing about single pages.
    assert!(body.get("page").is_none());

    // The signature has to verify over the exact bytes that were sent.
    let expected = format!("sha256={}", sl_cms_core::webhook::sign(&secret, &first.body));
    assert_eq!(first.signature.as_deref(), Some(expected.as_str()));

    // Unpublishing is an event too: the site has to be rebuilt without the page.
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/unpublish"),
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let deliveries = wait_for_webhooks(&received, 2).await;
    assert_eq!(
        deliveries[1].event.as_deref(),
        Some("collection_item.unpublished")
    );
    let body: Value = serde_json::from_slice(&deliveries[1].body).unwrap();
    assert_eq!(body["status"], "draft");
    // The publication date stays part of the record; the publisher does not.
    assert!(body["published_at"].is_string());
    assert!(body["published_by"].is_null());
}

// ------------------------------------------------------------------- preview links

/// A preview link shows one working copy to someone with no account, until it expires.
#[tokio::test]
async fn a_preview_link_shows_one_working_copy_without_a_token() {
    let app = test_app().await;
    // The first call also creates the collection's schema, so the second item goes straight
    // in.
    let first = create_sample_item(&app, "blog").await;
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&app.admin_token),
        Some(json!({ "title": "Second", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let second = body.as_u64().expect("the second item id");

    // An unpublished edit is what a preview is for.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/models/collections/blog/items/{first}"),
        Some(&app.admin_token),
        Some(json!({ "title": "Draft wording", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mint = |id: u64| {
        let app = &app;
        async move {
            let (status, body) = send(
                &app.router,
                Method::POST,
                &format!("/models/collections/blog/items/{id}/preview-link"),
                Some(&app.admin_token),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "preview link for {id}");
            body
        }
    };

    let link = mint(first).await;
    let path = link["path"].as_str().unwrap().to_string();
    assert!(path.starts_with(&format!("/preview/collections/blog/items/{first}?token=")));
    assert!(link["expires_at"].is_string());

    // Opening it needs no token: the signature is the credential.
    let (status, body) = send(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], first);
    assert_eq!(body["values"]["title"], "Draft wording", "作業コピーが見える");
    assert_eq!(body["schema"][0]["name"], "title");

    // Looking at a preview publishes nothing.
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/content/collections/blog/items/{first}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A link for the other item does not open this one - the target is part of what was
    // signed, so a holder cannot go wandering.
    let other_path = mint(second).await["path"].as_str().unwrap().to_string();
    let (status, _) = send(&app.router, Method::GET, &other_path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    let swapped = path.replace(
        &format!("items/{first}"),
        &format!("items/{second}"),
    );
    let (status, _) = send(&app.router, Method::GET, &swapped, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "差し替えたリンクは通らない");

    // A tampered token, a wrong one, and no token at all.
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("{path}x"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/preview/collections/blog/items/{first}?token=1.deadbeef"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/preview/collections/blog/items/{first}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "トークンが無ければクエリとして不正");
}

/// The same for a single page, and the kind of target is signed too: a page's link cannot be
/// pointed at a collection item or the other way round.
#[tokio::test]
async fn a_single_page_preview_link_is_public_and_only_opens_that_page() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Unpublished home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/preview-link",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let path = body["path"].as_str().unwrap().to_string();
    assert!(path.starts_with("/preview/single_pages/home?token="));

    let (status, body) = send(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Unpublished home");
    assert_eq!(body["schema"][0]["name"], "title");

    // Another page's link, and a link for a collection item, are both refused.
    let (status, _) = send(
        &app.router,
        Method::GET,
        &path.replace("single_pages/home", "single_pages/other"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(
        &app.router,
        Method::GET,
        &path.replace("/preview/single_pages/home", "/preview/collections/home/items/1"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A page that does not exist has no link to mint.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/missing/preview-link",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// An expired link is refused, and told apart from one that was never ours.
#[tokio::test]
async fn an_expired_preview_link_is_refused() {
    let app = test_app().await;
    let item_id = create_sample_item(&app, "blog").await;

    // Minted two hours in the past with a one-hour life, which is what waiting would take.
    let expired = app.module.preview_links.issue(
        &PreviewTarget::CollectionItem {
            collection: "blog".to_string(),
            item_id,
        },
        chrono::Utc::now() - chrono::Duration::hours(2),
    );

    let (status, _) = send(&app.router, Method::GET, &expired.path, None, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "期限切れは 403");

    // The same link with the expiry pushed out is not ours, which is the whole point of
    // signing the deadline.
    let (_, signature) = expired.path.rsplit_once('.').unwrap();
    let forged = format!(
        "/preview/collections/blog/items/{item_id}?token={}.{signature}",
        (chrono::Utc::now() + chrono::Duration::days(30)).timestamp()
    );
    let (status, _) = send(&app.router, Method::GET, &forged, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Per-resource permissions: an account can be trusted with one collection and not the rest,
/// and the account-wide role is what it has everywhere else.
#[tokio::test]
async fn a_collection_grant_applies_to_that_collection_only() {
    let app = test_app().await;
    let admin = app.admin_token.clone();

    for collection in ["blog", "news", "docs"] {
        create_sample_item(&app, collection).await;
    }
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&admin),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A viewer everywhere to start with...
    let (status, created) = create_account(&app, json!({
            "username": "scoped@example.com",
            "password": "scoped-password",
            "is_admin": false,
            "permission": Permission::viewer(),
        }))
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let scoped_id = created["id"].as_str().unwrap().to_string();

    // ...then trusted with `blog` (edit), `news` (edit and release) and nothing of `home`.
    let deny = json!({ "can_view": false, "can_edit": false, "can_publish": false });
    let (status, updated) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{scoped_id}"),
        Some(&admin),
        Some(json!({
            "collection_permissions": {
                "blog": Permission::editor(),
                "news": Permission { can_view: true, can_edit: true, can_publish: true },
            },
            "single_page_permissions": { "home": deny },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    // The overrides come back, which is what the account screen reads.
    assert_eq!(updated["collection_permissions"]["blog"]["can_edit"], true);
    assert_eq!(updated["collection_permissions"]["blog"]["can_publish"], false);
    assert_eq!(updated["single_page_permissions"]["home"]["can_view"], false);

    let (_, body) = login(&app, "scoped@example.com", "scoped-password").await;
    let scoped = body["token"].as_str().unwrap().to_string();
    let edit = json!({ "title": "edited by the scoped account", "tags": [] });

    // `blog` is editable but not releasable.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/collections/blog/items/1",
        Some(&scoped),
        Some(edit.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "grant したコレクションは編集できる");
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/items/1/publish",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "公開の grant は無い");

    // `news` carries the publish grant, so both work there.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/collections/news/items/1",
        Some(&scoped),
        Some(edit.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/news/items/1/publish",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // `docs` has no override: the account-wide viewer role applies, so it reads and cannot
    // write.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections/docs/items/1",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "共通ロールはどこでも効く");
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/collections/docs/items/1",
        Some(&scoped),
        Some(edit),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "grant の無いコレクションは読み取りだけ");

    // `home` was denied, so even reading it is refused...
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/home/item",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "拒否したページは読めない");
    // ...and the list does not offer it, so the sidebar cannot lead there.
    let (status, pages) = send(
        &app.router,
        Method::GET,
        "/models/single_pages",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pages, json!([]), "deny したページは一覧に出ない");

    let (status, collections) = send(
        &app.router,
        Method::GET,
        "/models/collections",
        Some(&scoped),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = collections
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect();
    assert!(names.contains(&"blog") && names.contains(&"news") && names.contains(&"docs"));

    // An administrator keeps seeing and doing everything.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/single_pages/home/item",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// A grant for something that does not exist is a typo, not a permission.
#[tokio::test]
async fn a_grant_for_an_unknown_resource_is_refused() {
    let app = test_app().await;
    let admin = app.admin_token.clone();
    create_sample_item(&app, "blog").await;

    let (status, created) = create_account(&app, json!({
            "username": "typo@example.com",
            "password": "typo-password",
            "is_admin": false,
            "permission": Permission::viewer(),
        }))
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_string();

    // A typo in the collection name is refused...
    let (status, body) = send_raw(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "collection_permissions": { "blgo": Permission::editor() } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("blgo"),
        "the answer names the typo: {}",
        String::from_utf8_lossy(&body)
    );

    // ...as is one in a page name, while the real names are accepted.
    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "single_page_permissions": { "nope": Permission::viewer() } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "collection_permissions": { "blog": Permission::editor() } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

/// The whole password reset flow over HTTP: an administrator mints a link, the account's owner
/// uses it without being signed in, and the link is spent.
#[tokio::test]
async fn an_administrator_can_issue_a_reset_link_that_works_once() {
    // This deployment leaves sign-in to an identity provider: there is no password endpoint to
    // drive, which is what `GET /auth/capabilities` reports to a client.
    if !Backend::PASSWORD_LOGIN {
        eprintln!("skipped: this deployment signs users in through an identity provider");
        return;
    }
    let app = test_app().await;
    let admin = app.admin_token.clone();

    let (status, created) = create_account(&app, json!({
            "username": "ops",
            "password": "ops-password",
            "is_admin": false,
            "permission": Permission::viewer(),
        }))
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_string();

    // A session that existed before the reset, to watch it die.
    let (_, body) = login(&app, "ops", "ops-password").await;
    let old_token = body["token"].as_str().unwrap().to_string();

    // Only an administrator may mint a link.
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/auth/users/{id}/password-reset-link"),
        Some(&old_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/auth/users/{id}/password-reset-link"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = body["token"].as_str().unwrap().to_string();
    assert!(body["expires_at"].is_string());

    // The link is used without any credentials at all.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/auth/password-reset",
        None,
        Some(json!({ "token": token, "new_password": "chosen-by-ops" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let fresh_token = body["token"]
        .as_str()
        .expect("the caller is signed in with the new password")
        .to_string();

    // The new password works, the old one does not, and the old session is gone.
    let (status, _) = login(&app, "ops", "chosen-by-ops").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = login(&app, "ops", "ops-password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(&app.router, Method::GET, "/auth/me", Some(&old_token), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(&app.router, Method::GET, "/auth/me", Some(&fresh_token), None).await;
    assert_eq!(status, StatusCode::OK);

    // Using the same link again is refused, and does not change the password back.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/password-reset",
        None,
        Some(json!({ "token": token, "new_password": "someone-elses-choice" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = login(&app, "ops", "chosen-by-ops").await;
    assert_eq!(status, StatusCode::OK, "パスワードは変わっていない");

    // Nothing usable comes out of a token that was never issued.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/password-reset",
        None,
        Some(json!({ "token": "nonsense", "new_password": "irrelevant-password" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A throttled sign-in is a 429 with `Retry-After`, so a client knows when to come back
/// instead of guessing.
#[tokio::test]
async fn a_throttled_sign_in_answers_429_with_retry_after() {
    // This deployment leaves sign-in to an identity provider: there is no password endpoint to
    // drive, which is what `GET /auth/capabilities` reports to a client.
    if !Backend::PASSWORD_LOGIN {
        eprintln!("skipped: this deployment signs users in through an identity provider");
        return;
    }
    let app = test_app().await;

    for attempt in 1..=5 {
        let (status, _) = login(&app, ADMIN_EMAIL, "not-the-password").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{attempt} 回目");
    }

    let request = Request::builder()
        .method(Method::POST)
        .uri("/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "username": ADMIN_EMAIL, "password": ADMIN_PASSWORD }).to_string(),
        ))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

    let retry_after: u64 = response
        .headers()
        .get(header::RETRY_AFTER)
        .expect("Retry-After が付く")
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=15 * 60).contains(&retry_after), "retry-after={retry_after}");

    // Someone else's failures are their own.
    let (status, _) = login(&app, "someone-else@example.com", "whatever").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A password change ends the sessions that came before it. The account that asked for the
/// change gets a token for the new generation, so the screen they are on keeps working.
#[tokio::test]
async fn changing_a_password_ends_the_tokens_that_came_before_it() {
    // This deployment leaves sign-in to an identity provider: there is no password endpoint to
    // drive, which is what `GET /auth/capabilities` reports to a client.
    if !Backend::PASSWORD_LOGIN {
        eprintln!("skipped: this deployment signs users in through an identity provider");
        return;
    }
    let app = test_app().await;
    let admin = app.admin_token.clone();

    // A second account, so the administrator's own session is not the one under test.
    let (status, created) = create_account(&app, json!({
            "username": "editor@example.com",
            "password": "editor-password",
            "is_admin": false,
            "permission": Permission::editor(),
        }))
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let editor_id = created["id"].as_str().unwrap().to_string();

    let (_, body) = login(&app, "editor@example.com", "editor-password").await;
    let editor = body["token"].as_str().unwrap().to_string();

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/auth/me/password",
        Some(&editor),
        Some(json!({ "current_password": "editor-password", "new_password": "editor-password-2" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let replacement = body["token"]
        .as_str()
        .expect("the caller gets a token for the new generation")
        .to_string();

    // The token that existed before the change is rejected...
    let (status, _) = send(&app.router, Method::GET, "/auth/me", Some(&editor), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // ...the replacement works...
    let (status, _) = send(&app.router, Method::GET, "/auth/me", Some(&replacement), None).await;
    assert_eq!(status, StatusCode::OK);

    // ...and an administrator resetting the password ends that one too, while leaving the
    // administrator's own session alone.
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/auth/users/{editor_id}/password"),
        Some(&admin),
        Some(json!({ "password": "reset-by-an-admin" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(&app.router, Method::GET, "/auth/me", Some(&replacement), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(&app.router, Method::GET, "/auth/me", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "自分のセッションは終わらない");
}

/// Publishing leaves an audit trail: the account is recorded on the item, the admin side
/// can read it back, and the public delivery API never sees it.
#[tokio::test]
async fn publishing_records_who_did_it_and_keeps_that_off_the_public_api() {
    let app = test_app().await;
    let item_id = create_sample_item(&app, "blog").await;

    // A second account, so "who published" is not just the admin who set everything up.
    let (status, body) = create_account(&app, json!({
            "username": "publisher@example.com",
            "password": "publisher-password",
            "is_admin": false,
            "permission": Permission { can_view: true, can_edit: true, can_publish: true },
        }))
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (_, body) = login(&app, "publisher@example.com", "publisher-password").await;
    let publisher = body["token"].as_str().unwrap().to_string();

    let publish = format!("/models/collections/blog/items/{item_id}/publish");
    let (status, body) = send(&app.router, Method::POST, &publish, Some(&publisher), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["published_by"]["username"], "publisher@example.com");
    assert!(body["published_by"]["id"].is_string(), "the account id travels too: {body}");

    // The admin metadata endpoint reports it as well: that is what the UI reads.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/models/collections/blog/items/{item_id}/metadata"),
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["published_by"]["username"], "publisher@example.com");

    // The delivery API says when it was published, never who did it: the operator's
    // address is not something a public site should be able to read.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/content/collections/blog/items/{item_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["published_at"].is_string());
    assert!(
        body.get("published_by").is_none(),
        "the publisher must not reach the public API: {body}"
    );

    // Unpublishing takes the item off the site, so it forgets who put it there. The
    // publication date stays: it is a fact about the item, not about the current state.
    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/unpublish"),
        Some(&publisher),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["published_by"].is_null());
    assert!(body["published_at"].is_string());
}

#[tokio::test]
async fn publishing_a_single_page_notifies_the_webhook() {
    let (url, received) = start_webhook_receiver(StatusCode::OK).await;
    let app =
        test_app_with_notifier(Arc::new(WebhookNotifier::new(vec![url], None).unwrap())).await;

    send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&app.admin_token),
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/publish",
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let deliveries = wait_for_webhooks(&received, 1).await;
    let body: Value = serde_json::from_slice(&deliveries[0].body).unwrap();
    assert_eq!(body["event"], "single_page.published");
    assert_eq!(body["page"], "home");
    assert!(body.get("collection").is_none());
    // Without a configured secret the payload is delivered unsigned.
    assert!(deliveries[0].signature.is_none());
}

#[tokio::test]
async fn an_unreachable_webhook_receiver_does_not_break_publishing() {
    // Nothing listens on this port, so delivery fails at connect time.
    let url = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}/hook")
    };
    let app =
        test_app_with_notifier(Arc::new(WebhookNotifier::new(vec![url], None).unwrap())).await;
    let item_id = create_sample_item(&app, "blog").await;

    let (status, body) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/publish"),
        Some(&app.admin_token),
        None,
    )
    .await;

    assert_eq!(
        status,
        StatusCode::OK,
        "publishing must not depend on a receiver being up"
    );
    assert_eq!(body["status"], "published");

    // The content is public even though the webhook could not be delivered.
    let (status, _) = send(
        &app.router,
        Method::GET,
        &format!("/content/collections/blog/items/{item_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// --------------------------------------------------------------- pagination / updated_at

/// Create `count` items in a fresh collection and publish all of them.
async fn create_published_items(app: &TestApp, collection: &str, count: u64) -> Vec<u64> {
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/{collection}/schema"),
        Some(&app.admin_token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mut ids = Vec::new();
    for index in 0..count {
        let (status, body) = send(
            &app.router,
            Method::POST,
            &format!("/models/collections/{collection}/item"),
            Some(&app.admin_token),
            Some(json!({ "title": format!("Item {index}"), "tags": [] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let id = body.as_u64().expect("the item id");

        let (status, _) = send(
            &app.router,
            Method::POST,
            &format!("/models/collections/{collection}/items/{id}/publish"),
            Some(&app.admin_token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        ids.push(id);
    }
    ids
}

#[tokio::test]
async fn the_content_api_pages_through_published_items() {
    let app = test_app().await;
    let ids = create_published_items(&app, "blog", 4).await;

    // A draft must not show up in any page, nor in the total.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/item",
        Some(&app.admin_token),
        Some(json!({ "title": "Draft", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let draft = body.as_u64().unwrap();

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog?limit=2",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 4);
    assert_eq!(body["limit"], 2);
    assert_eq!(body["offset"], 0);
    assert_eq!(body["next_offset"], 2);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    // Ordered by id, which is what makes walking the offsets safe.
    assert_eq!(body["items"][0]["id"], ids[0]);
    assert_eq!(body["items"][1]["id"], ids[1]);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog?limit=2&offset=2",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert_eq!(body["items"][0]["id"], ids[2]);
    assert_eq!(body["items"][1]["id"], ids[3]);
    assert_eq!(body["next_offset"], Value::Null);
    assert!(
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["id"] != draft),
        "the draft must stay hidden on every page"
    );

    // An offset past the end is an empty last page, not an error.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog?limit=2&offset=99",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"], json!([]));
    assert_eq!(body["total"], 4);
    assert_eq!(body["next_offset"], Value::Null);

    // A page size that cannot be answered is refused rather than guessed at.
    for query in ["limit=0", "limit=201", "limit=abc"] {
        let (status, _) = send_raw(
            &app.router,
            Method::GET,
            &format!("/content/collections/blog?{query}"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "?{query} should be rejected");
    }
}

#[tokio::test]
async fn the_content_api_returns_a_bounded_page_without_an_explicit_limit() {
    let app = test_app().await;
    let ids = create_published_items(&app, "blog", DEFAULT_PAGE_LIMIT as u64 + 1).await;
    assert_eq!(ids.len(), DEFAULT_PAGE_LIMIT + 1);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/collections/blog",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["items"].as_array().unwrap().len(),
        DEFAULT_PAGE_LIMIT,
        "a site build must not be handed an unbounded list"
    );
    assert_eq!(body["total"], DEFAULT_PAGE_LIMIT + 1);
    assert_eq!(body["next_offset"], DEFAULT_PAGE_LIMIT);

    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/content/collections/blog?offset={DEFAULT_PAGE_LIMIT}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["next_offset"], Value::Null);
}

#[tokio::test]
async fn the_admin_item_list_can_be_paged_and_reports_the_total() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for index in 0..3 {
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/models/collections/blog/item",
            Some(&token),
            Some(json!({ "title": format!("Item {index}"), "tags": [] })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // Without a query the whole list comes back (the UI renders every row), with the total
    // in a header.
    let (status, headers, bytes) = send_with_headers(
        &app.router,
        Method::GET,
        "/models/collections/blog/items",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body.as_array().unwrap().len(), 3);
    assert_eq!(headers.get("x-total-count").unwrap(), "3");

    // A page keeps the `[id, values]` body shape the UI already reads.
    let (status, headers, bytes) = send_with_headers(
        &app.router,
        Method::GET,
        "/models/collections/blog/items?limit=2&offset=1",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body.as_array().unwrap().len(), 2);
    assert_eq!(body[0][0], 2);
    assert_eq!(body[1][0], 3);
    assert_eq!(headers.get("x-total-count").unwrap(), "3");

    // Paging does not open the list up.
    let (status, _, _) = send_with_headers(
        &app.router,
        Method::GET,
        "/models/collections/blog/items?limit=2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// The content clock and the publication date answer different questions.
///
/// `updated_at` is when the content last changed - a save, or a publish that released a working
/// copy - and `published_at` is when the item was first published, which later releases do not
/// move.
#[tokio::test]
async fn saving_records_when_content_changed_and_releasing_does_too() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let item_id = create_sample_item(&app, "blog").await;
    let metadata_url = format!("/models/collections/blog/items/{item_id}/metadata");

    let (status, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let created_at = timestamp(&body["created_at"]);
    let updated_at = timestamp(&body["updated_at"]);
    assert_eq!(
        created_at, updated_at,
        "a new item is created and updated at the same moment"
    );

    // The first publish releases the working copy a new item is written to, so the content the
    // site serves changes with it.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert!(
        timestamp(&body["updated_at"]) > updated_at,
        "releasing the first version changes the content"
    );
    let first_published = timestamp(&body["published_at"]);
    let first_release = timestamp(&body["last_published_at"]);
    assert_eq!(
        first_release, first_published,
        "the first release is also the first publication"
    );

    // An edit moves `updated_at`, keeps `created_at`, and does not unpublish.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/models/collections/blog/items/{item_id}"),
        Some(&token),
        Some(json!({ "title": "Edited", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        timestamp(&body["updated_at"]) > updated_at,
        "an edit has to move updated_at"
    );
    assert_eq!(
        timestamp(&body["created_at"]),
        created_at,
        "created_at must not move"
    );
    assert_eq!(body["status"], "published");
    assert_eq!(
        timestamp(&body["published_at"]),
        first_published,
        "an edit does not move the publication date"
    );
    assert_eq!(
        timestamp(&body["last_published_at"]),
        first_release,
        "and it does not move the release time either"
    );

    // The edit is waiting in the working copy: the live site keeps serving what was
    // published until the item is published again.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/content/collections/blog/items/{item_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Hello");
    assert!(body["values"].get("updated_at").is_none());

    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("/content/collections/blog/items/{item_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Edited");
    assert_eq!(
        timestamp(&body["published_at"]),
        first_published,
        "the publication date is the first one"
    );

    // Releasing the edit moved the content clock and the release time, not the publication date.
    let (_, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    let released_at = timestamp(&body["updated_at"]);
    assert!(
        released_at > updated_at,
        "releasing an edit changes the content"
    );
    assert!(
        timestamp(&body["last_published_at"]) > first_release,
        "and it is a release"
    );
    assert_eq!(timestamp(&body["published_at"]), first_published);

    // ...while publishing again with nothing waiting moves neither.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/models/collections/blog/items/{item_id}/publish"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = send(&app.router, Method::GET, &metadata_url, Some(&token), None).await;
    assert_eq!(
        timestamp(&body["updated_at"]),
        released_at,
        "publishing with nothing to release is not a change"
    );
    assert_eq!(timestamp(&body["published_at"]), first_published);
}

/// The delivery API carries both dates: the publication date a site shows, and the release time
/// an incremental build compares.
#[tokio::test]
async fn the_delivery_api_reports_the_publication_date_and_the_release_time() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let item_id = create_sample_item(&app, "blog").await;
    let item_url = format!("/models/collections/blog/items/{item_id}");
    let content_url = format!("/content/collections/blog/items/{item_id}");

    let (status, body) = send(&app.router, Method::POST, &format!("{item_url}/publish"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let first_date = timestamp(&body["published_at"]);

    let (_, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(timestamp(&body["published_at"]), first_date);
    let first_release = timestamp(&body["last_published_at"]);
    assert_eq!(first_release, first_date);

    // A release moves the last-published time, and the publication date is still the first one.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Second", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&app.router, Method::POST, &format!("{item_url}/publish"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Second");
    assert_eq!(timestamp(&body["published_at"]), first_date, "公開日は初回のまま");
    assert!(
        timestamp(&body["last_published_at"]) > first_release,
        "公開物が変わった時刻は進む"
    );
}

#[tokio::test]
async fn saving_a_single_page_records_when_it_changed() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let metadata_url = "/models/single_pages/home/item/metadata";
    let (status, body) = send(&app.router, Method::GET, metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let updated_at = timestamp(&body["updated_at"]);
    assert_eq!(timestamp(&body["created_at"]), updated_at);

    // A published page that is saved again stays published.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home again", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(&app.router, Method::GET, metadata_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(timestamp(&body["updated_at"]) > updated_at);
    assert_eq!(body["status"], "published", "saving must not unpublish");

    // Saving a published page leaves the live copy alone until it is published again.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/single-pages/home",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Home");

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/content/single-pages/home",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Home again");
}

// ------------------------------------------------------------------------- concurrency

/// The admin UI loads a page with several requests in flight at once: the item window, the
/// statuses, and the navigation. LMDB refuses to open a database while another transaction
/// is active, so the adapter has to keep storage operations from overlapping. When it did
/// not, a page load answered 500 ("attempted to open DB during transaction").
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_requests_do_not_break_the_storage_environment() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let created = create_published_items(&app, "blog", 3).await;

    // The window the UI opens, split across the worker threads the way a browser's
    // separate connections are.
    let targets = [
        "/models/collections/blog/items?limit=25&offset=0",
        "/models/collections/blog/items/metadata",
        "/models/collections/blog/items?limit=25&offset=0",
        "/models/collections",
        "/models/single_pages",
        "/models/collections/blog/schema",
    ];

    // Several rounds: a race that misses in one may land in the next.
    for round in 0..5 {
        let mut handles = Vec::new();
        for target in targets {
            let router = app.router.clone();
            let token = token.clone();
            handles.push(tokio::spawn(async move {
                send(&router, Method::GET, target, Some(&token), None).await
            }));
        }

        for handle in handles {
            let (status, body) = handle.await.expect("the request task panicked");
            assert_eq!(status, StatusCode::OK, "round {round}: {body}");
        }
    }

    // The items are still readable afterwards.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/collections/blog/items?limit=25&offset=0",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().unwrap().len(), created.len());
}

// --------------------------------------------------------------------------- permissions

/// Roles and per-resource grants belong to the CMS, not to whoever authenticates the user: an
/// administrator changes them on the account record, and the change is in force on the very
/// next request rather than the next time a token is issued.
#[tokio::test]
async fn a_role_change_takes_effect_on_the_next_request() {
    let app = test_app().await;
    let admin = app.admin_token.clone();

    let (status, created) = create_account(
        &app,
        json!({ "username": "promoted@example.com", "password": "promoted-password" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().expect("an id").to_string();
    let (_, body) = login(&app, "promoted@example.com", "promoted-password").await;
    let token = body["token"].as_str().unwrap().to_string();

    // The collections have to exist before there is anything to edit in them.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/blog/schema",
        Some(&admin),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/models/collections/news/schema",
        Some(&admin),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // A viewer may read, but writing content needs `can_edit`.
    let write = |token: String, collection: &'static str| {
        let router = app.router.clone();
        async move {
            send(
                &router,
                Method::POST,
                &format!("/models/collections/{collection}/item"),
                Some(&token),
                Some(json!({ "title": "Hello", "tags": [] })),
            )
            .await
        }
    };
    assert_eq!(write(token.clone(), "blog").await.0, StatusCode::FORBIDDEN);

    // The administrator grants edit rights. The caller's token does not change.
    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "permission": Permission::editor() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        write(token.clone(), "blog").await.0,
        StatusCode::OK,
        "the stored permission is what decides, not what the token was issued with"
    );

    // A per-collection override narrows it again, for one collection only.
    let deny = json!({ "can_view": false, "can_edit": false, "can_publish": false });
    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "collection_permissions": { "blog": deny } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(write(token.clone(), "blog").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        write(token, "news").await.0,
        StatusCode::OK,
        "another collection is unaffected"
    );
}

/// The permission matrix, checked through the real routes rather than the helpers.
///
/// Reading needs `can_view`; editing content needs `can_edit`; releasing it needs
/// `can_publish`; and changing the shape of the site needs an administrator, because
/// editing content is not the same as deciding what content can exist.
#[tokio::test]
async fn each_role_can_do_exactly_what_it_is_granted() {
    let app = test_app().await;
    let item_id = create_sample_item(&app, "blog").await;

    let roles = [
        ("viewer@example.com", Permission::viewer()),
        ("editor@example.com", Permission::editor()),
        (
            "publisher@example.com",
            Permission { can_view: true, can_edit: true, can_publish: true },
        ),
        (
            "nobody@example.com",
            Permission { can_view: false, can_edit: false, can_publish: false },
        ),
    ];
    for (email, permission) in roles {
        let (status, body) = create_account(&app, json!({
                "username": email,
                "password": "role-password",
                "is_admin": false,
                "permission": permission,
            }))
        .await;
        assert_eq!(status, StatusCode::CREATED, "{email}: {body}");
    }

    let mut tokens = HashMap::new();
    for (email, _) in roles {
        let (status, body) = login(&app, email, "role-password").await;
        assert_eq!(status, StatusCode::OK, "{email} login: {body}");
        tokens.insert(email, body["token"].as_str().unwrap().to_string());
    }

    // Reading: everyone holding `can_view`.
    for email in ["viewer@example.com", "editor@example.com", "publisher@example.com"] {
        let (status, _) = send(
            &app.router,
            Method::GET,
            "/models/collections",
            Some(&tokens[email]),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{email} should be able to read");
    }
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/models/collections",
        Some(&tokens["nobody@example.com"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an account without can_view cannot read");

    // Editing content: editors and publishers, not viewers.
    for (email, expected) in [
        ("viewer@example.com", StatusCode::FORBIDDEN),
        ("editor@example.com", StatusCode::OK),
        ("publisher@example.com", StatusCode::OK),
    ] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/models/collections/blog/item",
            Some(&tokens[email]),
            Some(json!({ "title": "draft", "tags": [] })),
        )
        .await;
        assert_eq!(status, expected, "{email} creating an item");
    }

    // Releasing it: only a publisher.
    for (email, expected) in [
        ("viewer@example.com", StatusCode::FORBIDDEN),
        ("editor@example.com", StatusCode::FORBIDDEN),
        ("publisher@example.com", StatusCode::OK),
    ] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            &format!("/models/collections/blog/items/{item_id}/publish"),
            Some(&tokens[email]),
            None,
        )
        .await;
        assert_eq!(status, expected, "{email} publishing");
    }

    // Changing the shape of the site: administrators only.
    for email in ["editor@example.com", "publisher@example.com"] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/models/collections/another/schema",
            Some(&tokens[email]),
            Some(sample_schema()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{email} changing a schema");
    }

    // Deleting content takes it off the site, so it needs a publisher.
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/models/collections/blog/items/{item_id}"),
        Some(&tokens["editor@example.com"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an editor cannot delete content");

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/models/collections/blog/items/{item_id}"),
        Some(&tokens["publisher@example.com"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "a publisher may delete content");

    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/models/collections/blog",
        Some(&tokens["publisher@example.com"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "deleting a collection is structural");

    // The administrator can do all of it.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/users",
        Some(&tokens["publisher@example.com"]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "account management is administrator-only");
}

// --------------------------------------------------------------- draft / published copies

/// The draft/published model, end to end.
///
/// A save lands in the working copy; publishing copies it to the published copy, which is
/// the only thing the delivery API serves. That separation is what makes "may edit but may
/// not publish" a safe role.
#[tokio::test]
async fn edits_wait_in_the_working_copy_until_they_are_published() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let item_id = create_sample_item(&app, "blog").await;
    let item_url = format!("/models/collections/blog/items/{item_id}");
    let content_url = format!("/content/collections/blog/items/{item_id}");

    // A new item starts unpublished, even though its values are already stored.
    let (status, body) = send(&app.router, Method::GET, "/content/collections/blog", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"], json!([]));

    // Publishing puts the working copy in front of the delivery API.
    let (status, _) = send(&app.router, Method::POST, &format!("{item_url}/publish"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Hello");

    // An edit is visible to the editor...
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Edited", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::GET, &item_url, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Edited");

    // ...but the site keeps serving what was published.
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Hello");

    // The metadata says so, so the admin list can show it.
    let (status, body) = send(
        &app.router,
        Method::GET,
        &format!("{item_url}/metadata"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "published");
    assert_eq!(body["has_draft"], true, "an unpublished edit is pending");

    // Unpublishing hides the item without losing either copy.
    let (status, _) = send(&app.router, Method::POST, &format!("{item_url}/unpublish"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Publishing again releases the edit that was waiting, and clears the working copy.
    let (status, _) = send(&app.router, Method::POST, &format!("{item_url}/publish"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Edited");
    let (status, body) = send(&app.router, Method::GET, &format!("{item_url}/metadata"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["has_draft"], false, "nothing is pending after publishing");

    // A preview shows the working copy before it is released.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Previewed", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::GET, &format!("{item_url}/preview"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Previewed");
    assert_eq!(body["schema"][0]["name"], "title");
    // ...while the site still serves the published one.
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Edited");

    // Publishing again *while still published* releases the waiting edit without the item ever
    // leaving the site - which is what the "publish changes" control in the admin screens does.
    // The reply carries the new state, including whether anything is still waiting, because the
    // screens take it as the state they should show next.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Released", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::POST, &format!("{item_url}/publish"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "published");
    assert_eq!(body["has_draft"], false, "the release clears the working copy");
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK, "the item never left the site");
    assert_eq!(body["values"]["title"], "Released");

    // Taking it down keeps whatever was being worked on, and publishing after that restores it.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &item_url,
        Some(&token),
        Some(json!({ "title": "Waiting", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::POST, &format!("{item_url}/unpublish"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "draft");
    assert_eq!(body["has_draft"], true, "the working copy is kept");
    assert_eq!(
        send(&app.router, Method::GET, &content_url, None, None).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, _) = send(&app.router, Method::POST, &format!("{item_url}/publish"), Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(&app.router, Method::GET, &content_url, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["values"]["title"], "Waiting");
}

// ------------------------------------------------------------------- account management

/// Managing accounts over HTTP: administrators only, except that anyone may change their
/// own password, and the CMS can never be left without an active administrator.
#[tokio::test]
async fn accounts_can_be_managed_without_locking_the_cms_out() {
    // This deployment leaves sign-in to an identity provider: there is no password endpoint to
    // drive, which is what `GET /auth/capabilities` reports to a client.
    if !Backend::PASSWORD_LOGIN {
        eprintln!("skipped: this deployment signs users in through an identity provider");
        return;
    }
    let app = test_app().await;
    let admin = app.admin_token.clone();

    let (status, created) = create_account(&app, json!({ "username": "user@example.com", "password": "user-password" }))
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["is_active"], true, "the response says whether the account is usable");
    assert_eq!(created["permission"]["can_edit"], false, "new accounts are viewers");

    let (status, users) = send(&app.router, Method::GET, "/auth/users", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(users
        .as_array()
        .unwrap()
        .iter()
        .any(|user| user["username"] == "user@example.com"));

    // Promote to publisher, disable, and enable again.
    let (status, body) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "permission": { "can_view": true, "can_edit": true, "can_publish": true } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["permission"]["can_publish"], true);

    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({ "is_active": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        login(&app, "user@example.com", "user-password").await.0,
        StatusCode::FORBIDDEN,
        "a disabled account cannot sign in"
    );

    // Back to a read-only account, enabled.
    let (status, _) = send(
        &app.router,
        Method::PATCH,
        &format!("/auth/users/{id}"),
        Some(&admin),
        Some(json!({
            "is_active": true,
            "permission": { "can_view": true, "can_edit": false, "can_publish": false },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = login(&app, "user@example.com", "user-password").await;
    assert_eq!(status, StatusCode::OK);
    let user_token = body["token"].as_str().unwrap().to_string();

    // A read-only account cannot manage anyone, including itself.
    for method in [Method::PATCH, Method::DELETE] {
        let (status, _) = send(
            &app.router,
            method.clone(),
            &format!("/auth/users/{id}"),
            Some(&user_token),
            (method == Method::PATCH).then(|| json!({ "is_active": false })),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} /auth/users/{{id}}");
    }
    let (status, _) = send(&app.router, Method::GET, "/auth/users", Some(&user_token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a read-only account cannot list accounts");

    // ...but it can change its own password, which is self-service rather than a write.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/me/password",
        Some(&user_token),
        Some(json!({ "current_password": "wrong-password", "new_password": "another-password" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the current password is required");

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/me/password",
        Some(&user_token),
        Some(json!({ "current_password": "user-password", "new_password": "another-password" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        login(&app, "user@example.com", "another-password").await.0,
        StatusCode::OK
    );

    // An administrator can reset it without knowing the old one.
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/auth/users/{id}/password"),
        Some(&admin),
        Some(json!({ "password": "reset-password" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        login(&app, "user@example.com", "reset-password").await.0,
        StatusCode::OK
    );

    // The last administrator cannot be demoted, disabled or deleted...
    let (status, me) = send(&app.router, Method::GET, "/auth/me", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    let admin_id = me["id"].as_str().unwrap().to_string();
    for change in [json!({ "is_admin": false }), json!({ "is_active": false })] {
        let (status, _) = send(
            &app.router,
            Method::PATCH,
            &format!("/auth/users/{admin_id}"),
            Some(&admin),
            Some(change.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{change}");
    }
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/auth/users/{admin_id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // ...but anyone else can be removed, and then cannot sign in.
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/auth/users/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        login(&app, "user@example.com", "reset-password").await.0,
        StatusCode::UNAUTHORIZED
    );

    // Deleting an account that is already gone is a 404.
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        &format!("/auth/users/{id}"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

