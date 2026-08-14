use std::{net::SocketAddr, path::Path};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};

use demo0::{
    auth,
    config::{
        CheckInConfig, Config, CurrencyConfig, DisplayConfig, MemeConfig, MessageConfig,
        NovelConfig, ShopConfig, UpdateConfig,
    },
    model::Role,
    shop::{self, store},
    updates::UpdateEntry,
};
use http_body_util::BodyExt;
use tempfile::TempDir;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tower::ServiceExt;

const CREATED_AT: &str = "2026-08-13T12:00:00Z";
const SHOP_PRODUCT_BOUNDARY: &str = "demo0-product-boundary";

fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}

#[test]
fn shop_test_config_provides_runtime_icon_processing_limits() {
    let temporary = tempfile::tempdir().unwrap();
    let config = test_config(&temporary, sqlite_url(&temporary.path().join("config.db")));

    assert_eq!(config.shop.icon_upload_max_bytes, 5 * 1024 * 1024);
    assert_eq!(config.shop.icon_input_max_dimension, 4096);
    assert_eq!(config.shop.icon_max_gif_frames, 120);
    assert_eq!(config.shop.icon_max_decoded_pixels, 80_000_000);
    assert_eq!(config.shop.icon_max_stored_bytes, 1024 * 1024);
    assert_eq!(config.shop.icon_resize_dimensions, vec![512, 384, 256]);
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
async fn shop_product_persistence_enforces_required_database_constraints() {
    let temporary = tempfile::tempdir().unwrap();
    let pool = demo0::db::connect(&sqlite_url(&temporary.path().join("products.db")))
        .await
        .unwrap();
    let actor = auth::create_user(
        &pool,
        "product_actor",
        "商品操作者",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();

    let product = test_database_product("milk_tea");
    store::insert_product(&pool, &product, &actor.id, CREATED_AT)
        .await
        .unwrap();
    let saved = store::find_product(&pool, "milk_tea")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.sold_count, 0);
    assert_eq!(saved.total_limit, None);
    assert!(saved.enabled);
    assert_eq!(saved.icon_storage_name, "01K2H7V9W4RRDMC0P9A8C5I001.webp");

    let duplicate_id = insert_raw_product(
        &pool,
        &actor.id,
        RawProduct {
            id: "milk_tea",
            price: 10,
            valid_days: Some(30),
            max_active_per_user: 1,
            total_limit: None,
            sold_count: 0,
        },
    )
    .await;
    assert!(duplicate_id.is_err());
    for (id, price, valid_days, max_active_per_user, total_limit, sold_count) in [
        ("invalid_price", 0, Some(30), 1, None, 0),
        ("invalid_valid_days", 10, Some(0), 1, None, 0),
        ("invalid_personal_limit", 10, Some(30), 0, None, 0),
        ("invalid_total_limit", 10, Some(30), 1, Some(0), 0),
        ("invalid_sold_count", 10, Some(30), 1, None, -1),
        ("invalid_limit_below_sold", 10, Some(30), 1, Some(1), 2),
    ] {
        let result = insert_raw_product(
            &pool,
            &actor.id,
            RawProduct {
                id,
                price,
                valid_days,
                max_active_per_user,
                total_limit,
                sold_count,
            },
        )
        .await;
        assert!(result.is_err(), "database accepted invalid product {id}");
    }
}

#[tokio::test]
async fn shop_product_persistence_rejects_unsafe_or_mismatched_icon_metadata() {
    let product = test_database_product("icon_validation");

    for (storage_name, media_type) in [
        (".", "image/webp"),
        ("..", "image/webp"),
        ("01K2H7V9W4RRDMC0P9A8C5I001.gif", "image/webp"),
        ("01K2H7V9W4RRDMC0P9A8C5I001.webp", "image/gif"),
    ] {
        let mut invalid = product.clone();
        invalid.icon_storage_name = storage_name.to_owned();
        invalid.icon_media_type = media_type.to_owned();
        assert!(
            shop::validate_product_values(&invalid, 0).is_err(),
            "accepted invalid icon metadata {storage_name} / {media_type}"
        );
    }
}

async fn insert_raw_product(
    pool: &sqlx::SqlitePool,
    actor_user_id: &str,
    values: RawProduct<'_>,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO shop_products (id, name, description, icon_storage_name, icon_media_type, price, valid_days, max_active_per_user, total_limit, sold_count, enabled, sort_order, created_by_user_id, updated_by_user_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(values.id)
    .bind("无效价格")
    .bind("数据库约束应拒绝非正价格。")
    .bind("01K2H7V9W4RRDMC0P9A8C5I002.webp")
    .bind("image/webp")
    .bind(values.price)
    .bind(values.valid_days)
    .bind(values.max_active_per_user)
    .bind(values.total_limit)
    .bind(values.sold_count)
    .bind(true)
    .bind(2_i64)
    .bind(actor_user_id)
    .bind(actor_user_id)
    .bind(CREATED_AT)
    .bind(CREATED_AT)
    .execute(pool)
    .await
}

struct RawProduct<'a> {
    id: &'a str,
    price: i64,
    valid_days: Option<i64>,
    max_active_per_user: i64,
    total_limit: Option<i64>,
    sold_count: i64,
}

