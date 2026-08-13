use std::path::Path;

use demo0::{
    auth,
    config::{
        CheckInConfig, Config, CurrencyConfig, DisplayConfig, MemeConfig, MessageConfig,
        NovelConfig, ShopConfig, UpdateConfig,
    },
    model::Role,
    shop::store,
    updates::UpdateEntry,
};
use tempfile::TempDir;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const CREATED_AT: &str = "2026-08-13T12:00:00Z";

fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}

#[tokio::test]
async fn shop_migration_creates_order_voucher_and_audit_tables() {
    let temporary = tempfile::tempdir().unwrap();
    let pool = demo0::db::connect(&sqlite_url(&temporary.path().join("shop.db")))
        .await
        .unwrap();

    for table in ["shop_orders", "redemption_vouchers", "voucher_audit_logs"] {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1, "missing table {table}");
    }
}

#[tokio::test]
async fn store_user_voucher_page_does_not_include_another_users_order() {
    let temporary = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&temporary.path().join("store-page.db"));
    let pool = demo0::db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    create_test_icon(&config.shop.icon_dir);
    let owner = auth::create_user(
        &pool,
        "voucher_owner",
        "兑换凭证拥有者",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let other = auth::create_user(
        &pool,
        "voucher_other",
        "另一位用户",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();

    insert_test_voucher(
        &pool,
        &owner.id,
        "01K2H7V9W4RRDMC0P9A8C5M001",
        "01K2H7V9W4RRDMC0P9A8C5V001",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "owner-product",
        None,
    )
    .await;
    insert_test_voucher(
        &pool,
        &other.id,
        "01K2H7V9W4RRDMC0P9A8C5M002",
        "01K2H7V9W4RRDMC0P9A8C5V002",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "other-product",
        None,
    )
    .await;

    let page = store::list_user_vouchers(&pool, &owner.id, 1, config.shop.voucher_page_size)
        .await
        .unwrap();

    assert_eq!(page.current_page, 1);
    assert_eq!(page.total_pages, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].order.product_id, "owner-product");
    assert_eq!(page.items[0].order.user_id, owner.id);
}

