# Shop Redemption Token Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a configurable shop where users spend integer currency to receive a transferable, one-time-visible redemption Token that only super administrators can query, redeem, or cancel.

**Architecture:** Load product definitions and validated local icons at startup, keep purchase/order/voucher state in portable SQL tables, and isolate Token generation from persistence and web handlers. A purchase service owns the database transaction that serializes purchases per user, checks active-voucher limits, spends currency, and creates the order, voucher, and audit event atomically; player and administrator routes live in a focused `src/web/shop.rs` module.

**Tech Stack:** Rust 2024, Axum 0.8, Askama 0.14, SQLx 0.8 with SQLite, TOML/Serde, `rand_core::OsRng`, SHA-256, `image` with PNG/JPEG/WebP support, HTML/CSS and minimal browser JavaScript.

## Global Constraints

- Token products are transferable bearer vouchers: possession of the complete Token, not the original buyer account, authorizes redemption.
- Complete Tokens appear only in the first successful purchase response; store only a SHA-256 hash and a masked display value.
- Generate 20 random bytes from the operating-system RNG and encode exactly 32 Crockford Base32 payload characters, displayed as `ZV1-` plus eight groups of four.
- Never place a complete Token in a URL, Cookie, trace/error log, currency note, audit note, or database column.
- Purchases cannot be cancelled or automatically refunded; lost, leaked, expired, and administrator-cancelled vouchers are not automatically reissued.
- Only super administrators may query, redeem, or cancel vouchers; redemption and cancellation notes are required.
- Use portable standard SQL in business migrations and queries; generate timestamps in Rust and bind them as parameters.
- All limits, paths, sizes, pagination values, and rate limits must come from named constants or configuration rather than scattered numeric literals.
- Add concise Chinese comments at key transaction, concurrency, security, and boundary logic; add a Chinese comment to every committed TOML and `.env.example` option.
- Product icons are required local PNG, WebP, or JPEG files under `static/shop/products/`; reject SVG, GIF, directories, absolute paths, external URLs, missing files, oversized files, and decoded dimensions above configuration.
- Respect `prefers-reduced-motion`: the one-time Token warning stays visually prominent but does not animate when reduced motion is requested.
- Do not implement inventory, in-site transfers, refunds, product administration, stock counts, multiple fulfillment types, or automatic real-world fulfillment in this version.

## File Structure

- Create `src/shop/mod.rs`: public shop service API and purchase/redeem/cancel transaction orchestration.
- Create `src/shop/catalog.rs`: product TOML parsing, product validation, icon validation, sorting, and catalog lookup.
- Create `src/shop/token.rs`: Token generation, normalization, hashing, and masking only.
- Create `src/shop/store.rs`: SQL row types and focused order/voucher/audit queries used by the service.
- Create `src/web/shop.rs`: shop, voucher, icon, purchase, lookup, redeem, and cancel HTTP handlers.
- Modify `src/web/views.rs`: Askama view models for the four new pages.
- Create `templates/shop.html`, `templates/voucher_reveal.html`, `templates/vouchers.html`, and `templates/admin_vouchers.html`.
- Create `migrations/0012_create_shop_redemption_vouchers.sql`: orders, vouchers, audit records, constraints, and indexes.
- Create `content/shop.toml`: commented product catalog with no invented live product; operators add real products and icons before enabling them.
- Modify `src/config.rs`, `config/default.toml`, and `.env.example`: runtime shop configuration and validated product catalog.
- Modify `src/rate_limit.rs` and `src/app.rs`: configurable voucher lookup limiter and routes/state.
- Modify `src/currency.rs`: distinguish shop purchases in currency audit labels while reusing the unified debit path.
- Modify `templates/base.html`, `templates/profile.html`, `static/app.css`, and `content/updates.toml`: navigation, presentation, accessibility, and release notes.
- Create `tests/shop_flow.rs`: end-to-end purchase, voucher visibility, authorization, lifecycle, and rollback coverage.

---

