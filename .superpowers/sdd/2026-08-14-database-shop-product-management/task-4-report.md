# Task 4 Report: Super-Admin Product Management

## Implementation

- Added super-admin-only product list, create, edit, enable, disable, and delete routes under `/admin/shop/products`.
- Every mutation verifies the session CSRF token before changing product state.
- Added Askama list/form templates, management-dropdown entry, client-side icon preview, and the `正在处理图片` submit state.
- Product icons are processed with `IconProcessor` inside `spawn_blocking`, staged under server-generated temporary names, and cleaned up on validation, rename, or database failures.
- Updates preserve the existing icon when no replacement is uploaded; replacement files receive a new server-generated URL. Old icon files are retained for historical order snapshots.
- Product lifecycle service methods write audit rows transactionally and reject deletion when an order references the product.

## Verification

- `cargo test --test shop_flow admin_shop_routes_require_super_admin_and_csrf` — passed.
- `cargo test` — passed (46 unit, 30 auth-flow, 1 config-flow, 28 shop-flow tests).
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `git diff --check` — passed.

## Scope

Public database catalog reads and purchase-flow migration remain assigned to Task 5/6.
