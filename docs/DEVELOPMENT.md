# Local Development Guide

Practical guide for setting up and developing the Nova Launch TipJar API locally.

---

## Local Setup

### 1. Prerequisites
- **Rust**: [Installation Guide](https://rustup.rs/) (v1.75+ recommended for Axum 0.7)
- **PostgreSQL**: Local instance running or via Docker.
- **Redis**: Local instance running or via Docker.
- **SQLx CLI**: `cargo install sqlx-cli`

### 2. Environment Configuration
Copy the example environment file and update with your local credentials:
```bash
cp .env.example .env
```

### 3. Database Initialization
Create the database and apply migrations:
```bash
sqlx database create
sqlx migrate run
```

### 4. Running the Application
Start the development server:
```bash
cargo run
```
Access the API at `http://localhost:8000/health`.

### 5. Hot Reloading (Recommended)
Use `cargo-watch` for automatic re-compilation on file changes:
```bash
cargo install cargo-watch
cargo watch -x run
```

---

## Testing

### Running Tests
Execute the full test suite (Requires local Postgres/Redis as per `.env`):
```bash
cargo test
```

### Running Specific Tests
```bash
cargo test middleware::cache
cargo test db::transaction
```

---

## Code Quality

### Linting
We use `clippy` for code linting and style enforcement:
```bash
cargo clippy -- -D warnings
```

### Formatting
We use `rustfmt` to ensure uniform code formatting:
```bash
cargo fmt --all -- --check
```

---

## API Documentation (Swagger/OpenAPI)
When the application is running, the interactive Swagger UI is available at:

[http://localhost:8000/swagger-ui](http://localhost:8000/swagger-ui)

### Updating Documentation
We use `utoipa` macros to derive OpenAPI specs. To add a new endpoint to the documentation:
1. Ensure the handler has an `#[utoipa::path(...)]` attribute.
2. Register the handler in the `ApiDoc` struct within `src/docs/mod.rs`.

---

## Troubleshooting
- **`Database error: role "user" does not exist`**: Ensure your local PostgreSQL has the role specified in `DATABASE_URL`.
- **`Redis error: connection refused`**: Check if Redis is running locally on port 6379.
- **`Compile error: cannot find module...`**: Run `cargo build` to ensure all dependencies are fetched and built.
- **`Auth error: JWT_SECRET not set`**: Set a random string for `JWT_SECRET` in your local `.env`.