### Task 1: Product Catalog, Runtime Configuration, and Icon Validation

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/shop/mod.rs`
- Create: `src/shop/catalog.rs`
- Modify: `src/config.rs`
- Modify: `config/default.toml`
- Modify: `.env.example`
- Create: `content/shop.toml`
- Test: `src/shop/catalog.rs`

**Interfaces:**
- Produces: `catalog::ShopProduct`, `catalog::load_products(file, icon_dir, icon_max_bytes, icon_max_dimension)`, `catalog::find_product(products, id)`, `catalog::validate_icon_file_name(file_name)`, and `catalog::icon_media_type(file_name)`.
- Produces: `config::ShopConfig` stored as `Config.shop` and containing validated `products: Vec<ShopProduct>`.
- Consumes: existing TOML/environment loading conventions in `src/config.rs` and `image` decoding APIs.

- [ ] **Step 1: Enable WebP decoding and declare the shop module**

Change the existing image dependency and module exports exactly as follows:

```toml
image = { version = "0.25", default-features = false, features = ["gif", "jpeg", "png", "webp"] }
```

```rust
// src/lib.rs
pub mod shop;
```

- [ ] **Step 2: Write failing catalog tests**

Add unit tests that create a temporary icon directory and save a 2×2 PNG with `image::DynamicImage::new_rgba8(2, 2).save(icon_dir.join("token.png"))`. The tests must prove multiple products sort by `(sort_order, id)` and that unsafe names, missing icons, oversized files, unsupported extensions, duplicate IDs, zero prices, zero active limits, and zero `valid_days` are rejected.

```rust
#[test]
fn loads_and_sorts_multiple_token_products() {
    let temporary = tempfile::tempdir().unwrap();
    let icon_dir = temporary.path().join("icons");
    std::fs::create_dir_all(&icon_dir).unwrap();
    image::DynamicImage::new_rgba8(2, 2)
        .save(icon_dir.join("token.png"))
        .unwrap();
    let file = temporary.path().join("shop.toml");
    std::fs::write(
        &file,
        r#"
[[products]]
id = "second"
name = "第二件"
description = "第二件说明"
icon_file = "token.png"
price = 20
valid_days = 30
max_active_per_user = 2
enabled = true
sort_order = 20

[[products]]
id = "first"
name = "第一件"
description = "第一件说明"
icon_file = "token.png"
price = 10
max_active_per_user = 1
enabled = true
sort_order = 10
"#,
    )
    .unwrap();

    let products = load_products(&file, &icon_dir, 256 * 1024, 1024).unwrap();
    assert_eq!(products.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), ["first", "second"]);
    assert_eq!(products[0].valid_days, None);
}
```

- [ ] **Step 3: Run catalog tests and verify RED**

Run: `cargo test shop::catalog::tests --lib`

Expected: compilation fails because `ShopProduct` and `load_products` do not exist.

- [ ] **Step 4: Implement product parsing and validation**

Define the runtime type and deserialize-only file wrapper. Keep `fulfillment_type` out of product input in v1 and expose it through a named constant.

```rust
pub const FULFILLMENT_REDEMPTION_TOKEN: &str = "redemption_token";

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize)]
pub struct ShopProduct {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon_file: String,
    pub price: i64,
    pub valid_days: Option<i64>,
    pub max_active_per_user: i64,
    pub enabled: bool,
    pub sort_order: i64,
}

#[derive(serde::Deserialize)]
struct ProductFile {
    #[serde(default)]
    products: Vec<ShopProduct>,
}

pub fn load_products(
    file: &std::path::Path,
    icon_dir: &std::path::Path,
    icon_max_bytes: usize,
    icon_max_dimension: u32,
) -> anyhow::Result<Vec<ShopProduct>>;

pub fn find_product<'a>(products: &'a [ShopProduct], id: &str) -> Option<&'a ShopProduct>;
pub fn validate_icon_file_name(file_name: &str) -> anyhow::Result<()>;
pub fn icon_media_type(file_name: &str) -> Option<&'static str>;
```

Validation must use a single normal path component, compare the decoded `ImageFormat` with the extension, reject either dimension above `icon_max_dimension`, reject bytes above `icon_max_bytes`, enforce unique IDs, and report the offending product ID in every product-specific error. Product IDs use lowercase ASCII letters, digits, `_`, and `-`, are 1–64 characters, names are 1–80 visible characters, and descriptions are 1–500 visible characters; express these bounds as named constants in `catalog.rs`.

- [ ] **Step 5: Add `ShopConfig` and load the catalog at startup**

Add these exact fields and wire their TOML/environment sources into `Config::from_env()`:

```rust
#[derive(Clone, Debug)]
pub struct ShopConfig {
    pub enabled: bool,
    pub products_file: PathBuf,
    pub icon_dir: PathBuf,
    pub page_size: i64,
    pub voucher_page_size: i64,
    pub admin_note_max_length: usize,
    pub token_lookup_max_attempts: usize,
    pub token_lookup_window_seconds: u64,
    pub icon_max_bytes: usize,
    pub icon_max_dimension: u32,
    pub products: Vec<crate::shop::catalog::ShopProduct>,
}
```

Use defaults `enabled = true`, `products_file = "content/shop.toml"`, `icon_dir = "static/shop/products"`, `page_size = 12`, `voucher_page_size = 20`, `admin_note_max_length = 200`, `token_lookup_max_attempts = 20`, `token_lookup_window_seconds = 60`, `icon_max_bytes = 262144`, and `icon_max_dimension = 1024`. Validate all numeric values as positive before calling `load_products`.

- [ ] **Step 6: Add fully commented configuration files**

Add `[shop]` entries to `config/default.toml`, matching commented overrides in `.env.example` (`SHOP_ENABLED`, `SHOP_PRODUCTS_FILE`, `SHOP_ICON_DIR`, `SHOP_PAGE_SIZE`, `SHOP_VOUCHER_PAGE_SIZE`, `SHOP_ADMIN_NOTE_MAX_LENGTH`, `SHOP_TOKEN_LOOKUP_MAX_ATTEMPTS`, `SHOP_TOKEN_LOOKUP_WINDOW_SECONDS`, `SHOP_ICON_MAX_BYTES`, `SHOP_ICON_MAX_DIMENSION`). Create `content/shop.toml` with Chinese instructions and a fully commented example product so the default catalog is valid but empty until a real icon and product are intentionally added.

- [ ] **Step 7: Verify catalog and configuration**

Run: `cargo test shop::catalog::tests --lib && cargo check && git diff --check`

Expected: catalog tests pass, the application compiles, and configuration comments have no whitespace errors.

- [ ] **Step 8: Commit catalog support**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/shop/mod.rs src/shop/catalog.rs src/config.rs config/default.toml .env.example content/shop.toml
git commit -m "Add configurable shop catalog"
```

