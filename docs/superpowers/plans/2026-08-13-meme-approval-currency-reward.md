# Meme Approval Currency Reward Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reward a Meme provider with a configurable integer currency amount exactly once when an administrator approves a pending Meme.

**Architecture:** Extend `MemeConfig` with a reward switch and amount. Move approval plus reward into one transaction-aware service operation: a conditional pending-to-approved update gates the reward, then the existing currency service records the auditable balance change with a stable idempotency key. The HTTP handler remains responsible for session, role, and CSRF checks.

**Tech Stack:** Rust 2024, Axum, SQLx SQLite, TOML configuration, built-in integration tests.

## Global Constraints

- Reward amounts are positive `i64` integers and come from configuration; no floating point values.
- Only a `pending` to `approved` transition earns a reward.
- Meme status and currency balance/log writes share one transaction and roll back together.
- Reuse the existing currency service and append-only `currency_logs`; do not add a frontend claim endpoint.
- Add concise Chinese comments at transaction and status-transition boundaries.
- Keep SQL parameterized and portable; preserve all existing uncommitted currency and Meme changes.

---

### Task 1: Configuration and failing regression tests

**Files:**
- Modify: `src/config.rs`, `config/default.toml`, `.env.example`, `tests/auth_flow.rs`
- Test: `tests/auth_flow.rs`

- [ ] **Step 1: Add failing tests**

Add an integration test that creates a pending Meme, approves it, and asserts the provider balance increases by the configured reward amount and one `meme_approval_reward` log exists. Add assertions that a second approval does not increase the balance or log count, and that disabling the reward leaves approval working without changing balance.

- [ ] **Step 2: Run the focused test**

Run `cargo test meme_approval_rewards_provider_once --test auth_flow`. It must fail because the configuration field and approval reward flow do not exist.

- [ ] **Step 3: Add typed configuration**

Add `approval_reward_enabled: bool` and `approval_reward_amount: i64` to `MemeConfig` and its file/env sources (`MEME_APPROVAL_REWARD_ENABLED`, `MEME_APPROVAL_REWARD_AMOUNT`). Default to enabled and `2`; reject non-positive amounts. Add Chinese comments for both TOML and `.env.example` entries.

- [ ] **Step 4: Run the focused test again**

Run the same focused test and confirm it now reaches the missing approval integration rather than failing to compile configuration fixtures.

### Task 2: Transactional approval reward service

**Files:**
- Modify: `src/memes.rs`, `src/currency.rs`
- Test: `tests/auth_flow.rs`

- [ ] **Step 1: Add a failing service-level assertion**

Exercise the approval operation with a normal administrator and assert the Meme row becomes approved, the provider balance increases, and the audit row stores the Meme ID as `related_id`.

- [ ] **Step 2: Implement the transaction-aware approval operation**

Add an operation accepting a mutable `Transaction<'_, Sqlite>`, Meme ID, reviewer, and `MemeConfig`. Perform a conditional update with `WHERE id = ? AND status = pending`; return a no-op result when no row is affected. When enabled and the transition succeeds, call the currency service with `CurrencyReason::MemeApprovalReward`, the configured amount, reviewer ID, related Meme ID, stable idempotency key `meme-approval:{meme_id}`, and a concise note.

- [ ] **Step 3: Extend currency reasons and service inputs**

Add the new typed reason and allow reward operations to pass a positive configured amount while preserving super-admin-only checks for manual grant/deduct. Ensure the reward path records the reviewer as `operator_user_id` and never exposes a generic HTTP spend endpoint.

- [ ] **Step 4: Verify the service test**

Run `cargo test meme_approval_rewards_provider_once --test auth_flow`; confirm first approval rewards once, repeated approval is a no-op, and the audit row is present.

### Task 3: HTTP handler integration and rollback coverage

**Files:**
- Modify: `src/web/mod.rs`, `tests/auth_flow.rs`

- [ ] **Step 1: Route the existing approval handler through one transaction**

Keep authentication, administrator authorization, and CSRF verification unchanged. Begin a transaction, call the new approval-and-reward operation, commit it, then redirect to the existing admin Meme page. Do not reward when the operation returns a no-op.

- [ ] **Step 2: Add failure/rollback coverage**

Test that a deleted or already approved Meme cannot produce a reward, and that a disabled reward still approves without a currency log. Use the existing test configuration clone to vary the reward switch and amount.

- [ ] **Step 3: Run focused authorization and regression tests**

Run `cargo test --test auth_flow meme_approval` and the existing Meme authorization tests; all must pass.

### Task 4: Full verification

- [ ] **Step 1: Format and lint**

Run `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings`.

- [ ] **Step 2: Run all tests**

Run `cargo test` and confirm all unit, integration, and documentation tests pass.

- [ ] **Step 3: Inspect the diff**

Run `git diff --check` and `git status --short`; confirm only the planned reward/config/test files were changed in addition to the user’s existing uncommitted work.
