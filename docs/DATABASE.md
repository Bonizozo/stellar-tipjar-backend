# Database Schema and Migrations

Detailed documentation of the Nova Launch TipJar PostgreSQL schema.

---

## Schema Overview

The database uses PostgreSQL for persistent data storage, leveraging `gen_random_uuid()` for primary keys and `TIMESTAMPTZ` for all temporal fields.

---

## Tables

### `creators`
Stores basic creator profile information.

| Column | Type | Description |
| :--- | :--- | :--- |
| **`id`** | `UUID` | Primary Key. |
| **`username`** | `TEXT` | Unique handle (Indexed). |
| **`wallet_address`** | `TEXT` | Public Stellar Address. |
| **`created_at`** | `TIMESTAMPTZ` | Registration time. |

---

### `tips`
Records all tip transactions verified on the platform.

| Column | Type | Description |
| :--- | :--- | :--- |
| **`id`** | `UUID` | Primary Key. |
| **`creator_username`** | `TEXT` | FK to `creators.username` (Indexed). |
| **`amount`** | `TEXT` | Decimal string (e.g., "10.00"). |
| **`transaction_hash`** | `TEXT` | Unique hash on Stellar network. |
| **`created_at`** | `TIMESTAMPTZ` | Record creation time. |

---

### `webhooks`
Stores external integrations for push notifications.

| Column | Type | Description |
| :--- | :--- | :--- |
| **`id`** | `UUID` | Primary Key. |
| **`url`** | `TEXT` | Target HTTP POST URL. |
| **`secret`** | `TEXT` | HMAC signing secret. |
| **`enabled`** | `BOOLEAN` | Active status (Indexed). |
| **`events`** | `TEXT[]` | Filtered events (e.g., `["tip.recorded"]`). |

---

### `tip_logs` (Audit)
Internal audit log for background processes and verification.

| Column | Type | Description |
| :--- | :--- | :--- |
| **`id`** | `SERIAL` | Auto-increment ID. |
| **`tip_id`** | `UUID` | FK to `tips.id`. |
| **`creator_username`** | `TEXT` | FK to `creators.username`. |
| **`action`** | `TEXT` | Audit action (e.g., "recorded_atomic"). |
| **`logged_at`** | `TIMESTAMPTZ` | Log time. |

---

## Indexes

- **B-Tree**: On `username`, `creator_username`, `transaction_hash`, `enabled`.
- **Trigram (GIST)**: Supporting fuzzy full-text search on `creators.username`.
- **GIN**: Supporting array searches on `webhooks.events`.

---

## Migrations

The project uses SQL-based migrations managed by `sqlx-cli`.

### Creating a Migration
```bash
sqlx migrate add <description>
```

### Running Migrations
```bash
sqlx migrate run
```

### Rollbacks (Manual)
Migrations should be developed to be idempotent where possible. Rollbacks are performed by creating a compensating migration or using standard SQL commands in the development environment.