### Task 2: Token Generation, Normalization, Hashing, and Masking

**Files:**
- Create: `src/shop/token.rs`
- Modify: `src/shop/mod.rs`
- Test: `src/shop/token.rs`

**Interfaces:**
- Produces: `IssuedToken { plaintext: String, hash: String, mask: String }`.
- Produces: `issue() -> Result<IssuedToken, AppError>`, `normalize(input: &str) -> Result<String, AppError>`, and `hash_normalized(normalized: &str) -> String`.
- Consumes: `rand_core::{OsRng, RngCore}` and `sha2::{Digest, Sha256}`.

- [ ] **Step 1: Write failing Token behavior tests**

```rust
#[test]
fn issued_token_has_160_bits_and_round_trips_through_normalization() {
    let issued = issue().unwrap();
    assert_eq!(issued.plaintext.split('-').collect::<Vec<_>>().len(), 9);
    assert!(issued.plaintext.starts_with("ZV1-"));
    assert_eq!(normalize(&issued.plaintext.to_lowercase()).unwrap().len(), 35);
    assert_eq!(hash_normalized(&normalize(&issued.plaintext).unwrap()), issued.hash);
    assert!(issued.mask.starts_with("ZV1-****-"));
    assert!(issued.mask.ends_with(issued.plaintext.rsplit('-').next().unwrap()));
}

#[test]
fn normalization_accepts_spaces_and_hyphens_but_rejects_ambiguous_characters() {
    let issued = issue().unwrap();
    let spaced = issued.plaintext.replace('-', " ");
    assert_eq!(normalize(&spaced).unwrap(), normalize(&issued.plaintext).unwrap());
    assert!(normalize("ZV1-OOOO-OOOO-OOOO-OOOO-OOOO-OOOO-OOOO-OOOO").is_err());
}
```

- [ ] **Step 2: Run Token tests and verify RED**

Run: `cargo test shop::token::tests --lib`

Expected: compilation fails because the Token functions are absent.

- [ ] **Step 3: Implement the fixed Token format**

Use these exact constants and behavior:

```rust
const TOKEN_PREFIX: &str = "ZV1";
const RANDOM_BYTES: usize = 20;
const PAYLOAD_CHARACTERS: usize = 32;
const GROUP_CHARACTERS: usize = 4;
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedToken {
    pub plaintext: String,
    pub hash: String,
    pub mask: String,
}
```

`issue` fills `[u8; RANDOM_BYTES]` with `OsRng`, converts all 160 bits into exactly 32 alphabet characters, formats eight groups of four, normalizes the formatted value to the compact 35-character `ZV1` plus payload representation, hashes that compact representation, and keeps only the final four payload characters in the mask. `normalize` removes ASCII whitespace and hyphens, uppercases ASCII, validates exact length/prefix/alphabet, and never echoes invalid input in its error message.

- [ ] **Step 4: Run Token tests and verify GREEN**

Run: `cargo test shop::token::tests --lib`

Expected: all Token tests pass.

- [ ] **Step 5: Commit Token primitives**

```bash
git add src/shop/mod.rs src/shop/token.rs
git commit -m "Add secure redemption token primitives"
```

### Task 3: Orders, Vouchers, and Audit Persistence

**Files:**
- Create: `migrations/0012_create_shop_redemption_vouchers.sql`
- Create: `src/shop/store.rs`
- Modify: `src/shop/mod.rs`
- Test: `tests/shop_flow.rs`

**Interfaces:**
- Produces: `OrderRow`, `VoucherRow`, `VoucherWithOrder`, `VoucherPage`, and `EffectiveVoucherStatus`.
- Produces: owned insertion values `NewOrder` and `NewVoucher`, each accepted by reference by its corresponding store insertion function.
- Produces focused store functions: `find_order_by_purchase_key`, `count_active_for_user_product`, `active_counts_for_user`, `insert_order`, `insert_voucher`, `insert_audit`, `list_user_vouchers`, `find_voucher_by_hash`, and `find_voucher_with_order_by_id`.
- Consumes: validated product snapshots and application-generated RFC 3339 UTC timestamps.

- [ ] **Step 1: Add the migration test before the migration**

Create `tests/shop_flow.rs` with a local `sqlite_url`, `test_config`, and `create_test_icon` helper. The first test connects through `db::connect` and queries all three expected tables:

```rust
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
```

The test-only `sqlite_master` query is permitted as a SQLite adapter check; production business SQL remains portable.

- [ ] **Step 2: Run the migration test and verify RED**

Run: `cargo test shop_migration_creates_order_voucher_and_audit_tables --test shop_flow`

Expected: FAIL because the tables do not exist.

- [ ] **Step 3: Create the portable business schema**

Use this migration structure, preserving named constraints and indexes:

