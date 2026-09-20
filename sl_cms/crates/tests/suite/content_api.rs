//! The delivery API and the admin list: what is reachable, and how a page is bounded.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

/// Nothing is public until it is published, and unpublishing hides it again.
#[tokio::test]
async fn only_published_collection_items_reach_the_content_api() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for title in ["First", "Second"] {
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/api/models/collections/blog/item",
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
        "/api/content/collections/blog",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["schema"][0]["name"], "title");
    assert_eq!(body["items"], json!([]));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/collections",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    // A draft answers 404 rather than 403, so the delivery API does not confirm that
    // unpublished content exists.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/blog/items/1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The admin side still reports a status for every item, defaults included.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/collections/blog/items/1/metadata",
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
        "/api/models/collections/blog/items/metadata",
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
        "/api/models/collections/blog/items/1/publish",
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
        "/api/content/collections/blog/items/1",
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
        "/api/content/collections/blog",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["values"]["title"], "First");
    // The second item stays a draft inside the same collection.
    assert_eq!(body["items"][0]["id"], 1);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/collections",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(["blog"]));

    // Publishing something that does not exist is a 404, not a silent success.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/items/99/publish",
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
        "/api/models/collections/blog/items/1/unpublish",
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
        "/api/content/collections/blog/items/1",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/collections",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    // Deleting a published item must not leave its status behind either: the collection
    // index goes empty with it.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/items/1/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/api/models/collections/blog/items/1",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/collections",
        None,
        None,
    )
    .await;
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
        "/api/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages/home",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/models/single_pages/home/item/metadata",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "draft");

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "published");

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!(["home"]));

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages/home",
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
        "/api/models/single_pages/home/unpublish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages/home",
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
        "/api/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::DELETE,
        "/api/models/single_pages/home",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "title": "Home again", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn the_content_api_pages_through_published_items() {
    let app = test_app().await;
    let ids = create_published_items(&app, "blog", 4).await;

    // A draft must not show up in any page, nor in the total.
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/item",
        Some(&app.admin_token),
        Some(json!({ "title": "Draft", "tags": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let draft = body.as_u64().unwrap();

    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/collections/blog?limit=2",
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
        "/api/content/collections/blog?limit=2&offset=2",
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
        "/api/content/collections/blog?limit=2&offset=99",
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
            &format!("/api/content/collections/blog?{query}"),
            None,
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "?{query} should be rejected"
        );
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
        "/api/content/collections/blog",
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
        &format!("/api/content/collections/blog?offset={DEFAULT_PAGE_LIMIT}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["next_offset"], Value::Null);
}

/// A browser on another origin can read how many items there are.
///
/// The total travels in `X-Total-Count`, and a cross-origin fetch may only read the response
/// headers the policy *exposes*: `Access-Control-Allow-Origin` says who may ask, not what they may
/// read. Without the exposure the count is invisible, and a client the CMS does not serve itself
/// (the deployment's own front end on another host) would see a list that looks empty and a pager
/// with one page.
#[tokio::test]
async fn a_cross_origin_client_can_read_the_total() {
    let app = test_app().await;
    let token = app.admin_token.clone();
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/models/collections/blog/items")
        .header(header::ORIGIN, "http://localhost:4200")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("x-total-count").unwrap(), "0");
    let exposed = response
        .headers()
        .get("access-control-expose-headers")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        exposed.contains("x-total-count"),
        "a cross-origin reader cannot see the total: {exposed:?}"
    );
}

#[tokio::test]
async fn the_admin_item_list_can_be_paged_and_reports_the_total() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/blog/schema",
        Some(&token),
        Some(sample_schema()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for index in 0..3 {
        let (status, _) = send(
            &app.router,
            Method::POST,
            "/api/models/collections/blog/item",
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
        "/api/models/collections/blog/items",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body.as_array().unwrap().len(), 3);
    assert_eq!(headers.get("x-total-count").unwrap(), "3");

    // A page keeps the `[id, values]` body shape the UI already reads, and the order is newest
    // first (ids are creation order, and an editor opening a collection wants the last one).
    let (status, headers, bytes) = send_with_headers(
        &app.router,
        Method::GET,
        "/api/models/collections/blog/items?limit=2&offset=1",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body.as_array().unwrap().len(), 2);
    assert_eq!(body[0][0], 2);
    assert_eq!(body[1][0], 1);
    assert_eq!(headers.get("x-total-count").unwrap(), "3");

    // Paging does not open the list up.
    let (status, _, _) = send_with_headers(
        &app.router,
        Method::GET,
        "/api/models/collections/blog/items?limit=2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A page is cut by size as well as by count, and the cut is invisible to a client that walks it.
///
/// A page of large items is a response the platform refuses: an invocation answers with at most
/// 6MB, so a client asking for fifty items of a few hundred kilobytes each would get a failed
/// request instead of content. The page is therefore cut to the deployment's budget
/// (`MAX_RESPONSE_BYTES`), and `next_offset` moves to the first item that did not fit - so
/// nothing is skipped, and the walk ends where it should.
///
/// The budget here is a few hundred bytes rather than four megabytes: the number is the
/// deployment's, and a test that had to send four megabytes to see the rule would be a slow way
/// to check a subtraction.
#[tokio::test]
async fn a_page_is_cut_to_the_response_budget_and_says_where_to_continue() {
    let app = TestApp::with_limits(sl_cms_core::config::Limits {
        max_response_bytes: 400,
        ..sl_cms_core::config::Limits::default()
    })
    .await;

    // A schema of its own, because the schema counts against the same budget as the items.
    let collection = "paged";
    let (status, _) = send(
        &app.router,
        Method::POST,
        &format!("/api/models/collections/{collection}/schema"),
        Some(&app.admin_token),
        Some(json!([
            { "name": "title", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Three items, each big enough that one page cannot carry all of them.
    let mut published = Vec::new();
    for _ in 0..3 {
        let title = "x".repeat(120);
        let (status, body) = send(
            &app.router,
            Method::POST,
            &format!("/api/models/collections/{collection}/item"),
            Some(&app.admin_token),
            Some(json!({ "title": title })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let id = body.as_u64().expect("the item id");
        let (status, _) = send(
            &app.router,
            Method::POST,
            &format!("/api/models/collections/{collection}/items/{id}/publish"),
            Some(&app.admin_token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        published.push(id);
    }

    // Walk the pages the way a site build does: by `next_offset`, until there is none.
    let mut seen = Vec::new();
    let mut offset: Option<usize> = Some(0);
    let mut pages = 0;
    while let Some(from) = offset {
        let (status, body) = send(
            &app.router,
            Method::GET,
            &format!("/api/content/collections/{collection}?limit=3&offset={from}"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let items = body["items"].as_array().expect("items").clone();
        assert!(
            !items.is_empty(),
            "a page that carries nothing would loop: {body}"
        );
        if pages == 0 {
            assert!(
                items.len() < 3,
                "the budget should have cut this page: {body}"
            );
        }
        for item in &items {
            seen.push(item["id"].as_u64().expect("an item id"));
        }
        offset = body["next_offset"].as_u64().map(|value| value as usize);
        pages += 1;
        assert!(pages < 10, "the walk should end: {body}");
    }

    // Every published item exactly once, so the cut neither skipped nor repeated anything.
    assert_eq!(seen, published);
}

/// A consumer reading content has to be able to read the definitions that content names: a schema
/// points at a composite definition by id (`{"CompositeField": {"id": "block"}}`), and the fields
/// of a block exist only in the definition. Served without a token, beside the content that needs
/// it - the same answer the management API gives.
#[tokio::test]
async fn the_content_api_serves_the_composite_definitions_it_names() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    // Nothing is defined yet: an empty map, not an error.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/composite-fields",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({}));

    // The management API defines one, and a page schema names it.
    let definition = json!([
        { "name": "url", "field_type": { "Text": {} }, "required": false, "width": 12, "height": 1 }
    ]);
    let (status, body) = send_raw(
        &app.router,
        Method::POST,
        "/api/models/composite_fields/link",
        Some(&token),
        Some(definition),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/schema",
        Some(&token),
        Some(json!([{
            "name": "links",
            "field_type": { "Array": [{ "CompositeField": { "id": "link" } }] },
            "required": false, "width": 12, "height": 1,
        }])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A page is public once it is published, and publishing it is what puts the schema on the
    // public side.
    let (status, body) = send(
        &app.router,
        Method::PUT,
        "/api/models/single_pages/home/item",
        Some(&token),
        Some(json!({ "links": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/single_pages/home/publish",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The public side reads the definition without a token, and it is what the schema names.
    let (status, body) = send(
        &app.router,
        Method::GET,
        "/api/content/composite-fields",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, page) = send(
        &app.router,
        Method::GET,
        "/api/content/single-pages/home",
        None,
        None,
    )
    .await;
    assert_eq!(
        page["schema"][0]["field_type"]["Array"][0]["CompositeField"]["id"],
        "link"
    );
    assert_eq!(body["link"][0]["name"], "url");
    // The same answer the management API gives, which is what the editor's screens read.
    let (status, admin) = send(
        &app.router,
        Method::GET,
        "/api/models/composite_fields",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, admin);

    // Reading the same definitions through the management API still wants a token: this is a
    // delivery route, not a loosening of the one an editor's screens use.
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/api/models/composite_fields",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A site build asks for the order it wants to walk; the default stays the id order it always
/// had, and the id is the tie-break that keeps the pages from skipping.
#[tokio::test]
async fn the_content_api_orders_a_list_how_it_is_asked_to() {
    let app = test_app().await;
    let token = app.admin_token.clone();

    let (status, body) = send(
        &app.router,
        Method::POST,
        "/api/models/collections/posts/schema",
        Some(&token),
        Some(json!([
            { "name": "title", "field_type": { "Text": {} }, "required": true, "width": 12, "height": 1 },
            { "name": "rank", "field_type": "Number", "required": false, "width": 12, "height": 1 },
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Three items whose id order, title order and rank order all differ.
    for (title, rank) in [("b", 2), ("A", 3), ("c", 1)] {
        let (status, body) = send(
            &app.router,
            Method::POST,
            "/api/models/collections/posts/item",
            Some(&token),
            Some(json!({ "title": title, "rank": rank })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    for id in 1..=3 {
        let (status, body) = send(
            &app.router,
            Method::POST,
            &format!("/api/models/collections/posts/items/{id}/publish"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let ids = |body: &Value| -> Vec<u64> {
        body["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item["id"].as_u64())
                    .collect()
            })
            .unwrap_or_default()
    };
    let listed = |query: &'static str| {
        let app = &app;
        async move {
            let (status, body) = send(
                &app.router,
                Method::GET,
                &format!("/api/content/collections/posts{query}"),
                None,
                None,
            )
            .await;
            (status, body)
        }
    };

    // The default is what it always was: id ascending.
    let (status, body) = listed("").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ids(&body), vec![1, 2, 3]);

    // Newest first, oldest first, and by a field - text without regard to case, numbers as
    // numbers, and the id as the tie-break.
    let (_, body) = listed("?sort=-id").await;
    assert_eq!(ids(&body), vec![3, 2, 1]);
    let (_, body) = listed("?sort=title").await;
    assert_eq!(ids(&body), vec![2, 1, 3], "A, b, c");
    let (_, body) = listed("?sort=-title").await;
    assert_eq!(ids(&body), vec![3, 1, 2]);
    let (_, body) = listed("?sort=rank").await;
    assert_eq!(ids(&body), vec![3, 1, 2], "rank 1, 2, 3");

    // The window is over the order that was asked for, and the next page continues it.
    let (_, first) = listed("?sort=-id&limit=2").await;
    assert_eq!(ids(&first), vec![3, 2]);
    assert_eq!(first["next_offset"], 2);
    let (_, second) = listed("?sort=-id&limit=2&offset=2").await;
    assert_eq!(ids(&second), vec![1]);
    assert_eq!(second["next_offset"], json!(null));

    // A key that is not there, or is not something an order can be made of, is refused.
    for query in ["?sort=nope", "?sort=-", "?sort=-nope"] {
        let (status, _) = listed(query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
    }
}
