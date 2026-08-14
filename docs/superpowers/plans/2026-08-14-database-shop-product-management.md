# Database Shop Product Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Each task ends with a focused verification and commit.

**Goal:** Replace TOML-loaded shop products with immediately effective super-admin CRUD, atomic global sale limits, audited lifecycle rules, and safe automatic PNG/JPEG/WebP/GIF icon processing.

**Architecture:** SQLite stores the authoritative `shop_products` catalog and `shop_product_audit_logs`; public shop and purchase code query the database on each request. Product mutations are super-admin-only, CSRF-protected transactions. Icons are normalized in a blocking image-processing task and stored under `data/shop/product-icons` with server-generated names; existing order snapshots remain immutable.

**Tech Stack:** Rust 2024, Axum, Askama, SQLx/SQLite, `image`, Tokio, ULID, existing currency/token/audit services.

## Global Constraints

- Final state must not import or preserve old `content/shop.toml` product data; the new product table starts empty and the TOML file/loading code is removed in the database integration task. Earlier tasks may retain the legacy fields temporarily solely to keep the branch compiling.
- Use portable standard SQL; do not add avoidable SQLite functions or syntax. Bind Rust-generated UTC RFC 3339 timestamps.
- Only `Role::SuperAdmin` can read or mutate product management routes; every state-changing form uses POST and CSRF.
- Add concise Chinese comments at key logic and every new committed TOML/`.env.example` option.
- Product IDs are lowercase stable identifiers; a product ID recorded by the management audit log cannot be reused after deletion.
- `total_limit = NULL` means unlimited; `sold_count` starts at zero, increases only inside a successful purchase transaction, and never decreases after redemption, cancellation, or expiration.
- New user-facing/database functionality requires a dated `0.1.5` entry in `content/updates.toml` when implementation is complete.
- Run `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, and `git diff --check` before final handoff.

## Files and Responsibility Map

- Create `migrations/0013_create_shop_products.sql` for products and product audits.
- Create `src/shop/icon.rs` for image format detection, static/GIF normalization, limits, and cleanup-friendly results.
- Modify `src/shop/catalog.rs` to retain product validation/domain constants without TOML deserialization, or move its stable fulfillment constant into `src/shop/mod.rs` and remove the file when no callers remain.
- Modify `src/shop/store.rs` for product CRUD, public listing, audit writes, and atomic sale-count updates.
- Modify `src/shop/mod.rs` for product service validation, lifecycle rules, and purchase lookup by product ID.
- Modify `src/config.rs`, `config/default.toml`, `.env.example`, and `src/app.rs` for database-catalog configuration and icon processing limits/body size.
- Modify `src/web/shop.rs`, `src/web/views.rs`, `src/web/mod.rs`, and `src/app.rs` for routes, role checks, forms, and immediate public reads.
- Create `templates/admin_shop_products.html` and `templates/admin_shop_product_form.html`; update `templates/base.html`, `templates/shop.html`, and related styles in `static/app.css`.
- Remove `content/shop.toml` and all runtime product-file loading.
- Extend `tests/shop_flow.rs` and add focused unit tests beside icon/product code.

### Task 1: Replace TOML Product Configuration with Runtime Limits

**Files:**
- Modify: `src/config.rs`, `config/default.toml`, `.env.example`, `src/app.rs`
- Delete: `content/shop.toml`
- Test: `src/config.rs` tests and a new `tests/shop_flow.rs` configuration fixture assertion

**Interfaces:**
- Produces `ShopConfig` containing `enabled`, pagination, voucher limits, token lookup limits, `icon_dir`, `icon_upload_max_bytes`, `icon_input_max_dimension`, `icon_max_gif_frames`, `icon_max_decoded_pixels`, `icon_max_stored_bytes`, and `icon_resize_dimensions`.
- `ShopConfig` no longer contains `products_file` or an in-memory `products` vector.

- [ ] Add failing config assertions that a valid `[shop]` section loads all new values and rejects an empty/non-descending resize-dimension list.
- [ ] Run `cargo test shop_config --lib` and verify the new assertions fail because legacy product loading still exists.
- [ ] Add the documented defaults: 5 MiB upload, 4096 input dimension, 120 GIF frames, 80,000,000 decoded pixels, 1 MiB stored output, and `[512, 384, 256]` resize candidates. Keep the legacy product fields and TOML loader temporarily so existing consumers compile; Task 5 removes them after database reads are in place.
- [ ] Add Chinese comments for every TOML and `.env.example` setting; parse the resize list from a comma-separated environment value so deployment overrides remain possible.
- [ ] Increase `DefaultBodyLimit` calculation to include the configured icon upload limit plus multipart overhead, while preserving existing Meme/avatar/novel limits.
- [ ] Run `cargo test --lib`, `cargo fmt --check`, and `cargo check`.
- [ ] Commit: `Remove TOML shop product loading`.

### Task 2: Add Product and Audit Persistence

**Files:**
- Create: `migrations/0013_create_shop_products.sql`
- Modify: `src/shop/store.rs`, `src/shop/catalog.rs` or `src/shop/mod.rs`, `tests/shop_flow.rs`

**Interfaces:**
- Produces `store::ProductRow`, `store::ProductAuditRow`, `store::list_enabled_products`, `store::list_admin_products`, `store::find_product`, `store::insert_product`, `store::update_product`, `store::set_product_enabled`, `store::delete_product`, and `store::insert_product_audit`.
- Product writes accept explicit values and a Rust-generated `now` string; SQL never generates time values.

- [ ] Write RED migration/store tests for `shop_products` fields, uniqueness, positive checks, nullable `total_limit`, `sold_count` default zero, and audit rows surviving product deletion.
- [ ] Run `cargo test shop_product_persistence --test shop_flow` and verify failure because migration 0013 and store functions are absent.
- [ ] Add portable tables with ULID IDs, stable product ID primary key, nullable `total_limit`, integer `sold_count`, enabled flag, icon metadata, actor IDs, timestamps, and indexes for enabled/sort and audit/product time.
- [ ] Implement explicit-column SQLx queries and map rows into documented Rust structs; never use `SELECT *` for the new tables.
- [ ] Implement audit actions `created`, `updated`, `enabled`, `disabled`, and `deleted`. Keep the product ID as text in audit rows without a restrictive product foreign key so deletion history remains available.
- [ ] Add service-level validation that product IDs are stable, names/descriptions are bounded, positive fields are valid, total limit is null or at least sold count, and IDs present in product audit history cannot be reused.
- [ ] Run the migration tests, `cargo fmt --check`, and `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] Commit: `Add database shop product persistence`.

