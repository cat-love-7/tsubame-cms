//! Images that exist: the trash, the rename, the list, and who is using one.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// The trash: taking an image out of the library is undoable, and only deleting it for good
/// removes the record and the bytes. Content that uses a trashed image keeps resolving, which is
/// what makes the difference worth having.
#[tokio::test]
async fn an_image_can_be_trashed_and_restored_before_it_is_deleted_for_good() {
    if !Backend::SERVES_IMAGE_BYTES {
        eprintln!("skipped: this backend hands the browser a signed URL instead of serving bytes");
        return;
    }
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, info) = send(
        &app.router,
        Method::POST,
        "/api/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "kept.png", "ext": "png", "size": PNG_BYTES.len() })),
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
    assert_eq!(
        app.router.clone().oneshot(request).await.unwrap().status(),
        StatusCode::CREATED
    );
    let (_, listed) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    let path = listed[0]["url"].as_str().unwrap().to_string();

    // Deleting is the *second* step of the two-step delete: from the library it is refused, and
    // the image is still there. The screen only offers it from the trash; the API says the same,
    // because a library image deleted by a stray call is gone with nothing to undo it.
    let (status, body) = send(
        &app.router,
        Method::DELETE,
        &format!("/api/models/images/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "image_not_trashed");
    let (_, library) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(
        library[0]["id"], id,
        "a refused delete must not delete anything"
    );

    // Trash it: out of the library, and in the trash with the time it went there.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        &format!("/api/models/images/{id}/trash"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, library) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(
        library,
        json!([]),
        "a trashed image is not offered to editors"
    );
    let (_, trash) = send(
        &app.router,
        Method::GET,
        "/api/models/images/trash",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(trash[0]["id"], id);
    assert!(trash[0]["deleted_at"].is_string());

    // The bytes are still there, and so is the durable link content would be using.
    let (status, bytes) = send_raw(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"PNG-BYTES");

    // Trashing twice is not an error: the caller asked for a state.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        &format!("/api/models/images/{id}/trash"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Put it back.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        &format!("/api/models/images/{id}/restore"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, library) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(library[0]["id"], id);
    let (_, trash) = send(
        &app.router,
        Method::GET,
        "/api/models/images/trash",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(trash, json!([]));

    // Deleting for good takes the record and the bytes.
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        &format!("/api/models/images/{id}/trash"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_raw(
        &app.router,
        Method::DELETE,
        &format!("/api/models/images/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, trash) = send(
        &app.router,
        Method::GET,
        "/api/models/images/trash",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(
        trash,
        json!([]),
        "deleting for good empties the trash entry"
    );
    let (status, _) = send_raw(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the bytes went with it");
}

/// The reference index: which content uses which image, so a delete can say what it would break.
///
/// Both directions are exercised - an `Image` field, the same inside an array and a composite, a
/// Markdown body linking the durable id, and an item being deleted - because the point of the
/// index is that it is complete enough to warn from.
#[tokio::test]
async fn the_things_that_use_an_image_can_be_listed() {
    if !Backend::SERVES_IMAGE_BYTES {
        eprintln!("skipped: this backend hands the browser a signed URL instead of serving bytes");
        return;
    }
    let app = test_app().await;
    let token = app.admin_token.clone();

    // Two images, so the index has to tell them apart.
    let mut ids = Vec::new();
    for name in ["one.png", "two.png"] {
        let (status, info) = send(
            &app.router,
            Method::POST,
            "/api/models/images/get_upload_url",
            Some(&token),
            Some(json!({ "original_filename": name, "ext": "png", "size": PNG_BYTES.len() })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        ids.push(info["id"].as_u64().expect("the image id"));
        let upload_url = info["upload_url"].as_str().unwrap().to_string();
        let request = Request::builder()
            .method(Method::PUT)
            .uri(&upload_url)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from("PNG-BYTES"))
            .unwrap();
        assert_eq!(
            app.router.clone().oneshot(request).await.unwrap().status(),
            StatusCode::CREATED
        );
    }
    let (used, other) = (ids[0], ids[1]);

    // A collection whose schema has an image field, an array of images and a Markdown body.
    let schema = json!([
        { "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 },
        { "name": "cover", "field_type": "Image", "required": false, "width": 12, "height": 1 },
        { "name": "gallery", "field_type": { "Array": ["Image"] }, "required": false, "width": 12, "height": 1 },
        { "name": "body", "field_type": { "Markdown": {} }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/schema",
        Some(&token),
        Some(schema),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));

    let (status, created) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/pages/item",
        Some(&token),
        Some(json!({
            "title": "first",
            "cover": used,
            "gallery": [used],
            "body": format!("See ![it](/images/by-id/{used}) and [two](/images/by-id/{other})."),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first = created.as_u64().expect("the item id");

    // The page that uses the other image, so the two lists can be told apart. It needs a schema
    // of its own: a page without one has nothing to save against.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/about/schema",
        Some(&token),
        Some(json!([
            { "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 },
            { "name": "cover", "field_type": "Image", "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/about/item",
        Some(&token),
        Some(json!({ "title": "about", "cover": other })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, references) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/images/{used}/references"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        references,
        json!([{ "kind": "collection_item", "name": "pages", "item": first }]),
        "the field, the array and the Markdown link are one item, listed once"
    );

    let (_, references) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/images/{other}/references"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(
        references,
        json!([
            { "kind": "collection_item", "name": "pages", "item": first },
            { "kind": "single_page", "name": "about" }
        ])
    );

    // Saving the item without the images drops them from the index.
    let (status, _) = send(
        &app.router,
        Method::PUT,
        &format!("/api/models/collections/pages/items/{first}"),
        Some(&token),
        Some(json!({ "title": "first", "cover": null, "gallery": [], "body": "no images" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, references) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/images/{used}/references"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(references, json!([]), "nothing uses it any more");

    // Deleting the page drops its references too.
    let (status, _) = send_raw(
        &app.router,
        Method::DELETE,
        "/api/models/single_pages/about",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, references) = send(
        &app.router,
        Method::GET,
        &format!("/api/models/images/{other}/references"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(references, json!([]));

    // An image nobody knows about has no references, and says so rather than failing.
    let (status, _) = send_raw(
        &app.router,
        Method::GET,
        "/api/models/images/999/references",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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
        "/api/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "photo.png", "ext": "png", "size": PNG_BYTES.len() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = info["id"].as_u64().expect("the image id");
    let url = info["url"].as_str().unwrap().to_string();

    // The name can be anything a reader recognises, including non-ASCII.
    let (status, bytes) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/api/models/images/{id}"),
        Some(&token),
        Some(json!({ "original_filename": "  表紙の写真.png  " })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert!(bytes.is_empty(), "a mutation answers with an empty body");

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["id"], id, "the id does not change");
    assert_eq!(body[0]["url"], url, "nor does where the bytes live");
    assert_eq!(
        body[0]["original_filename"], "表紙の写真.png",
        "trimmed, and otherwise as given"
    );

    // When it arrived can be stated, for a library migrated from another CMS: it is display only,
    // so nothing else about the image moves.
    let imported = chrono::Utc::now() - chrono::Duration::days(500);
    let (status, bytes) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/api/models/images/{id}"),
        Some(&token),
        Some(json!({ "uploaded_at": imported })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let (_, body) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(timestamp(&body[0]["uploaded_at"]), imported);
    assert_eq!(
        body[0]["url"], url,
        "and the bytes are still where they were"
    );

    // A date that cannot be true is refused, as a content date is.
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/api/models/images/{id}"),
        Some(&token),
        Some(json!({ "uploaded_at": chrono::Utc::now() + chrono::Duration::days(1) })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A name the library could not show, or one that is a path, is refused.
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/api/models/images/{id}"),
        Some(&token),
        Some(json!({ "original_filename": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        &format!("/api/models/images/{id}"),
        Some(&token),
        Some(json!({ "original_filename": "../../etc/passwd" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Renaming something that is not there reports it instead of quietly succeeding.
    let (status, _) = send_raw(
        &app.router,
        Method::PUT,
        "/api/models/images/9999",
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
        &format!("/api/models/images/{id}"),
        Some(&viewer),
        Some(json!({ "original_filename": "theirs.png" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
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
    let (status, _) = send_raw(&app.router, Method::GET, "/api/models/images", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]), "nothing has been uploaded yet");

    // Upload one, through the same two steps the UI takes.
    let (status, info) = send(
        &app.router,
        Method::POST,
        "/api/models/images/get_upload_url",
        Some(&token),
        Some(json!({ "original_filename": "photo.png", "ext": "png", "size": PNG_BYTES.len() })),
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
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body[0]["id"], id);
    assert_eq!(body[0]["original_filename"], "photo.png");
    assert!(body[0]["url"].as_str().unwrap().starts_with("/api/images/"));
    assert!(body[0]["uploaded_at"].is_string());

    let path = body[0]["url"].as_str().unwrap().to_string();
    let (status, bytes) = send_raw(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"PNG-BYTES");

    // Deleting is the second step of the two-step delete, so the image goes to the trash first
    // (the route refuses a library image; `an_image_can_be_trashed_and_restored_before_it_is_deleted_for_good`
    // pins that).
    let (status, _) = send_raw(
        &app.router,
        Method::POST,
        &format!("/api/models/images/{id}/trash"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Deleting answers with an empty body, like every other mutation.
    let (status, bytes) = send_raw(
        &app.router,
        Method::DELETE,
        &format!("/api/models/images/{id}"),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.is_empty());

    // Gone from the list, and the bytes went with it.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
    let (status, _) = send_raw(&app.router, Method::GET, &path, None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Deleting it twice reports the missing image instead of succeeding quietly.
    let (status, _) = send_raw(
        &app.router,
        Method::DELETE,
        &format!("/api/models/images/{id}"),
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
        "/api/images/..%2F..%2F..%2FCargo.toml",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/images/get_upload_url",
        Some(&token),
        Some(
            json!({ "original_filename": "x", "ext": "../../etc/passwd", "size": PNG_BYTES.len() }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The small copy a browser makes: the library shows it instead of the original, one page of the
/// library is a window rather than everything, and the copy goes when the image does.
///
/// The copy is sent through the API on both backends - that is the point of making it in the
/// browser, no adapter has to decode an image - but only a backend that stores the bytes itself can
/// serve them back here, so the half that fetches the copy is asked of that one. The AWS adapter's
/// own tests cover its side against the emulators.
#[tokio::test]
async fn the_library_carries_a_small_copy_and_hands_out_one_page_at_a_time() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let upload_one = |name: &str| {
        let app = &app;
        let token = token.clone();
        let name = name.to_string();
        async move {
            let (status, info) = send(
                &app.router,
                Method::POST,
                "/api/models/images/get_upload_url",
                Some(&token),
                Some(json!({ "original_filename": name, "ext": "png", "size": 8 })),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            info["id"].as_u64().expect("the image id")
        }
    };
    // The thumbnail route takes the image itself as the body, so it is built by hand: the JSON
    // helpers are for JSON.
    let put_thumbnail = |url: String, body: Vec<u8>| {
        let app = &app;
        let token = token.clone();
        async move {
            let request = Request::builder()
                .method(Method::PUT)
                .uri(url)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(body))
                .expect("a request");
            let response = app
                .router
                .clone()
                .oneshot(request)
                .await
                .expect("a response");
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("a body")
                .to_vec();
            (status, bytes)
        }
    };

    let first = upload_one("first.png").await;
    let second = upload_one("second.png").await;

    // A record without a copy says nothing about one, which is how the screen knows to fall back
    // to the original.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/images",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().map(Vec::len), Some(2), "両方ある");
    assert!(
        body[0].get("thumbnail_url").is_none(),
        "無いものは黙っている: {body}"
    );

    // Storing one is the local backend's half of this: the AWS adapter puts an object in S3, and
    // the contract harness has no bucket to put it in (its adapter's own tests cover that path).
    // What is checked here either way is what the *API* says about a copy.
    let mut stored_thumbnail_url = String::new();
    if Backend::SERVES_IMAGE_BYTES {
        let (status, body) = put_thumbnail(
            format!("/api/models/images/{first}/thumbnail?ext=webp"),
            b"SMALL-COPY".to_vec(),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NO_CONTENT,
            "{}",
            String::from_utf8_lossy(&body)
        );

        let (status, body) = send(
            &app.router,
            Method::GET,
            "/api/models/images",
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let with_copy = body
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == first)
            .expect("the image that was given a copy");
        stored_thumbnail_url = with_copy["thumbnail_url"]
            .as_str()
            .expect("the small copy's URL")
            .to_string();
        assert!(
            stored_thumbnail_url.starts_with("/api/images/"),
            "the copy is served like the original: {stored_thumbnail_url}"
        );
        assert_ne!(
            with_copy["url"].as_str().unwrap(),
            stored_thumbnail_url,
            "小さなコピーは原本とは別のファイル"
        );
        // The other image is untouched: a copy belongs to one image.
        let without_copy = body
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == second)
            .expect("the other image");
        assert!(without_copy.get("thumbnail_url").is_none());

        // The bytes are what was sent.
        let (status, bytes) =
            send_raw(&app.router, Method::GET, &stored_thumbnail_url, None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, b"SMALL-COPY");
    }

    // A page of the library, with how much there is in a header - the same shape a collection's
    // items use.
    let (status, headers, bytes) = send_with_headers(
        &app.router,
        Method::GET,
        "/api/models/images?limit=1",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("x-total-count").unwrap(), "2");
    let page: Vec<Value> = serde_json::from_slice(&bytes).expect("a page of images");
    assert_eq!(page.len(), 1, "1 ページは 1 件");

    let (status, headers, bytes) = send_with_headers(
        &app.router,
        Method::GET,
        "/api/models/images?limit=1&offset=1",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("x-total-count").unwrap(), "2");
    let second_page: Vec<Value> = serde_json::from_slice(&bytes).expect("a page of images");
    assert_eq!(second_page.len(), 1);
    assert_ne!(page[0]["id"], second_page[0]["id"], "次のページは別の画像");

    // A limit nobody can answer is refused rather than guessed at.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/images?limit=0",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A copy that is not an image, or is not small, is refused with a sentence.
    let (status, body) = put_thumbnail(
        format!("/api/models/images/{first}/thumbnail?ext=tar.gz"),
        b"COPY".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8_lossy(&body).contains("not an image type"),
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );

    let too_large = vec![0u8; 512 * 1024 + 1];
    let (status, _) = put_thumbnail(
        format!("/api/models/images/{first}/thumbnail?ext=webp"),
        too_large,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    // The copy goes when the image does. Deleting an image removes its objects, and the AWS side of
    // this suite has no bucket for them to be in (`Backend::SERVES_IMAGE_BYTES` is false there),
    // which is why this half is asked of the backend that stores the bytes.
    if Backend::SERVES_IMAGE_BYTES {
        let (status, _) = send_raw(
            &app.router,
            Method::POST,
            &format!("/api/models/images/{first}/trash"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(
            &app.router,
            Method::DELETE,
            &format!("/api/models/images/{first}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) =
            send_raw(&app.router, Method::GET, &stored_thumbnail_url, None, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "小さなコピーも消える");
    }
}
