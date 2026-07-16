//! Multi-user isolation + auth guardrail tests (prompt.md §11, §13). Drives the real axum
//! router in-process against a throwaway SQLite DB - no network. This is the security backbone
//! test every later phase relies on.

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum_extra::extract::cookie::Key;
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use sqlx::Row;
use tower::ServiceExt;

use crate::auth::bootstrap;
use crate::db;
use crate::http::{self, AppState};

const ADMIN_PW: &str = "admin-secret-pw";

/// Build the app router backed by a fresh migrated + bootstrapped temp DB. The pool is returned
/// so tests can seed shared `items`/`feeds` directly (ingestion needs the network).
async fn test_app() -> (axum::Router, sqlx::SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::connect(&dir.path().join("test.db")).await.unwrap();
    db::migrate(&pool).await.unwrap();
    bootstrap::run(&pool, ADMIN_PW).await.unwrap();

    let key = Key::from(&Sha512::digest(b"test-secret-key-at-least-16"));
    let enc_key: [u8; 32] = sha2::Sha256::digest(b"test-secret-key-at-least-16").into();
    let state = AppState {
        pool: pool.clone(),
        static_dir: dir.path().to_path_buf(),
        index_html: "".into(),
        key,
        enc_key,
        http_client: crate::ingest::fetch::build_client(),
        ingest_trigger: std::sync::Arc::new(tokio::sync::Notify::new()),
        events: crate::events::EventBus::new(),
        webauthn: crate::auth::passkey::build("localhost", "http://localhost:8080", &[]),
        passkey_ceremonies: crate::auth::passkey::CeremonyStore::new(),
        oauth: std::sync::Arc::new(crate::oauth::OAuthSettings {
            google: None,
            reddit: None,
            redirect_base: "http://localhost:8080".into(),
        }),
        oauth_states: crate::oauth::OAuthStates::new(),
    };
    (http::router(state), pool, dir)
}

/// Result of an HTTP call: status, parsed JSON body, and the session cookie (name=value) if set.
struct Resp {
    status: StatusCode,
    body: Value,
    cookie: Option<String>,
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> Resp {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    let req = builder
        .body(
            body.map(|b| Body::from(b.to_string()))
                .unwrap_or(Body::empty()),
        )
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or("").to_string());
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Resp {
        status,
        body,
        cookie: set_cookie,
    }
}

/// Like `call` but returns the raw response body as text (for non-JSON endpoints, e.g. OPML export).
async fn call_text(
    app: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
) -> (StatusCode, String) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    let req = builder.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn login(app: &axum::Router, username: &str, password: &str) -> Resp {
    call(
        app,
        "POST",
        "/api/auth/login",
        Some(json!({ "username": username, "password": password })),
        None,
    )
    .await
}

async fn register(app: &axum::Router, username: &str, password: &str) -> Resp {
    call(
        app,
        "POST",
        "/api/auth/register",
        Some(json!({ "username": username, "password": password })),
        None,
    )
    .await
}

#[tokio::test]
async fn admin_can_log_in_and_bad_password_is_rejected() {
    let (app, _pool, _d) = test_app().await;

    let ok = login(&app, "admin", ADMIN_PW).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.body["role"], "admin");
    assert!(ok.cookie.is_some(), "login should set a session cookie");

    let bad = login(&app, "admin", "wrong").await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);

    let unknown = login(&app, "nobody", "whatever").await;
    assert_eq!(
        unknown.status,
        StatusCode::UNAUTHORIZED,
        "no username enumeration"
    );
}

#[tokio::test]
async fn registration_seeds_only_other_category_and_is_gated() {
    let (app, _pool, _d) = test_app().await;

    let alice = register(&app, "alice", "password123").await;
    assert_eq!(alice.status, StatusCode::OK);
    let cookie = alice.cookie.clone().unwrap();

    let cats = call(&app, "GET", "/api/categories", None, Some(&cookie)).await;
    assert_eq!(cats.status, StatusCode::OK);
    assert_eq!(
        cats.body.as_array().unwrap().len(),
        1,
        "only Other category seeded"
    );
    assert!(cats
        .body
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "Other"));

    // Disable registration as admin, then registration must 403.
    let admin = login(&app, "admin", ADMIN_PW).await;
    let admin_cookie = admin.cookie.unwrap();
    let put = call(
        &app,
        "PUT",
        "/api/admin/settings",
        Some(json!({ "allow_registration": false })),
        Some(&admin_cookie),
    )
    .await;
    assert_eq!(put.status, StatusCode::OK);

    let blocked = register(&app, "charlie", "password123").await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn users_cannot_read_each_others_rows() {
    let (app, _pool, _d) = test_app().await;

    let alice = register(&app, "alice", "password123").await;
    let bob = register(&app, "bob", "password123").await;
    let alice_c = alice.cookie.unwrap();
    let bob_c = bob.cookie.unwrap();

    let alice_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let bob_cats = call(&app, "GET", "/api/categories", None, Some(&bob_c)).await;

    let ids = |v: &Value| -> Vec<i64> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_i64().unwrap())
            .collect()
    };
    let alice_ids = ids(&alice_cats.body);
    let bob_ids = ids(&bob_cats.body);

    // Each sees exactly their own row; the id sets are disjoint (no shared/leaked rows).
    assert_eq!(alice_ids.len(), 1);
    assert_eq!(bob_ids.len(), 1);
    assert!(
        alice_ids.iter().all(|id| !bob_ids.contains(id)),
        "per-user rows must be disjoint"
    );
}