### Task 3: Implement Safe Automatic Icon Processing

**Files:**
- Create: `src/shop/icon.rs`
- Modify: `Cargo.toml` only if an existing image dependency lacks the required encoder/decoder
- Test: `src/shop/icon.rs` unit tests and `tests/shop_flow.rs` upload cases

**Interfaces:**
- Produces `IconProcessor::process(bytes, &ShopConfig) -> Result<ProcessedIcon, AppError>` with bytes, generated extension/media type, width, height, and frame count.
- `ProcessedIcon` never contains caller-provided paths and returns only validated server-safe output.

- [ ] Write RED tests for static resize/WebP output, GIF frame preservation, oversized input, invalid actual format, input dimensions, GIF frame count, decoded pixel budget, output-size retry candidates, and final rejection after the smallest candidate.
- [ ] Run the focused icon tests and verify they fail before implementation.
- [ ] Detect actual bytes with `image::guess_format`, reject SVG/unknown formats, enforce input size before decoding, and validate dimensions before iterating every GIF frame.
- [ ] Normalize PNG/JPEG/WebP to WebP while preserving alpha and aspect ratio. Resize GIF frames to each configured candidate, preserve delay/loop behavior, and retain GIF format.
- [ ] Execute processing through `tokio::task::spawn_blocking` in the web handler; never decode large images on the async executor.
- [ ] Return a clear BadRequest when the smallest candidate remains over the configured stored-byte limit; do not silently freeze an animated GIF.
- [ ] Add tests that temporary files are removed by callers after processing/database failure and that no raw upload filename enters output paths.
- [ ] Run icon unit tests, `cargo fmt --check`, and strict Clippy.
- [ ] Commit: `Add automatic shop icon processing`.

