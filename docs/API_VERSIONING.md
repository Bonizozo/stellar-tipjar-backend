# API Versioning

All API endpoints are versioned via URL prefix. Two versions are currently active.

| Version | Base URL | Status |
|---|---|---|
| v1 | `/api/v1` | Deprecated — sunset 2027-01-01 |
| v2 | `/api/v2` | Current (stable) |

## Version Headers

Every response includes:

```
X-API-Version: v1   (or v2)
```

v1 responses additionally include:

```
Deprecation: true
Sunset: Sat, 01 Jan 2027 00:00:00 GMT
Link: <https://docs.example.com/migration/v1-to-v2>; rel="deprecation"
```

## Response Shape Differences

### Creators

| Field | v1 | v2 |
|---|---|---|
| `id` | ✓ | ✓ |
| `username` | ✓ | ✓ |
| `wallet_address` | ✓ | ✓ |
| `email` | — | ✓ |
| `created_at` | — | ✓ |

### Tips

| Field | v1 | v2 |
|---|---|---|
| `id` | ✓ | ✓ |
| `creator_username` | ✓ | ✓ |
| `amount` | ✓ | ✓ |
| `transaction_hash` | ✓ | ✓ |
| `message` | — | ✓ |
| `created_at` | — | ✓ |

### Tip listing

- **v1** `/creators/:username/tips` — returns a flat `[]` array (first page, 20 items).
- **v2** `/creators/:username/tips` — returns a paginated envelope with `data`, `total`, `page`, `limit`, `total_pages`, `has_next`, `has_prev`. Supports `?page=`, `?limit=`, filter, and sort query params.

## Sunset Policy

- v1 will receive **security patches only** until the sunset date.
- No new features will be added to v1.
- v1 will be **removed** on **2027-01-01**.

## Migration Guide: v1 → v2

### 1. Update base URL

```diff
- https://api.example.com/api/v1/creators
+ https://api.example.com/api/v2/creators
```

### 2. Handle new fields

v2 creator responses include `email` and `created_at`. These are additive — existing code that ignores unknown fields requires no changes.

### 3. Update tip listing consumers

v2 wraps tip lists in a pagination envelope:

```json
{
  "data": [ ... ],
  "total": 42,
  "page": 1,
  "limit": 20,
  "total_pages": 3,
  "has_next": true,
  "has_prev": false
}
```

Update any code that expects a bare array:

```diff
- const tips = response;
+ const tips = response.data;
```

### 4. Use pagination params

```
GET /api/v2/creators/alice/tips?page=2&limit=50
```

### 5. Tip message field

v2 tip responses include an optional `message` field. Handle it as nullable.
