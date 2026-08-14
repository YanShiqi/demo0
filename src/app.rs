use std::{sync::Arc, time::Duration};

use axum::{Router, extract::DefaultBodyLimit, middleware, routing::get};
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

use crate::{
    avatar::MAX_UPLOAD_BYTES,
    config::Config,
    rate_limit::{AttemptLimiter, LoginLimiter},
    web,
};

// multipart 字段与边界会占用额外请求体空间，避免合法文件在解析前被拦截。
const MULTIPART_FORM_OVERHEAD_BYTES: usize = 128 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Arc<Config>,
    pub login_limiter: LoginLimiter,
    pub voucher_lookup_limiter: AttemptLimiter,
}

pub fn build(pool: SqlitePool, config: Config) -> Router {
    let body_limit = config
        .memes
        .max_upload_bytes
        .max(MAX_UPLOAD_BYTES)
        .max(config.novels.chapter_max_upload_bytes)
        .max(config.shop.icon_upload_max_bytes)
        + MULTIPART_FORM_OVERHEAD_BYTES;
    let voucher_lookup_limiter = AttemptLimiter::new(
        Duration::from_secs(config.shop.token_lookup_window_seconds),
        config.shop.token_lookup_max_attempts,
    );
    let state = AppState {
        pool,
        config: Arc::new(config),
        login_limiter: LoginLimiter::default(),
        voucher_lookup_limiter,
    };

    Router::new()
        .route("/", get(web::home))
        .route("/check-in", axum::routing::post(web::check_in))
        .route("/updates", get(web::updates_page))
        .route("/register", get(web::register_page).post(web::register))
        .route("/login", get(web::login_page).post(web::login))
        .route("/logout", axum::routing::post(web::logout))
        .route(
            "/password/change-required",
            get(web::change_password_required_page).post(web::change_required_password),
        )
        .route(
            "/messages",
            get(web::messages_page).post(web::create_message),
        )
        .route("/currency", get(web::currency_page))
        .route("/shop", get(web::shop_page))
        .route(
            "/shop/products/{product_id}/purchase",
            axum::routing::post(web::purchase_product),
        )
        .route("/vouchers", get(web::voucher_list))
        .route("/admin/vouchers", get(web::admin_vouchers))
        .route(
            "/admin/shop/products",
            get(web::admin_shop_products).post(web::create_admin_shop_product),
        )
        .route("/admin/shop/products/new", get(web::admin_shop_product_new))
        .route(
            "/admin/shop/products/{id}/edit",
            get(web::admin_shop_product_edit),
        )
        .route(
            "/admin/shop/products/{id}",
            axum::routing::post(web::update_admin_shop_product),
        )
        .route(
            "/admin/shop/products/{id}/enable",
            axum::routing::post(web::enable_admin_shop_product),
        )
        .route(
            "/admin/shop/products/{id}/disable",
            axum::routing::post(web::disable_admin_shop_product),
        )
        .route(
            "/admin/shop/products/{id}/delete",
            axum::routing::post(web::delete_admin_shop_product),
        )
        .route(
            "/admin/vouchers/lookup",
            axum::routing::post(web::lookup_voucher),
        )
        .route(
            "/admin/vouchers/{id}/redeem",
            axum::routing::post(web::redeem_voucher),
        )
        .route(
            "/admin/vouchers/{id}/cancel",
            axum::routing::post(web::cancel_voucher),
        )
        .route(
            "/static/shop/products/{file_name}",
            get(web::shop_product_icon),
        )
        .route(
            "/messages/{id}/delete",
            axum::routing::post(web::delete_message),
        )
        .route("/memes", get(web::memes_page).post(web::create_meme))
        .route("/memes/new", get(web::new_meme_page))
        .route("/memes/{id}", get(web::meme_detail_page))
        .route("/memes/{id}/image", get(web::meme_image))
        .route("/memes/{id}/download", get(web::meme_download))
        .route(
            "/memes/{id}/delete",
            axum::routing::post(web::delete_own_meme),
        )
        .route("/novels", get(web::novels_page))
        .route("/novels/{id}", get(web::novel_detail_page))
        .route(
            "/novels/{novel_id}/chapters/{chapter_id}",
            get(web::novel_chapter_page),
        )
        .route(
            "/novels/{novel_id}/chapters/{chapter_id}/comments",
            axum::routing::post(web::create_novel_chapter_comment),
        )
        .route("/admin/memes", get(web::admin_memes))
        .route("/admin/currency", get(web::admin_currency))
        .route(
            "/admin/currency/grant",
            axum::routing::post(web::grant_currency),
        )
        .route(
            "/admin/currency/deduct",
            axum::routing::post(web::deduct_currency),
        )
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
        .route(
            "/admin/users/{id}/password-reset",
            axum::routing::post(web::reset_user_password),
        )
        .route(
            "/admin/novels",
            get(web::admin_novels).post(web::create_novel),
        )
        .route(
            "/admin/novels/{id}/delete",
            axum::routing::post(web::delete_novel),
        )
        .route(
            "/admin/novels/{id}/chapters",
            axum::routing::post(web::create_novel_chapter),
        )
        .route(
            "/admin/novels/{novel_id}/chapters/{chapter_id}/delete",
            axum::routing::post(web::delete_novel_chapter),
        )
        .route(
            "/admin/novels/comments/{comment_id}/delete",
            axum::routing::post(web::delete_novel_chapter_comment),
        )
        .route("/static/app.css", get(web::app_css))
        .fallback(web::not_found)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            web::enforce_required_password_change,
        ))
        .layer(DefaultBodyLimit::max(body_limit))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