### Task 4: Build Super-Admin Product Management Service and Routes

**Files:**
- Modify: `src/shop/mod.rs`, `src/web/shop.rs`, `src/web/mod.rs`, `src/web/views.rs`, `src/app.rs`
- Create: `templates/admin_shop_products.html`, `templates/admin_shop_product_form.html`
- Modify: `templates/base.html`, `static/app.css`
- Test: `tests/shop_flow.rs`, `tests/auth_flow.rs`

**Interfaces:**
- Produces GET `/admin/shop/products`, GET `/admin/shop/products/new`, POST `/admin/shop/products`, GET `/admin/shop/products/{id}/edit`, POST `/admin/shop/products/{id}`, POST `/admin/shop/products/{id}/enable`, POST `/admin/shop/products/{id}/disable`, and POST `/admin/shop/products/{id}/delete`.
- All handlers require `require_super_admin`, use `auth::verify_csrf`, and return existing render/redirect response patterns.

- [ ] Write RED route tests proving anonymous/users/admins receive 401/403, super admins can load the list/form, and all mutation routes reject missing or invalid CSRF.
- [ ] Run the focused route tests and verify they fail because routes/templates are absent.
- [ ] Add Askama view structs for product rows, form values, validation errors, image preview URL, and deletion eligibility. Use separate create/edit forms so ID is readonly on edit.
- [ ] Implement multipart form handling for icon uploads: process bytes in the blocking processor, write a temporary server-named file, execute the product transaction and audit, then rename/commit with cleanup on every error path.
- [ ] Implement create/update/enable/disable/delete service methods. Hard delete only when no `shop_orders` row references the product ID; otherwise render an explanation and offer disable.
- [ ] Ensure edit does not replace the icon when no new file is supplied. New icon filenames must change the URL so browser caches cannot serve the old icon.
- [ ] Add a client-side preview and disabled submit state labelled “正在处理图片”; server-side validation remains authoritative.
- [ ] Add “商品管理” under the existing management dropdown only for super admins; do not create a second top-level admin menu.
- [ ] Run `cargo test admin_shop --test shop_flow`, `cargo test management_navigation_groups_admin_links_by_role --test auth_flow`, formatting, and Clippy.
- [ ] Commit: `Add super-admin shop product management`.

### Task 5: Integrate Database Products and Atomic Global Limits into Purchases

**Files:**
- Modify: `src/shop/mod.rs`, `src/shop/store.rs`, `src/web/shop.rs`, `src/web/views.rs`, `templates/shop.html`
- Test: `tests/shop_flow.rs`

**Interfaces:**
- Changes `shop::purchase` to accept `product_id: &str` instead of a TOML `ShopProduct`; it loads the current product inside the transaction and returns existing `PurchaseOutcome` variants.
- Produces `store::increment_product_sales_if_available` with a conditional update that treats null `total_limit` as unlimited.