#[tokio::test]
async fn shop_product_persistence_lists_updates_and_retains_deleted_audit_history() {
    let temporary = tempfile::tempdir().unwrap();
    let pool = demo0::db::connect(&sqlite_url(&temporary.path().join("product-history.db")))
        .await
        .unwrap();
    let actor = auth::create_user(
        &pool,
        "product_history_actor",
        "商品历史操作者",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();
    let product = test_database_product("gift_card");

    shop::validate_product_for_create(&pool, &product)
        .await
        .unwrap();
    store::insert_product(&pool, &product, &actor.id, CREATED_AT)
        .await
        .unwrap();
    store::insert_product_audit(
        &pool,
        &store::NewProductAudit {
            id: "01K2H7V9W4RRDMC0P9A8C5A001".to_owned(),
            product_id: "gift_card".to_owned(),
            action: store::PRODUCT_AUDIT_CREATED.to_owned(),
            actor_user_id: actor.id.clone(),
            before_snapshot: String::new(),
            after_snapshot: "{\"name\":\"礼品卡\"}".to_owned(),
            created_at: CREATED_AT.to_owned(),
        },
    )
    .await
    .unwrap();

    let mut updated = product.clone();
    updated.price = 20;
    updated.total_limit = Some(10);
    store::update_product(
        &pool,
        "gift_card",
        &updated,
        &actor.id,
        "2026-08-14T12:00:00Z",
    )
    .await
    .unwrap();
    store::set_product_enabled(&pool, "gift_card", false, &actor.id, "2026-08-14T12:00:00Z")
        .await
        .unwrap();
    let enabled = store::list_enabled_products(&pool).await.unwrap();
    assert!(enabled.is_empty());
    let admin_products = store::list_admin_products(&pool).await.unwrap();
    assert_eq!(admin_products.len(), 1);
    assert_eq!(admin_products[0].price, 20);
    assert_eq!(admin_products[0].total_limit, Some(10));
    assert!(!admin_products[0].enabled);

    store::insert_product_audit(
        &pool,
        &store::NewProductAudit {
            id: "01K2H7V9W4RRDMC0P9A8C5A002".to_owned(),
            product_id: "gift_card".to_owned(),
            action: store::PRODUCT_AUDIT_DELETED.to_owned(),
            actor_user_id: actor.id.clone(),
            before_snapshot: "{\"name\":\"礼品卡\"}".to_owned(),
            after_snapshot: String::new(),
            created_at: "2026-08-14T12:01:00Z".to_owned(),
        },
    )
    .await
    .unwrap();
    assert!(store::delete_product(&pool, "gift_card").await.unwrap());
    assert!(
        store::find_product(&pool, "gift_card")
            .await
            .unwrap()
            .is_none()
    );
    let audit_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM shop_product_audit_logs WHERE product_id = ?",
    )
    .bind("gift_card")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 2);

    let reuse_error = shop::validate_product_for_create(&pool, &product)
        .await
        .unwrap_err();
    assert!(reuse_error.to_string().contains("不可复用"));
}

#[tokio::test]
async fn transactional_product_delete_uses_affected_rows_and_preserves_icon_snapshot() {
    let temporary = tempfile::tempdir().unwrap();
    let pool = demo0::db::connect(&sqlite_url(&temporary.path().join("transaction-delete.db")))
        .await
        .unwrap();
    let actor = auth::create_user(
        &pool,
        "transaction_delete_actor",
        "事务删除操作者",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();
    let product = test_database_product("transaction_delete");
    store::insert_product(&pool, &product, &actor.id, CREATED_AT)
        .await
        .unwrap();

    let mut transaction = pool.begin().await.unwrap();
    let deleted = store::delete_product_in_transaction(&mut transaction, &product.id)
        .await
        .unwrap();
    assert!(deleted);
    transaction.commit().await.unwrap();
    assert!(
        store::find_product(&pool, &product.id)
            .await
            .unwrap()
            .is_none()
    );
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

#[tokio::test]
async fn purchase_creates_snapshot_debits_balance_and_keeps_plaintext_out_of_database() {
    let temporary = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&temporary.path().join("purchase-created.db"));
    let pool = demo0::db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let buyer = create_buyer_with_balance(&pool, "purchase_created", 100).await;
    let product = test_product("snapshot-token", 25, Some(7), 2);
    store::insert_product(&pool, &product, &buyer.id, CREATED_AT)
        .await
        .unwrap();
    let purchase_key = "01K2H7V9W4RRDMC0P9A8C5P001";
    let now = timestamp(CREATED_AT);

    let outcome = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        purchase_key,
        &config.currency,
        now,
    )
    .await
    .unwrap();
    let shop::PurchaseOutcome::Created(created) = outcome else {
        panic!("expected a newly created purchase");
    };

    assert!(created.plaintext_token.starts_with("ZV1-"));
    assert_eq!(created.expires_at.as_deref(), Some("2026-08-20T12:00:00Z"));
    assert_eq!(user_balance(&pool, &buyer.id).await, 75);
    let order = store::find_order_by_purchase_key(&pool, purchase_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(order.product_name, product.name);
    assert_eq!(order.product_description, product.description);
    assert_eq!(order.price_paid, product.price);
    let related_id =
        sqlx::query_scalar::<_, String>("SELECT related_id FROM currency_logs WHERE user_id = ?")
            .bind(&buyer.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(related_id, created.order_id);
    let leaked = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM shop_orders o
         JOIN redemption_vouchers v ON v.order_id = o.id
         JOIN voucher_audit_logs a ON a.voucher_id = v.id
         WHERE o.product_name = ? OR o.product_description = ? OR v.token_mask = ? OR a.note = ?",
    )
    .bind(&created.plaintext_token)
    .bind(&created.plaintext_token)
    .bind(&created.plaintext_token)
    .bind(&created.plaintext_token)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0);
}

#[tokio::test]
async fn purchase_reads_current_database_product_and_increments_sales_atomically() {
    let temporary = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&temporary.path().join("purchase-db-product.db"));
    let pool = demo0::db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let buyer = create_buyer_with_balance(&pool, "purchase_db_product", 100).await;
    let product = test_database_product("database-token");
    store::insert_product(&pool, &product, &buyer.id, CREATED_AT)
        .await
        .unwrap();

    let outcome = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        "01K2H7V9W4RRDMC0P9A8C5P101",
        &config.currency,
        timestamp(CREATED_AT),
    )
    .await
    .unwrap();
    assert!(matches!(outcome, shop::PurchaseOutcome::Created(_)));
    assert_eq!(user_balance(&pool, &buyer.id).await, 90);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT sold_count FROM shop_products WHERE id = ?")
            .bind(&product.id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    let order = store::find_order_by_purchase_key(&pool, "01K2H7V9W4RRDMC0P9A8C5P101")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(order.product_name, product.name);
    assert_eq!(order.icon_file, product.icon_storage_name);
}

