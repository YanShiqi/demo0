use std::{net::SocketAddr, path::Path};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use demo0::{
    app, auth,
    config::{Config, MessageConfig},
    db,
    error::AppError,
    model::{Role, User},
};
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;

#[tokio::test]
async fn public_registration_creates_an_ordinary_user() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("test.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let router = app::build(pool.clone(), config);

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/register")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    let csrf = between(&html, "name=\"csrf_token\" value=\"", "\"");
    let mismatched_body = format!(
        "csrf_token={csrf}&username=alice_1&nickname=Alice&password=correct+horse+battery&password_confirmation=different+secure+password"
    );
    let mismatched_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/register")
                .header(header::COOKIE, cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(mismatched_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatched_response.status(), StatusCode::BAD_REQUEST);
    let user_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(user_count, 0);

    let body = format!(
        "csrf_token={csrf}&username=alice_1&nickname=Alice&password=correct+horse+battery&password_confirmation=correct+horse+battery"
    );

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/register")
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .extension(ConnectInfo(
                    "127.0.0.1:43100".parse::<SocketAddr>().unwrap(),
                ))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let authenticated_cookie = response_cookie(&response);

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username_key = ?")
        .bind("alice_1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(user.role, "user");
    assert!(user.avatar_storage_name.is_none());
    assert_eq!(user.bio, "");

    let (_, profile_csrf) =
        page_session_with_cookie(&router, "/profile", &authenticated_cookie).await;
    let bio_body = format!("csrf_token={profile_csrf}&bio=Rust+learner%0AEnjoys+quiet+websites");
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/profile/bio")
                .header(header::COOKIE, authenticated_cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(bio_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let bio = sqlx::query_scalar::<_, String>("SELECT bio FROM users WHERE username_key = ?")
        .bind("alice_1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(bio, "Rust learner\nEnjoys quiet websites");

    let public_response = router
        .oneshot(
            Request::builder()
                .uri("/u/alice_1")
                .header(header::COOKIE, authenticated_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(public_response.status(), StatusCode::OK);
    let public_html = response_html(public_response).await;
    assert!(public_html.contains("@alice_1"));
    assert!(public_html.contains("Rust learner"));

    let duplicate_username = auth::create_user(
        &pool,
        "ALICE_1",
        "Another Name",
        "another secure password",
        Role::User,
    )
    .await
    .unwrap_err();
    assert!(matches!(duplicate_username, AppError::BadRequest(_)));
    let duplicate_nickname = auth::create_user(
        &pool,
        "another_user",
        "ＡＬＩＣＥ",
        "another secure password",
        Role::User,
    )
    .await
    .unwrap_err();
    assert!(matches!(duplicate_nickname, AppError::BadRequest(_)));
}

#[tokio::test]
async fn super_admin_can_promote_an_ordinary_user() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("roles.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let super_admin = auth::create_user(
        &pool,
        "root_user",
        "站长",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();
    let target = auth::create_user(
        &pool,
        "target_user",
        "目标用户",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let config = test_config(&temporary, database_url);
    let router = app::build(pool.clone(), config);
    let (anonymous_cookie, login_csrf) = page_session(&router, "/login").await;
    let login_body = format!(
        "csrf_token={login_csrf}&username={}&password=correct+horse+battery",
        super_admin.username
    );
    let login_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::COOKIE, anonymous_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .extension(ConnectInfo(
                    "127.0.0.1:43101".parse::<SocketAddr>().unwrap(),
                ))
                .body(Body::from(login_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::SEE_OTHER);
    let authenticated_cookie = response_cookie(&login_response);
    let (same_cookie, admin_csrf) =
        page_session_with_cookie(&router, "/admin/users", &authenticated_cookie).await;
    assert_eq!(same_cookie, authenticated_cookie);

    let role_body = format!("csrf_token={admin_csrf}&role=admin");
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/users/{}/role", target.id))
                .header(header::COOKIE, authenticated_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(role_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let role = sqlx::query_scalar::<_, String>("SELECT role FROM users WHERE id = ?")
        .bind(&target.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(role, "admin");
    let audit_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM role_audit_logs WHERE actor_user_id = ? AND target_user_id = ?",
    )
    .bind(super_admin.id)
    .bind(target.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 1);
}

#[tokio::test]
async fn users_can_share_and_manage_public_messages() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("messages.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let user = auth::create_user(
        &pool,
        "message_user",
        "留言用户",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let mut config = test_config(&temporary, database_url);
    config.messages.limit_per_user = 1;
    config.messages.home_preview_limit = 1;
    let router = app::build(pool.clone(), config);
    let (anonymous_cookie, login_csrf) = page_session(&router, "/login").await;
    let login_body = format!(
        "csrf_token={login_csrf}&username={}&password=correct+horse+battery",
        user.username
    );
    let login_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::COOKIE, anonymous_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .extension(ConnectInfo(
                    "127.0.0.1:43102".parse::<SocketAddr>().unwrap(),
                ))
                .body(Body::from(login_body))
                .unwrap(),
        )
        .await
        .unwrap();
    let authenticated_cookie = response_cookie(&login_response);
    let (_, csrf) = page_session_with_cookie(&router, "/messages", &authenticated_cookie).await;
    let message_body = format!("csrf_token={csrf}&body=大家好%F0%9F%90%B7");

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/messages")
                .header(header::COOKIE, authenticated_cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(message_body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);

    let limited_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/messages")
                .header(header::COOKIE, authenticated_cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(message_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(limited_response.status(), StatusCode::BAD_REQUEST);

    let messages_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/messages")
                .header(header::COOKIE, authenticated_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let html = response_html(messages_response).await;
    assert!(html.contains("大家好🐷"));
    assert!(html.contains("/u/message_user"));

    let home_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::COOKIE, authenticated_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let home_html = response_html(home_response).await;
    assert!(home_html.contains("公共留言板"));
    assert!(home_html.contains("最多展示 1 条最近留言"));
    assert!(home_html.contains("大家好🐷"));

    let message_id =
        sqlx::query_scalar::<_, String>("SELECT id FROM public_messages WHERE author_user_id = ?")
            .bind(&user.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let delete_body = format!("csrf_token={csrf}");
    let delete_response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/messages/{message_id}/delete"))
                .header(header::COOKIE, authenticated_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(delete_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::SEE_OTHER);
    let deleted_at = sqlx::query_scalar::<_, Option<String>>(
        "SELECT deleted_at FROM public_messages WHERE id = ?",
    )
    .bind(message_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(deleted_at.is_some());
}

fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}

fn test_config(temporary: &TempDir, database_url: String) -> Config {
    Config {
        host: "127.0.0.1".to_owned(),
        port: 6324,
        database_url,
        avatar_dir: temporary.path().join("avatars"),
        cookie_secure: false,
        messages: MessageConfig {
            retention_days: 5,
            limit_per_user: 5,
            max_length: 300,
            page_size: 30,
            home_preview_limit: 5,
            cleanup_interval_hours: 6,
        },
    }
}

fn between<'a>(input: &'a str, prefix: &str, suffix: &str) -> &'a str {
    let remainder = input.split_once(prefix).unwrap().1;
    remainder.split_once(suffix).unwrap().0
}

async fn page_session(router: &axum::Router, uri: &str) -> (String, String) {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let cookie = response_cookie(&response);
    let html = response_html(response).await;
    let csrf = between(&html, "name=\"csrf_token\" value=\"", "\"").to_owned();
    (cookie, csrf)
}

async fn page_session_with_cookie(
    router: &axum::Router,
    uri: &str,
    cookie: &str,
) -> (String, String) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = response_html(response).await;
    let csrf = between(&html, "name=\"csrf_token\" value=\"", "\"").to_owned();
    (cookie.to_owned(), csrf)
}

fn response_cookie(response: &axum::response::Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

async fn response_html(response: axum::response::Response) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}
