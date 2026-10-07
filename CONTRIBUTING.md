# Contributing to stellar-tipjar-backend

Thank you for taking the time to contribute! The following guidelines will help you get the project running locally and understand the workflow for submitting changes.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
  - [Toolchain Setup & Cargo Configuration](#toolchain-setup--cargo-configuration)
- [Development Workflow](#development-workflow)
- [Coding Guidelines](#coding-guidelines)
- [Adding a New Endpoint](#adding-a-new-endpoint)
- [Database Migrations](#database-migrations)
- [Testing](#testing)
- [Commit Messages](#commit-messages)
- [Pull Request Process](#pull-request-process)
- [Reporting Bugs](#reporting-bugs)
- [Suggesting Features](#suggesting-features)

## Code of Conduct

Please be respectful and constructive in all interactions. We follow the [Contributor Covenant](https://www.contributor-covenant.org/).

## Getting Started

1. **Fork** the repository and clone your fork locally.
2. Make sure you have [Rust](https://rustup.rs/) (edition 2021) and [PostgreSQL](https://www.postgresql.org/) 12+ installed.
3. Copy the example environment file and configure it:

   ```bash
   cp .env.example .env
   # Edit .env with your local PostgreSQL credentials and Stellar network settings
   ```

4. Build the project to confirm everything compiles:

   ```bash
   cargo build
   ```

5. Run the server:

   ```bash
   cargo run
   ```

   The server applies pending migrations automatically on startup and listens on `0.0.0.0:8000` by default.

### Toolchain Setup & Cargo Configuration

Please keep the following toolchain guidelines in mind to avoid cross-platform compilation regressions:

- **Shared Repository Configuration**: Files like `.cargo/config.toml` and `rust-toolchain.toml` in the repository root are shared, tracked files that dictate build and test behavior for **all contributors and automated CI workflows**.
- **No Forced Platform Targets**: Never commit platform-specific build targets (e.g., `[build] target = "x86_64-pc-windows-gnu"`) or host-specific linker paths into the repository's `.cargo/config.toml`. Doing so forces cross-compilation on other contributors and breaks standard `cargo build`, `cargo check`, and `cargo test` runs on Linux/macOS systems and CI runners.
- **Handling Local Platform Quirks**:
  - If your specific local environment (such as Git Bash, MSYS2, or custom MinGW setups on Windows) requires custom linker flags or target overrides, configure them in your user-global Cargo configuration at `$CARGO_HOME/config.toml` (typically `~/.cargo/config.toml` on Unix or `%USERPROFILE%\.cargo\config.toml` on Windows).
  - Alternatively, use environment variables (e.g., `CARGO_BUILD_TARGET`) for temporary local testing. Repository-level Cargo configuration must remain strictly platform-neutral.

## Development Workflow

1. Create a feature branch from `main`:

   ```bash
   git checkout -b feat/my-feature
   ```

2. Make your changes (see the guidelines below).
3. Run the test suite to make sure nothing is broken:

   ```bash
   cargo test
   ```

4. Run `cargo clippy` to catch common mistakes:

   ```bash
   cargo clippy -- -D warnings
   ```

5. Format your code with `rustfmt`:

   ```bash
   cargo fmt
   ```

6. Commit your changes (see [Commit Messages](#commit-messages)) and open a pull request.

## Coding Guidelines

- Follow standard Rust idioms and the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).
- Keep functions small and focused; delegate database work to the `controllers/` layer.
- Use `anyhow::Result` for fallible functions that call external services or the database.
- Use `thiserror` for domain-specific error types.
- Log with the `tracing` macros (`tracing::info!`, `tracing::error!`, etc.) rather than `println!`.
- Do not commit secrets or `.env` files. The `.gitignore` already excludes `.env`.

## Adding a New Endpoint

The project follows a layered architecture:

```
Routes → Controllers → Services → Database
```

1. **Model** – Add any new structs (request body, response body, DB row) to `src/models/`.
2. **Controller** – Add the database query or business logic function to `src/controllers/`.
3. **Route** – Wire the handler to an HTTP path in `src/routes/` and call the controller.
4. **Register** – Mount the new router in `src/main.rs` if you created a new route file.
5. **Migration** – If the feature requires a schema change, add a migration (see below).
6. **Tests** – Add at least one test that exercises the happy path.

## Database Migrations

Migrations are plain SQL files in the `migrations/` directory and run automatically on server startup.

Install the SQLx CLI if you need to manage them manually:

```bash
cargo install sqlx-cli
```

Common commands:

```bash
# Create a new migration pair (up + down)
sqlx migrate add -r <migration_name>

# Apply pending migrations
sqlx migrate run

# Revert the most recent migration
sqlx migrate revert
```

Name migrations sequentially (e.g. `0003_add_index_to_tips`) so they apply in the correct order.

## Testing

Tests use [`axum-test`](https://crates.io/crates/axum-test) to exercise handlers in-process.

```bash
# Run all tests
cargo test

# Include stdout/stderr output
cargo test -- --nocapture

# Run a specific test by name
cargo test <test_name>
```

Every new feature or bug fix should be accompanied by a test that demonstrates the expected behaviour.

## Commit Messages

Use the [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>(<optional scope>): <short description>

[optional body]

[optional footer]
```

Common types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`.

Examples:

```
feat(tips): add pagination to GET /creators/:username/tips
fix(stellar): handle timeout errors from Horizon API
docs: update API reference in README
```

## Pull Request Process

1. Ensure `cargo test`, `cargo clippy`, and `cargo fmt --check` all pass.
2. Give your PR a descriptive title following the Conventional Commits format.
3. Fill in the PR description with a summary of what changed and why.
4. Link any related issues (e.g. `Closes #42`).
5. Request a review from a maintainer.
6. Address review comments and push additional commits to the same branch.

A maintainer will merge the PR once it is approved.

## Reporting Bugs

Open a [GitHub issue](../../issues) and include:

- A clear, descriptive title.
- Steps to reproduce the problem.
- Expected behaviour vs. actual behaviour.
- Relevant logs, error messages, or screenshots.
- Your environment (OS, Rust version, PostgreSQL version).

## Suggesting Features

Open a [GitHub issue](../../issues) with the label `enhancement` and describe:

- The problem you want to solve.
- Your proposed solution or API changes.
- Any alternatives you considered.