#[tokio::test]
async fn purchase_rejects_sold_out_database_product_before_debiting_currency() {
    let temporary = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&temporary.path().join("purchase-sold-out.db"));
    let pool = demo0::db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let buyer = create_buyer_with_balance(&pool, "purchase_sold_out", 100).await;
    let mut product = test_database_product("sold-out-database-token");
    product.total_limit = Some(1);
    store::insert_product(&pool, &product, &buyer.id, CREATED_AT)
        .await
        .unwrap();

    let first = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        "01K2H7V9W4RRDMC0P9A8C5P102",
        &config.currency,
        timestamp(CREATED_AT),
    )
    .await
    .unwrap();
    assert!(matches!(first, shop::PurchaseOutcome::Created(_)));
    let second = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        "01K2H7V9W4RRDMC0P9A8C5P103",
        &config.currency,
        timestamp(CREATED_AT),
    )
    .await
    .unwrap_err();
    assert!(second.to_string().contains("售罄"));
    assert_eq!(user_balance(&pool, &buyer.id).await, 90);
    assert_eq!(table_count(&pool, "shop_orders").await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT sold_count FROM shop_products WHERE id = ?")
            .bind(&product.id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn product_sales_increment_is_unlimited_for_null_limit_and_rolls_back_with_transaction() {
    let temporary = tempfile::tempdir().unwrap();
    let pool = demo0::db::connect(&sqlite_url(&temporary.path().join("sales-update.db")))
        .await
        .unwrap();
    let actor = auth::create_user(
        &pool,
        "sales_update_actor",
        "销量更新操作者",
        "correct horse battery",
        Role::SuperAdmin,
    )
    .await
    .unwrap();
    let product = test_database_product("unlimited-sales");
    store::insert_product(&pool, &product, &actor.id, CREATED_AT)
        .await
        .unwrap();

    let mut transaction = pool.begin().await.unwrap();
    assert!(
        store::increment_product_sales_if_available(&mut transaction, &product.id)
            .await
            .unwrap()
    );
    transaction.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT sold_count FROM shop_products WHERE id = ?")
            .bind(&product.id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn purchase_returns_already_processed_for_duplicate_key_without_second_debit_or_token() {
    let temporary = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&temporary.path().join("purchase-idempotent.db"));
    let pool = demo0::db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let buyer = create_buyer_with_balance(&pool, "purchase_idempotent", 100).await;
    let product = test_product("idempotent-token", 25, None, 2);
    store::insert_product(&pool, &product, &buyer.id, CREATED_AT)
        .await
        .unwrap();
    let purchase_key = "01K2H7V9W4RRDMC0P9A8C5P002";

    let first = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        purchase_key,
        &config.currency,
        timestamp(CREATED_AT),
    )
    .await
    .unwrap();
    assert!(matches!(first, shop::PurchaseOutcome::Created(_)));
    let second = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        purchase_key,
        &config.currency,
        timestamp(CREATED_AT),
    )
    .await
    .unwrap();

    assert_eq!(second, shop::PurchaseOutcome::AlreadyProcessed);
    assert_eq!(user_balance(&pool, &buyer.id).await, 75);
    assert_eq!(table_count(&pool, "shop_orders").await, 1);
    assert_eq!(table_count(&pool, "redemption_vouchers").await, 1);
    assert_eq!(table_count(&pool, "currency_logs").await, 1);
}

#[tokio::test]
async fn purchase_rejects_active_limit_but_excludes_expired_vouchers() {
    let temporary = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&temporary.path().join("purchase-limit.db"));
    let pool = demo0::db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let buyer = create_buyer_with_balance(&pool, "purchase_limit", 100).await;
    let product = test_product("limited-token", 25, Some(1), 1);
    store::insert_product(&pool, &product, &buyer.id, CREATED_AT)
        .await
        .unwrap();
    let now = timestamp(CREATED_AT);

    let first = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        "01K2H7V9W4RRDMC0P9A8C5P003",
        &config.currency,
        now,
    )
    .await
    .unwrap();
    let shop::PurchaseOutcome::Created(first) = first else {
        panic!("expected first purchase to create a voucher");
    };
    let limited = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        "01K2H7V9W4RRDMC0P9A8C5P004",
        &config.currency,
        now,
    )
    .await
    .unwrap_err();
    assert!(limited.to_string().contains("持有数量"));
    assert_eq!(user_balance(&pool, &buyer.id).await, 75);

    sqlx::query("UPDATE redemption_vouchers SET expires_at = ? WHERE id = ?")
        .bind(CREATED_AT)
        .bind(&first.voucher_id)
        .execute(&pool)
        .await
        .unwrap();
    let after_expiry = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        "01K2H7V9W4RRDMC0P9A8C5P005",
        &config.currency,
        now,
    )
    .await;

    assert!(matches!(
        after_expiry,
        Ok(shop::PurchaseOutcome::Created(_))
    ));
    assert_eq!(user_balance(&pool, &buyer.id).await, 50);
}

#[tokio::test]
async fn purchase_rejects_insufficient_balance_without_creating_records() {
    let temporary = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&temporary.path().join("purchase-balance.db"));
    let pool = demo0::db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let buyer = create_buyer_with_balance(&pool, "purchase_balance", 24).await;
    let product = test_product("costly-token", 25, None, 1);
    store::insert_product(&pool, &product, &buyer.id, CREATED_AT)
        .await
        .unwrap();

    let error = shop::purchase(
        &pool,
        &buyer,
        &product.id,
        "01K2H7V9W4RRDMC0P9A8C5P006",
        &config.currency,
        timestamp(CREATED_AT),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("余额不足"));
    assert_eq!(user_balance(&pool, &buyer.id).await, 24);
    assert_eq!(table_count(&pool, "shop_orders").await, 0);
    assert_eq!(table_count(&pool, "redemption_vouchers").await, 0);
    assert_eq!(table_count(&pool, "currency_logs").await, 0);
}

