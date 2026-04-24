# Database Migrations

## Overview

Migrations live in `migrations/` and run automatically on startup via SQLx.
Every up migration (`NNNN_name.sql`) has a matching down migration (`NNNN_name.down.sql`).

## Running Migrations

```bash
# Apply all pending migrations
sqlx migrate run

# Revert the most recent migration
sqlx migrate revert

# Check migration status
sqlx migrate info
```

## Validation

Ensure every up migration has a down file:

```bash
bash scripts/validate_migrations.sh
```

## Naming Convention

```
NNNN_description.sql       # up migration
NNNN_description.down.sql  # down migration (reverses the up)
```

Numbers are zero-padded to 4 digits and must be unique per migration name.

## Writing Down Migrations

A down migration must fully reverse its up migration:

| Up operation | Down operation |
|---|---|
| `CREATE TABLE` | `DROP TABLE IF EXISTS` |
| `ALTER TABLE ADD COLUMN` | `ALTER TABLE DROP COLUMN IF EXISTS` |
| `CREATE INDEX` | `DROP INDEX IF EXISTS` |
| `CREATE TYPE` | `DROP TYPE IF EXISTS` |
| `ADD CONSTRAINT` | `DROP CONSTRAINT IF EXISTS` |

Always use `IF EXISTS` / `IF NOT EXISTS` to make migrations idempotent.

## Testing Migrations

```bash
# Apply then immediately revert to verify round-trip
sqlx migrate run && sqlx migrate revert
```