#[tokio::test]
async fn store_hash_lookup_returns_the_purchased_product_snapshot() {
    let temporary = tempfile::tempdir().unwrap();
    let pool = demo0::db::connect(&sqlite_url(&temporary.path().join("store-hash.db")))
        .await
        .unwrap();
    let owner = auth::create_user(
        &pool,
        "snapshot_owner",
        "快照拥有者",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let token_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    insert_test_voucher(
        &pool,
        &owner.id,
        "01K2H7V9W4RRDMC0P9A8C5M003",
        "01K2H7V9W4RRDMC0P9A8C5V003",
        token_hash,
        "snapshot-product",
        None,
    )
    .await;

    let found = store::find_voucher_by_hash(&pool, token_hash)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(found.order.product_id, "snapshot-product");
    assert_eq!(found.order.product_name, "商品快照");
    assert_eq!(found.order.product_description, "购买时保存的商品说明");
    assert_eq!(
        found.voucher.token_mask,
        "ZV1-****-****-****-****-****-****-****-TEST"
    );
}

#[tokio::test]
async fn store_effective_status_expires_at_the_bound_timestamp() {
    let temporary = tempfile::tempdir().unwrap();
    let pool = demo0::db::connect(&sqlite_url(&temporary.path().join("store-expiry.db")))
        .await
        .unwrap();
    let owner = auth::create_user(
        &pool,
        "expiry_owner",
        "过期拥有者",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    let token_hash = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    let expires_at = "2026-08-14T12:00:00Z";

    insert_test_voucher(
        &pool,
        &owner.id,
        "01K2H7V9W4RRDMC0P9A8C5M004",
        "01K2H7V9W4RRDMC0P9A8C5V004",
        token_hash,
        "expiring-product",
        Some(expires_at),
    )
    .await;

    let found = store::find_voucher_by_hash(&pool, token_hash)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        found.effective_status(timestamp("2026-08-14T11:59:59Z")),
        store::EffectiveVoucherStatus::Active
    );
    assert_eq!(
        found.effective_status(timestamp(expires_at)),
        store::EffectiveVoucherStatus::Expired
    );
    assert_eq!(
        store::count_active_for_user_product(
            &pool,
            &owner.id,
            "expiring-product",
            "2026-08-14T11:59:59Z",
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(
        store::count_active_for_user_product(&pool, &owner.id, "expiring-product", expires_at)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        store::active_counts_for_user(&pool, &owner.id, expires_at)
            .await
            .unwrap(),
        std::collections::HashMap::new()
    );
}

async fn insert_test_voucher(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    order_id: &str,
    voucher_id: &str,
    token_hash: &str,
    product_id: &str,
    expires_at: Option<&str>,
) {
    let mut transaction = pool.begin().await.unwrap();
    let order = store::NewOrder {
        id: order_id.to_owned(),
        user_id: user_id.to_owned(),
        product_id: product_id.to_owned(),
        product_name: "商品快照".to_owned(),
        product_description: "购买时保存的商品说明".to_owned(),
        icon_file: "token.png".to_owned(),
        fulfillment_type: "redemption_token".to_owned(),
        price_paid: 10,
        valid_days: expires_at.map(|_| 1),
        purchase_key: order_id.to_owned(),
        created_at: CREATED_AT.to_owned(),
    };
    store::insert_order(&mut transaction, &order).await.unwrap();
    let voucher = store::NewVoucher {
        id: voucher_id.to_owned(),
        order_id: order_id.to_owned(),
        token_hash: token_hash.to_owned(),
        token_mask: "ZV1-****-****-****-****-****-****-****-TEST".to_owned(),
        expires_at: expires_at.map(str::to_owned),
        created_at: CREATED_AT.to_owned(),
    };
    store::insert_voucher(&mut transaction, &voucher)
        .await
        .unwrap();
    store::insert_audit(
        &mut transaction,
        voucher_id,
        "created",
        Some(user_id),
        "",
        CREATED_AT,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

fn timestamp(value: &str) -> OffsetDateTime {
    OffsetDateTime::parse(value, &Rfc3339).unwrap()
}

fn create_test_icon(icon_dir: &Path) {
    std::fs::create_dir_all(icon_dir).unwrap();
    image::DynamicImage::new_rgba8(2, 2)
        .save(icon_dir.join("token.png"))
        .unwrap();
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
            profile_page_size: 12,
            home_preview_limit: 6,
            popular_tag_limit: 10,
            max_tags_per_meme: 5,
            max_tag_length: 20,
            max_title_length: 60,
            approval_reward_enabled: true,
            approval_reward_amount: 2,
        },
        novels: NovelConfig {
            home_preview_limit: 5,
            chapter_max_upload_bytes: 256 * 1024,
            max_title_length: 60,
            max_chapter_title_length: 80,
            chapter_comment_max_length: 300,
            chapter_comment_page_size: 50,
        },
        updates: UpdateConfig {
            file: temporary.path().join("updates.toml"),
            home_preview_limit: 3,
            entries: Vec::<UpdateEntry>::new(),
        },
        currency: CurrencyConfig {
            name: "洲币".to_owned(),
            symbol: "🪙".to_owned(),
            log_page_size: 30,
            admin_recent_log_limit: 10,
            max_admin_adjust_amount: 99_999,
            admin_user_search_limit: 20,
            max_note_length: 200,
        },
        check_in: CheckInConfig {
            enabled: true,
            reward_amount: 1,
        },
        shop: ShopConfig {
            enabled: true,
            products_file: temporary.path().join("shop.toml"),
            icon_dir: temporary.path().join("shop-icons"),
            page_size: 12,
            voucher_page_size: 20,
            admin_note_max_length: 200,
            token_lookup_max_attempts: 20,
            token_lookup_window_seconds: 60,
            icon_max_bytes: 256 * 1024,
            icon_max_dimension: 1024,
            products: Vec::new(),
        },
    }
}
