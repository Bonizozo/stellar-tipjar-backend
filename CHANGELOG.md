# Changelog

All notable changes to this project are documented here.

## [Unreleased]

### Fixed

- **Database Migrations** — Renumbered duplicate migration version numbers (many migration files independently reused the same version prefix, e.g. two `0003_*` and four `0029_*` files), which made `sqlx::migrate!` panic on a fresh database with a duplicate-primary-key error. Also fixed several migrations that referenced columns/tables never created (`creators.display_name`, `creators.bio`, `tips.creator_id`), a `CREATE INDEX` with an invalid subquery predicate, a stray `git` typo that broke a migration's SQL, and a table name collision between two unrelated `audit_logs` tables (renamed the generic one to `event_audit_logs`). `compose.yml` / `docker-compose.services.yml` now use a PostGIS-enabled Postgres image so the creator-locations migration can run. Closes #307

### Added

- **Rate Limiting and Request Throttling** — IP-based rate limiting via `tower_governor` with separate limits for read/write endpoints, IP whitelist, and JSON 429 responses with `Retry-After` header. Closes #107
- **Input Validation and Sanitization** — `ValidatedJson<T>` extractor with `validator`-based field validation on all request DTOs; Stellar address, transaction hash, amount, username, and email validation. Closes #108
- **Comprehensive Logging and Monitoring** — Structured tracing via `tracing-subscriber`, OpenTelemetry distributed tracing with OTLP export, Prometheus metrics endpoint at `/metrics`, and per-request trace middleware. Closes #109
- **Caching Layer with Redis** — Redis-backed cache with TTL support, cache-control middleware, and connection pooling. Closes #110
- **JWT Authentication and Authorization** — JWT-based auth middleware, role-based access control, and admin route protection. Closes #105
- **GraphQL API Endpoint** — Full GraphQL schema with queries, mutations, and WebSocket subscriptions via `async-graphql` at `/graphql`. Closes #123
- **API Versioning Strategy** — Versioned routes under `/api/v1` and `/api/v2` with deprecation notice middleware on v1. Closes #122
- **Database Transaction Management** — SQLx transaction support across multi-step operations. Closes #121
- **Request Signing and Verification** — HMAC-SHA256 request signature verification middleware. Closes #120
- **Real-time Notification System** — WebSocket broadcast channel for real-time tip notifications at `/ws`. Closes #132
- **Redis Caching Strategy** — Layered caching strategy with cache invalidation and TTL configuration per resource type. Closes #131
- **Database Query Optimization and Indexing** — Query performance monitoring, connection pool tuning, and indexed columns on hot query paths. Closes #130
- **Advanced Error Handling and Recovery** — Unified `AppError` type with structured JSON error responses and error recovery middleware. Closes #129
- **Complex Business Logic for Tip Validation** — On-chain Stellar transaction verification before persisting tips, duplicate hash detection, and saga-based rollback. Closes #128
- **SQL Injection Prevention and Parameterized Queries** — All database queries use SQLx parameterized bindings; no raw string interpolation. Closes #125
- **Enhanced CORS Configuration** — Per-method and per-origin CORS policy with configurable allowed origins. Closes #126
- **Admin API with Role-Based Access Control** — Admin-only routes under `/api/v*/admin` protected by role middleware. Closes #134
- **Analytics and Reporting** — Real-time analytics stream processor and aggregation pipeline for tip metrics. Closes #133
- **Audit Logging System** — Structured audit log entries for all mutating operations. Closes #135
- **Advanced Search with Full-Text Indexing** — Full-text search support for creators and tips. Closes #137
- **Email Notification System** — Async email worker with template rendering via `tera` and SMTP transport via `lettre`. Closes #139
- **Scheduled Tasks and Cron Jobs** — Background job scheduler with configurable cron expressions. Closes #140
- **Secrets Management** — Environment-based secrets configuration with validation at startup. Closes #127
- **Automated Database Backup** — Scheduled database export job. Closes #124
- **Data Export and Backup System** — CSV export endpoint for tip history. Closes #136
- **File Upload Service** — Creator media upload handling. Closes #138
