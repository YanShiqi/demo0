# Task 4 Report: Super-Admin Product Management

## Implementation

- Added super-admin-only product list, create, edit, enable, disable, and delete routes under `/admin/shop/products`.
- Every mutation verifies the session CSRF token before changing product state.
- Added Askama list/form templates, management-dropdown entry, client-side icon preview, and the `正在处理图片` submit state.
- Product icons are processed with `IconProcessor` inside `spawn_blocking`, staged under server-generated temporary names, and cleaned up on validation, rename, or database failures.
- Updates preserve the existing icon when no replacement is uploaded; replacement files receive a new server-generated URL. After a successful replacement, the old file is removed only when no current product or historical order snapshot references it.
- Product lifecycle service methods write audit rows transactionally and reject deletion when an order references the product.
- Delete reads the product snapshot and writes the audit in the same transaction as a portable conditional `DELETE`; after the affected-row check succeeds, the handler cleans up the icon name from that snapshot, avoiding update/delete cleanup races. Generated icon serving accepts only canonical lowercase ULIDs with the expected media extension.
- Expected product form validation errors re-render the form with HTTP 400, preserving submitted values and edit previews.

## Verification

- `cargo test --test shop_flow admin_shop_routes_require_super_admin_and_csrf` — passed.
- `cargo test --all-targets` — passed (47 library, 30 auth-flow, 1 config-flow, 30 shop-flow tests).
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `git diff --check` — passed.
- `cargo test --test shop_flow admin_shop_create_invalid_price_rerenders_form_with_submitted_values` — passed.
- `cargo test --lib web::shop::tests::generated_icon_names_require_canonical_lowercase_ulids` — passed.
- `cargo test --test shop_flow transactional_product_delete_uses_affected_rows_and_preserves_icon_snapshot` — passed.

## Scope

Public database catalog reads and purchase-flow migration remain assigned to Task 5/6.
