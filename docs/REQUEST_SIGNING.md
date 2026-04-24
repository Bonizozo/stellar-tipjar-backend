# Request Signing

All API requests can be authenticated using HMAC-SHA256 request signing. This guarantees that requests are authentic and haven't been tampered with in transit.

## How It Works

1. You hold an **API key** (public identifier) and a **secret** (private, never sent over the wire).
2. For each request you compute an HMAC-SHA256 signature over `"{timestamp}.{body}"` using your secret.
3. You send the key, timestamp, and signature as HTTP headers.
4. The server recomputes the signature and rejects any request where they don't match, the timestamp is stale, or the nonce has already been seen.

## Required Headers

| Header | Description |
|---|---|
| `x-api-key` | Your public API key |
| `x-timestamp` | Unix timestamp (seconds) at time of request |
| `x-signature` | Hex-encoded HMAC-SHA256 signature |

## Signature Algorithm

```
message   = "{timestamp}.{raw_request_body}"
signature = HMAC-SHA256(secret, message)  →  hex-encoded
```

For `GET` requests with no body, use an empty string as the body.

## Timestamp Tolerance

The server accepts requests within **±5 minutes** of its own clock. Requests outside this window are rejected with `401 Unauthorized`.

## Nonce / Replay Prevention

Each `(api_key, timestamp)` pair is stored for 5 minutes. Replaying an identical request within that window returns `401 Unauthorized`.

---

## Client SDK Examples

### Rust

```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;

fn sign(secret: &str, body: &str, timestamp: i64) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let message = format!("{}.{}", timestamp, body);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

// Usage
let ts = chrono::Utc::now().timestamp();
let body = r#"{"username":"alice","amount":"5.0","transaction_hash":"abc123"}"#;
let sig = sign("your_secret", body, ts);

let client = reqwest::Client::new();
let resp = client
    .post("https://api.example.com/tips")
    .header("x-api-key", "your_api_key")
    .header("x-timestamp", ts.to_string())
    .header("x-signature", sig)
    .header("content-type", "application/json")
    .body(body)
    .send()
    .await?;
```

### Python

```python
import hmac, hashlib, time, requests

def sign(secret: str, body: str, timestamp: int) -> str:
    message = f"{timestamp}.{body}".encode()
    return hmac.new(secret.encode(), message, hashlib.sha256).hexdigest()

ts = int(time.time())
body = '{"username":"alice","amount":"5.0","transaction_hash":"abc123"}'
sig = sign("your_secret", body, ts)

resp = requests.post(
    "https://api.example.com/tips",
    data=body,
    headers={
        "x-api-key": "your_api_key",
        "x-timestamp": str(ts),
        "x-signature": sig,
        "content-type": "application/json",
    },
)
```

### JavaScript / Node.js

```js
const crypto = require("crypto");

function sign(secret, body, timestamp) {
  const message = `${timestamp}.${body}`;
  return crypto.createHmac("sha256", secret).update(message).digest("hex");
}

const ts = Math.floor(Date.now() / 1000);
const body = JSON.stringify({ username: "alice", amount: "5.0", transaction_hash: "abc123" });
const sig = sign("your_secret", body, ts);

await fetch("https://api.example.com/tips", {
  method: "POST",
  headers: {
    "x-api-key": "your_api_key",
    "x-timestamp": String(ts),
    "x-signature": sig,
    "content-type": "application/json",
  },
  body,
});
```

### cURL

```bash
SECRET="your_secret"
API_KEY="your_api_key"
BODY='{"username":"alice","amount":"5.0","transaction_hash":"abc123"}'
TS=$(date +%s)
SIG=$(printf '%s.%s' "$TS" "$BODY" | openssl dgst -sha256 -hmac "$SECRET" -hex | awk '{print $2}')

curl -X POST https://api.example.com/tips \
  -H "x-api-key: $API_KEY" \
  -H "x-timestamp: $TS" \
  -H "x-signature: $SIG" \
  -H "content-type: application/json" \
  -d "$BODY"
```

---

## Key Management

### Create a key

```
POST /api/v1/api-keys
Content-Type: application/json

{ "name": "my-integration" }
```

Response `201 Created`:

```json
{
  "id": "...",
  "key": "a1b2c3...",
  "secret": "d4e5f6...",
  "name": "my-integration",
  "created_at": "2026-04-24T14:00:00Z"
}
```

> The `secret` is returned **only once**. Store it securely.

### Rotate a key

```
POST /api/v1/api-keys/{key}/rotate
```

The old key is immediately deactivated and a new `key`/`secret` pair is returned.

---

## Error Responses

| Status | Cause |
|---|---|
| `401 Unauthorized` | Missing headers, invalid signature, stale timestamp, or replayed nonce |
| `400 Bad Request` | Request body could not be read |

---

## Applying the Middleware

To protect a route group, add the middleware in `src/main.rs`:

```rust
use axum::middleware;
use crate::middleware::signature::verify_request_signature;

let protected = Router::new()
    .merge(routes::tips::router())
    .layer(middleware::from_fn_with_state(
        Arc::clone(&state),
        verify_request_signature,
    ));
```
