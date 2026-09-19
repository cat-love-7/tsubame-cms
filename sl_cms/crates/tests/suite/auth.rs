//! Signing in, the accounts behind it, and the endpoints that carry a password.
//!
//! Part of the contract suite; `mod.rs` has the harness and says how the directory is put
//! together.

use super::*;

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
    // The image limit travels with the answer, so a browser can refuse a file it already knows is
    // too big instead of uploading it to be told afterwards. The harness runs with the defaults,
    // so this is also what a deployment that sets nothing reports.
    assert_eq!(
        body["max_image_bytes"].as_u64(),
        Some(sl_cms_core::config::DEFAULT_MAX_IMAGE_BYTES as u64),
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

    // Creating an account chooses no password, and a `password` in the body does not change that:
    // the account cannot sign in with it, so this endpoint cannot be used to choose someone else's
    // credential.
    let (status, created) = send(
        &app.router,
        Method::POST,
        "/auth/users",
        Some(&app.admin_token),
        Some(json!({
            "username": "typed@example.com",
            "password": "typed-by-someone-else",
            "is_admin": false,
            "permission": Permission::viewer(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        login(&app, "typed@example.com", "typed-by-someone-else")
            .await
            .0,
        StatusCode::UNAUTHORIZED,
        "a password in the request must not become the account's"
    );

    // Too short -> 400, refused where a password is chosen: the owner completing a reset link.
    let id = created["id"].as_str().expect("the new account's id");
    let (status, link) = send(
        &app.router,
        Method::POST,
        &format!("/auth/users/{id}/password-reset-link"),
        Some(&app.admin_token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app.router,
        Method::POST,
        "/auth/password-reset",
        None,
        Some(json!({ "token": link["token"], "new_password": "short" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "weak_password");
}

/// An account can be created without a password: it exists, and cannot sign in until its owner
/// sets one through a reset link. That is the path the account screen takes, so an administrator
/// never chooses someone else's password.
#[tokio::test]
async fn an_account_can_be_created_without_a_password() {
    let app = test_app().await;
    if !app.password_login() {
        eprintln!("skipped: this deployment signs users in through an identity provider");
        return;
    }
    let admin = app.admin_token.clone();

    let (status, created) = create_account(
        &app,
        json!({
            "username": "no-password@example.com",
            "is_admin": false,
            "permission": Permission::editor(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_string();

    // No credential yet, so the sign-in is refused - and refused the same way a wrong password is,
    // because the answer must not say whether an account exists.
    let (status, _) = login(&app, "no-password@example.com", "anything").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // The reset link is the way in, and the password it sets is the owner's.
    let (status, link) = send(
        &app.router,
        Method::POST,
        &format!("/auth/users/{id}/password-reset-link"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = link["token"].as_str().expect("the reset token").to_string();
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/password-reset",
        None,
        Some(json!({ "token": token, "new_password": "chosen-by-the-owner" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        login(&app, "no-password@example.com", "chosen-by-the-owner")
            .await
            .0,
        StatusCode::OK
    );
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

    let (status, created) = create_account(
        &app,
        json!({
            "username": "ops",
            "password": "ops-password",
            "is_admin": false,
            "permission": Permission::viewer(),
        }),
    )
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
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/me",
        Some(&fresh_token),
        None,
    )
    .await;
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
    assert!(
        (1..=15 * 60).contains(&retry_after),
        "retry-after={retry_after}"
    );

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
    let (status, created) = create_account(
        &app,
        json!({
            "username": "editor@example.com",
            "password": "editor-password",
            "is_admin": false,
            "permission": Permission::editor(),
        }),
    )
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
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/me",
        Some(&replacement),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ...and a reset the editor completes themselves ends that one too, while leaving the
    // administrator's own session alone. (An administrator cannot choose someone else's password:
    // they hand over a reset link, so the password is known to its owner and nobody else.)
    let (status, link) = send(
        &app.router,
        Method::POST,
        &format!("/auth/users/{editor_id}/password-reset-link"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = link["token"].as_str().expect("the reset token").to_string();

    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/password-reset",
        None,
        Some(json!({ "token": token, "new_password": "chosen-by-the-editor" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/me",
        Some(&replacement),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = send(&app.router, Method::GET, "/auth/me", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK, "自分のセッションは終わらない");
}

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

    let (status, created) = create_account(
        &app,
        json!({ "username": "user@example.com", "password": "user-password" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(
        created["is_active"], true,
        "the response says whether the account is usable"
    );
    assert_eq!(
        created["permission"]["can_edit"], false,
        "new accounts are viewers"
    );

    let (status, users) = send(&app.router, Method::GET, "/auth/users", Some(&admin), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        users
            .as_array()
            .unwrap()
            .iter()
            .any(|user| user["username"] == "user@example.com")
    );

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
    let (status, _) = send(
        &app.router,
        Method::GET,
        "/auth/users",
        Some(&user_token),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a read-only account cannot list accounts"
    );

    // ...but it can change its own password, which is self-service rather than a write.
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/me/password",
        Some(&user_token),
        Some(json!({ "current_password": "wrong-password", "new_password": "another-password" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the current password is required"
    );

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

    // An administrator can hand over a reset link without knowing the old one, and the account
    // owner chooses the replacement.
    let (status, link) = send(
        &app.router,
        Method::POST,
        &format!("/auth/users/{id}/password-reset-link"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = link["token"].as_str().expect("the reset token").to_string();
    let (status, _) = send(
        &app.router,
        Method::POST,
        "/auth/password-reset",
        None,
        Some(json!({ "token": token, "new_password": "chosen-after-a-reset" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        login(&app, "user@example.com", "chosen-after-a-reset")
            .await
            .0,
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