#[tokio::test]
async fn purchase_rolls_back_order_and_debit_when_voucher_insert_fails() {
    let temporary = tempfile::tempdir().unwrap();
    let database_url = sqlite_url(&temporary.path().join("purchase-rollback.db"));
    let pool = demo0::db::connect(&database_url).await.unwrap();
    let config = test_config(&temporary, database_url);
    let buyer = create_buyer_with_balance(&pool, "purchase_rollback", 100).await;
    let product = test_product("rollback-token", 25, None, 1);
    store::insert_product(&pool, &product, &buyer.id, CREATED_AT)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER fail_voucher_insert BEFORE INSERT ON redemption_vouchers BEGIN SELECT RAISE(ABORT, 'forced voucher failure'); END",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert!(
        shop::purchase(
            &pool,
            &buyer,
            &product.id,
            "01K2H7V9W4RRDMC0P9A8C5P007",
            &config.currency,
            timestamp(CREATED_AT),
        )
        .await
        .is_err()
    );
    assert_eq!(user_balance(&pool, &buyer.id).await, 100);
    assert_eq!(table_count(&pool, "shop_orders").await, 0);
    assert_eq!(table_count(&pool, "redemption_vouchers").await, 0);
    assert_eq!(table_count(&pool, "currency_logs").await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT sold_count FROM shop_products WHERE id = ?")
            .bind(&product.id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
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

async fn create_buyer_with_balance(
    pool: &sqlx::SqlitePool,
    username: &str,
    balance: i64,
) -> demo0::model::User {
    let buyer = auth::create_user(
        pool,
        username,
        "购买者",
        "correct horse battery",
        Role::User,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE users SET currency_balance = ? WHERE id = ?")
        .bind(balance)
        .bind(&buyer.id)
        .execute(pool)
        .await
        .unwrap();
    buyer
}

fn test_product(
    id: &str,
    price: i64,
    valid_days: Option<i64>,
    max_active_per_user: i64,
) -> store::NewProduct {
    store::NewProduct {
        id: id.to_owned(),
        name: "兑换码商品".to_owned(),
        description: "购买时写入的商品快照".to_owned(),
        icon_storage_name: "01K2H7V9W4RRDMC0P9A8C5I001.webp".to_owned(),
        icon_media_type: "image/webp".to_owned(),
        price,
        valid_days,
        max_active_per_user,
        total_limit: None,
        sort_order: 1,
    }
}

async fn user_balance(pool: &sqlx::SqlitePool, user_id: &str) -> i64 {
    sqlx::query_scalar("SELECT currency_balance FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn table_count(pool: &sqlx::SqlitePool, table: &str) -> i64 {
    let statement = format!("SELECT COUNT(*) FROM {table}");
    sqlx::query_scalar(&statement)
        .fetch_one(pool)
        .await
        .unwrap()
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

fn test_database_product(id: &str) -> store::NewProduct {
    store::NewProduct {
        id: id.to_owned(),
        name: "礼品卡".to_owned(),
        description: "可用于线下兑换的礼品卡。".to_owned(),
        icon_storage_name: "01K2H7V9W4RRDMC0P9A8C5I001.webp".to_owned(),
        icon_media_type: "image/webp".to_owned(),
        price: 10,
        valid_days: Some(30),
        max_active_per_user: 2,
        total_limit: None,
        sort_order: 1,
    }
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
            icon_dir: temporary.path().join("shop-icons"),
            page_size: 12,
            voucher_page_size: 20,
            admin_note_max_length: 200,
            token_lookup_max_attempts: 20,
            token_lookup_window_seconds: 60,
            icon_upload_max_bytes: 5 * 1024 * 1024,
            icon_input_max_dimension: 4096,
            icon_max_gif_frames: 120,
            icon_max_decoded_pixels: 80_000_000,
            icon_max_stored_bytes: 1024 * 1024,
            icon_resize_dimensions: vec![512, 384, 256],
        },
    }
}

#[tokio::test]
async fn first_purchase_reveals_token_once_and_duplicate_request_does_not() {
    let fixture = ShopFixture::new().await;
    fixture.grant_buyer_currency(50).await;
    let (cookie, csrf) = fixture.sign_in_buyer().await;
    let purchase_key = ulid::Ulid::new().to_string();

    let first = fixture
        .purchase(&cookie, &csrf, "milk_tea", &purchase_key)
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()[header::CACHE_CONTROL], "no-store");
    let first_html = response_html(first).await;
    assert!(first_html.contains("请立即复制并妥善保存"));
    assert!(first_html.contains("data-token=\"ZV1-"));

    let duplicate = fixture
        .purchase(&cookie, &csrf, "milk_tea", &purchase_key)
        .await;
    assert_eq!(duplicate.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        duplicate.headers()[header::LOCATION],
        "/vouchers?purchase=already"
    );
    let list_html = fixture.get_html("/vouchers", &cookie).await;
    assert!(list_html.contains("ZV1-****-"));
    assert!(!list_html.contains("data-token=\"ZV1-"));
}

#[tokio::test]
async fn player_catalog_is_public_but_purchase_and_vouchers_require_login() {
    let fixture = ShopFixture::new().await;

    let catalog = fixture.get("/shop", None).await;
    assert_eq!(catalog.status(), StatusCode::OK);
    assert!(response_html(catalog).await.contains("奶茶兑换码"));

    let purchase = fixture
        .post(
            "/shop/products/milk_tea/purchase",
            None,
            "csrf_token=x&purchase_key=01K2H7V9W4RRDMC0P9A8C5P001",
        )
        .await;
    assert_eq!(purchase.status(), StatusCode::UNAUTHORIZED);
    let vouchers = fixture.get("/vouchers", None).await;
    assert_eq!(vouchers.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn player_catalog_respects_the_configured_page_size() {
    let fixture = ShopFixture::new().await;

    let first_page = response_html(fixture.get("/shop?page=1", None).await).await;
    assert!(first_page.contains("奶茶兑换码"));
    assert!(first_page.contains("礼品卡兑换码"));
    assert!(!first_page.contains("茶饮兑换码"));
    assert!(first_page.contains("第 1 / 2 页"));

    let second_page = response_html(fixture.get("/shop?page=2", None).await).await;
    assert!(second_page.contains("茶饮兑换码"));
    assert!(!second_page.contains("奶茶兑换码"));
    assert!(second_page.contains("第 2 / 2 页"));
}

#[tokio::test]
async fn player_catalog_marks_insufficient_balance_and_active_limit_as_disabled() {
    let fixture = ShopFixture::new().await;
    let (cookie, csrf) = fixture.sign_in_buyer().await;

    let insufficient = fixture.get_html("/shop", &cookie).await;
    assert!(insufficient.contains("余额不足"));

    fixture.grant_buyer_currency(50).await;
    let first = fixture
        .purchase(&cookie, &csrf, "milk_tea", &ulid::Ulid::new().to_string())
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let limited = fixture.get_html("/shop", &cookie).await;
    assert!(limited.contains("有效兑换码持有数量已达上限"));
}

#[tokio::test]
async fn player_rejects_unknown_or_disabled_products_and_serves_safe_icons() {
    let fixture = ShopFixture::new().await;
    fixture.grant_buyer_currency(50).await;
    let (cookie, csrf) = fixture.sign_in_buyer().await;

    for product_id in ["missing", "sold_out"] {
        let response = fixture
            .purchase(&cookie, &csrf, product_id, &ulid::Ulid::new().to_string())
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    let icon = fixture
        .get("/static/shop/products/milk-tea.png", None)
        .await;
    assert_eq!(icon.status(), StatusCode::OK);
    assert_eq!(icon.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(icon.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(
        icon.headers()[header::CACHE_CONTROL],
        "public, max-age=86400"
    );
    let unsafe_icon = fixture
        .get("/static/shop/products/../milk-tea.png", None)
        .await;
    assert_eq!(unsafe_icon.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn disabled_shop_hides_pages_but_keeps_historical_icons_available() {
    let fixture = ShopFixture::new_with_shop_enabled(false).await;

    assert_eq!(
        fixture.get("/shop", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture.get("/vouchers", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture
            .get("/static/shop/products/milk-tea.png", None)
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn shop_navigation_is_role_appropriate() {
    let fixture = ShopFixture::new().await;

    let anonymous_html = response_html(fixture.get("/shop", None).await).await;
    assert!(anonymous_html.contains("href=\"/shop\""));

    let buyer_cookie = fixture.sign_in_user(&fixture.buyer.username).await;
    let shop_html = fixture.get_html("/shop", &buyer_cookie).await;
    let profile_html = fixture.get_html("/profile", &buyer_cookie).await;
    assert!(shop_html.contains("href=\"/vouchers\""));
    assert!(shop_html.contains("我的兑换凭证"));
    assert!(profile_html.contains("href=\"/vouchers\""));
    assert!(profile_html.contains("我的兑换凭证"));

    let admin_cookie = fixture.sign_in_user(&fixture.admin.username).await;
    let admin_html = fixture.get_html("/shop", &admin_cookie).await;
    assert!(!admin_html.contains("href=\"/admin/vouchers\""));
    assert!(!admin_html.contains("Token 核销"));

    let super_cookie = fixture.sign_in_user(&fixture.super_admin.username).await;
    let super_admin_html = fixture.get_html("/shop", &super_cookie).await;
    assert!(super_admin_html.contains("href=\"/admin/vouchers\""));
    assert!(super_admin_html.contains("Token 核销"));
}

#[tokio::test]
async fn complete_token_appears_only_in_the_one_time_reveal_response() {
    let fixture = ShopFixture::new().await;
    fixture.grant_buyer_currency(50).await;
    let (buyer_cookie, csrf) = fixture.sign_in_buyer().await;
    let reveal_response = fixture
        .purchase(
            &buyer_cookie,
            &csrf,
            "milk_tea",
            &ulid::Ulid::new().to_string(),
        )
        .await;
    let reveal_html = response_html(reveal_response).await;
    let token = between(&reveal_html, "data-token=\"", "\"").to_owned();
    assert!(token.starts_with("ZV1-"));
    assert!(reveal_html.contains(&token));

    for path in ["/shop", "/profile", "/vouchers"] {
        let html = fixture.get_html(path, &buyer_cookie).await;
        assert!(!html.contains(&token), "complete Token leaked on {path}");
    }

    let super_cookie = fixture.sign_in_user(&fixture.super_admin.username).await;
    let admin_html = fixture.get_html("/admin/vouchers", &super_cookie).await;
    assert!(!admin_html.contains(&token));

    let admin_csrf = between(&admin_html, "name=\"csrf_token\" value=\"", "\"");
    let lookup_html = response_html(
        fixture
            .post(
                "/admin/vouchers/lookup",
                Some(&super_cookie),
                &format!("csrf_token={admin_csrf}&token={token}"),
            )
            .await,
    )
    .await;
    assert!(!lookup_html.contains(&token));
}

#[tokio::test]
async fn player_vouchers_are_paginated_and_isolated_by_user() {
    let fixture = ShopFixture::new().await;
    fixture.grant_buyer_currency(100).await;
    let (buyer_cookie, buyer_csrf) = fixture.sign_in_buyer().await;
    for _ in 0..3 {
        let response = fixture
            .purchase(
                &buyer_cookie,
                &buyer_csrf,
                "gift_card",
                &ulid::Ulid::new().to_string(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let first_page = fixture.get_html("/vouchers?page=1", &buyer_cookie).await;
    assert!(first_page.contains("第 1 / 2 页"));
    let second_page = fixture.get_html("/vouchers?page=2", &buyer_cookie).await;
    assert!(second_page.contains("第 2 / 2 页"));

    let admin_cookie = fixture.sign_in_user(&fixture.admin.username).await;
    let admin_page = fixture.get_html("/vouchers", &admin_cookie).await;
    assert!(!admin_page.contains("礼品卡兑换码"));
}

#[tokio::test]
async fn admin_voucher_routes_require_super_admin() {
    let fixture = ShopFixture::new().await;
    let admin_cookie = fixture.sign_in_user(&fixture.admin.username).await;

    assert_eq!(
        fixture
            .get("/admin/vouchers", Some(&admin_cookie))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        fixture
            .post(
                "/admin/vouchers/lookup",
                Some(&admin_cookie),
                "csrf_token=unused&token=ZV1-0000-0000-0000-0000-0000-0000-0000-0000",
            )
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn admin_shop_routes_require_super_admin_and_csrf() {
    let fixture = ShopFixture::new().await;
    assert_eq!(
        fixture.get("/admin/shop/products", None).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let buyer_cookie = fixture.sign_in_user(&fixture.buyer.username).await;
    assert_eq!(
        fixture
            .get("/admin/shop/products", Some(&buyer_cookie))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    let admin_cookie = fixture.sign_in_user(&fixture.admin.username).await;
    assert_eq!(
        fixture
            .get("/admin/shop/products/new", Some(&admin_cookie))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    let super_cookie = fixture.sign_in_user(&fixture.super_admin.username).await;
    let super_html = fixture
        .get_html("/admin/shop/products", &super_cookie)
        .await;
    assert!(super_html.contains("商品管理"));
    for path in [
        "/admin/shop/products/milk_tea/enable",
        "/admin/shop/products/milk_tea/disable",
        "/admin/shop/products/milk_tea/delete",
    ] {
        assert_eq!(
            fixture
                .post(path, Some(&super_cookie), "csrf_token=invalid")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn admin_shop_create_invalid_price_rerenders_form_with_submitted_values() {
    let fixture = ShopFixture::new().await;
    let super_cookie = fixture.sign_in_user(&fixture.super_admin.username).await;
    let form_html = fixture
        .get_html("/admin/shop/products/new", &super_cookie)
        .await;
    let csrf = between(&form_html, "name=\"csrf_token\" value=\"", "\"");
    let upload_dir = fixture.temporary.path().join("upload");
    create_shop_icon(&upload_dir, "icon.png");
    let icon_bytes = std::fs::read(upload_dir.join("icon.png")).unwrap();
    let body = shop_product_multipart_body(
        csrf,
        [
            ("id", "new-product"),
            ("name", "提交后的商品名"),
            ("description", "提交后的商品说明"),
            ("price", "-1"),
            ("valid_days", ""),
            ("max_active_per_user", "2"),
            ("total_limit", ""),
            ("sort_order", "1"),
        ],
        &icon_bytes,
    );
    let response = fixture
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/admin/shop/products")
                .header(header::COOKIE, &super_cookie)
                .header(
                    header::CONTENT_TYPE,
                    "multipart/form-data; boundary=demo0-product-boundary",
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let html = response_html(response).await;
    assert!(html.contains("商品价格必须大于 0"));
    assert!(html.contains("value=\"new-product\""));
    assert!(html.contains("value=\"提交后的商品名\""));
}

#[tokio::test]
async fn admin_voucher_lookup_normalizes_token_without_changing_status_or_echoing_it() {
    let fixture = ShopFixture::new().await;
    fixture.grant_buyer_currency(50).await;
    let (buyer_cookie, buyer_csrf) = fixture.sign_in_buyer().await;
    let purchase = fixture
        .purchase(
            &buyer_cookie,
            &buyer_csrf,
            "milk_tea",
            &ulid::Ulid::new().to_string(),
        )
        .await;
    let token = between(&response_html(purchase).await, "data-token=\"", "\"").to_owned();
    let super_cookie = fixture.sign_in_user(&fixture.super_admin.username).await;
    let csrf = fixture.admin_voucher_csrf(&super_cookie).await;
    let submitted = token.to_lowercase().replace('-', " ");

    let response = fixture
        .post(
            "/admin/vouchers/lookup",
            Some(&super_cookie),
            &format!("csrf_token={csrf}&token={}", submitted.replace(' ', "+")),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let html = response_html(response).await;
    assert!(html.contains("奶茶兑换码"));
    assert!(html.contains("原购买者：商城买家 @shop_buyer（不代表当前持有人）"));
    assert!(!html.contains(&token));
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT status FROM redemption_vouchers WHERE token_hash = ?",
        )
        .bind(shop::token::hash_normalized(
            &shop::token::normalize(&token).unwrap()
        ))
        .fetch_one(&fixture.pool)
        .await
        .unwrap(),
        store::STATUS_ACTIVE
    );
}

#[tokio::test]
async fn admin_voucher_redeem_requires_bounded_note_and_only_succeeds_once() {
    let fixture = ShopFixture::new().await;
    let voucher_id = fixture.create_admin_voucher(None).await;
    let super_cookie = fixture.sign_in_user(&fixture.super_admin.username).await;
    let csrf = fixture.admin_voucher_csrf(&super_cookie).await;

    for note in ["", &"x".repeat(201)] {
        let response = fixture
            .post(
                &format!("/admin/vouchers/{voucher_id}/redeem"),
                Some(&super_cookie),
                &format!("csrf_token={csrf}&note={note}"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    let redeemed = fixture
        .post(
            &format!("/admin/vouchers/{voucher_id}/redeem"),
            Some(&super_cookie),
            &format!("csrf_token={csrf}&note=已向顾客兑换"),
        )
        .await;
    assert_eq!(redeemed.status(), StatusCode::SEE_OTHER);
    let repeated = fixture
        .post(
            &format!("/admin/vouchers/{voucher_id}/redeem"),
            Some(&super_cookie),
            &format!("csrf_token={csrf}&note=再次尝试"),
        )
        .await;
    assert_eq!(repeated.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        voucher_status(&fixture.pool, &voucher_id).await,
        store::STATUS_REDEEMED
    );
    assert_eq!(
        audit_actor_and_note(&fixture.pool, &voucher_id, "redeemed").await,
        (fixture.super_admin.id.clone(), "已向顾客兑换".to_owned())
    );
}

#[tokio::test]
async fn admin_voucher_cancel_succeeds_once_and_expired_or_final_vouchers_reject_transitions() {
    let fixture = ShopFixture::new().await;
    let cancellable_id = fixture.create_admin_voucher(None).await;
    let expired_id = fixture.create_admin_voucher(Some(CREATED_AT)).await;
    let super_cookie = fixture.sign_in_user(&fixture.super_admin.username).await;
    let csrf = fixture.admin_voucher_csrf(&super_cookie).await;

    let cancelled = fixture
        .post(
            &format!("/admin/vouchers/{cancellable_id}/cancel"),
            Some(&super_cookie),
            &format!("csrf_token={csrf}&reason=客户退款"),
        )
        .await;
    assert_eq!(cancelled.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        voucher_status(&fixture.pool, &cancellable_id).await,
        store::STATUS_CANCELLED
    );
    assert_eq!(
        audit_actor_and_note(&fixture.pool, &cancellable_id, "cancelled").await,
        (fixture.super_admin.id.clone(), "客户退款".to_owned())
    );

    for (path, note) in [
        (
            format!("/admin/vouchers/{cancellable_id}/cancel"),
            "重复取消",
        ),
        (
            format!("/admin/vouchers/{cancellable_id}/redeem"),
            "已取消不可兑换",
        ),
        (
            format!("/admin/vouchers/{expired_id}/redeem"),
            "已过期不可兑换",
        ),
        (
            format!("/admin/vouchers/{expired_id}/cancel"),
            "已过期不可取消",
        ),
    ] {
        let field = if path.ends_with("/cancel") {
            "reason"
        } else {
            "note"
        };
        let response = fixture
            .post(
                &path,
                Some(&super_cookie),
                &format!("csrf_token={csrf}&{field}={note}"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(
        voucher_status(&fixture.pool, &expired_id).await,
        store::STATUS_ACTIVE
    );
}

#[tokio::test]
async fn admin_voucher_lookup_is_rate_limited_without_echoing_submitted_token() {
    let fixture = ShopFixture::new_with_lookup_limit(2).await;
    let super_cookie = fixture.sign_in_user(&fixture.super_admin.username).await;
    let csrf = fixture.admin_voucher_csrf(&super_cookie).await;
    let supplied_token = "ZV1-0000-0000-0000-0000-0000-0000-0000-0000";

    for _ in 0..2 {
        let response = fixture
            .post(
                "/admin/vouchers/lookup",
                Some(&super_cookie),
                &format!("csrf_token={csrf}&token={supplied_token}"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let limited = fixture
        .post(
            "/admin/vouchers/lookup",
            Some(&super_cookie),
            &format!("csrf_token={csrf}&token={supplied_token}"),
        )
        .await;
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = response_html(limited).await;
    assert!(body.contains("Token 查询过于频繁，请稍后再试"));
    assert!(!body.contains(supplied_token));
}

#[allow(dead_code)]
struct ShopFixture {
    temporary: TempDir,
    pool: sqlx::SqlitePool,
    router: axum::Router,
    buyer: demo0::model::User,
    admin: demo0::model::User,
    super_admin: demo0::model::User,
}

impl ShopFixture {
    async fn new() -> Self {
        Self::new_with_settings(true, 20).await
    }

    async fn new_with_shop_enabled(shop_enabled: bool) -> Self {
        Self::new_with_settings(shop_enabled, 20).await
    }

    async fn new_with_lookup_limit(max_attempts: usize) -> Self {
        Self::new_with_settings(true, max_attempts).await
    }

    async fn new_with_settings(shop_enabled: bool, token_lookup_max_attempts: usize) -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let database_url = sqlite_url(&temporary.path().join("shop-flow.db"));
        let pool = demo0::db::connect(&database_url).await.unwrap();
        let mut config = test_config(&temporary, database_url);
        create_shop_icon(&config.shop.icon_dir, "milk-tea.png");
        create_shop_icon(&config.shop.icon_dir, "gift-card.png");
        create_shop_icon(&config.shop.icon_dir, "tea-coupon.png");
        config.shop.page_size = 2;
        config.shop.voucher_page_size = 2;
        config.shop.enabled = shop_enabled;
        config.shop.token_lookup_max_attempts = token_lookup_max_attempts;
        let buyer = auth::create_user(
            &pool,
            "shop_buyer",
            "商城买家",
            "correct horse battery",
            Role::User,
        )
        .await
        .unwrap();
        let admin = auth::create_user(
            &pool,
            "shop_admin",
            "商城管理员",
            "correct horse battery",
            Role::Admin,
        )
        .await
        .unwrap();
        let super_admin = auth::create_user(
            &pool,
            "shop_super",
            "商城超管",
            "correct horse battery",
            Role::SuperAdmin,
        )
        .await
        .unwrap();
        for (
            id,
            name,
            description,
            icon_file,
            price,
            valid_days,
            max_active_per_user,
            enabled,
            sort_order,
        ) in [
            (
                "milk_tea",
                "奶茶兑换码",
                "一杯奶茶的兑换凭证",
                "milk-tea.png",
                50,
                Some(30),
                1,
                true,
                1,
            ),
            (
                "gift_card",
                "礼品卡兑换码",
                "可重复购买的礼品卡凭证",
                "gift-card.png",
                10,
                None,
                5,
                true,
                2,
            ),
            (
                "tea_coupon",
                "茶饮兑换码",
                "另一种可兑换的茶饮凭证",
                "tea-coupon.png",
                10,
                None,
                1,
                true,
                3,
            ),
            (
                "sold_out",
                "已下架商品",
                "不应允许购买",
                "gift-card.png",
                10,
                None,
                1,
                false,
                4,
            ),
        ] {
            sqlx::query(
                "INSERT INTO shop_products (id, name, description, icon_storage_name, icon_media_type, price, valid_days, max_active_per_user, total_limit, sold_count, enabled, sort_order, created_by_user_id, updated_by_user_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, 0, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(name)
            .bind(description)
            .bind(icon_file)
            .bind("image/png")
            .bind(price)
            .bind(valid_days)
            .bind(max_active_per_user)
            .bind(enabled)
            .bind(sort_order)
            .bind(&super_admin.id)
            .bind(&super_admin.id)
            .bind(CREATED_AT)
            .bind(CREATED_AT)
            .execute(&pool)
            .await
            .unwrap();
        }
        let router = demo0::app::build(pool.clone(), config);
        Self {
            temporary,
            pool,
            router,
            buyer,
            admin,
            super_admin,
        }
    }

    async fn grant_buyer_currency(&self, amount: i64) {
        sqlx::query("UPDATE users SET currency_balance = ? WHERE id = ?")
            .bind(amount)
            .bind(&self.buyer.id)
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn sign_in_buyer(&self) -> (String, String) {
        let cookie = self.sign_in_user(&self.buyer.username).await;
        let html = self.get_html("/shop", &cookie).await;
        (
            cookie,
            between(&html, "name=\"csrf_token\" value=\"", "\"").to_owned(),
        )
    }

    async fn admin_voucher_csrf(&self, cookie: &str) -> String {
        let html = self.get_html("/admin/vouchers", cookie).await;
        between(&html, "name=\"csrf_token\" value=\"", "\"").to_owned()
    }

    async fn create_admin_voucher(&self, expires_at: Option<&str>) -> String {
        let order_id = ulid::Ulid::new().to_string();
        let voucher_id = ulid::Ulid::new().to_string();
        let issued = shop::token::issue().unwrap();
        insert_test_voucher(
            &self.pool,
            &self.buyer.id,
            &order_id,
            &voucher_id,
            &issued.hash,
            "admin-voucher",
            expires_at,
        )
        .await;
        voucher_id
    }

    async fn sign_in_user(&self, username: &str) -> String {
        let session = self.get("/login", None).await;
        let cookie = response_cookie(&session);
        let csrf = between(
            &response_html(session).await,
            "name=\"csrf_token\" value=\"",
            "\"",
        )
        .to_owned();
        let response = self
            .post_with_connect_info(
                "/login",
                &cookie,
                &format!("csrf_token={csrf}&username={username}&password=correct+horse+battery"),
            )
            .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        response_cookie(&response)
    }

    async fn purchase(
        &self,
        cookie: &str,
        csrf: &str,
        product_id: &str,
        purchase_key: &str,
    ) -> axum::response::Response {
        self.post(
            &format!("/shop/products/{product_id}/purchase"),
            Some(cookie),
            &format!("csrf_token={csrf}&purchase_key={purchase_key}"),
        )
        .await
    }

    async fn get_html(&self, path: &str, cookie: &str) -> String {
        response_html(self.get(path, Some(cookie)).await).await
    }

    async fn get(&self, uri: &str, cookie: Option<&str>) -> axum::response::Response {
        let mut request = Request::builder().uri(uri);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn post(&self, uri: &str, cookie: Option<&str>, body: &str) -> axum::response::Response {
        let mut request = Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        self.router
            .clone()
            .oneshot(request.body(Body::from(body.to_owned())).unwrap())
            .await
            .unwrap()
    }

    async fn post_with_connect_info(
        &self,
        uri: &str,
        cookie: &str,
        body: &str,
    ) -> axum::response::Response {
        self.router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .extension(ConnectInfo(
                        "127.0.0.1:43199".parse::<SocketAddr>().unwrap(),
                    ))
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
}

async fn voucher_status(pool: &sqlx::SqlitePool, voucher_id: &str) -> String {
    sqlx::query_scalar("SELECT status FROM redemption_vouchers WHERE id = ?")
        .bind(voucher_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn audit_actor_and_note(
    pool: &sqlx::SqlitePool,
    voucher_id: &str,
    event_type: &str,
) -> (String, String) {
    sqlx::query_as(
        "SELECT actor_user_id, note FROM voucher_audit_logs WHERE voucher_id = ? AND event_type = ?",
    )
    .bind(voucher_id)
    .bind(event_type)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn create_shop_icon(icon_dir: &Path, name: &str) {
    std::fs::create_dir_all(icon_dir).unwrap();
    image::DynamicImage::new_rgba8(2, 2)
        .save(icon_dir.join(name))
        .unwrap();
}

fn shop_product_multipart_body<const N: usize>(
    csrf: &str,
    fields: [(&str, &str); N],
    icon_bytes: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    push_shop_product_text_part(&mut body, "csrf_token", csrf);
    for (name, value) in fields {
        push_shop_product_text_part(&mut body, name, value);
    }
    body.extend_from_slice(format!("--{SHOP_PRODUCT_BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"icon\"; filename=\"icon.png\"\r\nContent-Type: image/png\r\n\r\n",
    );
    body.extend_from_slice(icon_bytes);
    body.extend_from_slice(format!("\r\n--{SHOP_PRODUCT_BOUNDARY}--\r\n").as_bytes());
    body
}

fn push_shop_product_text_part(body: &mut Vec<u8>, name: &str, value: &str) {
    body.extend_from_slice(format!("--{SHOP_PRODUCT_BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
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

fn between<'a>(input: &'a str, prefix: &str, suffix: &str) -> &'a str {
    input
        .split_once(prefix)
        .unwrap()
        .1
        .split_once(suffix)
        .unwrap()
        .0
}
