# Currency System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an integer-only currency system with auditable adjustments, internal spending support, user/admin pages, and a profile balance summary.

**Architecture:** Add `currency_balance` to users and append-only `currency_logs`. A `currency` module owns validation, atomic balance changes, idempotency, and transaction boundaries. Configuration supplies the display name (“洲币”), symbol, limits, and pagination values; handlers and templates consume typed views.

**Tech Stack:** Rust 2024, Axum, Askama, SQLx SQLite migrations, TOML configuration, built-in unit/integration tests.

## Global Constraints

- All amounts are positive/negative `i64` integers; never use floats.
- All balance changes go through the currency service and an explicit database transaction.
- Admin permissions are checked server-side; ordinary admins are read-only, super admins adjust balances.
- Every committed configuration option has a concise Chinese comment.
- Add concise Chinese comments at key logic and concurrency/transaction boundaries.
- Preserve existing uncommitted update-log and admin-navigation changes.

---

### Task 1: Schema, model, and configuration

**Files:** Create `migrations/0009_create_currency.sql`; modify `src/model.rs`, `src/config.rs`, `src/lib.rs`, `config/default.toml`, `.env.example`; test in `tests/auth_flow.rs`.

- [ ] Write a failing test that new users expose `currency_balance = 0` and configured currency text appears in `/profile`.
- [ ] Run the focused test and observe the missing database column/config/template failure.
- [ ] Add the portable migration for `users.currency_balance` and `currency_logs` with indexes and constraints.
- [ ] Add `CurrencyConfig` with name, symbol, page size, adjustment limit, search limit, and note limit; load it from TOML/env with positive-limit validation.
- [ ] Add `currency_balance: i64` to `User` and test configuration fixtures.
- [ ] Run the focused test and `cargo fmt --check`.

### Task 2: Currency domain service

**Files:** Create `src/currency.rs`; modify `src/lib.rs`; test in `src/currency.rs` and `tests/auth_flow.rs`.

- [ ] Add failing tests for grant, deduct, insufficient balance, negative/zero amounts, audit rows, and idempotent spend.
- [ ] Implement typed reasons and `CurrencyChange`/log rows.
- [ ] Implement `grant_currency`, `deduct_currency`, and `spend_currency` using transaction-local atomic updates, checked arithmetic, and unique idempotency keys.
- [ ] Ensure a failed log insert rolls back the balance and duplicate idempotency returns a stable business error.
- [ ] Run domain tests and inspect SQL for portable parameterized statements.

### Task 3: User currency page and profile summary

**Files:** Modify `src/web/views.rs`, `src/web/mod.rs`, `src/app.rs`, `templates/profile.html`, `static/app.css`; create `templates/currency.html`; test in `tests/auth_flow.rs`.

- [ ] Add failing coverage for `/currency`, own-user access, paginated logs, and the profile balance summary using configured name/symbol.
- [ ] Implement authenticated `/currency` with server-side pagination and friendly timestamps.
- [ ] Add profile fields for balance, symbol, and configured display name in the existing profile header.
- [ ] Add route and compact balance/log styles; do not add a generic spend button.
- [ ] Run focused page tests.

### Task 4: Admin read-only query and super-admin adjustments

**Files:** Modify `src/web/mod.rs`, `src/app.rs`, `templates/base.html`, `static/app.css`; create `templates/admin_currency.html`; test in `tests/auth_flow.rs`.

- [ ] Add failing coverage proving ordinary admins can view but cannot modify, while super admins can grant/deduct with required note and CSRF.
- [ ] Implement `/admin/currency` user search and balance/log display with configured result limits.
- [ ] Add POST handlers for grant and deduct, validating positive integer amounts, limits, notes, target user, and role.
- [ ] Add “货币管理” to the existing management dropdown only for admins; expose adjustment forms only to super admins.
- [ ] Run focused authorization and audit tests.

### Task 5: Full verification

- [ ] Run `cargo fmt --check`.
- [ ] Run `cargo test`.
- [ ] Run `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] Run `git diff --check` and inspect status to confirm no unrelated files were overwritten.
