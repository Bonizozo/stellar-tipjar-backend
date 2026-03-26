# Backend Architecture

Detailed overview of the Nova Launch TipJar API technical design.

---

## Technical Stack

- **Language**: [Rust](https://www.rust-lang.org/) (Edition 2021) 
- **Web Framework**: [Axum](https://github.com/tokio-rs/axum) (High-performance, async-first)
- **Database Driver**: [SQLx](https://github.com/launchbadge/sqlx) (Type-safe async SQL)
- **Runtime**: [Tokio](https://tokio.rs/) (Event-driven async runtime)
- **Cache**: Redis via [redis-rs](https://github.com/redis-rs/redis-rs)
- **API Documentation**: [utoipa](https://github.com/juhaku/utoipa) (Swagger/OpenAPI-compliant)

---

## Project Structure

```text
src/
├── cache/          # Redis client and key management
├── controllers/    # Business logic (DB & Cache coordination)
├── db/             # Persistence Layer
│   ├── connection  # Pool initialization
│   ├── health      # DB health checks
│   ├── query_logger# Audit logging for queries
│   └── transaction # ACID transaction helpers
├── middleware/     # Request pipeline extensions (Cache, Limiter)
├── models/         # Data structures (Requests, SQL Rows, Responses)
├── routes/         # Endpoint definitions and handlers
├── search/         # Fuzzy search and Postgres Trigram logic
├── services/       # External integrations (Stellar, Retry, Circuit Breaker)
├── webhooks/       # Notification delivery system
└── main.rs         # Application entry point and layer configuration
```

---

## Request Lifecycle

1. **Routing**: `src/routes/` maps incoming HTTP paths to specific handlers.
2. **Middleware**:
    - **Tracing**: Request logging (via `tower-http`).
    - **Timeout**: Enforces a 5-second request limit.
    - **Rate Limiter**: Applies IP-based quotas.
    - **Cache**: Checks for pre-computed GET responses.
3. **Controller**: `src/controllers/` combines database updates and cache invalidation logic.
4. **Database**: `SQLx` executes queries against PostgreSQL.
5. **Background Task**: Operations like Webhook delivery are spawned as non-blocking `tokio::spawn` tasks.
6. **Response**: Result is serialized back to JSON and returned to the client.

---

## Error Handling Pattern

The application uses the `anyhow` library for general error propagation and `thiserror` for domain-specific errors. Handlers convert these results into appropriate HTTP status codes and standardized error JSON bodies.

---

## Testing Strategy

- **Unit Tests**: Critical logic (Signature, Search parsing, Date logic) in the respective `mod.rs` files.
- **Integration Tests**: End-to-end API verification in `src/middleware/cache.rs` and `src/db/transaction.rs` using `axum-test`.
- **Database Tests**: Run against a local PostgreSQL instance during CI using `dotenvy` for configuration.
- **Mocking**: External services like Stellar Horizon are mocked or bypassed during internal logic testing.