```sql
CREATE TABLE shop_orders (
    id CHAR(26) PRIMARY KEY NOT NULL,
    user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    product_id VARCHAR(64) NOT NULL,
    product_name VARCHAR(80) NOT NULL,
    product_description VARCHAR(500) NOT NULL,
    icon_file VARCHAR(128) NOT NULL,
    fulfillment_type VARCHAR(32) NOT NULL,
    price_paid BIGINT NOT NULL CHECK (price_paid > 0),
    valid_days BIGINT CHECK (valid_days IS NULL OR valid_days > 0),
    purchase_key CHAR(26) NOT NULL UNIQUE,
    created_at VARCHAR(40) NOT NULL
);

CREATE TABLE redemption_vouchers (
    id CHAR(26) PRIMARY KEY NOT NULL,
    order_id CHAR(26) NOT NULL UNIQUE REFERENCES shop_orders(id) ON DELETE RESTRICT,
    token_hash CHAR(64) NOT NULL UNIQUE,
    token_mask VARCHAR(80) NOT NULL,
    status VARCHAR(20) NOT NULL CHECK (status IN ('active', 'redeemed', 'cancelled')),
    expires_at VARCHAR(40),
    redeemed_at VARCHAR(40),
    redeemed_by_user_id CHAR(26) REFERENCES users(id) ON DELETE SET NULL,
    redemption_note VARCHAR(200) NOT NULL DEFAULT '',
    cancelled_at VARCHAR(40),
    cancelled_by_user_id CHAR(26) REFERENCES users(id) ON DELETE SET NULL,
    cancellation_reason VARCHAR(200) NOT NULL DEFAULT '',
    created_at VARCHAR(40) NOT NULL
);

CREATE TABLE voucher_audit_logs (
    id CHAR(26) PRIMARY KEY NOT NULL,
    voucher_id CHAR(26) NOT NULL REFERENCES redemption_vouchers(id) ON DELETE RESTRICT,
    event_type VARCHAR(20) NOT NULL CHECK (event_type IN ('created', 'redeemed', 'cancelled')),
    actor_user_id CHAR(26) REFERENCES users(id) ON DELETE SET NULL,
    note VARCHAR(200) NOT NULL DEFAULT '',
    created_at VARCHAR(40) NOT NULL
);

CREATE INDEX shop_orders_user_created_idx ON shop_orders(user_id, created_at);
CREATE INDEX redemption_vouchers_status_expiry_idx ON redemption_vouchers(status, expires_at);
CREATE INDEX voucher_audit_voucher_created_idx ON voucher_audit_logs(voucher_id, created_at);
```

- [ ] **Step 4: Run the migration test and verify GREEN**

Run: `cargo test shop_migration_creates_order_voucher_and_audit_tables --test shop_flow`

Expected: PASS.

- [ ] **Step 5: Implement store row types and page queries**

Define `EffectiveVoucherStatus::{Active, Redeemed, Cancelled, Expired}` and compute `Expired` in Rust when a row remains active and its parsed `expires_at` is not later than the supplied current time. Use this exact pagination shape:

```rust
pub struct VoucherPage {
    pub items: Vec<VoucherWithOrder>,
    pub current_page: i64,
    pub total_pages: i64,
}

pub async fn list_user_vouchers(
    pool: &SqlitePool,
    user_id: &str,
    page: i64,
    page_size: i64,
) -> Result<VoucherPage, AppError>;

pub async fn find_voucher_by_hash(
    pool: &SqlitePool,
    token_hash: &str,
) -> Result<Option<VoucherWithOrder>, AppError>;

pub async fn active_counts_for_user(
    pool: &SqlitePool,
    user_id: &str,
    now: &str,
) -> Result<std::collections::HashMap<String, i64>, AppError>;

pub struct NewOrder {
    pub id: String,
    pub user_id: String,
    pub product_id: String,
    pub product_name: String,
    pub product_description: String,
    pub icon_file: String,
    pub fulfillment_type: String,
    pub price_paid: i64,
    pub valid_days: Option<i64>,
    pub purchase_key: String,
    pub created_at: String,
}

pub struct NewVoucher {
    pub id: String,
    pub order_id: String,
    pub token_hash: String,
    pub token_mask: String,
    pub expires_at: Option<String>,
    pub created_at: String,
}

pub async fn insert_order(
    transaction: &mut Transaction<'_, Sqlite>,
    order: &NewOrder,
) -> Result<(), AppError>;

pub async fn insert_voucher(
    transaction: &mut Transaction<'_, Sqlite>,
    voucher: &NewVoucher,
) -> Result<(), AppError>;

pub async fn insert_audit(
    transaction: &mut Transaction<'_, Sqlite>,
    voucher_id: &str,
    event_type: &str,
    actor_user_id: Option<&str>,
    note: &str,
    created_at: &str,
) -> Result<(), AppError>;
```

Every SELECT must name columns explicitly. The active-limit count must bind `user_id`, `product_id`, `STATUS_ACTIVE`, and an application-generated current timestamp, with `expires_at IS NULL OR expires_at > ?`. `active_counts_for_user` performs one grouped query for the shop page and returns product ID to active-count mappings, avoiding one query per product card.

- [ ] **Step 6: Add and pass store query tests**

Insert two orders/vouchers for different users through store functions, then assert user pagination does not leak the other user, hash lookup returns the order snapshot, and an active voucher before/after its bound expiration is classified as `Active`/`Expired`.

Run: `cargo test store_ --test shop_flow`

Expected: all store tests pass.

- [ ] **Step 7: Commit persistence**

```bash
git add migrations/0012_create_shop_redemption_vouchers.sql src/shop/mod.rs src/shop/store.rs tests/shop_flow.rs
git commit -m "Add shop order and voucher persistence"
```

### Task 4: Atomic Purchase Service and Currency Integration

**Files:**
- Modify: `src/shop/mod.rs`
- Modify: `src/shop/store.rs`
- Modify: `src/currency.rs`
- Test: `tests/shop_flow.rs`

