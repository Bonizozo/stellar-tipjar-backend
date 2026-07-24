# Stellar TipJar — Webhook Verification Guide

## Signature format

Every delivery from the Stellar TipJar platform carries three headers:

| Header | Example | Description |
|---|---|---|
| `X-TipJar-Signature` | `t=1715000000,v1=abcd1234…` | Versioned HMAC signature |
| `X-TipJar-Delivery-Id` | `550e8400-e29b-41d4-a716-446655440000` | Stable UUID — same on every retry |
| `X-TipJar-Event-Type` | `tip.created` | Event type string |

### Signature anatomy

```
X-TipJar-Signature: t=<unix_timestamp>,v1=<hmac_hex>[,v1=<hmac_hex2>]
```

- `t` — Unix timestamp (seconds UTC) when the delivery was sent.
- `v1` — HMAC-SHA256 hex of `"{t}.{raw_body}"` signed with your webhook secret.
- During a secret rotation, **two `v1` values** are present. Accept the delivery
  if **either** matches your known secret — this ensures zero missed verifications
  during the rotation overlap period.

### Signed message

```
signed_payload = "<unix_ts>" + "." + <raw_request_body>
hmac           = HMAC-SHA256(key=secret, data=signed_payload)
v1             = hex(hmac)
```

---

## Step-by-step verification

### 1. Extract the timestamp and signature values

```
header = request.headers["X-TipJar-Signature"]
parts  = header.split(",")
ts     = parts["t=…"]          # e.g. 1715000000
sigs   = parts with prefix "v1="
```

### 2. Reject stale deliveries (replay protection)

```
tolerance = 300  # seconds (5 minutes)
now       = current_unix_timestamp()

if abs(now - ts) > tolerance:
    reject("timestamp outside tolerance window")
```

> **Why 5 minutes?** This is the default tolerance. It guards against replay
> attacks while giving receivers generous slack for clock drift and processing
> delays. Never set the tolerance to zero in production.

### 3. Compute the expected HMAC

```
signed_payload = str(ts) + "." + raw_body
expected       = hmac_sha256(key=YOUR_SECRET, data=signed_payload)
```

### 4. Compare using constant-time equality

```python
import hmac as hmac_lib
for sig in sigs:
    if hmac_lib.compare_digest(sig, expected.hex()):
        accept()  # delivery is authentic
reject("signature mismatch")
```

> **Always use constant-time comparison** (`hmac.compare_digest` in Python,
> `crypto.timingSafeEqual` in Node.js, etc.) to prevent timing side-channel attacks.

---

## Reference implementations

### Python

```python
import hashlib
import hmac as hmac_lib
import time

TOLERANCE_SECS = 300

def verify_webhook(
    signature_header: str,
    raw_body: bytes,
    secret: str,
) -> bool:
    """Return True if the delivery is authentic and within the tolerance window."""
    parts = {
        k: v
        for part in signature_header.split(",")
        for k, v in [part.split("=", 1)]
    }
    ts = int(parts.get("t", 0))

    # Step 2: replay guard
    if abs(time.time() - ts) > TOLERANCE_SECS:
        return False

    # Step 3: compute expected HMAC
    signed = f"{ts}.".encode() + raw_body
    expected = hmac_lib.new(secret.encode(), signed, hashlib.sha256).hexdigest()

    # Step 4: constant-time compare against every v1= value
    v1_values = [v for k, v in parts.items() if k == "v1"]
    return any(hmac_lib.compare_digest(v, expected) for v in v1_values)
```

### Node.js / TypeScript

```typescript
import { createHmac, timingSafeEqual } from "crypto";

const TOLERANCE_SECS = 300;

export function verifyWebhook(
  signatureHeader: string,
  rawBody: Buffer,
  secret: string
): boolean {
  const parts = Object.fromEntries(
    signatureHeader.split(",").map((p) => p.split("=", 2) as [string, string])
  );

  const ts = parseInt(parts["t"] ?? "0", 10);

  // Replay guard
  if (Math.abs(Date.now() / 1000 - ts) > TOLERANCE_SECS) return false;

  // Expected HMAC
  const signed = Buffer.concat([Buffer.from(`${ts}.`), rawBody]);
  const expected = createHmac("sha256", secret).update(signed).digest("hex");

  // Constant-time compare
  const v1Values = signatureHeader
    .split(",")
    .filter((p) => p.startsWith("v1="))
    .map((p) => p.slice(3));

  const expectedBuf = Buffer.from(expected);
  return v1Values.some((v) => {
    try {
      return timingSafeEqual(Buffer.from(v), expectedBuf);
    } catch {
      return false; // length mismatch
    }
  });
}
```

### Rust

```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;

const TOLERANCE_SECS: u64 = 300;

pub fn verify_webhook(header: &str, raw_body: &[u8], secret: &str) -> bool {
    // Parse t= and v1= values
    let mut ts: Option<u64> = None;
    let mut v1_values: Vec<String> = Vec::new();
    for part in header.split(',') {
        if let Some(t) = part.trim().strip_prefix("t=") {
            ts = t.parse().ok();
        } else if let Some(v) = part.trim().strip_prefix("v1=") {
            v1_values.push(v.to_string());
        }
    }
    let ts = match ts { Some(t) => t, None => return false };

    // Replay guard
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now.saturating_sub(ts) > TOLERANCE_SECS || ts.saturating_sub(now) > TOLERANCE_SECS {
        return false;
    }

    // Compute expected HMAC
    let mut signed = format!("{}.", ts).into_bytes();
    signed.extend_from_slice(raw_body);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(&signed);
    let expected = hex::encode(mac.finalize().into_bytes());

    // Constant-time compare
    let exp_bytes = expected.as_bytes();
    v1_values.iter().any(|v| {
        if v.len() != expected.len() { return false; }
        let mut diff = 0u8;
        for (a, b) in v.bytes().zip(exp_bytes.iter()) { diff |= a ^ b; }
        diff == 0
    })
}
```

---

## Secret rotation

When a secret rotation is requested via `POST /webhooks/:id/rotate-secret`:

1. A new primary secret is generated and stored.
2. The retiring secret is kept active for the **tolerance window** (5 minutes default).
3. During the overlap, deliveries carry **two `v1=` values** — one per secret.
4. Receivers that have not yet updated will still verify against the retiring secret.
5. After the overlap window, update your receiver to the new secret.

**Zero missed verifications are guaranteed** during the overlap period.

---

## Deduplication using `X-TipJar-Delivery-Id`

The `X-TipJar-Delivery-Id` header contains a stable UUID that is **identical
across all retry attempts** for the same logical delivery. Store it in your
idempotency log:

```sql
CREATE TABLE received_webhooks (
    delivery_id UUID PRIMARY KEY,
    processed_at TIMESTAMPTZ DEFAULT NOW()
);
```

```python
delivery_id = request.headers["X-TipJar-Delivery-Id"]
if db.exists("SELECT 1 FROM received_webhooks WHERE delivery_id = %s", delivery_id):
    return 200  # already processed — acknowledge without side-effects
db.insert("received_webhooks", delivery_id=delivery_id)
process_event(request.json())
```

---

## Error responses

Return a **2xx** status to acknowledge receipt. Any other status (or a
connection timeout) causes the platform to retry with exponential backoff
+ jitter, up to 5 attempts, then dead-letter the delivery.

Dead-lettered deliveries are accessible at `GET /webhooks/dlq` and can be
re-driven individually via `POST /webhooks/dlq/:id/replay`.
