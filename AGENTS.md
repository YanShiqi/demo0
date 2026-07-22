# Repository Guidelines

## Project Structure & Module Organization

This is a Rust 2024 binary crate. Cargo metadata lives in `Cargo.toml` and `Cargo.lock`, and application code starts in `src/main.rs`. As the project grows, move reusable logic to `src/lib.rs` and modules such as `src/config.rs`. Keep unit tests beside their code in `#[cfg(test)]` modules and integration tests in `tests/`. Never edit or commit generated `target/` output.

## Build, Test, and Development Commands

- `cargo run` builds and runs the binary locally.
- `cargo check` performs a fast compile-time validation without producing a final executable.
- `cargo build` creates a debug build under `target/debug/`.
- `cargo test` runs all unit, integration, and documentation tests.
- `cargo fmt --check` verifies formatting; use `cargo fmt` to apply fixes.
- `cargo clippy --all-targets --all-features -- -D warnings` runs the Rust linter and treats warnings as failures.

Run formatting, Clippy, and tests before opening a pull request.

## Coding Style & Naming Conventions

Use rustfmt defaults (four-space indentation) and write idiomatic, warning-free Rust. Name functions, variables, and modules with `snake_case`; types and traits with `UpperCamelCase`; constants with `SCREAMING_SNAKE_CASE`. Prefer small modules, explicit error handling, and meaningful names over comments that restate the code. Because this is a learning project, add concise Chinese comments at key logic, boundary conditions, and non-obvious implementation choices; explain why the code works that way instead of translating each statement. Keep business limits, sizes, timeouts, pagination counts, and similar literals configurable or expressed as named constants; avoid hard-coded numbers in application logic unless the value is a local convention such as `0`, `1`, or an obvious index. Add a concise Chinese comment for every option in committed configuration files such as TOML and `.env.example`, describing its purpose and when to change it. Document public APIs with `///` comments and include examples when behavior is not obvious.

## Testing Guidelines

The project uses Rust's built-in test framework. Name tests after observable behavior, for example `prints_default_greeting`. Cover success paths, edge cases, and expected failures. When fixing a defect, add a regression test that fails without the fix. No coverage threshold is configured, so prioritize useful behavioral coverage over a numeric target.

## Database Conventions

Prefer portable standard SQL in migrations and application queries. Avoid SQLite-specific syntax and functions such as `INSERT OR REPLACE`, `AUTOINCREMENT`, and `datetime('now')` when a portable alternative exists. Isolate unavoidable SQLite configuration, including `PRAGMA` statements, in the database initialization layer and add a concise Chinese comment explaining why it is required.

## Commit & Pull Request Guidelines

The repository has no commit convention yet. Use short, imperative subjects such as `Add argument parsing` and keep commits focused. Pull requests should explain the motivation and behavior, list validation commands, and link relevant issues. Include output or screenshots for user-visible changes and call out compatibility concerns.