**Interfaces:**
- Produces: `PurchaseOutcome::Created(PurchasedVoucher)` and `PurchaseOutcome::AlreadyProcessed`.
- Produces: `purchase(pool, user, product, purchase_key, currency_config, now) -> Result<PurchaseOutcome, AppError>`.
- Produces: `PurchasedVoucher { order_id, voucher_id, plaintext_token, token_mask, expires_at }` only for the first successful request.
- Consumes: Task 1 product snapshots, Task 2 Token primitives, Task 3 store functions, and `currency::spend_currency`.

- [ ] **Step 1: Write failing transaction behavior tests**

Add tests for first purchase, duplicate purchase key, active-limit rejection, expired-voucher exclusion, insufficient balance, and forced voucher insert failure. The primary assertion must prove the plaintext is absent from all database text columns:

```rust
let outcome = shop::purchase(&pool, &buyer, &product, &purchase_key, &config.currency, now)
    .await
    .unwrap();
let PurchaseOutcome::Created(created) = outcome else { panic!("expected created") };
assert!(created.plaintext_token.starts_with("ZV1-"));
assert_eq!(user_balance(&pool, &buyer.id).await, starting_balance - product.price);
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
```

For duplicate submission, assert the second result is `AlreadyProcessed`, the balance changes once, exactly one order/voucher exists, and no plaintext Token is returned.

- [ ] **Step 2: Run purchase tests and verify RED**

Run: `cargo test purchase_ --test shop_flow`

Expected: compilation fails because `shop::purchase` and its outcome types are absent.

- [ ] **Step 3: Add the shop purchase currency reason**

Add `REASON_SHOP_PURCHASE`, `CurrencyReason::ShopPurchase`, allow it through the existing debit validation alongside `Spend`, and map it to “商城购买” in all currency page view labels. Keep the amount negative and use order ID as `related_id`.

```rust
if !matches!(reason, CurrencyReason::Spend | CurrencyReason::ShopPurchase) {
    return Err(AppError::BadRequest("消费原因无效".to_owned()));
}
```

- [ ] **Step 4: Implement the purchase transaction**

Use the following operation order inside one SQLx transaction:

```rust
pub async fn purchase(
    pool: &SqlitePool,
    user: &User,
    product: &ShopProduct,
    purchase_key: &str,
    currency_config: &CurrencyConfig,
    now: time::OffsetDateTime,
) -> Result<PurchaseOutcome, AppError> {
    let purchase_key = validate_purchase_key(purchase_key)?;
    let mut transaction = pool.begin().await?;

    // 对用户行执行无值变化的更新，序列化同一用户的并发购买，再检查动态持有限额。
    store::lock_user_for_purchase(&mut transaction, &user.id).await?;
    if let Some(existing) = store::find_order_by_purchase_key(&mut transaction, purchase_key).await? {
        ensure_same_request(&existing, &user.id, &product.id)?;
        transaction.commit().await?;
        return Ok(PurchaseOutcome::AlreadyProcessed);
    }

    let now_string = format_utc(now)?;
    let active = store::count_active_for_user_product(
        &mut transaction, &user.id, &product.id, &now_string,
    ).await?;
    ensure_active_limit(active, product.max_active_per_user)?;

    let order_id = Ulid::new().to_string();
    let voucher_id = Ulid::new().to_string();
    let issued = token::issue()?;
    let expires_at = calculate_expiration(now, product.valid_days)?;
    let order = store::NewOrder {
        id: order_id.clone(),
        user_id: user.id.clone(),
        product_id: product.id.clone(),
        product_name: product.name.clone(),
        product_description: product.description.clone(),
        icon_file: product.icon_file.clone(),
        fulfillment_type: catalog::FULFILLMENT_REDEMPTION_TOKEN.to_owned(),
        price_paid: product.price,
        valid_days: product.valid_days,
        purchase_key: purchase_key.to_owned(),
        created_at: now_string.clone(),
    };
    store::insert_order(&mut transaction, &order).await?;
    currency::spend_currency(
        &mut transaction,
        &user.id,
        product.price,
        CurrencyReason::ShopPurchase,
        Some(&order_id),
        &format!("shop-purchase:{purchase_key}"),
        &format!("购买商品：{}", product.name),
    ).await?;
    let voucher = store::NewVoucher {
        id: voucher_id.clone(),
        order_id: order_id.clone(),
        token_hash: issued.hash.clone(),
        token_mask: issued.mask.clone(),
        expires_at: expires_at.clone(),
        created_at: now_string.clone(),
    };
    store::insert_voucher(&mut transaction, &voucher).await?;
    store::insert_audit(&mut transaction, &voucher_id, "created", Some(&user.id), "", &now_string).await?;
    transaction.commit().await?;
    Ok(PurchaseOutcome::Created(PurchasedVoucher {
        order_id,
        voucher_id,
        plaintext_token: issued.plaintext,
        token_mask: issued.mask,
        expires_at,
    }))
}
```

Use the `NewOrder` and `NewVoucher` values defined in Task 3 rather than long positional parameter lists. Add these private helpers with the exact contracts below:

```rust
fn validate_purchase_key(value: &str) -> Result<&str, AppError>;
fn ensure_same_request(order: &store::OrderRow, user_id: &str, product_id: &str) -> Result<(), AppError>;
fn ensure_active_limit(active: i64, maximum: i64) -> Result<(), AppError>;
fn calculate_expiration(now: OffsetDateTime, valid_days: Option<i64>) -> Result<Option<String>, AppError>;
fn format_utc(value: OffsetDateTime) -> Result<String, AppError>;
```

