# stellar-tipjar-backend

A REST API backend for a Stellar-based tip jar. Creators register with a username and Stellar wallet address, and supporters send tips by submitting verified on-chain Stellar transactions.

## Table of Contents

- [Features](#features)
- [Prerequisites](#prerequisites)
- [Getting Started](#getting-started)
- [Configuration](#configuration)
- [Running the Server](#running-the-server)
- [API Reference](#api-reference)
- [Database Migrations](#database-migrations)
- [Testing](#testing)
- [Project Structure](#project-structure)
- [License](#license)

## Features

- Register creators with a username and Stellar wallet address
- Record tips by verifying Stellar transactions on-chain (via Horizon API) before persisting them
- Retrieve creator details and tip history
- Automatic database migrations on startup
- CORS enabled for all origins

## Prerequisites

- [Rust](https://rustup.rs/) (edition 2021)
- [PostgreSQL](https://www.postgresql.org/) 12 or later
- A Stellar account and access to the Stellar Horizon API (testnet or mainnet)

## Getting Started

```bash
# Clone the repository
git clone https://github.com/Bonizozo/stellar-tipjar-backend.git
cd stellar-tipjar-backend

# Copy the example environment file and fill in your values
cp .env.example .env

# Build the project
cargo build
```

## Configuration

All configuration is provided through environment variables. Copy `.env.example` to `.env` and set each value:

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | Yes | — | PostgreSQL connection string, e.g. `postgres://user:password@localhost/tipjar` |
| `STELLAR_NETWORK` | No | `testnet` | Stellar network to use: `testnet` or `mainnet` |
| `STELLAR_RPC_URL` | No | `https://soroban-testnet.stellar.org` | Soroban RPC endpoint |
| `PORT` | No | `8000` | Port the HTTP server listens on |

Logging verbosity is controlled by the `RUST_LOG` environment variable (defaults to `stellar_tipjar_backend=debug,tower_http=debug`).

## Running the Server

```bash
# Development
cargo run

# Production (optimised binary)
cargo build --release
./target/release/stellar-tipjar-backend
```

The server will apply any pending database migrations automatically on startup and then begin listening on `0.0.0.0:<PORT>`.

## API Reference

### Creators

#### Register a creator

```
POST /creators
```

**Request body**

```json
{
  "username": "alice",
  "wallet_address": "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN"
}
```

**Response** `201 Created`

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "username": "alice",
  "wallet_address": "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN",
  "created_at": "2024-03-14T10:30:00Z"
}
```

#### Get a creator

```
GET /creators/:username
```

**Response** `200 OK` or `404 Not Found`

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "username": "alice",
  "wallet_address": "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN",
  "created_at": "2024-03-14T10:30:00Z"
}
```

#### List tips for a creator

```
GET /creators/:username/tips
```

**Response** `200 OK`

```json
[
  {
    "id": "660e8400-e29b-41d4-a716-446655440001",
    "creator_username": "alice",
    "amount": "10.5",
    "transaction_hash": "abc123...",
    "created_at": "2024-03-14T11:00:00Z"
  }
]
```

### Tips

#### Record a tip

The transaction is verified on the Stellar network before the tip is saved.

```
POST /tips
```

**Request body**

```json
{
  "username": "alice",
  "amount": "10.5",
  "transaction_hash": "abc123def456..."
}
```

**Response** `201 Created`

```json
{
  "id": "660e8400-e29b-41d4-a716-446655440001",
  "creator_username": "alice",
  "amount": "10.5",
  "transaction_hash": "abc123def456...",
  "created_at": "2024-03-14T11:00:00Z"
}
```

**Error responses**

| Status | Meaning |
|---|---|
| `422 Unprocessable Entity` | Transaction not found or unsuccessful on the Stellar network |
| `502 Bad Gateway` | Could not reach the Stellar network to verify the transaction |
| `500 Internal Server Error` | Unexpected server-side error |

## Database Migrations

Migrations live in the `migrations/` directory and run automatically at startup via SQLx.

To manage migrations manually, install the SQLx CLI:

```bash
cargo install sqlx-cli

# Create a new migration
sqlx migrate add -r <migration_name>

# Run pending migrations
sqlx migrate run

# Revert the last migration
sqlx migrate revert
```

## Testing

```bash
# Run the full test suite
cargo test

# Show println!/dbg! output
cargo test -- --nocapture
```

Tests use the [`axum-test`](https://crates.io/crates/axum-test) crate to exercise handlers in-process.

## Project Structure

```
src/
├── main.rs                  # Server bootstrap (env, DB pool, router, CORS)
├── controllers/
│   ├── creator_controller.rs  # Creator CRUD queries
│   └── tip_controller.rs      # Tip insert and list queries
├── db/
│   └── connection.rs          # AppState (DB pool + StellarService)
├── models/
│   ├── creator.rs             # Creator entity, request/response DTOs
│   └── tip.rs                 # Tip entity, request/response DTOs
├── routes/
│   ├── creators.rs            # /creators endpoints
│   └── tips.rs                # /tips endpoints
└── services/
    ├── stellar_service.rs     # Horizon API transaction verification
    └── tip_service.rs         # Tip business logic
migrations/
├── 0001_create_creators.sql
└── 0002_create_tips.sql
```

## License

This project is licensed under the [MIT License](LICENSE).