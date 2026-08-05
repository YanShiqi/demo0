use std::{net::SocketAddr, path::Path};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{HeaderValue, Request, StatusCode, header},
};
use demo0::{
    app, auth,
    config::{Config, DisplayConfig, MemeConfig, MessageConfig, NovelConfig},
    db,
    error::AppError,
    memes::{self, NewMeme},
    model::{Role, User},
    novels, public_messages,
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
async fn super_admin_can_reset_password_and_force_user_to_change_it() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("password_reset.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let super_admin = auth::create_user(
        &pool,
        "reset_root",
        "重置站长",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();
    let target = auth::create_user(
        &pool,
        "bob",
        "忘记密码的人",
        "old correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let config = test_config(&temporary, database_url);
    let router = app::build(pool.clone(), config);
    let super_cookie = sign_in(&router, &super_admin.username, "127.0.0.1:43120").await;
    let (_, admin_csrf) = page_session_with_cookie(&router, "/admin/users", &super_cookie).await;

    let reset_body = format!("csrf_token={admin_csrf}");
    let reset_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/users/{}/password-reset", target.id))
                .header(header::COOKIE, super_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(reset_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reset_response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        reset_response.headers().get(header::LOCATION).unwrap(),
        "/admin/users?password_reset=bob"
    );

    let must_change =
        sqlx::query_scalar::<_, bool>("SELECT must_change_password FROM users WHERE id = ?")
            .bind(&target.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(must_change);
    let audit_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM password_reset_audit_logs WHERE actor_user_id = ? AND target_user_id = ?",
    )
    .bind(&super_admin.id)
    .bind(&target.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 1);

    let old_password_response = login_with_password(
        &router,
        &target.username,
        "old correct horse battery",
        "127.0.0.1:43121",
    )
    .await;
    assert_eq!(old_password_response.status(), StatusCode::UNAUTHORIZED);

    let temporary_login_response = login_with_password(
        &router,
        &target.username,
        &target.username,
        "127.0.0.1:43122",
    )
    .await;
    assert_eq!(temporary_login_response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        temporary_login_response
            .headers()
            .get(header::LOCATION)
            .unwrap(),
        "/password/change-required"
    );
    let temporary_cookie = response_cookie(&temporary_login_response);

    let profile_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/profile")
                .header(header::COOKIE, temporary_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(profile_response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        profile_response.headers().get(header::LOCATION).unwrap(),
        "/password/change-required"
    );

    let (_, change_csrf) =
        page_session_with_cookie(&router, "/password/change-required", &temporary_cookie).await;
    let change_body = format!(
        "csrf_token={change_csrf}&password=new+correct+horse+battery&password_confirmation=new+correct+horse+battery"
    );
    let change_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/password/change-required")
                .header(header::COOKIE, temporary_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(change_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(change_response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        change_response.headers().get(header::LOCATION).unwrap(),
        "/profile"
    );

    let must_change =
        sqlx::query_scalar::<_, bool>("SELECT must_change_password FROM users WHERE id = ?")
            .bind(&target.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!must_change);
    let temporary_password_response = login_with_password(
        &router,
        &target.username,
        &target.username,
        "127.0.0.1:43123",
    )
    .await;
    assert_eq!(
        temporary_password_response.status(),
        StatusCode::UNAUTHORIZED
    );
    let new_password_response = login_with_password(
        &router,
        &target.username,
        "new correct horse battery",
        "127.0.0.1:43124",
    )
    .await;
    assert_eq!(new_password_response.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn password_reset_requires_super_admin_and_rejects_super_admin_targets() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("password_reset_permissions.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let super_admin = auth::create_user(
        &pool,
        "reset_owner",
        "重置负责人",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();
    let other_super_admin = auth::create_user(
        &pool,
        "other_root",
        "另一个站长",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();
    let admin = auth::create_user(
        &pool,
        "reset_admin",
        "重置管理员",
        "correct horse battery",
        Role::Admin,
    )
    .await
    .unwrap();
    let target = auth::create_user(
        &pool,
        "reset_target",
        "重置目标",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let config = test_config(&temporary, database_url);
    let router = app::build(pool.clone(), config);

    let admin_cookie = sign_in(&router, &admin.username, "127.0.0.1:43125").await;
    let (_, admin_csrf) = page_session_with_cookie(&router, "/profile", &admin_cookie).await;
    let admin_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/users/{}/password-reset", target.id))
                .header(header::COOKIE, admin_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("csrf_token={admin_csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_response.status(), StatusCode::FORBIDDEN);

    let super_cookie = sign_in(&router, &super_admin.username, "127.0.0.1:43126").await;
    let (_, super_csrf) = page_session_with_cookie(&router, "/admin/users", &super_cookie).await;
    let super_target_response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/admin/users/{}/password-reset",
                    other_super_admin.id
                ))
                .header(header::COOKIE, super_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("csrf_token={super_csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(super_target_response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn home_uses_server_tabs_for_messages_and_memes() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("home_tabs.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let user = auth::create_user(
        &pool,
        "home_tab_user",
        "首页页签用户",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let admin = auth::create_user(
        &pool,
        "home_tab_admin",
        "首页页签管理员",
        "correct horse battery",
        Role::Admin,
    )
    .await
    .unwrap();
    let config = test_config(&temporary, database_url);
    public_messages::create(&pool, &user.id, "首页留言内容", &config.messages)
        .await
        .unwrap();
    approved_meme(&pool, &user, &admin, "home-tab.png", "首页 Meme 标题", 30).await;
    let router = app::build(pool, config);
    let cookie = sign_in(&router, &user.username, "127.0.0.1:43127").await;

    let default_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::COOKIE, cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(default_response.status(), StatusCode::OK);
    let default_html = response_html(default_response).await;
    assert!(default_html.contains("首页动态"));
    assert!(default_html.contains("aria-current=\"page\">留言板"));
    assert!(default_html.contains("href=\"/?tab=memes\""));
    assert!(default_html.contains("首页留言内容"));
    assert!(!default_html.contains("首页 Meme 标题"));

    let memes_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/?tab=memes")
                .header(header::COOKIE, cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(memes_response.status(), StatusCode::OK);
    let memes_html = response_html(memes_response).await;
    assert!(memes_html.contains("aria-current=\"page\">Memes"));
    assert!(memes_html.contains("href=\"/?tab=messages\""));
    assert!(memes_html.contains("首页 Meme 标题"));
    assert!(!memes_html.contains("首页留言内容"));

    let fallback_response = router
        .oneshot(
            Request::builder()
                .uri("/?tab=unknown")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fallback_response.status(), StatusCode::OK);
    let fallback_html = response_html(fallback_response).await;
    assert!(fallback_html.contains("aria-current=\"page\">留言板"));
    assert!(fallback_html.contains("首页留言内容"));
    assert!(!fallback_html.contains("首页 Meme 标题"));
}

#[tokio::test]
async fn super_admin_can_publish_serialized_novel_chapters() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("novels.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let super_admin = auth::create_user(
        &pool,
        "novel_root",
        "小说站长",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();
    let reader = auth::create_user(
        &pool,
        "novel_reader",
        "小说读者",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let config = test_config(&temporary, database_url);
    let router = app::build(pool.clone(), config);
    let admin_cookie = sign_in(&router, &super_admin.username, "127.0.0.1:43128").await;
    let (_, admin_csrf) = page_session_with_cookie(&router, "/admin/novels", &admin_cookie).await;

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/novels")
                .header(header::COOKIE, admin_cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "csrf_token={admin_csrf}&title=%E9%9B%AA%E4%B8%AD%E5%B0%8F%E7%8C%AA"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::SEE_OTHER);
    let novel_id = sqlx::query_scalar::<_, String>("SELECT id FROM novels WHERE title = ?")
        .bind("雪中小猪")
        .fetch_one(&pool)
        .await
        .unwrap();

    let chapter_markdown = "# 开头\n\n这是一段**正文**。\n\n<script>alert(1)</script>";
    let chapter_body = multipart_body(
        &admin_csrf,
        &[("title", "风起")],
        "chapter",
        "chapter.md",
        "text/markdown",
        chapter_markdown.as_bytes(),
    );
    let upload_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/novels/{novel_id}/chapters"))
                .header(header::COOKIE, admin_cookie.clone())
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
                )
                .body(Body::from(chapter_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(upload_response.status(), StatusCode::SEE_OTHER);
    let chapter_id =
        sqlx::query_scalar::<_, String>("SELECT id FROM novel_chapters WHERE novel_id = ?")
            .bind(&novel_id)
            .fetch_one(&pool)
            .await
            .unwrap();

    let reader_cookie = sign_in(&router, &reader.username, "127.0.0.1:43129").await;
    let home_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/?tab=novels")
                .header(header::COOKIE, reader_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(home_response.status(), StatusCode::OK);
    let home_html = response_html(home_response).await;
    assert!(home_html.contains("aria-current=\"page\">连载小说"));
    assert!(home_html.contains("雪中小猪"));
    assert!(home_html.contains("1. 风起"));
    assert!(!home_html.contains("第 1 章 风起"));

    let list_html = response_html(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/novels")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(list_html.contains("雪中小猪"));
    assert!(list_html.contains(">风起</a>"));
    assert!(!list_html.contains(">1. 风起</a>"));
    assert!(!list_html.contains("第 1 章 风起"));

    let detail_html = response_html(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/novels/{novel_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(detail_html.contains(">风起</a>"));
    assert!(!detail_html.contains(">1. 风起</a>"));
    assert!(!detail_html.contains("第 1 章 风起"));

    let admin_list_html = response_html(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/novels")
                    .header(header::COOKIE, admin_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(admin_list_html.contains(">风起</a>"));
    assert!(!admin_list_html.contains(">1. 风起</a>"));

    let chapter_html = response_html(
        router
            .oneshot(
                Request::builder()
                    .uri(format!("/novels/{novel_id}/chapters/{chapter_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(chapter_html.contains("<strong>正文</strong>"));
    assert!(chapter_html.contains(">1.<"));
    assert!(chapter_html.contains("<h1>风起</h1>"));
    assert!(!chapter_html.contains("CHAPTER 1"));
    assert!(!chapter_html.contains("<script"));
    assert!(!chapter_html.contains("alert(1)"));
}

#[tokio::test]
async fn only_super_admin_can_manage_novels() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("novel-permissions.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let admin = auth::create_user(
        &pool,
        "novel_admin",
        "小说管理员",
        "correct horse battery",
        Role::Admin,
    )
    .await
    .unwrap();
    let config = test_config(&temporary, database_url);
    let router = app::build(pool, config);
    let admin_cookie = sign_in(&router, &admin.username, "127.0.0.1:43130").await;
    let (_, csrf) = page_session_with_cookie(&router, "/profile", &admin_cookie).await;

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/novels")
                .header(header::COOKIE, admin_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "csrf_token={csrf}&title=%E6%99%AE%E9%80%9A%E7%AE%A1%E7%90%86%E5%91%98%E7%9A%84%E5%B0%8F%E8%AF%B4"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn novel_chapter_pages_show_previous_and_next_navigation() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("novel-navigation.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let novel_id = novels::create_novel(&pool, "章节导航小说", &config.novels)
        .await
        .unwrap();
    let first_id = novels::create_chapter(&pool, &novel_id, "序章", "序章正文", &config.novels)
        .await
        .unwrap();
    let middle_id =
        novels::create_chapter(&pool, &novel_id, "盛大登场", "中间正文", &config.novels)
            .await
            .unwrap();
    let deleted_id = novels::create_chapter(
        &pool,
        &novel_id,
        "会被隐藏的一章",
        "隐藏正文",
        &config.novels,
    )
    .await
    .unwrap();
    novels::soft_delete_chapter(&pool, &novel_id, &deleted_id)
        .await
        .unwrap();
    let last_id = novels::create_chapter(&pool, &novel_id, "余波", "余波正文", &config.novels)
        .await
        .unwrap();
    let router = app::build(pool, config);

    let middle_html = response_html(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/novels/{novel_id}/chapters/{middle_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(middle_html.contains("← 上一章：序章"));
    assert!(middle_html.contains(&format!("/novels/{novel_id}/chapters/{first_id}")));
    assert!(middle_html.contains("下一章：余波 →"));
    assert!(middle_html.contains(&format!("/novels/{novel_id}/chapters/{last_id}")));
    assert!(!middle_html.contains("会被隐藏的一章"));

    let first_html = response_html(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/novels/{novel_id}/chapters/{first_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(!first_html.contains("上一章："));
    assert!(first_html.contains("下一章：盛大登场 →"));

    let last_html = response_html(
        router
            .oneshot(
                Request::builder()
                    .uri(format!("/novels/{novel_id}/chapters/{last_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(last_html.contains("← 上一章：盛大登场"));
    assert!(!last_html.contains("下一章："));
}

#[tokio::test]
async fn super_admin_can_soft_delete_novels_and_chapters() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("novel-deletions.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let super_admin = auth::create_user(
        &pool,
        "novel_delete_root",
        "小说删除站长",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();
    let config = test_config(&temporary, database_url);
    let novel_id = novels::create_novel(&pool, "会被删除的小说", &config.novels)
        .await
        .unwrap();
    let chapter_id = novels::create_chapter(
        &pool,
        &novel_id,
        "会被删除的章节",
        "这章会被删除",
        &config.novels,
    )
    .await
    .unwrap();
    let router = app::build(pool, config);
    let cookie = sign_in(&router, &super_admin.username, "127.0.0.1:43131").await;
    let (_, csrf) = page_session_with_cookie(&router, "/admin/novels", &cookie).await;

    let delete_chapter_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/admin/novels/{novel_id}/chapters/{chapter_id}/delete"
                ))
                .header(header::COOKIE, cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("csrf_token={csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_chapter_response.status(), StatusCode::SEE_OTHER);

    let deleted_chapter_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/novels/{novel_id}/chapters/{chapter_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deleted_chapter_response.status(), StatusCode::NOT_FOUND);

    let delete_novel_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/novels/{novel_id}/delete"))
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("csrf_token={csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_novel_response.status(), StatusCode::SEE_OTHER);

    let deleted_novel_response = router
        .oneshot(
            Request::builder()
                .uri(format!("/novels/{novel_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deleted_novel_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn logged_in_users_can_leave_anonymous_novel_chapter_comments() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("novel-comments.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let reader = auth::create_user(
        &pool,
        "chapter_reader",
        "真实读者昵称",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let admin = auth::create_user(
        &pool,
        "chapter_comment_admin",
        "评论管理员",
        "correct horse battery",
        Role::Admin,
    )
    .await
    .unwrap();
    let config = test_config(&temporary, database_url);
    let novel_id = novels::create_novel(&pool, "可评论的小说", &config.novels)
        .await
        .unwrap();
    let chapter_id = novels::create_chapter(
        &pool,
        &novel_id,
        "可以评论的一章",
        "正文内容",
        &config.novels,
    )
    .await
    .unwrap();
    let router = app::build(pool.clone(), config);
    let chapter_path = format!("/novels/{novel_id}/chapters/{chapter_id}");
    let reader_cookie = sign_in(&router, &reader.username, "127.0.0.1:43132").await;
    let (_, reader_csrf) = page_session_with_cookie(&router, &chapter_path, &reader_cookie).await;

    let comment_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{chapter_path}/comments"))
                .header(header::COOKIE, reader_cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "csrf_token={reader_csrf}&body=%E8%BF%99%E7%AB%A0%E5%BE%88%E6%9C%89%E8%B6%A3+%F0%9F%90%B7%3Cscript%3Ealert(1)%3C%2Fscript%3E"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(comment_response.status(), StatusCode::SEE_OTHER);

    let reader_html = response_html(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&chapter_path)
                    .header(header::COOKIE, reader_cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(reader_html.contains("匿名读者"));
    assert!(reader_html.contains("这章很有趣"));
    assert!(reader_html.contains("🐷"));
    assert!(reader_html.contains("&#60;script&#62;alert(1)&#60;/script&#62;"));
    assert!(!reader_html.contains("<script>alert(1)</script>"));
    assert!(!reader_html.contains("/u/chapter_reader"));
    assert!(!reader_html.contains("/admin/novels/comments/"));

    let anonymous_html = response_html(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&chapter_path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(anonymous_html.contains("匿名读者"));
    assert!(anonymous_html.contains("登录后可以匿名评论"));

    let admin_cookie = sign_in(&router, &admin.username, "127.0.0.1:43133").await;
    let (_, admin_csrf) = page_session_with_cookie(&router, &chapter_path, &admin_cookie).await;
    let admin_html = response_html(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&chapter_path)
                    .header(header::COOKIE, admin_cookie.clone())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(admin_html.contains("/admin/novels/comments/"));
    let comment_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM novel_chapter_comments WHERE chapter_id = ?",
    )
    .bind(&chapter_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let delete_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/novels/comments/{comment_id}/delete"))
                .header(header::COOKIE, admin_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "csrf_token={admin_csrf}&return_to={chapter_path}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        delete_response.headers().get(header::LOCATION).unwrap(),
        &HeaderValue::from_str(&chapter_path).unwrap()
    );

    let after_delete_html = response_html(
        router
            .oneshot(
                Request::builder()
                    .uri(&chapter_path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert!(!after_delete_html.contains("这章很有趣"));
}

#[tokio::test]
async fn anonymous_visitors_cannot_comment_on_novel_chapters() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("novel-comments-auth.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let novel_id = novels::create_novel(&pool, "游客不能评论的小说", &config.novels)
        .await
        .unwrap();
    let chapter_id = novels::create_chapter(
        &pool,
        &novel_id,
        "游客不能评论的一章",
        "正文内容",
        &config.novels,
    )
    .await
    .unwrap();
    let router = app::build(pool.clone(), config);
    let chapter_path = format!("/novels/{novel_id}/chapters/{chapter_id}");
    let (anonymous_cookie, csrf) = page_session(&router, "/login").await;

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{chapter_path}/comments"))
                .header(header::COOKIE, anonymous_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("csrf_token={csrf}&body=visitor")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let comment_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM novel_chapter_comments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(comment_count, 0);
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
    let admin_meme_id = memes::create(
        &pool,
        &admin,
        NewMeme {
            storage_name: "adminmeme.png".to_owned(),
            media_type: "image/png".to_owned(),
            title: "管理员自己的 Meme".to_owned(),
            tags: vec!["ops".to_owned()],
        },
    )
    .await
    .unwrap();
    memes::approve(&pool, &admin_meme_id, &admin).await.unwrap();

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
        .clone()
        .oneshot(
            Request::builder()
                .uri("/memes?tag=rust")
                .header(header::COOKIE, admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let approved_html = response_html(approved_response).await;
    assert!(approved_html.contains("Rust 小猪"));
    assert!(approved_html.contains("梗图用户"));
    assert!(approved_html.contains("🐷"));

    let approved_admin_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/memes?status=approved")
                .header(header::COOKIE, admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(approved_admin_response.status(), StatusCode::OK);
    let approved_admin_html = response_html(approved_admin_response).await;
    assert!(approved_admin_html.contains("Rust 小猪"));
    assert!(approved_admin_html.contains("管理员自己的 Meme"));
    assert!(approved_admin_html.contains("梗图用户"));
    assert!(approved_admin_html.contains("已通过"));

    let username_search_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/memes?status=approved&q=meme_user")
                .header(header::COOKIE, admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(username_search_response.status(), StatusCode::OK);
    let username_search_html = response_html(username_search_response).await;
    assert!(username_search_html.contains("Rust 小猪"));
    assert!(!username_search_html.contains("管理员自己的 Meme"));

    let tag_search_response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/memes?status=approved&q=rust")
                .header(header::COOKIE, admin_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tag_search_response.status(), StatusCode::OK);
    let tag_search_html = response_html(tag_search_response).await;
    assert!(tag_search_html.contains("Rust 小猪"));
    assert!(!tag_search_html.contains("管理员自己的 Meme"));
    assert!(
        tag_search_html
            .contains("name=\"return_to\" value=\"/admin/memes?status=approved&#38;q=rust\"")
    );

    let admin_delete_body = format!(
        "csrf_token={admin_csrf}&return_to=%2Fadmin%2Fmemes%3Fstatus%3Dapproved%26q%3Drust"
    );
    let delete_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/admin/memes/{meme_id}/delete"))
                .header(header::COOKIE, admin_cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(admin_delete_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        delete_response.headers().get(header::LOCATION).unwrap(),
        "/admin/memes?status=approved&q=rust"
    );

    let deleted_status = sqlx::query_scalar::<_, String>("SELECT status FROM memes WHERE id = ?")
        .bind(&meme_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(deleted_status, "deleted");

    let deleted_public_response = router
        .oneshot(
            Request::builder()
                .uri("/memes?tag=rust")
                .header(header::COOKIE, admin_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let deleted_public_html = response_html(deleted_public_response).await;
    assert!(!deleted_public_html.contains("Rust 小猪"));
}

#[tokio::test]
async fn authors_can_delete_their_own_memes_but_not_someone_elses() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("meme-owner-delete.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let owner = auth::create_user(
        &pool,
        "meme_owner",
        "Meme 作者",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let other_user = auth::create_user(
        &pool,
        "other_meme_user",
        "另一位用户",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let meme_id = memes::create(
        &pool,
        &owner,
        NewMeme {
            storage_name: "ownermeme.png".to_owned(),
            media_type: "image/png".to_owned(),
            title: "只属于作者的 Meme".to_owned(),
            tags: vec!["测试".to_owned()],
        },
    )
    .await
    .unwrap();
    let config = test_config(&temporary, database_url);
    tokio::fs::create_dir_all(&config.memes.dir).await.unwrap();
    tokio::fs::write(config.memes.dir.join("ownermeme.png"), tiny_png())
        .await
        .unwrap();
    let router = app::build(pool.clone(), config);

    let owner_cookie = sign_in(&router, &owner.username, "127.0.0.1:43105").await;
    let (_, owner_csrf) = page_session_with_cookie(&router, "/profile", &owner_cookie).await;
    let owner_profile = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/profile")
                .header(header::COOKIE, owner_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response_html(owner_profile)
            .await
            .contains("只属于作者的 Meme")
    );
    let own_pending_image = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/memes/{meme_id}/image"))
                .header(header::COOKIE, owner_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(own_pending_image.status(), StatusCode::OK);

    let other_cookie = sign_in(&router, &other_user.username, "127.0.0.1:43106").await;
    let (_, other_csrf) = page_session_with_cookie(&router, "/profile", &other_cookie).await;
    let other_delete_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/memes/{meme_id}/delete"))
                .header(header::COOKIE, other_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("csrf_token={other_csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(other_delete_response.status(), StatusCode::NOT_FOUND);

    let owner_delete_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/memes/{meme_id}/delete"))
                .header(header::COOKIE, owner_cookie.clone())
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("csrf_token={owner_csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(owner_delete_response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        owner_delete_response
            .headers()
            .get(header::LOCATION)
            .unwrap(),
        "/profile?updated=meme"
    );

    let status = sqlx::query_scalar::<_, String>("SELECT status FROM memes WHERE id = ?")
        .bind(&meme_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "deleted");

    let deleted_profile = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/profile")
                .header(header::COOKIE, owner_cookie.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        !response_html(deleted_profile)
            .await
            .contains("只属于作者的 Meme")
    );
    let deleted_image = router
        .oneshot(
            Request::builder()
                .uri(format!("/memes/{meme_id}/image"))
                .header(header::COOKIE, owner_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deleted_image.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn meme_wall_uses_numbered_pagination_with_previous_and_next_links() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("meme-numbered-pages.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let author = auth::create_user(
        &pool,
        "page_meme_author",
        "分页作者",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let admin = auth::create_user(
        &pool,
        "page_meme_admin",
        "分页管理员",
        "correct horse battery",
        Role::Admin,
    )
    .await
    .unwrap();
    let newest = approved_meme(&pool, &author, &admin, "newest.png", "最新 Meme", 30).await;
    let middle = approved_meme(&pool, &author, &admin, "middle.png", "中间 Meme", 20).await;
    let oldest = approved_meme(&pool, &author, &admin, "oldest.png", "最早 Meme", 10).await;
    assert_ne!(newest, middle);
    assert_ne!(middle, oldest);
    let mut config = test_config(&temporary, database_url);
    config.memes.page_size = 2;
    let router = app::build(pool, config);

    let first_page = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/memes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_html = response_html(first_page).await;
    assert!(first_html.contains("最新 Meme"));
    assert!(first_html.contains("中间 Meme"));
    assert!(!first_html.contains("最早 Meme"));
    assert!(first_html.contains("第 1 页"));
    assert!(first_html.contains("href=\"/memes?page=2\""));
    assert!(!first_html.contains("加载更多"));

    let second_page = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/memes?page=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second_page.status(), StatusCode::OK);
    let second_html = response_html(second_page).await;
    assert!(!second_html.contains("最新 Meme"));
    assert!(!second_html.contains("中间 Meme"));
    assert!(second_html.contains("最早 Meme"));
    assert!(second_html.contains("第 2 页"));
    assert!(second_html.contains("href=\"/memes?page=1\""));

    let tagged_second_page = router
        .oneshot(
            Request::builder()
                .uri("/memes?tag=page&page=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tagged_second_page.status(), StatusCode::OK);
    let tagged_second_html = response_html(tagged_second_page).await;
    assert!(tagged_second_html.contains("最早 Meme"));
    assert!(tagged_second_html.contains("href=\"/memes?tag=page&amp;page=1\""));
}

#[tokio::test]
async fn meme_wall_shows_popular_approved_tags_as_filter_links() {
    let temporary = TempDir::new().unwrap();
    let database_path = temporary.path().join("meme-popular-tags.db");
    let database_url = sqlite_url(&database_path);
    let pool = db::connect(&database_url).await.unwrap();
    let author = auth::create_user(
        &pool,
        "tag_meme_author",
        "标签作者",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let admin = auth::create_user(
        &pool,
        "tag_meme_admin",
        "标签管理员",
        "correct horse battery",
        Role::Admin,
    )
    .await
    .unwrap();
    approved_meme_with_tags(
        &pool,
        &author,
        &admin,
        "rust_one.png",
        "Rust 标签一",
        30,
        &["rust", "搞笑"],
    )
    .await;
    approved_meme_with_tags(
        &pool,
        &author,
        &admin,
        "rust_two.png",
        "Rust 标签二",
        20,
        &["rust", "🐷"],
    )
    .await;
    let hidden_pending_id = memes::create(
        &pool,
        &author,
        NewMeme {
            storage_name: "pendingtag.png".to_owned(),
            media_type: "image/png".to_owned(),
            title: "待审核标签不应统计".to_owned(),
            tags: vec!["待审核".to_owned()],
        },
    )
    .await
    .unwrap();
    assert!(!hidden_pending_id.is_empty());
    let mut config = test_config(&temporary, database_url);
    config.memes.popular_tag_limit = 2;
    let router = app::build(pool, config);

    let response = router
        .oneshot(
            Request::builder()
                .uri("/memes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let html = response_html(response).await;
    assert!(html.contains("热门标签"));
    assert!(html.contains("href=\"/memes?tag=rust\""));
    assert!(html.contains("#rust"));
    assert!(html.contains("2"));
    assert!(html.contains("href=\"/memes?tag="));
    assert!(!html.contains("#待审核"));
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
            max_upload_bytes: 3 * 1024 * 1024,
            max_dimension: 3000,
            max_gif_frames: 120,
            max_decoded_pixels: 50_000_000,
            page_size: 20,
            home_preview_limit: 6,
            popular_tag_limit: 10,
            max_tags_per_meme: 5,
            max_tag_length: 20,
            max_title_length: 60,
        },
        novels: NovelConfig {
            home_preview_limit: 5,
            chapter_max_upload_bytes: 256 * 1024,
            max_title_length: 60,
            max_chapter_title_length: 80,
            chapter_comment_max_length: 300,
            chapter_comment_page_size: 50,
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

async fn login_with_password(
    router: &axum::Router,
    username: &str,
    password: &str,
    address: &str,
) -> axum::response::Response {
    let (anonymous_cookie, login_csrf) = page_session(router, "/login").await;
    let body = format!(
        "csrf_token={login_csrf}&username={username}&password={}",
        password.replace(' ', "+")
    );
    router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::COOKIE, anonymous_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .extension(ConnectInfo(address.parse::<SocketAddr>().unwrap()))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
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

async fn approved_meme(
    pool: &sqlx::SqlitePool,
    author: &User,
    admin: &User,
    storage_name: &str,
    title: &str,
    created_at_epoch: i64,
) -> String {
    approved_meme_with_tags(
        pool,
        author,
        admin,
        storage_name,
        title,
        created_at_epoch,
        &["page"],
    )
    .await
}

async fn approved_meme_with_tags(
    pool: &sqlx::SqlitePool,
    author: &User,
    admin: &User,
    storage_name: &str,
    title: &str,
    created_at_epoch: i64,
    tags: &[&str],
) -> String {
    let meme_id = memes::create(
        pool,
        author,
        NewMeme {
            storage_name: storage_name.to_owned(),
            media_type: "image/png".to_owned(),
            title: title.to_owned(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE memes SET created_at_epoch = ? WHERE id = ?")
        .bind(created_at_epoch)
        .bind(&meme_id)
        .execute(pool)
        .await
        .unwrap();
    memes::approve(pool, &meme_id, admin).await.unwrap();
    meme_id
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