`calculate_expiration` uses checked `time::Duration::days` addition and returns RFC 3339 UTC. `lock_user_for_purchase` executes `UPDATE users SET updated_at = updated_at WHERE id = ?` and requires exactly one affected row. Validate `purchase_key` as a ULID before opening the transaction. Generate all error text without including Token input.

- [ ] **Step 5: Run purchase tests and verify GREEN**

Run: `cargo test purchase_ --test shop_flow`

Expected: purchase, idempotency, limit, insufficient-balance, expiration, and rollback tests pass.

- [ ] **Step 6: Run existing currency regression tests**

Run: `cargo test super_admin_can_adjust_currency_and_spend_is_idempotent --test auth_flow && cargo test meme_approval_rewards_provider_once --test auth_flow && cargo test weekly_check_in_awards_currency_once --test auth_flow`

Expected: all existing currency consumers still pass.

- [ ] **Step 7: Commit purchase service**

```bash
git add src/shop/mod.rs src/shop/store.rs src/currency.rs tests/shop_flow.rs
git commit -m "Add atomic shop token purchases"
```

### Task 5: Player Shop, One-Time Reveal, Voucher List, and Product Icons

**Files:**
- Create: `src/web/shop.rs`
- Modify: `src/web/mod.rs`
- Modify: `src/web/views.rs`
- Modify: `src/app.rs`
- Create: `templates/shop.html`
- Create: `templates/voucher_reveal.html`
- Create: `templates/vouchers.html`
- Modify: `static/app.css`
- Test: `tests/shop_flow.rs`

**Interfaces:**
- Produces HTTP handlers: `shop_page`, `purchase_product`, `voucher_list`, and `shop_product_icon`.
- Produces routes: `GET /shop`, `POST /shop/products/{product_id}/purchase`, `GET /vouchers`, and `GET /static/shop/products/{file_name}`.
- Consumes: `shop::purchase`, `store::list_user_vouchers`, catalog products, existing `PageContext`, CSRF/session helpers, and `binary_response`.

- [ ] **Step 1: Write failing player HTTP tests**

Test these observable behaviors with a temporary product config and PNG icon:

```rust
#[tokio::test]
async fn first_purchase_reveals_token_once_and_duplicate_request_does_not() {
    let fixture = ShopFixture::new().await;
    fixture.grant_buyer_currency(50).await;
    let (cookie, csrf) = fixture.sign_in_buyer().await;
    let purchase_key = ulid::Ulid::new().to_string();

    let first = fixture.purchase(&cookie, &csrf, "milk_tea", &purchase_key).await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()[header::CACHE_CONTROL], "no-store");
    let first_html = response_html(first).await;
    assert!(first_html.contains("请立即复制并妥善保存"));
    assert!(first_html.contains("data-token=\"ZV1-"));

    let duplicate = fixture.purchase(&cookie, &csrf, "milk_tea", &purchase_key).await;
    assert_eq!(duplicate.status(), StatusCode::SEE_OTHER);
    assert_eq!(duplicate.headers()[header::LOCATION], "/vouchers?purchase=already");
    let list_html = fixture.get_html("/vouchers", &cookie).await;
    assert!(list_html.contains("ZV1-****-"));
    assert!(!list_html.contains("data-token=\"ZV1-"));
}
```

Also test anonymous catalog browsing, login-required purchase/list, configured page limits, balance/active-limit disabled messages, unknown/down product rejection, icon media type plus `nosniff`, and another user being unable to see the buyer's voucher.

- [ ] **Step 2: Run player HTTP tests and verify RED**

Run: `cargo test player_ first_purchase_ --test shop_flow`

Expected: routes return 404 or tests fail because templates/handlers are absent.

- [ ] **Step 3: Add shop view models and handlers**

Add `ShopTemplate`, `VoucherRevealTemplate`, and `VouchersTemplate` plus focused item views. Each product view carries a newly generated ULID purchase key, computed affordability/limit state, icon URL, and a server-generated disabled reason. Never derive trusted price or validity from form data.

```rust
#[derive(serde::Deserialize)]
pub struct PurchaseForm {
    csrf_token: String,
    purchase_key: String,
}

pub async fn purchase_product(
    State(state): State<AppState>,
    Path(product_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<PurchaseForm>,
) -> Result<Response, AppError>;
```

`GET /shop` is public; anonymous users see product cards and a login prompt. Purchase and `/vouchers` require an authenticated user. If `shop.enabled` is false, all shop/voucher handlers return `NotFound` except historical icon access, which remains available for existing voucher snapshots.

In `tests/shop_flow.rs`, define `ShopFixture` with owned `TempDir`, `SqlitePool`, `Router`, buyer/admin/super-admin users, and product config. Its methods must be exactly `new()`, `grant_buyer_currency(amount)`, `sign_in_buyer()`, `purchase(cookie, csrf, product_id, purchase_key)`, and `get_html(path, cookie)`. The helper may extract CSRF and cookies but must call real routes rather than invoking service functions directly.

- [ ] **Step 4: Implement the one-time reveal response and copy behavior**

Render the plaintext only into `voucher_reveal.html`, set `Cache-Control: no-store`, `Pragma: no-cache`, and `X-Content-Type-Options: nosniff`, and do not redirect the first successful purchase. Use a safe `data-token` attribute and this browser behavior:

