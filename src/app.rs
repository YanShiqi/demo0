use std::sync::Arc;

use axum::{Router, extract::DefaultBodyLimit, routing::get};
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

use crate::{avatar::MAX_UPLOAD_BYTES, config::Config, rate_limit::LoginLimiter, web};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Arc<Config>,
    pub login_limiter: LoginLimiter,
}

pub fn build(pool: SqlitePool, config: Config) -> Router {
    let body_limit = config.memes.max_upload_bytes.max(MAX_UPLOAD_BYTES) + 128 * 1024;
    let state = AppState {
        pool,
        config: Arc::new(config),
        login_limiter: LoginLimiter::default(),
    };

    Router::new()
        .route("/", get(web::home))
        .route("/register", get(web::register_page).post(web::register))
        .route("/login", get(web::login_page).post(web::login))
        .route("/logout", axum::routing::post(web::logout))
        .route(
            "/messages",
            get(web::messages_page).post(web::create_message),
        )
        .route(
            "/messages/{id}/delete",
            axum::routing::post(web::delete_message),
        )
        .route("/memes", get(web::memes_page).post(web::create_meme))
        .route("/memes/new", get(web::new_meme_page))
        .route("/memes/{id}/image", get(web::meme_image))
        .route("/admin/memes", get(web::admin_memes))
        .route(
            "/admin/memes/{id}/approve",
            axum::routing::post(web::approve_meme),
        )
        .route(
            "/admin/memes/{id}/delete",
            axum::routing::post(web::delete_meme),
        )
        .route("/profile", get(web::profile_page))
        .route(
            "/profile/nickname",
            axum::routing::post(web::update_nickname),
        )
        .route("/profile/bio", axum::routing::post(web::update_bio))
        .route("/profile/avatar", axum::routing::post(web::update_avatar))
        .route("/u/{username}", get(web::public_profile))
        .route("/users/{id}/avatar", get(web::user_avatar))
        .route("/admin/users", get(web::admin_users))
        .route(
            "/admin/users/{id}/role",
            axum::routing::post(web::update_role),
        )
        .route("/static/app.css", get(web::app_css))
        .fallback(web::not_found)
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