- [ ] Write RED tests for public DB product listing, unknown/disabled product rejection, last available sale, sold-out display, total-limit race protection, and rollback of `sold_count` when currency/order/voucher creation fails.
- [ ] Run `cargo test purchase_ --test shop_flow` and verify the new tests fail because purchases still read `state.config.shop.products`.
- [ ] Load enabled products from `store::list_enabled_products` on each `/shop` request; calculate personal active counts from database rows and show both personal-limit and sold-out reasons.
- [ ] In `shop::purchase`, start a transaction, re-read the product, validate enabled/current fields, retain existing user lock/idempotency checks, conditionally increment sales, create the snapshot from the database row, spend currency, create voucher/audit, and commit.
- [ ] Remove `DEFAULT_SHOP_PRODUCTS_FILE`, `products_file`, the in-memory `products` field, `content/shop.toml`, and all TOML catalog loading only after every public/purchase caller uses the database. Confirm `rg -n "products_file|content/shop.toml|shop.products" src config .env.example templates` returns no runtime references.
- [ ] Ensure an unsuccessful conditional sales update returns a sold-out business error before currency is touched. Keep all failure paths transactional.
- [ ] Preserve one-time Token behavior and existing order/voucher snapshots when the product is edited after purchase.
- [ ] Run all shop integration tests, `cargo fmt --check`, and strict Clippy.
- [ ] Commit: `Use database products for atomic purchases`.

### Task 6: Update Public Shop and Product Icon Serving

**Files:**
- Modify: `src/web/shop.rs`, `src/app.rs`, `templates/shop.html`, `templates/base.html`, `static/app.css`
- Test: `tests/shop_flow.rs`

**Interfaces:**
- Product icon route serves only validated names from `ShopConfig.icon_dir`, with immutable caching because each replacement receives a new server-generated filename.
- Public `/shop` remains paginated and anonymous-readable; purchases remain authenticated and CSRF-protected.

- [ ] Write RED tests for immediate product creation/edit visibility after the admin response, icon replacement URL changes, GIF content type, pagination, and disabled products disappearing from public results.
- [ ] Run the focused public shop tests and verify they fail before route integration.
- [ ] Implement secure icon lookup that rejects path components and serves only database-referenced validated storage names; return `Cache-Control: public, max-age=31536000, immutable` for generated names.
- [ ] Keep product cards accessible with alt text, lazy-loaded images, price/validity/stock status, and a disabled purchase button for insufficient balance, personal limit, disabled, or sold-out state.
- [ ] Run `cargo test --test shop_flow`, formatting, and Clippy.
- [ ] Commit: `Refresh public shop from database`.

### Task 7: Release Record, Security Regression, and Final Verification

**Files:**
- Modify: `content/updates.toml`, `tests/shop_flow.rs`, `tests/auth_flow.rs`
- Inspect: `src/shop`, `src/web/shop.rs`, `migrations/0013_create_shop_products.sql`, `config/default.toml`, `.env.example`

**Interfaces:**
- Produces a visible `0.1.5` update record dated `2026-08-14` describing database-managed products, global limits, icon compression/GIF support, and super-admin controls.

- [ ] Add a regression test that edits a product price/description/icon and verifies the public page changes immediately while an existing order retains its original snapshot.
- [ ] Add a regression test that submits a product ID/price mismatch attempt and proves the server uses database price, not form data.
- [ ] Run `cargo test --test shop_flow` and verify all new security regressions pass.
- [ ] Add the descending `0.1.5` update entry with Chinese-commented fields; do not add an entry for this plan-only change.
- [ ] Run portability/config scans:
  `rg -n "INSERT OR|AUTOINCREMENT|datetime\(|strftime\(|julianday\(" migrations/0013_create_shop_products.sql src/shop`
  `rg -n "^\s*[A-Za-z_][A-Za-z0-9_]*\s*=" config/default.toml .env.example`
- [ ] Manually inspect `rg -n "tracing::|info!|warn!|error!|debug!" src/shop src/web/shop.rs` to ensure raw icon bytes, complete Token, form secrets, and untrusted paths are not logged.
- [ ] Run the full verification suite:
  `cargo fmt --check`
  `cargo test`
  `cargo clippy --all-targets --all-features -- -D warnings`
  `git diff --check`
- [ ] Commit: `Finish database shop product management`.
