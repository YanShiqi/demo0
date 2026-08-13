# Weekly Check-In Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a once-per-week check-in that awards configurable integer currency and appears beside the logged-in home feed heading.

**Architecture:** Add typed check-in configuration and a `weekly_check_ins` table with a unique `(user_id, week_start)` constraint. A `check_in` module computes the configured local week and performs the check-in, currency grant, and audit log in one transaction. The home handler exposes only read-only status while a CSRF-protected POST endpoint performs the action.

**Tech Stack:** Rust 2024, Axum, SQLx SQLite migrations, Askama templates, TOML configuration, built-in tests.

## Global Constraints

- The week starts Monday 00:00 in `DISPLAY_UTC_OFFSET_HOURS`; timestamps remain UTC in storage.
- Each user can succeed once per `week_start`; duplicate and concurrent requests must not award twice.
- Check-in reward is a positive integer from configuration and all balance changes use the existing currency service.
- Check-in row, currency balance, and currency log commit or roll back together.
- Add concise Chinese comments at time-zone and transaction boundaries; keep SQL parameterized and portable.
- Add a dated `content/updates.toml` entry because this is a new user-facing feature, configuration group, migration, and currency behavior.

---

### Task 1: Configuration, schema, and time-period helper

**Files:**
- Create: `migrations/0011_create_weekly_check_ins.sql`, `src/check_in.rs`
- Modify: `src/config.rs`, `src/lib.rs`, `config/default.toml`, `.env.example`
- Test: `src/check_in.rs`

- [x] **Step 1: Write failing period tests**

Add tests for `week_start_for` using fixed UTC instants: a Sunday returns the preceding Monday, Monday midnight returns that Monday, and a year boundary returns the correct local Monday. The helper must accept an explicit `UtcOffset` and `OffsetDateTime`.

- [x] **Step 2: Run the period tests and observe failure**

Run `cargo test week_start_for --lib`; expect failure because the helper and module do not exist.

- [x] **Step 3: Add `CheckInConfig` and migration**

Add `enabled: bool` and `reward_amount: i64` to `Config`, load TOML/env values (`CHECK_IN_ENABLED`, `CHECK_IN_REWARD_AMOUNT`), validate a positive amount, and add Chinese comments to committed config examples. Create `weekly_check_ins` with ULID primary key, user foreign key, `week_start`, `reward_amount`, UTC `created_at`, and `UNIQUE(user_id, week_start)`.

- [x] **Step 4: Implement and verify the pure period helper**

Implement `week_start_for(now_utc, offset)` using `time` APIs and return the local Monday date as `YYYY-MM-DD`. Run `cargo test week_start_for --lib` and `cargo fmt --check`.

### Task 2: Transactional check-in domain service

**Files:**
- Modify: `src/check_in.rs`, `src/currency.rs`
- Test: `tests/auth_flow.rs`

- [x] **Step 1: Write a failing check-in integration test**

Create a user and test configuration, call the check-in service with a fixed week, then assert one check-in row, one `weekly_check_in` currency log, and a balance increase equal to the configured amount. Call it again for the same week and assert no second reward.

- [x] **Step 2: Run the focused test and verify the expected failure**

Run `cargo test weekly_check_in_awards_currency_once --test auth_flow`; expect failure because the service and reason do not exist.

- [x] **Step 3: Extend currency reasons and implement the service**

Add `CurrencyReason::WeeklyCheckIn` and a `weekly_check_in` currency grant path that accepts the user ID, check-in ID, amount, and stable idempotency key. Implement `check_in::perform` with a mutable SQLite transaction: insert the unique check-in row, grant currency, and return an enum distinguishing `Awarded` from `AlreadyCheckedIn`.

- [x] **Step 4: Verify duplicate safety and rollback behavior**

Run the focused test. Add assertions that the duplicate call leaves balance and log count unchanged and that a failed currency write does not leave a check-in row.

### Task 3: Home status card and POST endpoint

**Files:**
- Modify: `src/app.rs`, `src/web/mod.rs`, `src/web/views.rs`, `templates/home.html`, `static/app.css`
- Test: `tests/auth_flow.rs`

- [x] **Step 1: Add failing page and authorization assertions**

Assert an authenticated home page shows an unchecked card with the configured reward and CSRF form, a checked-in user sees the completed state without a submit button, anonymous home does not show the card, and a POST without a valid session or CSRF is rejected.

- [x] **Step 2: Add route and view state**

Register `POST /check-in`. Extend `HomeTemplate` with `check_in_enabled`, `check_in_completed`, `check_in_reward_amount`, `check_in_currency_name`, `check_in_currency_symbol`, and an optional result message. The home handler calculates the current week and queries status without mutating data.

- [x] **Step 3: Implement the endpoint**

Require a session and user, verify CSRF, reject disabled configuration, begin a transaction, call `check_in::perform`, commit, and redirect to `/?check_in=success` or `/?check_in=already`. Never accept user ID, week, or amount from the form.

- [x] **Step 4: Add responsive heading styles and verify focused tests**

Place the compact card in the existing `preview-heading` beside “首页动态”; use a flex layout that wraps below the heading on narrow screens. Run `cargo test home_check_in --test auth_flow`.

### Task 4: Update record and full verification

**Files:**
- Modify: `content/updates.toml`
- Test: all existing tests

- [x] **Step 1: Add the server update entry**

Append a dated entry describing weekly check-in, Monday reset, and one-currency reward. Keep the existing update ordering convention.

- [x] **Step 2: Run complete verification**

Run `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, and `git diff --check`.

- [x] **Step 3: Inspect scope**

Run `git status --short` and confirm the check-in files plus the already-present Meme reward changes are preserved without unrelated edits.
