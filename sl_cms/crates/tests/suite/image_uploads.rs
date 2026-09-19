//! Getting bytes in: the upload target, who may use it, and replacing an image.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// An image only ever takes the upload it was given: the file name is the server's to choose, and
/// the server records which one it chose *for this image* when the replacement is requested.
///
/// The file name is not part of the API, so it is read from the upload URLs the server hands out -
/// and the point of the test is that the server decides by id, not from a URL it happens to serve
/// with: in a deployment that signs those URLs, a comparison against the URL would never match, and
/// the check would stop working without saying anything.
#[tokio::test]
async fn an_image_only_takes_the_upload_it_was_given() {
    // The bytes are put through this router, which is only how they travel where the CMS serves
    // them itself. An object-storage deployment hands the browser a presigned URL to S3 instead;
    // the same rule is pinned there, in `aws::repository::images`, against those URLs.
    if !Backend::SERVES_IMAGE_BYTES {
        eprintln!("skipped: this backend hands the browser a signed URL instead of serving bytes");
        return;
    }
    let app = test_app().await;
    let token = app.admin_token.clone();

    /// Ask for an upload, send some bytes to it, and answer with the file name it went to.
    async fn upload(app: &TestApp, token: &str, what: &str) -> String {
        let (status, info) = send(
            &app.router,
            Method::POST,
            "/models/images/get_upload_url",
            Some(token),
            Some(json!({ "original_filename": what, "ext": "png" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        put_bytes(app, token, info["upload_url"].as_str().unwrap()).await;
        file_name_of(info["upload_url"].as_str().unwrap())
    }

    // Two images, each with its own upload and its own bytes.
    let first = upload(&app, &token, "one.png").await;
    let second = upload(&app, &token, "two.png").await;
    assert_ne!(first, second, "two uploads share a file name");

    // Image 1, pointed at the file image 2 owns and nobody asked it to take: refused.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/images/1",
        Some(&token),
        Some(json!({ "file_name": second })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "image 1 took a file it was not given: {}",
        String::from_utf8_lossy(&body)
    );

    // A replacement it *was* given: the request names the file, the upload arrives, and the apply
    // takes it.
    let (status, replacement) = send(
        &app.router,
        Method::POST,
        "/models/images/1/replace",
        Some(&token),
        Some(json!({ "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pending = replacement["file_name"].as_str().unwrap().to_string();
    put_bytes(&app, &token, replacement["upload_url"].as_str().unwrap()).await;

    // Even now, someone else's file is not this image's to take...
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        "/models/images/1",
        Some(&token),
        Some(json!({ "file_name": second })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // ...and naming the file it already serves is a no-op that leaves the upload it is waiting for
    // waiting: re-clicking the old file must not cancel the replacement that was just signed.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/images/1",
        Some(&token),
        Some(json!({ "file_name": first })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    // The upload it asked for is applied, and applying it again is a no-op rather than a refusal:
    // the screen sends what it was given.
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/images/1",
        Some(&token),
        Some(json!({ "file_name": pending })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = send_raw(
        &app.router,
        Method::PUT,
        "/models/images/1",
        Some(&token),
        Some(json!({ "file_name": pending })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
}

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
    assert!(
        upload_url.starts_with("/images/"),
        "unexpected url: {upload_url}"
    );

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

    let (_, before) = send(
        &app.router,
        Method::GET,
        "/models/images",
        Some(&token),
        None,
    )
    .await;
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
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/models/images",
        Some(&token),
        None,
    )
    .await;
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
    let (status, bytes) = send_raw(
        &app.router,
        Method::GET,
        &format!("/images/by-id/{id}"),
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the id link should serve what the image shows now"
    );
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

/// An image the size of a photograph is accepted where the CMS takes the bytes itself.
///
/// axum's own extractor limit is 2MB, and until the limits were made explicit that was the real
/// ceiling for an upload on this deployment - a size no photograph has ever been, and nothing said
/// so. The route carries the deployment's image limit now (`MAX_IMAGE_BYTES`, ten megabytes by
/// default), which is also why a body this size is not refused by the JSON limit: the two limits
/// belong to two different routes.
#[tokio::test]
async fn an_image_larger_than_two_megabytes_is_accepted() {
    if !Backend::SERVES_IMAGE_BYTES {
        eprintln!("skipped: this backend hands the browser a signed URL instead of serving bytes");
        return;
    }
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, info) = send(
        &app.router,
        Method::POST,
        "/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "photograph.png", "ext": "png" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{info}");

    // Three megabytes: over axum's 2MB default and over the JSON limit, under the image one.
    let bytes = vec![0x89u8; 3 * 1024 * 1024];
    put_bytes_of(
        &app,
        &token,
        info["upload_url"].as_str().expect("an upload URL"),
        &bytes,
    )
    .await;

    // And it is really there, at the size it was sent.
    let file = file_name_of(info["upload_url"].as_str().unwrap());
    let (status, body) = send_raw(
        &app.router,
        Method::GET,
        &format!("/images/{file}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.len(), bytes.len());
}
