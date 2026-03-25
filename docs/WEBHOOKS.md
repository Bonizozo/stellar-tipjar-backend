# Webhook Integration Guide

The Nova Launch Webhook system allows you to receive real-time HTTP notifications when specific events occur on the platform.

---

## Webhook Events

| Event Type | Description |
| :--- | :--- |
| **`creator.created`** | Triggered when a new creator profile is successfully registered. |
| **`tip.recorded`** | Triggered when a new tip is recorded and verified on-chain. |

---

## Signature Verification
All webhook requests include an `X-Webhook-Signature` header to allow you to verify the authenticity of the request. The signature is a **SHA-256 HMAC** of the JSON payload, using your webhook secret as the key.

### Verification (Pseudo-code)
```javascript
const crypto = require('crypto');

// 1. Get the signature from the header
const signature = headers['X-Webhook-Signature'];

// 2. Generate your own signature using the request body and secret
const hmac = crypto.createHmac('sha256', YOUR_WEBHOOK_SECRET);
const computedSignature = hmac.update(JSON.stringify(request.body)).digest('hex');

// 3. Compare the signatures
if (crypto.timingSafeEqual(Buffer.from(signature), Buffer.from(computedSignature))) {
    // Signature verified!
}
```

---

## Delivery and Retries
- **Retry Strategy**: Exponential backoff (Starts at 500ms, max 10s delay).
- **Max Retries**: 3 attempts.
- **Timeout**: Each delivery attempt has a 5-second timeout.
- **Ordering**: Delivery order is not guaranteed.

---

## Payload Format
Webhooks are delivered as `POST` requests with a JSON body:

```json
{
  "id": "event_uuid_...",
  "event_type": "tip.recorded",
  "payload": {
    "id": "tip_uuid_...",
    "creator_username": "alice",
    "amount": "50.0",
    "transaction_hash": "stellar_tx_hash_...",
    "created_at": "2024-03-14T12:00:00Z"
  },
  "timestamp": "2024-03-14T12:00:05Z"
}
```

---

## Best Practices
1. **Acknowledge quickly**: Your endpoint should return a `2xx` status code immediately after receiving the request. Avoid performing slow operations (like sending emails) directly within the webhook handler.
2. **Handle duplicates**: Use the `id` field to ensure your system only processes each event once (idempotency).
3. **Use HTTPS**: Ensure your webhook URL uses SSL/TLS for secure transmission.
4. **Whitelist IPs (Recommended)**: If possible, restrict access to your webhook endpoint to the known IP addresses of the Nova Launch platform.
5. **Periodic Reconciliation**: Occasionally poll the API to reconcile your local data in case of delivery failure beyond the retry limit.
