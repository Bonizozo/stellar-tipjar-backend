# API Reference

Complete reference for the Nova Launch TipJar API.

## Base URL
```
Production: https://api.tipjar.stellar.org
Testnet: https://testnet-api.tipjar.stellar.org
Local: http://localhost:8000
```

## Authentication
All authenticated endpoints require a JWT token in the Authorization header (currently used for admin operations):
```
Authorization: Bearer <token>
```
Public endpoints (creators, searching, tips) do not require authentication but are subject to rate limiting.

---

## Endpoints

### Creators

#### Create Creator
Registers a new creator profile on the platform.

```http
POST /creators
Content-Type: application/json
```

**Request Body:**
```json
{
  "username": "alice",
  "wallet_address": "GABC..."
}
```

**Response (201 Created):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "username": "alice",
  "wallet_address": "GABC...",
  "created_at": "2024-03-14T10:30:00Z"
}
```

**Errors:**
- `400 Bad Request` - Invalid input
- `409 Conflict` - Username already exists
- `429 Too Many Requests` - Rate limit exceeded (Write-specific quota)

---

#### Get Creator
Retrieves profile information for a specific creator.

```http
GET /creators/:username
```

**Parameters:**
- `username` (path) - Unique username of the creator

**Response (200 OK):**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "username": "alice",
  "wallet_address": "GABC...",
  "created_at": "2024-03-14T10:30:00Z"
}
```

**Errors:**
- `404 Not Found` - Creator not found

---

#### Search Creators
Searches creators by username using fuzzy matching.

```http
GET /creators/search?q=alice&limit=20
```

**Query Parameters:**
- `q` (required) - Search term m
- `limit` (optional) - Results limit (default: 20, max: 100)

**Response (200 OK):**
```json
[
  {
    "id": "...",
    "username": "alice",
    "wallet_address": "...",
    "created_at": "..."
  }
]
```

---

#### List Creator Tips
Retrieves all tips received by a specific creator.

```http
GET /creators/:username/tips
```

**Response (200 OK):**
```json
[
  {
    "id": "...",
    "creator_username": "alice",
    "amount": "10.5",
    "transaction_hash": "abc123...",
    "created_at": "2024-03-14T11:00:00Z"
  }
]
```

---

### Tips

#### Record Tip
Records a new tip transaction after it has been executed on the Stellar network.

```http
POST /tips
Content-Type: application/json
```

**Request Body:**
```json
{
  "username": "alice",
  "amount": "50.0",
  "transaction_hash": "stellar_tx_hash_..."
}
```

**Response (201 Created):**
```json
{
  "id": "...",
  "creator_username": "alice",
  "amount": "50.0",
  "transaction_hash": "...",
  "created_at": "..."
}
```

**Errors:**
- `404 Not Found` - Creator username does not exist
- `409 Conflict` - Transaction hash already recorded
- `422 Unprocessable Entity` - Invalid transaction format

---

### Health

#### Check Status
Returns the health status of the API and its dependencies (Database, Redis).

```http
GET /health
```

**Response (200 OK):**
```json
{
  "status": "up",
  "version": "0.1.0",
  "database": "connected",
  "redis": "connected"
}
```

---

## Error Format
When an error occurs, the API returns a standard JSON response:

```json
{
  "error": "Detailed error message"
}
```
