use std::{net::SocketAddr, path::Path};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use demo0::{
    app, auth,
    config::{Config, DisplayConfig, MemeConfig, MessageConfig},
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
    let raw_created_at =
        sqlx::query_scalar::<_, String>("SELECT created_at FROM public_messages WHERE id = ?")
            .bind(&message_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!html.contains(&raw_created_at));
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

#[tokio::test]
async fn memes_require_review_before_public_listing() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("memes.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let user = auth::create_user(
        &pool,
        "meme_user",
        "梗图用户",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let admin = auth::create_user(
        &pool,
        "meme_admin",
        "梗图管理员",
        "correct horse battery",
        Role::Admin,
    )
    .await
    .unwrap();
    let router = app::build(pool.clone(), test_config(&temporary, database_url));

    let user_cookie = sign_in(&router, &user.username, "127.0.0.1:43103").await;
    let (_, csrf) = page_session_with_cookie(&router, "/profile", &user_cookie).await;
    let image_bytes = tiny_png();
    let body = multipart_body(
        &csrf,
        &[("title", "Rust 小猪"), ("tags", "rust, 🐷, rust")],
        "meme",
        "pig.png",
        "image/png",
        &image_bytes,
    );

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/memes")
                .header(header::COOKIE, user_cookie.clone())
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={}", MULTIPART_BOUNDARY),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);

    let meme_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM memes WHERE author_user_id = ? AND status = 'pending'",
    )
    .bind(&user.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let deduped_tag_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meme_tags")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(deduped_tag_count, 2);

    let public_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/memes?tag=rust")
                .header(header::COOKIE, user_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(public_response.status(), StatusCode::OK);
    let public_html = response_html(public_response).await;
    assert!(!public_html.contains("Rust 小猪"));

    let user_review_body = format!("csrf_token={csrf}");
    let forbidden_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/memes/{meme_id}/approve"))
                .header(header::COOKIE, user_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(user_review_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_response.status(), StatusCode::FORBIDDEN);

    let admin_cookie = sign_in(&router, &admin.username, "127.0.0.1:43104").await;
    let (_, admin_csrf) = page_session_with_cookie(&router, "/admin/memes", &admin_cookie).await;
    let approve_body = format!("csrf_token={admin_csrf}");
    let approve_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/memes/{meme_id}/approve"))
                .header(header::COOKIE, admin_cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(approve_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::SEE_OTHER);

    let admin_page_after_approval = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/memes")
                .header(header::COOKIE, admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let admin_html_after_approval = response_html(admin_page_after_approval).await;
    assert!(!admin_html_after_approval.contains("Rust 小猪"));
    assert!(admin_html_after_approval.contains("暂无需要处理的 Meme"));

    let approved_response = router
        .oneshot(
            Request::builder()
                .uri("/memes?tag=rust")
                .header(header::COOKIE, admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let approved_html = response_html(approved_response).await;
    assert!(approved_html.contains("Rust 小猪"));
    assert!(approved_html.contains("梗图用户"));
    assert!(approved_html.contains("🐷"));
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
        display: DisplayConfig {
            utc_offset_hours: 8,
        },
        messages: MessageConfig {
            retention_days: 5,
            limit_per_user: 5,
            max_length: 300,
            page_size: 30,
            home_preview_limit: 5,
            cleanup_interval_hours: 6,
        },
        memes: MemeConfig {
            dir: temporary.path().join("memes"),
            max_upload_bytes: 5 * 1024 * 1024,
            max_dimension: 3000,
            max_gif_frames: 120,
            page_size: 20,
            home_preview_limit: 6,
            max_tags_per_meme: 5,
            max_tag_length: 20,
            max_title_length: 60,
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

async fn sign_in(router: &axum::Router, username: &str, address: &str) -> String {
    let (anonymous_cookie, login_csrf) = page_session(router, "/login").await;
    let login_body =
        format!("csrf_token={login_csrf}&username={username}&password=correct+horse+battery");
    let login_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::COOKIE, anonymous_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .extension(ConnectInfo(address.parse::<SocketAddr>().unwrap()))
                .body(Body::from(login_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login_response.status(), StatusCode::SEE_OTHER);
    response_cookie(&login_response)
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

const MULTIPART_BOUNDARY: &str = "demo0-test-boundary";

fn multipart_body(
    csrf: &str,
    fields: &[(&str, &str)],
    file_field: &str,
    file_name: &str,
    media_type: &str,
    file_bytes: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    push_text_part(&mut body, "csrf_token", csrf);
    for (name, value) in fields {
        push_text_part(&mut body, name, value);
    }
    body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{file_field}\"; filename=\"{file_name}\"\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {media_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(format!("\r\n--{MULTIPART_BOUNDARY}--\r\n").as_bytes());
    body
}

fn push_text_part(body: &mut Vec<u8>, name: &str, value: &str) {
    body.extend_from_slice(format!("--{MULTIPART_BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

fn tiny_png() -> Vec<u8> {
    use image::{ImageFormat, Rgba, RgbaImage};

    let mut image = RgbaImage::new(2, 2);
    image.put_pixel(0, 0, Rgba([255, 0, 128, 255]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, ImageFormat::Png)
        .expect("测试图片应能编码为 PNG");
    encoded.into_inner()
}
