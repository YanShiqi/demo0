# Server Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a file-backed server update log shown in a public homepage section and a full `/updates` page.

**Architecture:** Parse and validate `content/updates.toml` during configuration loading, sort entries by ISO date descending, and keep the immutable entries in `Config`. Server-rendered handlers reuse the same entries: the homepage takes a configured preview slice, while `/updates` renders all entries.

**Tech Stack:** Rust 2024, Axum, Askama, serde/toml, time, built-in tests.

## Global Constraints

- Add concise Chinese comments at key logic and non-obvious boundaries.
- Keep configuration options commented in Chinese and expose preview limits as configuration.
- Avoid SQLite changes and SQLite-specific SQL.
- Keep literal limits in named constants or TOML configuration.

---

### Task 1: Add validated update data and configuration

**Files:**
- Create: `src/updates.rs`
- Create: `content/updates.toml`
- Modify: `src/config.rs`
- Modify: `src/lib.rs`
- Modify: `config/default.toml`
- Modify: `.env.example`
- Test: `src/updates.rs` unit tests

**Interfaces:**
- Produce `updates::UpdateEntry` with public `date`, `version`, `title`, `summary`, and `changes` fields.
- Produce `updates::load_file(path: &Path) -> anyhow::Result<Vec<UpdateEntry>>`.
- Add `Config::updates: UpdateConfig` with `file`, `home_preview_limit`, and loaded `entries`.

- [ ] **Step 1: Write failing parser tests** for valid sorting, invalid date, missing title, and missing file error.
- [ ] **Step 2: Run `cargo test updates`** and confirm failure because the module and loader are absent.
- [ ] **Step 3: Implement TOML parsing, required-field validation, ISO date parsing, and descending sort.** Add Chinese comments for file loading and validation boundaries.
- [ ] **Step 4: Add `[updates]` config resolution** with `UPDATES_FILE` and `UPDATES_HOME_PREVIEW_LIMIT`, positive-limit validation, and default values; load the file during `Config::from_env`.
- [ ] **Step 5: Add a sample `content/updates.toml`** with Chinese comments and update `config/default.toml`/`.env.example` comments.
- [ ] **Step 6: Run `cargo test updates` and `cargo fmt --check`**; expect parser tests to pass.

### Task 2: Render homepage preview and full updates page

**Files:**
- Modify: `src/web/views.rs`
- Modify: `src/web/mod.rs`
- Modify: `src/app.rs`
- Create: `templates/updates.html`
- Modify: `templates/home.html`
- Modify: `static/app.css`
- Test: `tests/auth_flow.rs`

**Interfaces:**
- Add `UpdateView`, `HomeTemplate` update fields, and `UpdatesTemplate`.
- Add `GET /updates` handler `web::updates_page`.

- [ ] **Step 1: Add failing integration coverage** for anonymous homepage visibility, preview limit, and complete `/updates` output.
- [ ] **Step 2: Run the focused integration test** and confirm the expected missing-section/404 failures.
- [ ] **Step 3: Map update entries to views and add `updates_page`**, reusing `PageContext` and the existing `render` helper.
- [ ] **Step 4: Register `/updates` and render a public homepage section** parallel to the authenticated “首页动态” section, with a “查看全部” link and empty state.
- [ ] **Step 5: Add compact timeline/card styles** that work on desktop and narrow screens without JavaScript.
- [ ] **Step 6: Run the focused integration test** and verify anonymous users see only the configured number on `/` and all entries on `/updates`.

### Task 3: Full verification and cleanup

**Files:**
- Modify: `tests/auth_flow.rs` if edge-case assertions need tightening.

- [ ] **Step 1: Run `cargo fmt --check`.**
- [ ] **Step 2: Run `cargo test`.**
- [ ] **Step 3: Run `cargo clippy --all-targets --all-features -- -D warnings`.**
- [ ] **Step 4: Run `git diff --check` and inspect the final diff**, ensuring the pre-existing `templates/home.html` changes remain intact.