```javascript
const copyButton = document.querySelector("[data-copy-token]");
copyButton?.addEventListener("click", async () => {
  const token = copyButton.dataset.token;
  if (!token) return;
  await navigator.clipboard.writeText(token);
  copyButton.textContent = "已复制，请妥善保存";
});
```

Add a visible selectable `<code>` value so copying remains possible when the Clipboard API is unavailable. Add `@keyframes voucher-warning-pulse` and disable its animation under the existing `@media (prefers-reduced-motion: reduce)` block.

- [ ] **Step 5: Implement safe icon serving**

The icon handler validates a single safe file name with `catalog::validate_icon_file_name`, maps the supported extension with `icon_media_type`, reads only `state.config.shop.icon_dir.join(file_name)`, and returns `binary_response(bytes, media_type, "public, max-age=86400")`. It must never log the path supplied by the user on rejection.

- [ ] **Step 6: Run player tests and verify GREEN**

Run: `cargo test player_ first_purchase_ --test shop_flow`

Expected: player catalog, purchase, one-time reveal, isolation, pagination, and icon tests pass.

- [ ] **Step 7: Commit player experience**

```bash
git add src/web/shop.rs src/web/mod.rs src/web/views.rs src/app.rs templates/shop.html templates/voucher_reveal.html templates/vouchers.html static/app.css tests/shop_flow.rs
git commit -m "Add shop and player voucher pages"
```

### Task 6: Super Administrator Lookup, Redemption, Cancellation, and Rate Limiting

**Files:**
- Modify: `src/rate_limit.rs`
- Modify: `src/app.rs`
- Modify: `src/error.rs`
- Modify: `src/shop/mod.rs`
- Modify: `src/shop/store.rs`
- Modify: `src/web/shop.rs`
- Modify: `src/web/views.rs`
- Create: `templates/admin_vouchers.html`
- Modify: `static/app.css`
- Test: `src/rate_limit.rs`
- Test: `tests/shop_flow.rs`

**Interfaces:**
- Produces: configurable `AttemptLimiter::new(window, max_attempts)` and `check_and_record(key)`.
- Produces service functions `lookup_by_token`, `redeem_voucher`, and `cancel_voucher`.
- Produces handlers `admin_vouchers`, `lookup_voucher`, `redeem_voucher`, and `cancel_voucher`.
- Consumes: Task 2 normalization/hash, Task 3 conditional persistence, Task 1 note limit, super-admin authorization, and CSRF helpers.

Use these service signatures so handlers cannot supply an amount, product snapshot, purchaser, or target status:

```rust
pub async fn lookup_by_token(
    pool: &SqlitePool,
    raw_token: &str,
    now: OffsetDateTime,
) -> Result<Option<store::VoucherWithOrder>, AppError>;

pub async fn redeem_voucher(
    pool: &SqlitePool,
    voucher_id: &str,
    actor: &User,
    note: &str,
    note_max_length: usize,
    now: OffsetDateTime,
) -> Result<(), AppError>;

pub async fn cancel_voucher(
    pool: &SqlitePool,
    voucher_id: &str,
    actor: &User,
    reason: &str,
    note_max_length: usize,
    now: OffsetDateTime,
) -> Result<(), AppError>;
```

- [ ] **Step 1: Write failing limiter and voucher lifecycle tests**

```rust
#[tokio::test]
async fn configurable_attempt_limiter_blocks_after_the_limit() {
    let limiter = AttemptLimiter::new(Duration::from_secs(60), 2);
    assert!(limiter.check_and_record("actor-1").await);
    assert!(limiter.check_and_record("actor-1").await);
    assert!(!limiter.check_and_record("actor-1").await);
}
```

Integration tests must prove: ordinary admins receive 403; super admins can query a Token with lowercase/spaces; query does not change status; lookup output labels the original buyer as not necessarily the holder; notes are required and bounded; redeem succeeds once; cancel succeeds once; expired/redeemed/cancelled vouchers reject further transitions; audit rows contain actor and note; and the configured attempt count produces HTTP 429 without echoing the supplied Token.

- [ ] **Step 2: Run lifecycle tests and verify RED**

Run: `cargo test configurable_attempt_limiter --lib && cargo test admin_voucher_ --test shop_flow`

Expected: limiter API and administrator routes do not exist.

- [ ] **Step 3: Implement the configurable limiter and state field**

Use a per-actor in-memory sliding-window limiter separate from login failures:

```rust
#[derive(Clone)]
pub struct AttemptLimiter {
    attempts: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    window: Duration,
    max_attempts: usize,
}

impl AttemptLimiter {
    pub fn new(window: Duration, max_attempts: usize) -> Self;
    pub async fn check_and_record(&self, key: &str) -> bool;
}
```

Construct `voucher_lookup_limiter` in `app::build` from `ShopConfig`, store it in `AppState`, and key attempts by authenticated super-admin user ID. Record every lookup, whether valid or invalid, so successful guesses cannot bypass throttling.

Add `AppError::TooManyRequests(String)` in `src/error.rs` and map it to HTTP 429. The public error must be the fixed text “Token 查询过于频繁，请稍后再试” and must never include submitted input.

- [ ] **Step 4: Implement conditional lifecycle transactions**