#[tokio::test]
async fn non_admin_is_forbidden_from_admin_endpoints() {
    let (app, _pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let alice_c = alice.cookie.unwrap();

    let listed = call(&app, "GET", "/api/admin/users", None, Some(&alice_c)).await;
    assert_eq!(listed.status, StatusCode::FORBIDDEN);

    // Unauthenticated is 401.
    let anon = call(&app, "GET", "/api/admin/users", None, None).await;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn builtin_admin_and_last_admin_are_protected() {
    let (app, _pool, _d) = test_app().await;
    let admin = login(&app, "admin", ADMIN_PW).await;
    let admin_c = admin.cookie.unwrap();

    // Discover the admin's id.
    let users = call(&app, "GET", "/api/admin/users", None, Some(&admin_c)).await;
    let admin_id = users
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["username"] == "admin")
        .and_then(|u| u["id"].as_i64())
        .unwrap();

    // Cannot demote the built-in admin.
    let demote = call(
        &app,
        "PATCH",
        &format!("/api/admin/users/{admin_id}"),
        Some(json!({ "role": "user" })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(demote.status, StatusCode::FORBIDDEN);

    // Cannot delete the built-in admin.
    let del = call(
        &app,
        "DELETE",
        &format!("/api/admin/users/{admin_id}"),
        None,
        Some(&admin_c),
    )
    .await;
    assert_eq!(del.status, StatusCode::FORBIDDEN);

    // Admin cannot delete their own account either.
    let del_self = call(&app, "DELETE", "/api/me", None, Some(&admin_c)).await;
    assert_eq!(del_self.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn change_password_requires_current() {
    let (app, _pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let alice_c = alice.cookie.unwrap();

    let wrong = call(
        &app,
        "PATCH",
        "/api/me",
        Some(json!({ "current_password": "nope", "new_password": "newpassword1" })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST);

    let ok = call(
        &app,
        "PATCH",
        "/api/me",
        Some(json!({ "current_password": "password123", "new_password": "newpassword1" })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK);

    // New password now works.
    assert_eq!(
        login(&app, "alice", "newpassword1").await.status,
        StatusCode::OK
    );
    assert_eq!(
        login(&app, "alice", "password123").await.status,
        StatusCode::UNAUTHORIZED
    );
}

// ---------------------------------------------------------------------------
// Phase 3: feeds/categories scoping + category rules (prompt.md §11)
// ---------------------------------------------------------------------------

fn cat_id(body: &Value, name: &str) -> i64 {
    body.as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .and_then(|c| c["id"].as_i64())
        .unwrap_or_else(|| panic!("category {name} not found"))
}

async fn subscribe(app: &axum::Router, cookie: &str, feed_url: &str, category_id: i64) -> Resp {
    call(
        app,
        "POST",
        "/api/feeds",
        Some(json!({ "feed_url": feed_url, "kind": "rss", "category_id": category_id })),
        Some(cookie),
    )
    .await
}

#[tokio::test]
async fn subscribe_requires_a_valid_owned_category() {
    let (app, _pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let bob = register(&app, "bob", "password123").await;
    let alice_c = alice.cookie.unwrap();
    let bob_c = bob.cookie.unwrap();

    let bob_cats = call(&app, "GET", "/api/categories", None, Some(&bob_c)).await;
    let bob_other = cat_id(&bob_cats.body, "Other");

    // A category id that isn't the caller's is rejected (no cross-user category use).
    let cross = subscribe(&app, &alice_c, "https://ex.com/feed.xml", bob_other).await;
    assert_eq!(
        cross.status,
        StatusCode::BAD_REQUEST,
        "adding without a valid own category is blocked"
    );

    // A bogus id is rejected too.
    let bogus = subscribe(&app, &alice_c, "https://ex.com/feed.xml", 999_999).await;
    assert_eq!(bogus.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn deleting_category_reassigns_feeds_to_other_and_other_is_protected() {
    let (app, _pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let alice_c = alice.cookie.unwrap();

    // Only "Other" is seeded - insert a second category to test deletion + reassignment.
    sqlx::query("INSERT INTO categories (user_id, name, position) VALUES ((SELECT id FROM users WHERE username = 'alice'), 'AI', 1)")
        .execute(&_pool)
        .await
        .unwrap();

    let cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let ai = cat_id(&cats.body, "AI");
    let other = cat_id(&cats.body, "Other");

    let sub = subscribe(&app, &alice_c, "https://ex.com/ai.xml", ai).await;
    assert_eq!(sub.status, StatusCode::OK);
    assert_eq!(sub.body["category_name"], "AI");

    // Delete AI → its feed reassigns to Other.
    let del = call(
        &app,
        "DELETE",
        &format!("/api/categories/{ai}"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(del.status, StatusCode::OK);

    let feeds = call(&app, "GET", "/api/feeds", None, Some(&alice_c)).await;
    let f = &feeds.body.as_array().unwrap()[0];
    assert_eq!(
        f["category_id"].as_i64().unwrap(),
        other,
        "feed moved to Other"
    );
    assert_eq!(f["category_name"], "Other");

    // Other itself cannot be deleted.
    let del_other = call(
        &app,
        "DELETE",
        &format!("/api/categories/{other}"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(del_other.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn feeds_are_per_user_scoped_over_a_shared_catalog() {
    let (app, _pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let bob = register(&app, "bob", "password123").await;
    let alice_c = alice.cookie.unwrap();
    let bob_c = bob.cookie.unwrap();

    let alice_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let bob_cats = call(&app, "GET", "/api/categories", None, Some(&bob_c)).await;

    // Both subscribe to the SAME feed URL - one shared global feed, two private subscriptions.
    let a_sub = subscribe(
        &app,
        &alice_c,
        "https://shared.example/feed.xml",
        cat_id(&alice_cats.body, "Other"),
    )
    .await;
    let b_sub = subscribe(
        &app,
        &bob_c,
        "https://shared.example/feed.xml",
        cat_id(&bob_cats.body, "Other"),
    )
    .await;
    assert_eq!(a_sub.status, StatusCode::OK);
    assert_eq!(b_sub.status, StatusCode::OK);
    assert_eq!(
        a_sub.body["feed_id"], b_sub.body["feed_id"],
        "polled once: shared feed row"
    );
    assert_ne!(
        a_sub.body["id"], b_sub.body["id"],
        "distinct per-user subscriptions"
    );

    // Each user sees only their own subscription.
    let alice_feeds = call(&app, "GET", "/api/feeds", None, Some(&alice_c)).await;
    assert_eq!(alice_feeds.body.as_array().unwrap().len(), 1);

    // Bob cannot edit or delete Alice's subscription (404, not another user's row).
    let alice_sub_id = a_sub.body["id"].as_i64().unwrap();
    let patch = call(
        &app,
        "PATCH",
        &format!("/api/feeds/{alice_sub_id}"),
        Some(json!({ "disabled": true })),
        Some(&bob_c),
    )
    .await;
    assert_eq!(patch.status, StatusCode::NOT_FOUND);

    let del = call(
        &app,
        "DELETE",
        &format!("/api/feeds/{alice_sub_id}"),
        None,
        Some(&bob_c),
    )
    .await;
    assert_eq!(del.status, StatusCode::NOT_FOUND);

    // Duplicate subscription (same URL) is rejected for the same user.
    let dup = subscribe(
        &app,
        &alice_c,
        "https://shared.example/feed.xml",
        cat_id(&alice_cats.body, "Other"),
    )
    .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
}

// ---------------------------------------------------------------------------
// Phase 4: items API - per-user state over shared content, facets/sorts/search (§10, §11)
// ---------------------------------------------------------------------------

/// Insert a shared item directly (ingestion needs the network). FTS stays in sync via triggers.
#[allow(clippy::too_many_arguments)]
async fn insert_item(
    pool: &sqlx::SqlitePool,
    feed_id: i64,
    guid: &str,
    title: &str,
    text: &str,
    published_at: &str,
    score: Option<i64>,
    comments: Option<i64>,
    reading_time: Option<i64>,
) -> i64 {
    sqlx::query(
        "INSERT INTO items
            (feed_id, guid, url, title, author, content_html, content_text,
             reading_time_secs, published_at, score, comments_count, dedup_hash)
         VALUES (?, ?, ?, ?, 'author', ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(feed_id)
    .bind(guid)
    .bind(format!("https://ex.example/{guid}"))
    .bind(title)
    .bind(format!("<p>{text}</p>"))
    .bind(text)
    .bind(reading_time)
    .bind(published_at)
    .bind(score)
    .bind(comments)
    .bind(guid)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id")
}

fn item_ids(body: &Value) -> Vec<i64> {
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn items_are_per_user_over_shared_content_with_independent_state() {
    let (app, pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let bob = register(&app, "bob", "password123").await;
    let carol = register(&app, "carol", "password123").await;
    let alice_c = alice.cookie.unwrap();
    let bob_c = bob.cookie.unwrap();
    let carol_c = carol.cookie.unwrap();

    let a_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let b_cats = call(&app, "GET", "/api/categories", None, Some(&bob_c)).await;

    // Both subscribe to the same shared feed (polled once).
    let a_sub = subscribe(
        &app,
        &alice_c,
        "https://shared.example/feed.xml",
        cat_id(&a_cats.body, "Other"),
    )
    .await;
    let b_sub = subscribe(
        &app,
        &bob_c,
        "https://shared.example/feed.xml",
        cat_id(&b_cats.body, "Other"),
    )
    .await;
    let feed_id = a_sub.body["feed_id"].as_i64().unwrap();
    assert_eq!(
        feed_id,
        b_sub.body["feed_id"].as_i64().unwrap(),
        "shared feed row"
    );

    let item = insert_item(
        &pool,
        feed_id,
        "g1",
        "Hello World",
        "some body text",
        "2021-06-15 12:00:00",
        None,
        None,
        Some(120),
    )
    .await;

    // Both see the shared item.
    let a_items = call(&app, "GET", "/api/items", None, Some(&alice_c)).await;
    assert_eq!(a_items.body["total_count"].as_i64().unwrap(), 1);
    assert_eq!(
        call(&app, "GET", "/api/items", None, Some(&bob_c))
            .await
            .body["total_count"]
            .as_i64()
            .unwrap(),
        1
    );

    // Alice stars + reads; no `item_states` row was pre-created, this upserts one.
    let star = call(
        &app,
        "POST",
        &format!("/api/items/{item}/star"),
        Some(json!({ "value": true })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(star.status, StatusCode::OK);
    assert_eq!(star.body["is_starred"], true);
    let read = call(
        &app,
        "POST",
        &format!("/api/items/{item}/read"),
        Some(json!({ "value": true })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(read.body["is_read"], true);
    assert_eq!(read.body["is_starred"], true, "read upsert preserves star");

    // Alice's detail reflects her state; Bob (same shared item) is untouched.
    let a_view = call(
        &app,
        "GET",
        &format!("/api/items/{item}"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(a_view.body["is_starred"], true);
    assert_eq!(a_view.body["is_read"], true);
    let b_view = call(
        &app,
        "GET",
        &format!("/api/items/{item}"),
        None,
        Some(&bob_c),
    )
    .await;
    assert_eq!(b_view.body["is_starred"], false, "star is per-user");
    assert_eq!(b_view.body["is_read"], false, "read is per-user");

    // Alice's starred filter finds it; Bob's does not.
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/items?status=starred",
            None,
            Some(&alice_c)
        )
        .await
        .body["total_count"],
        1
    );
    assert_eq!(
        call(&app, "GET", "/api/items?status=starred", None, Some(&bob_c))
            .await
            .body["total_count"],
        0
    );

    // Carol doesn't subscribe → she can neither read nor mutate the item (404, not another feed's data).
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/items/{item}"),
            None,
            Some(&carol_c)
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/items/{item}/star"),
            Some(json!({ "value": true })),
            Some(&carol_c)
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        call(&app, "GET", "/api/items", None, Some(&carol_c))
            .await
            .body["total_count"]
            .as_i64()
            .unwrap(),
        0
    );

    // Toggle (no body value) flips Alice's read back to unread.
    let toggled = call(
        &app,
        "POST",
        &format!("/api/items/{item}/read"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(toggled.body["is_read"], false);
}

#[tokio::test]
async fn item_facets_sorts_and_search_work_end_to_end() {
    let (app, pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let c = alice.cookie.unwrap();
    let cats = call(&app, "GET", "/api/categories", None, Some(&c)).await;
    let sub = subscribe(
        &app,
        &c,
        "https://ex.example/feed.xml",
        cat_id(&cats.body, "Other"),
    )
    .await;
    let feed_id = sub.body["feed_id"].as_i64().unwrap();

    let a = insert_item(
        &pool,
        feed_id,
        "g1",
        "Rust async runtime",
        "tokio internals",
        "2021-06-10 10:00:00",
        Some(100),
        Some(5),
        Some(600),
    )
    .await;
    let b = insert_item(
        &pool,
        feed_id,
        "g2",
        "Gardening tips",
        "grow tomatoes",
        "2021-06-11 10:00:00",
        None,
        None,
        Some(120),
    )
    .await;
    let cc = insert_item(
        &pool,
        feed_id,
        "g3",
        "Rust web servers",
        "axum guide",
        "2021-06-12 10:00:00",
        Some(5),
        Some(50),
        Some(300),
    )
    .await;

    // sort=top → score DESC, NULLs LAST: a(100), cc(5), then b(NULL).
    let top = call(&app, "GET", "/api/items?sort=top", None, Some(&c)).await;
    assert_eq!(item_ids(&top.body), vec![a, cc, b]);

    // sort=discussed → comments DESC, NULLs LAST: cc(50), a(5), then b(NULL).
    let disc = call(&app, "GET", "/api/items?sort=discussed", None, Some(&c)).await;
    assert_eq!(item_ids(&disc.body), vec![cc, a, b]);

    // sort=quick → reading-time ASC: b(120), cc(300), a(600).
    let quick = call(&app, "GET", "/api/items?sort=quick", None, Some(&c)).await;
    assert_eq!(item_ids(&quick.body), vec![b, cc, a]);

    // sort=new / old by published_at.
    assert_eq!(
        item_ids(
            &call(&app, "GET", "/api/items?sort=new", None, Some(&c))
                .await
                .body
        ),
        vec![cc, b, a]
    );
    assert_eq!(
        item_ids(
            &call(&app, "GET", "/api/items?sort=old", None, Some(&c))
                .await
                .body
        ),
        vec![a, b, cc]
    );

    // FTS search: "rust" matches the two Rust titles only.
    let search = call(&app, "GET", "/api/items?q=rust", None, Some(&c)).await;
    assert_eq!(search.body["total_count"].as_i64().unwrap(), 2);
    let mut found = item_ids(&search.body);
    found.sort();
    assert_eq!(found, vec![a, cc]);
    // A punctuation-only query can't crash FTS; it just matches nothing meaningful.
    assert!(call(&app, "GET", "/api/items?q=%21%40%23", None, Some(&c))
        .await
        .status
        .is_success());

    // Pagination: page_size=2 over 3 items → 2 pages.
    let p1 = call(
        &app,
        "GET",
        "/api/items?page_size=2&page=1&sort=old",
        None,
        Some(&c),
    )
    .await;
    assert_eq!(p1.body["total_pages"].as_i64().unwrap(), 2);
    assert_eq!(p1.body["page_size"].as_i64().unwrap(), 2);
    assert_eq!(item_ids(&p1.body), vec![a, b]);
    let p2 = call(
        &app,
        "GET",
        "/api/items?page_size=2&page=2&sort=old",
        None,
        Some(&c),
    )
    .await;
    assert_eq!(item_ids(&p2.body), vec![cc]);

    // type=video → none (reading subscription); type=reading → all three.
    assert_eq!(
        call(&app, "GET", "/api/items?type=video", None, Some(&c))
            .await
            .body["total_count"]
            .as_i64()
            .unwrap(),
        0
    );
    assert_eq!(
        call(&app, "GET", "/api/items?type=reading", None, Some(&c))
            .await
            .body["total_count"]
            .as_i64()
            .unwrap(),
        3
    );
}

/// A NULL score (e.g. Reddit's JSON endpoint got blocked and ingestion fell back to plain `.rss`,
/// which carries no vote data) must not bypass a real `min_score` threshold - unknown score is
/// treated the same as "too low", not "let it through". min_score=0 (the default / "off") still
/// shows everything regardless of score, known or not.
#[tokio::test]
async fn min_score_hides_low_and_unknown_score_items_but_not_when_unset() {
    let (app, pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let c = alice.cookie.unwrap();
    let cats = call(&app, "GET", "/api/categories", None, Some(&c)).await;
    let cat = cat_id(&cats.body, "Other");

    let sub = call(
        &app,
        "POST",
        "/api/feeds",
        Some(json!({
            "feed_url": "https://www.reddit.com/r/rust/.rss",
            "kind": "reddit",
            "category_id": cat,
            "min_score": 50,
        })),
        Some(&c),
    )
    .await;
    let feed_id = sub.body["feed_id"].as_i64().unwrap();

    let high = insert_item(
        &pool,
        feed_id,
        "g1",
        "High score post",
        "text",
        "2021-06-10 10:00:00",
        Some(100),
        None,
        None,
    )
    .await;
    let low = insert_item(
        &pool,
        feed_id,
        "g2",
        "Low score post",
        "text",
        "2021-06-11 10:00:00",
        Some(4),
        None,
        None,
    )
    .await;
    let unknown = insert_item(
        &pool,
        feed_id,
        "g3",
        "Unknown score post",
        "text",
        "2021-06-12 10:00:00",
        None,
        None,
        None,
    )
    .await;

    // min_score=50: only the 100-score post clears the bar - the 4-score and unknown-score
    // posts are both hidden.
    let list = call(&app, "GET", "/api/items", None, Some(&c)).await;
    assert_eq!(item_ids(&list.body), vec![high]);

    // Turning the threshold off (0) shows everything again, known score or not.
    call(
        &app,
        "PATCH",
        &format!("/api/feeds/{feed_id}"),
        Some(json!({ "min_score": 0 })),
        Some(&c),
    )
    .await;
    let list2 = call(&app, "GET", "/api/items?sort=old", None, Some(&c)).await;
    assert_eq!(item_ids(&list2.body), vec![high, low, unknown]);
}

#[tokio::test]
async fn category_counts_reflect_active_facets() {
    let (app, pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let c = alice.cookie.unwrap();

    // Only "Other" is seeded - insert categories needed for this multi-category test (§TODO-9).
    let alice_id: i64 = alice.body["id"].as_i64().unwrap();
    sqlx::query("INSERT INTO categories (user_id, name, position) VALUES (?, 'AI', 1)")
        .bind(alice_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO categories (user_id, name, position) VALUES (?, 'Software Engineering', 2)",
    )
    .bind(alice_id)
    .execute(&pool)
    .await
    .unwrap();
    let cats = call(&app, "GET", "/api/categories", None, Some(&c)).await;
    let ai = cat_id(&cats.body, "AI");
    let se = cat_id(&cats.body, "Software Engineering");

    let ai_sub = subscribe(&app, &c, "https://ai.example/feed.xml", ai).await;
    let se_sub = subscribe(&app, &c, "https://se.example/feed.xml", se).await;
    let ai_feed = ai_sub.body["feed_id"].as_i64().unwrap();
    let se_feed = se_sub.body["feed_id"].as_i64().unwrap();

    insert_item(
        &pool,
        ai_feed,
        "a1",
        "one",
        "x",
        "2021-06-10 10:00:00",
        None,
        None,
        None,
    )
    .await;
    insert_item(
        &pool,
        ai_feed,
        "a2",
        "two",
        "x",
        "2021-06-10 10:00:00",
        None,
        None,
        None,
    )
    .await;
    let se_item = insert_item(
        &pool,
        se_feed,
        "s1",
        "three",
        "x",
        "2021-06-10 10:00:00",
        None,
        None,
        None,
    )
    .await;

    let count_for = |body: &Value, cid: i64| -> i64 {
        body["categories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["category_id"].as_i64() == Some(cid))
            .unwrap()["count"]
            .as_i64()
            .unwrap()
    };

    let all = call(&app, "GET", "/api/categories/counts", None, Some(&c)).await;
    assert_eq!(all.body["total"].as_i64().unwrap(), 3);
    assert_eq!(count_for(&all.body, ai), 2);
    assert_eq!(count_for(&all.body, se), 1);

    // Read the SE item, then status=unread counts must drop it (facet-aware chips).
    call(
        &app,
        "POST",
        &format!("/api/items/{se_item}/read"),
        Some(json!({ "value": true })),
        Some(&c),
    )
    .await;
    let unread = call(
        &app,
        "GET",
        "/api/categories/counts?status=unread",
        None,
        Some(&c),
    )
    .await;
    assert_eq!(unread.body["total"].as_i64().unwrap(), 2);
    assert_eq!(count_for(&unread.body, ai), 2);
    assert_eq!(count_for(&unread.body, se), 0);
}

// ===========================================================================
// Phase 5 - Pluggable AI (admin-global): admin gating, write-only keys, SSRF,
// shared summary-cache reuse. (Live provider calls are covered by the docker gate.)
// ===========================================================================

/// Insert an active provider directly (bypassing the HTTP layer) for cache-reuse tests. `key_enc`
/// is left NULL (keyless, Ollama-style) so no decryption is needed.
async fn seed_active_provider(pool: &sqlx::SqlitePool, model: &str) {
    sqlx::query(
        "INSERT INTO ai_providers (name, provider_type, api_style, base_url, model, api_key_enc, is_active)
         VALUES ('Seed', 'ollama', 'openai', 'http://localhost:11434/v1', ?, NULL, 1)",
    )
    .bind(model)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn ai_endpoints_require_admin() {
    let (app, _pool, _d) = test_app().await;
    let alice = register(&app, "alice", "password123").await;
    let alice_c = alice.cookie.unwrap();

    for (method, uri, body) in [
        ("GET", "/api/ai/presets", None),
        ("GET", "/api/ai/providers", None),
        ("GET", "/api/ai/settings", None),
        (
            "POST",
            "/api/ai/providers",
            Some(
                json!({ "name": "x", "provider_type": "custom", "api_style": "openai", "base_url": "https://api.example.com/v1", "model": "m", "key": "secret" }),
            ),
        ),
    ] {
        let user = call(&app, method, uri, body.clone(), Some(&alice_c)).await;
        assert_eq!(
            user.status,
            StatusCode::FORBIDDEN,
            "non-admin must get 403 on {method} {uri}"
        );
        let anon = call(&app, method, uri, body, None).await;
        assert_eq!(
            anon.status,
            StatusCode::UNAUTHORIZED,
            "anon must get 401 on {method} {uri}"
        );
    }
}

#[tokio::test]
async fn ai_provider_key_is_never_returned() {
    let (app, _pool, _d) = test_app().await;
    let admin_c = login(&app, "admin", ADMIN_PW).await.cookie.unwrap();

    let secret = "sk-super-secret-key-value-XYZ";
    let created = call(
        &app,
        "POST",
        "/api/ai/providers",
        Some(json!({ "name": "OpenAI", "provider_type": "openai", "api_style": "openai",
                     "base_url": "https://api.openai.com/v1", "model": "gpt-4o-mini", "key": secret })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    // The create response never echoes the key.
    assert!(!created.body.to_string().contains(secret));

    let list = call(&app, "GET", "/api/ai/providers", None, Some(&admin_c)).await;
    let raw = list.body.to_string();
    assert!(!raw.contains(secret), "list must not leak the key");
    assert!(!raw.contains("api_key"), "no key field is serialized");
    let p = &list.body.as_array().unwrap()[0];
    assert_eq!(p["has_key"], true);
    assert_eq!(p["is_active"], true, "first provider auto-activates");
    // There is no read path for the key.
    assert!(p.get("key").is_none() && p.get("api_key_enc").is_none());
}

#[tokio::test]
async fn ai_ssrf_guard_allows_ollama_but_blocks_private_custom() {
    let (app, _pool, _d) = test_app().await;
    let admin_c = login(&app, "admin", ADMIN_PW).await.cookie.unwrap();

    // Ollama on localhost is intentionally allowed (prompt.md §6, §11).
    let ollama = call(
        &app,
        "POST",
        "/api/ai/providers",
        Some(
            json!({ "name": "Ollama", "provider_type": "ollama", "api_style": "openai",
                     "base_url": "http://localhost:11434/v1", "model": "llama3.2" }),
        ),
        Some(&admin_c),
    )
    .await;
    assert_eq!(
        ollama.status,
        StatusCode::OK,
        "localhost Ollama passes SSRF"
    );

    // A custom provider pointed at a private IP is rejected while allow-private is off.
    let private = call(
        &app,
        "POST",
        "/api/ai/providers",
        Some(
            json!({ "name": "Evil", "provider_type": "custom", "api_style": "openai",
                     "base_url": "http://127.0.0.1:9000/v1", "model": "m", "key": "k" }),
        ),
        Some(&admin_c),
    )
    .await;
    assert_eq!(
        private.status,
        StatusCode::BAD_REQUEST,
        "private custom URL blocked by default"
    );

    // Turn allow-private on → the same custom URL is now accepted.
    call(
        &app,
        "PUT",
        "/api/admin/settings",
        Some(json!({ "allow_registration": true })),
        Some(&admin_c),
    )
    .await;
    sqlx::query("INSERT INTO app_settings (key, value) VALUES ('ingest.allow_private', 'true') ON CONFLICT(key) DO UPDATE SET value = 'true'")
        .execute(&_pool)
        .await
        .unwrap();
    let allowed = call(
        &app,
        "POST",
        "/api/ai/providers",
        Some(
            json!({ "name": "LAN", "provider_type": "custom", "api_style": "openai",
                     "base_url": "http://127.0.0.1:9000/v1", "model": "m", "key": "k" }),
        ),
        Some(&admin_c),
    )
    .await;
    assert_eq!(
        allowed.status,
        StatusCode::OK,
        "allow-private lets a private custom URL through"
    );
}

#[tokio::test]
async fn summarize_reuses_shared_cache_across_users() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let bob_c = register(&app, "bob", "password123").await.cookie.unwrap();

    // Both users subscribe to the same shared feed; seed one item on it.
    let a_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let b_cats = call(&app, "GET", "/api/categories", None, Some(&bob_c)).await;
    let a_sub = subscribe(
        &app,
        &alice_c,
        "https://shared.example/feed.xml",
        cat_id(&a_cats.body, "Other"),
    )
    .await;
    subscribe(
        &app,
        &bob_c,
        "https://shared.example/feed.xml",
        cat_id(&b_cats.body, "Other"),
    )
    .await;
    let feed_id = a_sub.body["feed_id"].as_i64().unwrap();
    let item = insert_item(
        &pool,
        feed_id,
        "sg1",
        "A title",
        "some body text",
        "2021-06-10 10:00:00",
        None,
        None,
        None,
    )
    .await;

    // Active provider + a pre-populated shared cache entry keyed by (item, model).
    seed_active_provider(&pool, "test-model").await;
    sqlx::query("INSERT INTO item_summaries (item_id, model, api_style, summary_text) VALUES (?, 'test-model', 'openai', 'CACHED SUMMARY')")
        .bind(item)
        .execute(&pool)
        .await
        .unwrap();

    // Alice summarizes → cache hit, no network.
    let a = call(
        &app,
        "POST",
        &format!("/api/items/{item}/summarize"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK);
    assert_eq!(a.body["cached"], true);
    assert_eq!(a.body["summary"], "CACHED SUMMARY");

    // Bob (a different user) reuses the SAME shared cache entry.
    let b = call(
        &app,
        "POST",
        &format!("/api/items/{item}/summarize"),
        None,
        Some(&bob_c),
    )
    .await;
    assert_eq!(b.status, StatusCode::OK);
    assert_eq!(b.body["cached"], true);
    assert_eq!(b.body["summary"], "CACHED SUMMARY");

    // Only one cache row exists (no duplicate token spend).
    let n: i64 = sqlx::query("SELECT COUNT(*) AS n FROM item_summaries WHERE item_id = ?")
        .bind(item)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("n");
    assert_eq!(n, 1);
}

#[tokio::test]
async fn video_provider_setting_is_admin_only_and_gemini_only() {
    let (app, _pool, _d) = test_app().await;
    let admin_c = login(&app, "admin", ADMIN_PW).await.cookie.unwrap();

    let gem = call(
        &app,
        "POST",
        "/api/ai/providers",
        Some(json!({
            "name": "Gemini", "provider_type": "gemini", "api_style": "openai",
            "base_url": "https://generativelanguage.googleapis.com/v1beta/openai",
            "model": "gemini-2.0-flash", "key": "k"
        })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(gem.status, StatusCode::OK);
    let gem_id = gem.body["id"].as_i64().unwrap();

    let groq = call(
        &app,
        "POST",
        "/api/ai/providers",
        Some(json!({
            "name": "Groq", "provider_type": "groq", "api_style": "openai",
            "base_url": "https://api.groq.com/openai/v1",
            "model": "llama-3.3-70b-versatile", "key": "k"
        })),
        Some(&admin_c),
    )
    .await;
    let groq_id = groq.body["id"].as_i64().unwrap();

    // Non-admin cannot touch it.
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let denied = call(
        &app,
        "PUT",
        "/api/ai/video-provider",
        Some(json!({ "provider_id": gem_id })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);

    // Only Gemini providers qualify (the only API that accepts a video URL).
    let bad = call(
        &app,
        "PUT",
        "/api/ai/video-provider",
        Some(json!({ "provider_id": groq_id })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    // Unknown id → 404.
    let missing = call(
        &app,
        "PUT",
        "/api/ai/video-provider",
        Some(json!({ "provider_id": 9999 })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    // Set + read back via the AI settings DTO.
    let ok = call(
        &app,
        "PUT",
        "/api/ai/video-provider",
        Some(json!({ "provider_id": gem_id })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK);
    let settings = call(&app, "GET", "/api/ai/settings", None, Some(&admin_c)).await;
    assert_eq!(settings.body["video_provider_id"].as_i64(), Some(gem_id));

    // Clear with null.
    let cleared = call(
        &app,
        "PUT",
        "/api/ai/video-provider",
        Some(json!({ "provider_id": null })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(cleared.status, StatusCode::OK);
    let settings = call(&app, "GET", "/api/ai/settings", None, Some(&admin_c)).await;
    assert!(settings.body["video_provider_id"].is_null());
}

#[tokio::test]
async fn summarize_without_provider_is_a_clear_error() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let sub = subscribe(
        &app,
        &alice_c,
        "https://ex.example/feed.xml",
        cat_id(&cats.body, "Other"),
    )
    .await;
    let feed_id = sub.body["feed_id"].as_i64().unwrap();
    let item = insert_item(
        &pool,
        feed_id,
        "np1",
        "t",
        "body",
        "2021-06-10 10:00:00",
        None,
        None,
        None,
    )
    .await;

    // No active provider configured → 400 with a clear, key-free message (never a crash).
    let r = call(
        &app,
        "POST",
        &format!("/api/items/{item}/summarize"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.body["error"].as_str().unwrap().contains("not configured"));

    // A stranger's item id is 404 (per-user scoping), not a provider call.
    let bob_c = register(&app, "bob", "password123").await.cookie.unwrap();
    let other = call(
        &app,
        "POST",
        &format!("/api/items/{item}/summarize"),
        None,
        Some(&bob_c),
    )
    .await;
    assert_eq!(other.status, StatusCode::NOT_FOUND);
}

// ===========================================================================
// Phase 6 - Digest engine + per-user ntfy (prompt.md §7, §7a, §11)
// ===========================================================================

/// Insert a shared item published "now" so it lands inside the digest look-back window.
async fn insert_recent_item(pool: &sqlx::SqlitePool, feed_id: i64, guid: &str, title: &str) -> i64 {
    sqlx::query(
        "INSERT INTO items (feed_id, guid, url, title, content_text, published_at, dedup_hash)
         VALUES (?, ?, ?, ?, ?, datetime('now'), ?) RETURNING id",
    )
    .bind(feed_id)
    .bind(guid)
    .bind(format!("https://ex.example/{guid}"))
    .bind(title)
    .bind("body text")
    .bind(guid)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id")
}

async fn set_ntfy(app: &axum::Router, cookie: &str, topic: &str, on_health: bool) -> Resp {
    call(
        app,
        "PUT",
        "/api/notifications",
        Some(json!({
            "ntfy_server_url": "https://ntfy.example",
            "ntfy_topic": topic,
            "notify_on_digest": true,
            "notify_on_feed_health": on_health,
        })),
        Some(cookie),
    )
    .await
}

#[tokio::test]
async fn feed_health_transition_is_detected_once_per_episode() {
    use crate::ingest::fetch::{FetchError, Fetched};
    use crate::ingest::store;

    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let sub = subscribe(
        &app,
        &alice_c,
        "https://health.example/feed.xml",
        cat_id(&cats.body, "Other"),
    )
    .await;
    let feed_id = sub.body["feed_id"].as_i64().unwrap();

    // First failure = healthy→failing transition → notify once.
    let t1 = store::record_failure(&pool, feed_id, &FetchError::Transient("boom".into()))
        .await
        .unwrap();
    assert!(t1, "first failure is the healthy→failing transition");

    // Subsequent failures are NOT transitions (throttled - not one per poll).
    let t2 = store::record_failure(&pool, feed_id, &FetchError::Transient("boom".into()))
        .await
        .unwrap();
    assert!(!t2, "still-failing must not re-notify every poll");

    // Recovery resets health; the next failure is a fresh transition again.
    let fetched = Fetched {
        body: Vec::new(),
        etag: None,
        last_modified: None,
        permanent_url: None,
    };
    store::record_success(&pool, feed_id, &fetched, 3600)
        .await
        .unwrap();
    let t3 = store::record_failure(&pool, feed_id, &FetchError::Transient("boom".into()))
        .await
        .unwrap();
    assert!(t3, "after recovery, going unhealthy is a new transition");
}

#[tokio::test]
async fn feed_health_recipients_are_deduped_and_scoped() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let bob_c = register(&app, "bob", "password123").await.cookie.unwrap();
    let carol_c = register(&app, "carol", "password123").await.cookie.unwrap();
    let dave_c = register(&app, "dave", "password123").await.cookie.unwrap();

    let a_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let b_cats = call(&app, "GET", "/api/categories", None, Some(&bob_c)).await;
    let c_cats = call(&app, "GET", "/api/categories", None, Some(&carol_c)).await;
    let d_cats = call(&app, "GET", "/api/categories", None, Some(&dave_c)).await;

    // Alice + Bob subscribe to the SAME shared feed and enable feed-health pushes.
    let a_sub = subscribe(
        &app,
        &alice_c,
        "https://shared.example/feed.xml",
        cat_id(&a_cats.body, "Other"),
    )
    .await;
    subscribe(
        &app,
        &bob_c,
        "https://shared.example/feed.xml",
        cat_id(&b_cats.body, "Other"),
    )
    .await;
    let feed_id = a_sub.body["feed_id"].as_i64().unwrap();
    set_ntfy(&app, &alice_c, "alice-topic", true).await;
    set_ntfy(&app, &bob_c, "bob-topic", true).await;

    // Carol subscribes to the same feed but turned feed-health OFF → excluded.
    subscribe(
        &app,
        &carol_c,
        "https://shared.example/feed.xml",
        cat_id(&c_cats.body, "Other"),
    )
    .await;
    set_ntfy(&app, &carol_c, "carol-topic", false).await;

    // Dave has feed-health ON but subscribes to a DIFFERENT feed → not a recipient (no leakage).
    subscribe(
        &app,
        &dave_c,
        "https://other.example/feed.xml",
        cat_id(&d_cats.body, "Other"),
    )
    .await;
    set_ntfy(&app, &dave_c, "dave-topic", true).await;

    let mut recipients = crate::notify::feed_health_recipients(&pool, feed_id)
        .await
        .unwrap();
    recipients.sort_unstable();

    // Resolve alice & bob user ids to compare.
    async fn uid(pool: &sqlx::SqlitePool, username: &str) -> i64 {
        sqlx::query("SELECT id FROM users WHERE username = ?")
            .bind(username)
            .fetch_one(pool)
            .await
            .unwrap()
            .get("id")
    }
    let mut expected = vec![uid(&pool, "alice").await, uid(&pool, "bob").await];
    expected.sort_unstable();

    assert_eq!(
        recipients, expected,
        "exactly the two subscribers with health-on + a channel, de-duped"
    );
}

#[tokio::test]
async fn digest_run_is_admin_only_and_archived_per_user_with_raw_fallback() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();

    // Alice subscribes and has recent items across two categories.
    let cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let ai_sub = subscribe(
        &app,
        &alice_c,
        "https://ai.example/feed.xml",
        cat_id(&cats.body, "Other"),
    )
    .await;
    let feed_id = ai_sub.body["feed_id"].as_i64().unwrap();
    insert_recent_item(&pool, feed_id, "d1", "New model released").await;
    insert_recent_item(&pool, feed_id, "d2", "Framework 2.0 ships").await;

    // Non-admin cannot run the engine (§11).
    let denied = call(&app, "POST", "/api/digest/run", None, Some(&alice_c)).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let anon = call(&app, "POST", "/api/digest/run", None, None).await;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);

    // Admin runs it for all users. No AI provider is configured → raw fallback, never a failure.
    let admin_c = login(&app, "admin", ADMIN_PW).await.cookie.unwrap();
    let run = call(&app, "POST", "/api/digest/run", None, Some(&admin_c)).await;
    assert_eq!(run.status, StatusCode::OK);
    assert!(run.body["digests"].as_i64().unwrap() >= 1);

    // Alice sees her archived digest and can open it.
    let list = call(&app, "GET", "/api/digest", None, Some(&alice_c)).await;
    assert_eq!(list.status, StatusCode::OK);
    let digest = &list.body.as_array().unwrap()[0];
    assert_eq!(digest["item_count"].as_i64().unwrap(), 2);
    let id = digest["id"].as_i64().unwrap();

    let detail = call(
        &app,
        "GET",
        &format!("/api/digest/{id}"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(detail.status, StatusCode::OK);
    let payload = &detail.body["payload"];
    assert_eq!(payload["ai_used"], false, "no provider → no AI");
    assert!(payload["fallback_note"]
        .as_str()
        .unwrap()
        .contains("no active provider"));
    // The Other category section is present with raw headlines.
    let other_section = payload["categories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Other")
        .unwrap();
    assert_eq!(other_section["raw"], true);
    assert_eq!(other_section["items"].as_array().unwrap().len(), 2);

    // Another user's digest is not visible (per-user scoping): 404 for a stranger.
    let bob_c = register(&app, "bob", "password123").await.cookie.unwrap();
    let cross = call(
        &app,
        "GET",
        &format!("/api/digest/{id}"),
        None,
        Some(&bob_c),
    )
    .await;
    assert_eq!(cross.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn digest_run_accepts_a_custom_lookback_override() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let ai_sub = subscribe(
        &app,
        &alice_c,
        "https://ai.example/feed.xml",
        cat_id(&cats.body, "Other"),
    )
    .await;
    let feed_id = ai_sub.body["feed_id"].as_i64().unwrap();

    // One item from today, one from 10 days ago.
    insert_recent_item(&pool, feed_id, "fresh", "Fresh news").await;
    let ten_days_ago = (chrono::Utc::now() - chrono::Duration::days(10))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    insert_item(
        &pool,
        feed_id,
        "old",
        "Old news",
        "old text",
        &ten_days_ago,
        None,
        None,
        None,
    )
    .await;

    let admin_c = login(&app, "admin", ADMIN_PW).await.cookie.unwrap();

    // Default config has a 1-day look-back (§4 plan) - only the fresh item is included.
    let run_default = call(&app, "POST", "/api/digest/run", None, Some(&admin_c)).await;
    assert_eq!(run_default.status, StatusCode::OK);
    let list1 = call(&app, "GET", "/api/digest", None, Some(&alice_c)).await;
    assert_eq!(
        list1.body.as_array().unwrap()[0]["item_count"]
            .as_i64()
            .unwrap(),
        1
    );

    // A manual run with a 30-day override picks up the old item too.
    let run_override = call(
        &app,
        "POST",
        "/api/digest/run",
        Some(json!({ "lookback_days": 30 })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(run_override.status, StatusCode::OK);
    let list2 = call(&app, "GET", "/api/digest", None, Some(&alice_c)).await;
    assert_eq!(
        list2.body.as_array().unwrap()[0]["item_count"]
            .as_i64()
            .unwrap(),
        2,
        "30-day override should include the 10-day-old item"
    );

    // The persisted config must be untouched by the one-off override.
    let cfg = call(&app, "GET", "/api/digest/config", None, Some(&admin_c)).await;
    assert_eq!(
        cfg.body["lookback_days"].as_i64().unwrap(),
        1,
        "manual override must not persist to app_settings"
    );
}

#[tokio::test]
async fn regular_users_can_read_the_digest_schedule_but_not_its_config() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();

    let schedule = call(&app, "GET", "/api/digest/schedule", None, Some(&alice_c)).await;
    assert_eq!(schedule.status, StatusCode::OK);
    assert_eq!(schedule.body.as_object().unwrap().len(), 4);
    assert!(schedule.body.get("enabled").is_some());
    assert!(schedule.body.get("description").is_some());
    assert!(schedule.body.get("timezone").is_some());
    assert!(schedule.body.get("next_run_at").is_some());
    assert_eq!(schedule.body["enabled"], true);
    assert_eq!(schedule.body["timezone"], "UTC");
    assert!(schedule.body["description"].as_str().is_some());
    assert!(schedule.body["next_run_at"].as_str().is_some());

    let config = call(&app, "GET", "/api/digest/config", None, Some(&alice_c)).await;
    assert_eq!(config.status, StatusCode::FORBIDDEN);

    sqlx::query("INSERT INTO app_settings (key, value) VALUES ('digest.enabled', 'false')")
        .execute(&pool)
        .await
        .unwrap();
    let disabled = call(&app, "GET", "/api/digest/schedule", None, Some(&alice_c)).await;
    assert_eq!(disabled.status, StatusCode::OK);
    assert_eq!(disabled.body["enabled"], false);
    assert!(disabled.body["next_run_at"].is_null());
}

#[tokio::test]
async fn notifications_config_never_returns_the_token() {
    let (app, _pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();

    let secret = "tk_super_secret_ntfy_token";
    let put = call(
        &app,
        "PUT",
        "/api/notifications",
        Some(json!({
            "ntfy_server_url": "http://localhost:8080",
            "ntfy_topic": "digestly",
            "auth_token": secret,
            "notify_on_digest": true,
            "notify_on_feed_health": true,
        })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(put.status, StatusCode::OK);
    assert!(
        !put.body.to_string().contains(secret),
        "PUT response must not echo the token"
    );
    assert_eq!(put.body["has_token"], true);

    let get = call(&app, "GET", "/api/notifications", None, Some(&alice_c)).await;
    let raw = get.body.to_string();
    assert!(!raw.contains(secret), "GET must not leak the token");
    assert!(!raw.contains("auth_token"), "no token field is serialized");
    assert_eq!(get.body["has_token"], true);
    assert_eq!(get.body["ntfy_topic"], "digestly");

    // Digest config is admin-only.
    let cfg = call(&app, "GET", "/api/digest/config", None, Some(&alice_c)).await;
    assert_eq!(cfg.status, StatusCode::FORBIDDEN);
}

// ===========================================================================
// Phase 7 - Settings, OPML, retention, offline fixtures (§8, §9.7, §11, §13)
// ===========================================================================

#[tokio::test]
async fn per_user_settings_round_trip_and_validate() {
    let (app, _pool, _d) = test_app().await;
    let c = register(&app, "alice", "password123").await.cookie.unwrap();

    // Defaults on a fresh account.
    let def = call(&app, "GET", "/api/settings", None, Some(&c)).await;
    assert_eq!(def.body["timezone"], "UTC");
    assert_eq!(def.body["onboarded"], false);

    // Valid partial update persists; unspecified fields keep defaults.
    let ok = call(
        &app,
        "PUT",
        "/api/settings",
        Some(json!({ "timezone": "Europe/Warsaw", "page_size": 25, "onboarded": true })),
        Some(&c),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.body["timezone"], "Europe/Warsaw");
    assert_eq!(ok.body["page_size"], 25);
    assert_eq!(ok.body["onboarded"], true);
    assert_eq!(ok.body["theme"], "dark", "unspecified field keeps default");

    // Invalid values are rejected.
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/settings",
            Some(json!({ "timezone": "Mars/Olympus" })),
            Some(&c)
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/settings",
            Some(json!({ "sort": "sideways" })),
            Some(&c)
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn opml_export_import_round_trips_losslessly_via_api() {
    let (app, _pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();

    // Only "Other" is seeded - insert "Software Engineering" for this OPML test (§TODO-9).
    let alice_id: i64 = sqlx::query("SELECT id FROM users WHERE username = 'alice'")
        .fetch_one(&_pool)
        .await
        .unwrap()
        .get("id");
    sqlx::query(
        "INSERT INTO categories (user_id, name, position) VALUES (?, 'Software Engineering', 1)",
    )
    .bind(alice_id)
    .execute(&_pool)
    .await
    .unwrap();
    let a_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    subscribe(
        &app,
        &alice_c,
        "https://news.ycombinator.com/rss",
        cat_id(&a_cats.body, "Software Engineering"),
    )
    .await;

    // Export → OPML with the feed under its category.
    let (status, xml) = call_text(&app, "GET", "/api/opml/export", Some(&alice_c)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(xml.contains("news.ycombinator.com/rss"));
    assert!(xml.contains("Software Engineering"));

    // Import that exact OPML into a fresh user (bob) → the category is recreated and feed subscribed.
    let bob_c = register(&app, "bob", "password123").await.cookie.unwrap();
    let preview = call(
        &app,
        "POST",
        "/api/opml/import",
        Some(json!({ "opml": xml })),
        Some(&bob_c),
    )
    .await;
    assert_eq!(preview.status, StatusCode::OK);
    let entries = preview.body["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["already_subscribed"], false);
    assert_eq!(entries[0]["category"], "Software Engineering");

    let items: Vec<Value> = entries
        .iter()
        .map(|e| json!({ "feed_url": e["feed_url"], "title": e["title"], "kind": e["kind"], "category": e["category"] }))
        .collect();
    let confirm = call(
        &app,
        "POST",
        "/api/opml/import",
        Some(json!({ "items": items })),
        Some(&bob_c),
    )
    .await;
    assert_eq!(confirm.body["imported"], 1);

    // Bob now has the feed under a (recreated) "Software Engineering" category.
    let bob_feeds = call(&app, "GET", "/api/feeds", None, Some(&bob_c)).await;
    let f = &bob_feeds.body.as_array().unwrap()[0];
    assert_eq!(f["feed_url"], "https://news.ycombinator.com/rss");
    assert_eq!(f["category_name"], "Software Engineering");

    // Re-importing is idempotent (skipped, not duplicated).
    let again = call(
        &app,
        "POST",
        "/api/opml/import",
        Some(json!({ "items": items })),
        Some(&bob_c),
    )
    .await;
    assert_eq!(again.body["imported"], 0);
    assert_eq!(again.body["skipped"], 1);
}

#[tokio::test]
async fn retention_purge_removes_old_but_keeps_starred_forever() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let sub = subscribe(
        &app,
        &alice_c,
        "https://ex.example/feed.xml",
        cat_id(&cats.body, "Other"),
    )
    .await;
    let feed_id = sub.body["feed_id"].as_i64().unwrap();

    let old_starred = insert_item(
        &pool,
        feed_id,
        "os",
        "Old Starred",
        "x",
        "2000-01-01 00:00:00",
        None,
        None,
        None,
    )
    .await;
    let old_plain = insert_item(
        &pool,
        feed_id,
        "op",
        "Old Plain",
        "x",
        "2000-01-02 00:00:00",
        None,
        None,
        None,
    )
    .await;
    let recent = insert_recent_item(&pool, feed_id, "rc", "Recent").await;

    // Alice stars the old item → it must survive retention forever.
    call(
        &app,
        "POST",
        &format!("/api/items/{old_starred}/star"),
        Some(json!({ "value": true })),
        Some(&alice_c),
    )
    .await;

    // Purge everything older than 30 days.
    sqlx::query("INSERT INTO app_settings (key, value) VALUES ('retention.max_age_days', '30')")
        .execute(&pool)
        .await
        .unwrap();
    let removed = crate::maintenance::purge(&pool).await.unwrap();
    assert_eq!(removed, 1, "only the old, non-starred item is purged");

    let exists = |id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query("SELECT 1 FROM items WHERE id = ?")
                .bind(id)
                .fetch_optional(&pool)
                .await
                .unwrap()
                .is_some()
        }
    };
    assert!(exists(old_starred).await, "starred item kept forever");
    assert!(!exists(old_plain).await, "old non-starred item purged");
    assert!(exists(recent).await, "recent item kept");
}

#[tokio::test]
async fn retention_purge_endpoint_is_admin_only_and_deletes_now() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let sub = subscribe(
        &app,
        &alice_c,
        "https://ex.example/feed.xml",
        cat_id(&cats.body, "Other"),
    )
    .await;
    let feed_id = sub.body["feed_id"].as_i64().unwrap();

    let old_starred = insert_item(
        &pool,
        feed_id,
        "os",
        "Old Starred",
        "x",
        "2000-01-01 00:00:00",
        None,
        None,
        None,
    )
    .await;
    let old_plain = insert_item(
        &pool,
        feed_id,
        "op",
        "Old Plain",
        "x",
        "2000-01-02 00:00:00",
        None,
        None,
        None,
    )
    .await;
    let recent = insert_recent_item(&pool, feed_id, "rc", "Recent").await;
    call(
        &app,
        "POST",
        &format!("/api/items/{old_starred}/star"),
        Some(json!({ "value": true })),
        Some(&alice_c),
    )
    .await;

    let admin_c = login(&app, "admin", ADMIN_PW).await.cookie.unwrap();

    // Non-admin cannot trigger a purge (§11).
    let denied = call(
        &app,
        "POST",
        "/api/admin/retention/purge",
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);

    // No retention limit set yet → purge is a safe no-op.
    let noop = call(
        &app,
        "POST",
        "/api/admin/retention/purge",
        None,
        Some(&admin_c),
    )
    .await;
    assert_eq!(noop.status, StatusCode::OK);
    assert_eq!(noop.body["removed"], 0);

    // Admin sets a 30-day retention limit via the real settings endpoint, then purges now.
    let put = call(
        &app,
        "PUT",
        "/api/admin/ingestion",
        Some(json!({
            "concurrency": 4, "per_host_delay_ms": 1000, "timeout_secs": 20,
            "default_interval_secs": 900, "allow_private": false, "max_item_age_days": 0,
            "retention_max_age_days": 30, "retention_max_per_feed": 0
        })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(put.status, StatusCode::OK);

    let run = call(
        &app,
        "POST",
        "/api/admin/retention/purge",
        None,
        Some(&admin_c),
    )
    .await;
    assert_eq!(run.status, StatusCode::OK);
    assert_eq!(
        run.body["removed"], 1,
        "only the old, non-starred item is purged"
    );

    let exists = |id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query("SELECT 1 FROM items WHERE id = ?")
                .bind(id)
                .fetch_optional(&pool)
                .await
                .unwrap()
                .is_some()
        }
    };
    assert!(exists(old_starred).await, "starred item kept forever");
    assert!(!exists(old_plain).await, "old non-starred item purged");
    assert!(exists(recent).await, "recent item kept");
}

#[tokio::test]
async fn bundled_fixture_feeds_parse_offline_and_sanitize() {
    use crate::ingest::settings::IngestSettings;
    use crate::ingest::{parse, FeedKind};
    use chrono::Utc;

    let base = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let cfg = IngestSettings::default();

    for (file, kind, expected) in [
        ("sample_rss.xml", FeedKind::Rss, 2usize),
        ("sample_atom.xml", FeedKind::Atom, 2),
        ("sample_jsonfeed.json", FeedKind::JsonFeed, 2),
    ] {
        let bytes = std::fs::read(format!("{base}/{file}")).unwrap();
        let parsed = parse::parse_feed(
            &bytes,
            "https://fixtures.example/feed",
            kind,
            &cfg,
            Utc::now(),
        )
        .unwrap();
        assert_eq!(parsed.items.len(), expected, "{file} item count");
    }

    // The RSS fixture carries an XSS payload that must be stripped by ammonia (§11).
    let rss = std::fs::read(format!("{base}/sample_rss.xml")).unwrap();
    let parsed = parse::parse_feed(
        &rss,
        "https://fixtures.example/feed",
        FeedKind::Rss,
        &cfg,
        Utc::now(),
    )
    .unwrap();
    let html = parsed
        .items
        .iter()
        .filter_map(|i| i.content_html.clone())
        .collect::<String>();
    assert!(!html.contains("<script"), "scripts stripped");
    assert!(
        !html.to_lowercase().contains("javascript:"),
        "javascript: URLs stripped"
    );
}

// ── Passkeys / WebAuthn (Stretch S1) ─────────────────────────────────────────

/// Look up a user's id by username (tests seed passkey rows directly - a valid credential blob
/// requires a real authenticator, but the management endpoints never deserialize it).
async fn user_id_of(pool: &sqlx::SqlitePool, username: &str) -> i64 {
    sqlx::query("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(pool)
        .await
        .unwrap()
        .get("id")
}

async fn insert_passkey(pool: &sqlx::SqlitePool, user_id: i64, cred: &str, name: &str) -> i64 {
    sqlx::query(
        "INSERT INTO passkeys (user_id, credential_id, public_key, sign_count, name)
         VALUES (?, ?, ?, 0, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(cred)
    .bind(b"dummy-blob".as_slice())
    .bind(name)
    .fetch_one(pool)
    .await
    .unwrap()
    .get("id")
}

#[tokio::test]
async fn passkey_endpoints_are_wired_scoped_and_report_enabled() {
    let (app, pool, _d) = test_app().await;

    // The public capability flag drives the login button (RP built in test_app).
    let status = call(&app, "GET", "/api/auth/registration", None, None).await;
    assert_eq!(status.body["passkeys_enabled"], json!(true));

    // Listing requires auth.
    let anon = call(&app, "GET", "/api/passkeys", None, None).await;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);

    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let bob_c = register(&app, "bob", "password123").await.cookie.unwrap();

    // Fresh account has no passkeys, and starting registration yields a real challenge.
    let empty = call(&app, "GET", "/api/passkeys", None, Some(&alice_c)).await;
    assert_eq!(empty.body.as_array().unwrap().len(), 0);
    let opts = call(
        &app,
        "POST",
        "/api/passkeys/register/options",
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(opts.status, StatusCode::OK);
    assert!(opts.body["ceremony_id"].as_str().is_some());
    assert!(opts.body["options"]["publicKey"]["challenge"]
        .as_str()
        .is_some());

    // Passwordless login for an account with no passkeys is a clear, non-crashing error.
    let no_pk = call(
        &app,
        "POST",
        "/api/auth/passkey/login/options",
        Some(json!({ "username": "bob" })),
        None,
    )
    .await;
    assert_eq!(no_pk.status, StatusCode::BAD_REQUEST);

    // Seed a passkey for alice directly and prove strict per-user scoping.
    let alice_id = user_id_of(&pool, "alice").await;
    let pk_id = insert_passkey(&pool, alice_id, "cred-alice-1", "Alice Key").await;

    let alice_list = call(&app, "GET", "/api/passkeys", None, Some(&alice_c)).await;
    assert_eq!(alice_list.body.as_array().unwrap().len(), 1);
    let bob_list = call(&app, "GET", "/api/passkeys", None, Some(&bob_c)).await;
    assert_eq!(
        bob_list.body.as_array().unwrap().len(),
        0,
        "bob never sees alice's passkey"
    );

    // Bob cannot rename or delete alice's passkey.
    let steal_rename = call(
        &app,
        "PATCH",
        &format!("/api/passkeys/{pk_id}"),
        Some(json!({ "name": "hax" })),
        Some(&bob_c),
    )
    .await;
    assert_eq!(steal_rename.status, StatusCode::NOT_FOUND);
    let steal_del = call(
        &app,
        "DELETE",
        &format!("/api/passkeys/{pk_id}"),
        None,
        Some(&bob_c),
    )
    .await;
    assert_eq!(steal_del.status, StatusCode::NOT_FOUND);

    // The list never leaks credential material.
    assert!(alice_list.body[0].get("public_key").is_none());
    assert!(alice_list.body[0].get("credential_id").is_none());
}

#[tokio::test]
async fn last_sign_in_method_cannot_be_deleted() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let alice_id = user_id_of(&pool, "alice").await;
    let pk_id = insert_passkey(&pool, alice_id, "cred-alice-1", "Only Key").await;

    // With a password set, deleting the passkey is fine (password remains a sign-in method).
    // First verify that; then re-add and remove the password to prove the guard fires.
    let del_ok = call(
        &app,
        "DELETE",
        &format!("/api/passkeys/{pk_id}"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(
        del_ok.status,
        StatusCode::OK,
        "deleting a passkey is allowed while a password exists"
    );

    // Now make the passkey the ONLY sign-in method: no password, one passkey.
    let pk_id2 = insert_passkey(&pool, alice_id, "cred-alice-2", "Sole Key").await;
    sqlx::query("UPDATE users SET password_hash = NULL WHERE id = ?")
        .bind(alice_id)
        .execute(&pool)
        .await
        .unwrap();

    let blocked = call(
        &app,
        "DELETE",
        &format!("/api/passkeys/{pk_id2}"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(
        blocked.status,
        StatusCode::BAD_REQUEST,
        "cannot delete the only sign-in method"
    );
    // It's still there.
    let still = call(&app, "GET", "/api/passkeys", None, Some(&alice_c)).await;
    assert_eq!(still.body.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn passkey_end_to_end_register_login_and_sign_count_regression() {
    // A software WebAuthn authenticator drives the real endpoints headless, exercising the full
    // S1 acceptance gate: register a passkey, sign in passwordless, and reject a cloned
    // authenticator whose sign-count regressed.
    use webauthn_authenticator_rs::softpasskey::SoftPasskey;
    use webauthn_authenticator_rs::WebauthnAuthenticator;
    use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let origin = Url::parse("http://localhost:8080").unwrap();
    let mut authr = WebauthnAuthenticator::new(SoftPasskey::new(true));

    // 1) Register a passkey through the real options → ceremony → verify flow.
    let opts = call(
        &app,
        "POST",
        "/api/passkeys/register/options",
        None,
        Some(&alice_c),
    )
    .await;
    let ceremony_id = opts.body["ceremony_id"].as_str().unwrap().to_string();
    let ccr: CreationChallengeResponse =
        serde_json::from_value(opts.body["options"].clone()).unwrap();
    let reg_cred = authr.do_registration(origin.clone(), ccr).unwrap();
    let verify = call(
        &app,
        "POST",
        "/api/passkeys/register/verify",
        Some(json!({ "ceremony_id": ceremony_id, "credential": reg_cred, "name": "Soft Key" })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(verify.status, StatusCode::OK, "passkey registered");

    // 2) Passwordless sign-in - no password is ever sent, yet a session is established.
    let lo = call(
        &app,
        "POST",
        "/api/auth/passkey/login/options",
        Some(json!({ "username": "alice" })),
        None,
    )
    .await;
    let lcid = lo.body["ceremony_id"].as_str().unwrap().to_string();
    let rcr: RequestChallengeResponse = serde_json::from_value(lo.body["options"].clone()).unwrap();
    let cred = authr.do_authentication(origin.clone(), rcr).unwrap();
    let lv = call(
        &app,
        "POST",
        "/api/auth/passkey/login/verify",
        Some(json!({ "ceremony_id": lcid, "credential": cred })),
        None,
    )
    .await;
    assert_eq!(
        lv.status,
        StatusCode::OK,
        "passwordless passkey sign-in succeeds"
    );
    let pk_cookie = lv.cookie.unwrap();
    let me = call(&app, "GET", "/api/me", None, Some(&pk_cookie)).await;
    assert_eq!(
        me.body["username"], "alice",
        "the passkey session is a real, scoped session"
    );

    // 3) Simulate a cloned authenticator: force the stored counter above what the token will
    //    present next. The regression guard must reject the assertion.
    sqlx::query("UPDATE passkeys SET sign_count = 100000")
        .execute(&pool)
        .await
        .unwrap();
    let lo2 = call(
        &app,
        "POST",
        "/api/auth/passkey/login/options",
        Some(json!({ "username": "alice" })),
        None,
    )
    .await;
    let lcid2 = lo2.body["ceremony_id"].as_str().unwrap().to_string();
    let rcr2: RequestChallengeResponse =
        serde_json::from_value(lo2.body["options"].clone()).unwrap();
    let cred2 = authr.do_authentication(origin, rcr2).unwrap();
    let lv2 = call(
        &app,
        "POST",
        "/api/auth/passkey/login/verify",
        Some(json!({ "ceremony_id": lcid2, "credential": cred2 })),
        None,
    )
    .await;
    assert_eq!(
        lv2.status,
        StatusCode::UNAUTHORIZED,
        "sign-count regression (cloned authenticator) is rejected"
    );
}

#[tokio::test]
async fn passkey_discoverable_login_signs_in_without_a_username_and_guards_regression() {
    // Conditional-UI / autofill sign-in: register a passkey, then authenticate through the
    // discoverable endpoints - no username is ever sent; the server resolves the user from the
    // credential's embedded handle. The sign-count regression guard must fire on this path too.
    //
    // The software authenticator has no discoverable-credential support (it needs `allowCredentials`
    // populated and never emits a `userHandle`), so we drive it at the wire level: inject the known
    // credential id so it can sign, then inject the user handle the way a real platform authenticator
    // with a resident key would - everything the server does with those values is exercised for real.
    use webauthn_authenticator_rs::softpasskey::SoftPasskey;
    use webauthn_authenticator_rs::WebauthnAuthenticator;
    use webauthn_rs::prelude::{
        Base64UrlSafeData, CreationChallengeResponse, RequestChallengeResponse, Url,
    };

    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let alice_id = user_id_of(&pool, "alice").await;
    let origin = Url::parse("http://localhost:8080").unwrap();
    let mut authr = WebauthnAuthenticator::new(SoftPasskey::new(true));

    // 1) Register a passkey through the real options → ceremony → verify flow.
    let opts = call(
        &app,
        "POST",
        "/api/passkeys/register/options",
        None,
        Some(&alice_c),
    )
    .await;
    let ceremony_id = opts.body["ceremony_id"].as_str().unwrap().to_string();
    let ccr: CreationChallengeResponse =
        serde_json::from_value(opts.body["options"].clone()).unwrap();
    let reg_cred = authr.do_registration(origin.clone(), ccr).unwrap();
    let reg_val = serde_json::to_value(&reg_cred).unwrap();
    let cred_id_b64 = reg_val["rawId"].clone();
    let verify = call(
        &app,
        "POST",
        "/api/passkeys/register/verify",
        Some(json!({ "ceremony_id": ceremony_id, "credential": reg_cred, "name": "Soft Key" })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(verify.status, StatusCode::OK, "passkey registered");

    // The user handle a real resident credential would return: the same one embedded at registration.
    let user_handle = crate::auth::passkey::user_handle(alice_id);
    let user_handle_b64 =
        serde_json::to_value(Base64UrlSafeData::from(user_handle.as_bytes().to_vec())).unwrap();

    // A single discoverable assertion: no username sent; options carry no allowCredentials, so we
    // seed the chosen credential and stamp the userHandle a platform authenticator would supply.
    let discoverable_assert = |authr: &mut WebauthnAuthenticator<SoftPasskey>,
                               options: serde_json::Value| {
        let mut opts_val = options;
        opts_val["publicKey"]["allowCredentials"] =
            json!([{ "type": "public-key", "id": cred_id_b64 }]);
        let rcr: RequestChallengeResponse = serde_json::from_value(opts_val).unwrap();
        let cred = authr.do_authentication(origin.clone(), rcr).unwrap();
        let mut cred_val = serde_json::to_value(&cred).unwrap();
        cred_val["response"]["userHandle"] = user_handle_b64.clone();
        cred_val
    };

    // 2) Discoverable sign-in - the server resolves alice from the credential's user handle.
    let lo = call(
        &app,
        "POST",
        "/api/auth/passkey/discoverable/login/options",
        None,
        None,
    )
    .await;
    assert_eq!(lo.status, StatusCode::OK);
    assert_eq!(
        lo.body["options"]["mediation"], "conditional",
        "server forces conditional mediation"
    );
    let lcid = lo.body["ceremony_id"].as_str().unwrap().to_string();
    let cred = discoverable_assert(&mut authr, lo.body["options"].clone());
    let lv = call(
        &app,
        "POST",
        "/api/auth/passkey/discoverable/login/verify",
        Some(json!({ "ceremony_id": lcid, "credential": cred })),
        None,
    )
    .await;
    assert_eq!(
        lv.status,
        StatusCode::OK,
        "discoverable passkey sign-in succeeds without a username"
    );
    let pk_cookie = lv.cookie.unwrap();
    let me = call(&app, "GET", "/api/me", None, Some(&pk_cookie)).await;
    assert_eq!(
        me.body["username"], "alice",
        "the discoverable session is a real, scoped session"
    );

    // last_used_at is stamped on this path.
    let last_used: Option<String> = sqlx::query("SELECT last_used_at FROM passkeys")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("last_used_at");
    assert!(
        last_used.is_some(),
        "last_used_at updated via the discoverable path"
    );

    // 3) Cloned-authenticator guard fires here too.
    sqlx::query("UPDATE passkeys SET sign_count = 100000")
        .execute(&pool)
        .await
        .unwrap();
    let lo2 = call(
        &app,
        "POST",
        "/api/auth/passkey/discoverable/login/options",
        None,
        None,
    )
    .await;
    let lcid2 = lo2.body["ceremony_id"].as_str().unwrap().to_string();
    let cred2 = discoverable_assert(&mut authr, lo2.body["options"].clone());
    let lv2 = call(
        &app,
        "POST",
        "/api/auth/passkey/discoverable/login/verify",
        Some(json!({ "ceremony_id": lcid2, "credential": cred2 })),
        None,
    )
    .await;
    assert_eq!(
        lv2.status,
        StatusCode::UNAUTHORIZED,
        "sign-count regression is rejected on the discoverable path too"
    );
}

// ── Offline write-sync replay (Stretch S3) ───────────────────────────────────

#[tokio::test]
async fn offline_replay_of_explicit_state_mutations_is_idempotent_and_converges() {
    // Simulates the outbox draining after reconnect: the client coalesces per (kind,item) to the
    // latest value and replays explicit-value writes in order. The server must converge on that
    // final intent regardless of duplicates/order, and stay strictly per-user.
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let bob_c = register(&app, "bob", "password123").await.cookie.unwrap();
    let a_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let b_cats = call(&app, "GET", "/api/categories", None, Some(&bob_c)).await;
    let sub = subscribe(
        &app,
        &alice_c,
        "https://shared.example/feed.xml",
        cat_id(&a_cats.body, "Other"),
    )
    .await;
    subscribe(
        &app,
        &bob_c,
        "https://shared.example/feed.xml",
        cat_id(&b_cats.body, "Other"),
    )
    .await;
    let feed_id = sub.body["feed_id"].as_i64().unwrap();
    let item = insert_item(
        &pool,
        feed_id,
        "g1",
        "Item",
        "body",
        "2021-06-15 12:00:00",
        None,
        None,
        Some(60),
    )
    .await;

    let read = |c: &str, v: bool| {
        let app = app.clone();
        let c = c.to_string();
        async move {
            call(
                &app,
                "POST",
                &format!("/api/items/{item}/read"),
                Some(json!({ "value": v })),
                Some(&c),
            )
            .await
        }
    };

    // Idempotent: replaying the same explicit write twice leaves the same state.
    assert_eq!(read(&alice_c, true).await.body["is_read"], true);
    assert_eq!(
        read(&alice_c, true).await.body["is_read"],
        true,
        "replaying read=true is a no-op"
    );

    // Superseding write (a coalesced flip) wins - final intent is unread.
    assert_eq!(read(&alice_c, false).await.body["is_read"], false);

    // A star write is independent of read state and also idempotent.
    call(
        &app,
        "POST",
        &format!("/api/items/{item}/star"),
        Some(json!({ "value": true })),
        Some(&alice_c),
    )
    .await;
    call(
        &app,
        "POST",
        &format!("/api/items/{item}/star"),
        Some(json!({ "value": true })),
        Some(&alice_c),
    )
    .await;

    // Server state matches the replayed final intent.
    let a_view = call(
        &app,
        "GET",
        &format!("/api/items/{item}"),
        None,
        Some(&alice_c),
    )
    .await;
    assert_eq!(
        a_view.body["is_read"], false,
        "converged to the last read intent"
    );
    assert_eq!(
        a_view.body["is_starred"], true,
        "star preserved across read writes"
    );

    // Bob's replay of his own queue is scoped to bob - alice is untouched.
    call(
        &app,
        "POST",
        &format!("/api/items/{item}/read"),
        Some(json!({ "value": true })),
        Some(&bob_c),
    )
    .await;
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/items/{item}"),
            None,
            Some(&bob_c)
        )
        .await
        .body["is_read"],
        true
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/items/{item}"),
            None,
            Some(&alice_c)
        )
        .await
        .body["is_read"],
        false,
        "bob's replayed mutation never touches alice's state",
    );
}

// ── OAuth import helpers (Stretch S4) ────────────────────────────────────────

#[tokio::test]
async fn oauth_hidden_and_locked_down_when_unconfigured() {
    // test_app() has no OAuth client credentials → the feature is hidden and endpoints refuse.
    let (app, _pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();

    let status = call(&app, "GET", "/api/oauth/status", None, Some(&alice_c)).await;
    assert_eq!(status.status, StatusCode::OK);
    for p in status.body.as_array().unwrap() {
        assert_eq!(p["configured"], false, "no client creds → not configured");
        assert_eq!(p["connected"], false);
    }
    // Cannot start a flow or sync when unconfigured.
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/oauth/youtube/authorize",
            None,
            Some(&alice_c)
        )
        .await
        .status,
        StatusCode::BAD_REQUEST,
    );
    assert_eq!(
        call(&app, "POST", "/api/oauth/reddit/sync", None, Some(&alice_c))
            .await
            .status,
        StatusCode::BAD_REQUEST,
    );
    // Auth is still required, and unknown providers 404.
    assert_eq!(
        call(&app, "GET", "/api/oauth/status", None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/oauth/myspace/authorize",
            None,
            Some(&alice_c)
        )
        .await
        .status,
        StatusCode::NOT_FOUND,
    );
}

#[tokio::test]
async fn oauth_reconcile_import_is_idempotent_and_per_user() {
    use crate::ingest::settings::IngestSettings;
    use crate::oauth::{reconcile, reddit_subscription, youtube_subscription};

    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let bob_c = register(&app, "bob", "password123").await.cookie.unwrap();
    let a_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let alice_id = user_id_of(&pool, "alice").await;
    let bob_id = user_id_of(&pool, "bob").await;
    let ai = cat_id(&a_cats.body, "Other");
    let cfg = IngestSettings::default();

    // A fixed "remote subscription list" (what fetch_subscriptions would return).
    let subs = vec![
        youtube_subscription("UC_x5XG1OV2P6uZZ5FSM9Ttw", "Google Developers"),
        youtube_subscription("UCabcabcabcabcabcabcabc", "Some Channel"),
        reddit_subscription("rust"),
    ];

    // First sync adds all three.
    let first = reconcile(&pool, &cfg, alice_id, ai, &subs).await.unwrap();
    assert_eq!((first.added, first.skipped, first.total), (3, 0, 3));

    // Re-running is idempotent - nothing new (the whole point of the repeatable button).
    let second = reconcile(&pool, &cfg, alice_id, ai, &subs).await.unwrap();
    assert_eq!(
        (second.added, second.skipped),
        (0, 3),
        "repeat sync adds nothing already present"
    );

    // A partial re-sync with one extra channel adds only the new one.
    let mut more = subs.clone();
    more.push(youtube_subscription(
        "UCnewnewnewnewnewnewnew",
        "New Channel",
    ));
    let third = reconcile(&pool, &cfg, alice_id, ai, &more).await.unwrap();
    assert_eq!(
        (third.added, third.skipped),
        (1, 3),
        "only the genuinely-new channel is added"
    );

    // Alice now has 4 feeds; the YouTube ones carry the right kind + polled feed URL.
    let feeds = call(&app, "GET", "/api/feeds", None, Some(&alice_c)).await;
    let list = feeds.body.as_array().unwrap();
    assert_eq!(list.len(), 4);
    assert!(list.iter().any(|f| f["kind"] == "youtube"
        && f["feed_url"]
            == "https://www.youtube.com/feeds/videos.xml?channel_id=UC_x5XG1OV2P6uZZ5FSM9Ttw"));
    assert!(list.iter().any(|f| f["kind"] == "reddit"));

    // Bob importing the same list is independent (per-user), and Bob starts empty.
    assert_eq!(
        call(&app, "GET", "/api/feeds", None, Some(&bob_c))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let b_cats = call(&app, "GET", "/api/categories", None, Some(&bob_c)).await;
    let bob_added = reconcile(&pool, &cfg, bob_id, cat_id(&b_cats.body, "Other"), &subs)
        .await
        .unwrap();
    assert_eq!(bob_added.added, 3, "bob's import is scoped to bob");
    assert_eq!(
        call(&app, "GET", "/api/feeds", None, Some(&alice_c))
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        4,
        "bob's import didn't change alice"
    );
}

#[tokio::test]
async fn oauth_sync_creates_feeds_that_are_not_immediately_due() {
    use crate::ingest::settings::IngestSettings;
    use crate::oauth::{reconcile, youtube_subscription};

    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let alice_id = user_id_of(&pool, "alice").await;
    let a_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let ai = cat_id(&a_cats.body, "Other");
    let cfg = IngestSettings::default();

    let subs = vec![youtube_subscription(
        "UC_x5XG1OV2P6uZZ5FSM9Ttw",
        "Google Developers",
    )];
    reconcile(&pool, &cfg, alice_id, ai, &subs).await.unwrap();

    let row = sqlx::query("SELECT next_fetch_at FROM feeds WHERE feed_url = ?")
        .bind("https://www.youtube.com/feeds/videos.xml?channel_id=UC_x5XG1OV2P6uZZ5FSM9Ttw")
        .fetch_one(&pool)
        .await
        .unwrap();
    let next_fetch_at: String = row.get("next_fetch_at");

    let now_row = sqlx::query("SELECT datetime('now') AS n")
        .fetch_one(&pool)
        .await
        .unwrap();
    let now: String = now_row.get("n");

    assert!(
        next_fetch_at > now,
        "OAuth-synced feed must be scheduled for a future poll, not due immediately \
         (next_fetch_at={next_fetch_at}, now={now})"
    );
}

#[tokio::test]
async fn manual_add_feed_still_polls_immediately() {
    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let a_cats = call(&app, "GET", "/api/categories", None, Some(&alice_c)).await;
    let cat_id = cat_id(&a_cats.body, "Other");

    let resp = call(
        &app,
        "POST",
        "/api/feeds",
        Some(json!({
            "feed_url": "https://example.com/manual-feed.xml",
            "kind": "rss",
            "category_id": cat_id
        })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK);

    // Manual add-feed must remain "due now" - regression guard for existing behavior.
    let row = sqlx::query("SELECT next_fetch_at FROM feeds WHERE feed_url = ?")
        .bind("https://example.com/manual-feed.xml")
        .fetch_one(&pool)
        .await
        .unwrap();
    let next_fetch_at: String = row.get("next_fetch_at");
    let now_row = sqlx::query("SELECT datetime('now') AS n")
        .fetch_one(&pool)
        .await
        .unwrap();
    let now: String = now_row.get("n");
    assert!(
        next_fetch_at <= now,
        "manual add-feed must still be due immediately (next_fetch_at={next_fetch_at}, now={now})"
    );
}

#[tokio::test]
async fn refresh_all_updates_only_the_caller_s_subscribed_feeds() {
    let (app, pool, _dir) = test_app().await;

    // Alice: two feeds via her "Other" category.
    let r = register(&app, "alice", "alice-password-1").await;
    let alice_cookie = r.cookie.clone().unwrap();
    let alice_id: i64 = r.body["id"].as_i64().unwrap();
    let alice_other: i64 =
        sqlx::query("SELECT id FROM categories WHERE user_id = ? AND name = 'Other'")
            .bind(alice_id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("id");

    // Bob: one feed, should be untouched by Alice's refresh-all.
    let r2 = register(&app, "bob", "bob-password-1").await;
    let bob_id: i64 = r2.body["id"].as_i64().unwrap();
    let bob_other: i64 =
        sqlx::query("SELECT id FROM categories WHERE user_id = ? AND name = 'Other'")
            .bind(bob_id)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("id");

    let far_future = "9999-01-01 00:00:00";
    let mk_feed = |url: &str| {
        let pool = pool.clone();
        let url = url.to_string();
        async move {
            sqlx::query(
                "INSERT INTO feeds (feed_url, kind, next_fetch_at) VALUES (?, 'rss', ?) RETURNING id",
            )
            .bind(&url)
            .bind(far_future)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get::<i64, _>("id")
        }
    };
    let feed_a1 = mk_feed("https://a1.example/feed.xml").await;
    let feed_a2 = mk_feed("https://a2.example/feed.xml").await;
    let feed_b1 = mk_feed("https://b1.example/feed.xml").await;

    for (feed_id, user_id, cat) in [
        (feed_a1, alice_id, alice_other),
        (feed_a2, alice_id, alice_other),
        (feed_b1, bob_id, bob_other),
    ] {
        sqlx::query("INSERT INTO subscriptions (user_id, feed_id, category_id) VALUES (?, ?, ?)")
            .bind(user_id)
            .bind(feed_id)
            .bind(cat)
            .execute(&pool)
            .await
            .unwrap();
    }

    // All three feeds start parked far in the future (not due).
    let resp = call(
        &app,
        "POST",
        "/api/feeds/refresh-all",
        None,
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(resp.body["ok"], true);
    assert_eq!(resp.body["feeds"], 2); // only Alice's two feeds

    let due_now = |feed_id: i64| {
        let pool = pool.clone();
        async move {
            let v: String = sqlx::query("SELECT next_fetch_at FROM feeds WHERE id = ?")
                .bind(feed_id)
                .fetch_one(&pool)
                .await
                .unwrap()
                .get("next_fetch_at");
            v
        }
    };
    assert_ne!(
        due_now(feed_a1).await,
        far_future,
        "Alice's feed 1 should now be due"
    );
    assert_ne!(
        due_now(feed_a2).await,
        far_future,
        "Alice's feed 2 should now be due"
    );
    assert_eq!(
        due_now(feed_b1).await,
        far_future,
        "Bob's feed must be untouched"
    );
}

#[tokio::test]
async fn oauth_refresh_token_is_encrypted_and_never_returned() {
    use crate::oauth::{self, Provider};

    let (app, pool, _d) = test_app().await;
    let alice_c = register(&app, "alice", "password123").await.cookie.unwrap();
    let alice_id = user_id_of(&pool, "alice").await;
    let enc_key: [u8; 32] = sha2::Sha256::digest(b"test-secret-key-at-least-16").into();
    let secret = "super-secret-refresh-token-value";

    oauth::save_connection(
        &pool,
        &enc_key,
        alice_id,
        Provider::Reddit,
        secret,
        Some("read"),
        Some("u/alice"),
    )
    .await
    .unwrap();

    // Stored blob is ciphertext - the plaintext token never appears at rest.
    let blob: Vec<u8> = sqlx::query(
        "SELECT refresh_token_enc FROM user_oauth WHERE user_id = ? AND provider = 'reddit'",
    )
    .bind(alice_id)
    .fetch_one(&pool)
    .await
    .unwrap()
    .get("refresh_token_enc");
    assert_ne!(blob, secret.as_bytes(), "token must be encrypted at rest");
    assert!(!String::from_utf8_lossy(&blob).contains(secret));

    // It round-trips only via the key.
    let back = oauth::load_refresh_token(&pool, &enc_key, alice_id, Provider::Reddit)
        .await
        .unwrap();
    assert_eq!(back.as_deref(), Some(secret));

    // The status endpoint reports connected but never serializes the token.
    let status = call(&app, "GET", "/api/oauth/status", None, Some(&alice_c)).await;
    let reddit = status
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["provider"] == "reddit")
        .unwrap()
        .clone();
    assert_eq!(reddit["connected"], true);
    assert_eq!(reddit["account_label"], "u/alice");
    let dump = status.body.to_string();
    assert!(
        !dump.contains(secret),
        "status response must not contain the token"
    );
    assert!(!dump.contains("refresh_token"), "no token field is exposed");
}

// ---------------------------------------------------------------------------
// Issue #6: Unicode-aware usernames + rename via PATCH /api/me.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unicode_username_registers_and_logs_in_case_insensitively() {
    let (app, _pool, _d) = test_app().await;

    let reg = register(&app, "Łukasz", "password123").await;
    assert_eq!(reg.status, StatusCode::OK);
    assert_eq!(reg.body["username"], "Łukasz");

    for casing in ["łukasz", "ŁUKASZ", "Łukasz"] {
        let lg = login(&app, casing, "password123").await;
        assert_eq!(lg.status, StatusCode::OK, "login failed for {casing}");
        assert_eq!(
            lg.body["username"], "Łukasz",
            "response carries display casing for {casing}"
        );
    }
}

#[tokio::test]
async fn unicode_username_collision_is_rejected() {
    let (app, _pool, _d) = test_app().await;

    let first = register(&app, "Łukasz", "password123").await;
    assert_eq!(first.status, StatusCode::OK);

    let second = register(&app, "łukasz", "password123").await;
    assert_eq!(second.status, StatusCode::CONFLICT);
    assert_eq!(second.body["error"], "username already taken");
}

#[tokio::test]
async fn combining_marks_are_rejected() {
    let (app, _pool, _d) = test_app().await;

    // "e" + U+0301 COMBINING ACUTE ACCENT - decomposed form, category Mn. is_alphanumeric()
    // returns false for the combining mark so the whole name fails charset validation.
    let reg = register(&app, "e\u{0301}lise", "password123").await;
    assert_eq!(reg.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rename_happy_path() {
    let (app, _pool, _d) = test_app().await;

    let bob = register(&app, "bob", "password123").await;
    assert_eq!(bob.status, StatusCode::OK);
    let cookie = bob.cookie.unwrap();

    let rename = call(
        &app,
        "PATCH",
        "/api/me",
        Some(json!({ "username": "Bobby" })),
        Some(&cookie),
    )
    .await;
    assert_eq!(rename.status, StatusCode::OK);
    assert_eq!(rename.body["username"], "Bobby");

    let me = call(&app, "GET", "/api/me", None, Some(&cookie)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["username"], "Bobby");

    let relogin = login(&app, "BOBBY", "password123").await;
    assert_eq!(relogin.status, StatusCode::OK);
    assert_eq!(relogin.body["username"], "Bobby");
}

#[tokio::test]
async fn rename_collision_returns_409() {
    let (app, _pool, _d) = test_app().await;

    let alice = register(&app, "alice", "password123").await;
    assert_eq!(alice.status, StatusCode::OK);
    let bob = register(&app, "bob", "password123").await;
    assert_eq!(bob.status, StatusCode::OK);
    let bob_c = bob.cookie.unwrap();

    // Any casing of the taken name should collide - the DB stores the normalized form.
    let clash = call(
        &app,
        "PATCH",
        "/api/me",
        Some(json!({ "username": "ALICE" })),
        Some(&bob_c),
    )
    .await;
    assert_eq!(clash.status, StatusCode::CONFLICT);
    assert_eq!(clash.body["error"], "username already taken");
}

#[tokio::test]
async fn rename_builtin_admin_forbidden() {
    let (app, _pool, _d) = test_app().await;

    let admin = login(&app, "admin", ADMIN_PW).await;
    assert_eq!(admin.status, StatusCode::OK);
    let admin_c = admin.cookie.unwrap();

    let attempt = call(
        &app,
        "PATCH",
        "/api/me",
        Some(json!({ "username": "root" })),
        Some(&admin_c),
    )
    .await;
    assert_eq!(attempt.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn rename_to_admin_blocked() {
    let (app, _pool, _d) = test_app().await;

    let alice = register(&app, "alice", "password123").await;
    let alice_c = alice.cookie.unwrap();

    let attempt = call(
        &app,
        "PATCH",
        "/api/me",
        Some(json!({ "username": "Admin" })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(attempt.status, StatusCode::CONFLICT);
    assert_eq!(attempt.body["error"], "username already taken");
}

#[tokio::test]
async fn legacy_change_password_still_works() {
    let (app, _pool, _d) = test_app().await;

    let alice = register(&app, "alice", "password123").await;
    let alice_c = alice.cookie.unwrap();

    let change = call(
        &app,
        "PATCH",
        "/api/me",
        Some(json!({
            "current_password": "password123",
            "new_password": "newpassword1"
        })),
        Some(&alice_c),
    )
    .await;
    assert_eq!(change.status, StatusCode::OK);

    assert_eq!(
        login(&app, "alice", "newpassword1").await.status,
        StatusCode::OK
    );
    assert_eq!(
        login(&app, "alice", "password123").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn rename_does_not_invalidate_session() {
    let (app, _pool, _d) = test_app().await;

    let bob = register(&app, "bob", "password123").await;
    let cookie = bob.cookie.unwrap();

    let rename = call(
        &app,
        "PATCH",
        "/api/me",
        Some(json!({ "username": "Bobby" })),
        Some(&cookie),
    )
    .await;
    assert_eq!(rename.status, StatusCode::OK);

    // Same cookie, no re-login: the session survives a rename because sessions key on user_id.
    let me = call(&app, "GET", "/api/me", None, Some(&cookie)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["username"], "Bobby");
}
