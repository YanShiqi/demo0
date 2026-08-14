# Task 4 Report: Super-Admin Product Management

## Implementation

- Added super-admin-only product list, create, edit, enable, disable, and delete routes under `/admin/shop/products`.
- Every mutation verifies the session CSRF token before changing product state.
- Added Askama list/form templates, management-dropdown entry, client-side icon preview, and the `正在处理图片` submit state.
- Product icons are processed with `IconProcessor` inside `spawn_blocking`, staged under server-generated temporary names, and cleaned up on validation, rename, or database failures.
- Updates preserve the existing icon when no replacement is uploaded; replacement files receive a new server-generated URL. After a successful replacement, the old file is removed only when no current product or historical order snapshot references it.
- Product lifecycle service methods write audit rows transactionally and reject deletion when an order references the product.
- Delete snapshots and removes the icon name returned by the same database transaction, avoiding update/delete cleanup races. Generated icon serving accepts only canonical lowercase ULIDs with the expected media extension.
- Expected product form validation errors re-render the form with HTTP 400, preserving submitted values and edit previews.

## Verification

- `cargo test --test shop_flow admin_shop_routes_require_super_admin_and_csrf` — passed.
- `cargo test` — passed (46 unit, 30 auth-flow, 1 config-flow, 28 shop-flow tests).
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `git diff --check` — passed.
- `cargo test --test shop_flow admin_shop_create_invalid_price_rerenders_form_with_submitted_values` — passed.
- `cargo test --lib web::shop::tests::generated_icon_names_require_canonical_lowercase_ulids` — passed.

## Scope

Public database catalog reads and purchase-flow migration remain assigned to Task 5/6.