Validate trimmed notes as 1 through `admin_note_max_length` visible characters. `lookup_by_token` normalizes and hashes without logging input. Redemption and cancellation begin a transaction, load the voucher/order by ID, derive effective status using the supplied current time, and issue a conditional update:

```sql
UPDATE redemption_vouchers
SET status = ?, redeemed_at = ?, redeemed_by_user_id = ?, redemption_note = ?
WHERE id = ? AND status = ? AND (expires_at IS NULL OR expires_at > ?)
```

Cancellation uses the corresponding cancellation fields and requires only `status = active`; if the voucher is already expired, reject it rather than converting it to cancelled. Require exactly one affected row, write the matching audit event in the same transaction, then commit. Never delete a voucher or audit row.

- [ ] **Step 5: Implement administrator routes and two-step page**

Add routes:

```rust
.route("/admin/vouchers", get(web::admin_vouchers))
.route("/admin/vouchers/lookup", post(web::lookup_voucher))
.route("/admin/vouchers/{id}/redeem", post(web::redeem_voucher))
.route("/admin/vouchers/{id}/cancel", post(web::cancel_voucher))
```

The lookup POST renders the same management template with an optional result and `Cache-Control: no-store`. The result includes product snapshot, effective state, expiration, and `原购买者：昵称 @用户名（不代表当前持有人）`. Redeem/cancel forms contain only CSRF, voucher ID from the path, and the required note/reason; they never carry the complete Token forward.

- [ ] **Step 6: Run lifecycle tests and verify GREEN**

Run: `cargo test configurable_attempt_limiter --lib && cargo test admin_voucher_ --test shop_flow`

Expected: limiter, permissions, lookup, transitions, audit, expiry, and no-echo tests pass.

- [ ] **Step 7: Commit administrator lifecycle**

```bash
git add src/rate_limit.rs src/app.rs src/error.rs src/shop/mod.rs src/shop/store.rs src/web/shop.rs src/web/views.rs templates/admin_vouchers.html static/app.css tests/shop_flow.rs
git commit -m "Add voucher redemption administration"
```

### Task 7: Navigation, Release Record, Security Regression, and Full Verification

**Files:**
- Modify: `templates/base.html`
- Modify: `templates/profile.html`
- Modify: `content/updates.toml`
- Modify: `tests/auth_flow.rs`
- Modify: `tests/shop_flow.rs`

**Interfaces:**
- Consumes: all completed shop/player/admin routes.
- Produces: discoverable navigation, role-correct management links, release documentation, and final verification evidence.

- [ ] **Step 1: Write failing navigation tests**

Extend navigation assertions so anonymous visitors see “商城”, authenticated users can reach “我的兑换凭证” from the shop/profile experience, ordinary administrators do not see “Token 核销”, and super administrators do.

```rust
assert!(anonymous_html.contains("href=\"/shop\""));
assert!(profile_html.contains("href=\"/vouchers\""));
assert!(!admin_html.contains("href=\"/admin/vouchers\""));
assert!(super_admin_html.contains("href=\"/admin/vouchers\""));
```

- [ ] **Step 2: Run navigation tests and verify RED**

Run: `cargo test management_navigation_groups_admin_links_by_role --test auth_flow && cargo test shop_navigation_is_role_appropriate --test shop_flow`

Expected: assertions fail because links are absent.

- [ ] **Step 3: Add role-correct navigation**

Add `/shop` to the main navigation. Add a “我的兑换凭证” button to the authenticated shop header and profile currency/action area. Add “Token 核销” inside the existing management dropdown only under `ctx.is_super_admin`; do not add another top-level management button.

- [ ] **Step 4: Add the required server update record**

Prepend a dated, descending `0.1.4` entry to `content/updates.toml` with title “商城与兑换凭证”, a short summary, and changes covering configurable products/icons, currency purchases, one-time Token display, personal voucher history, and super-admin redemption/cancellation.

- [ ] **Step 5: Add explicit security regression checks**

Add a test that buys a product, extracts the generated complete Token from the first reveal HTML, visits every player/admin page, and asserts that extracted value appears only in the first reveal HTML. Inspect tracing calls manually with:

```bash
rg -n "tracing::|info!|warn!|error!|debug!" src/shop src/web/shop.rs
```

Expected: no logging statement includes raw form Token, normalized Token, plaintext Token, token hash, or `IssuedToken` debug output.

- [ ] **Step 6: Run focused feature verification**

Run: `cargo test --test shop_flow`

Expected: all shop tests pass with no ignored failures.

- [ ] **Step 7: Run repository-wide verification**

Run:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

Expected: formatting passes; all unit, integration, and documentation tests pass; Clippy emits no warnings; diff check reports no whitespace errors.

- [ ] **Step 8: Review migration and configuration portability**

Run:

```bash
rg -n "INSERT OR|AUTOINCREMENT|datetime\(|strftime\(|julianday\(" migrations/0012_create_shop_redemption_vouchers.sql src/shop
rg -n "^\s*[A-Za-z_][A-Za-z0-9_]*\s*=" config/default.toml content/shop.toml
```

Expected: the first command returns no SQLite-specific business SQL; every active configuration assignment reported by the second command has an immediately preceding Chinese explanatory comment.

- [ ] **Step 9: Commit navigation and release documentation**

```bash
git add templates/base.html templates/profile.html content/updates.toml tests/auth_flow.rs tests/shop_flow.rs
git commit -m "Finish shop redemption token feature"
```
