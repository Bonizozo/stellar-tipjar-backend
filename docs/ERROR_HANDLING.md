# Error Handling Guide

The Nova Launch API uses standard HTTP status codes to indicate success or failure.

---

## Error Response Format
All errors return a JSON response body:

```json
{
  "error": "Reason for failure"
}
```

---

## Common Status Codes

| Code | Status | Meaning |
| :--- | :--- | :--- |
| **200** | OK | Request succeeded. |
| **201** | Created | Resource successfully created (e.g., Profile created). |
| **400** | Bad Request | Invalid input or malformed request body. |
| **401** | Unauthorized | Authentication token is missing or invalid. |
| **404** | Not Found | Requested resource (Creator, Tip) does not exist. |
| **408** | Request Timeout | The request took too long (Configured timeout). |
| **409** | Conflict | Resource already exists (Duplicate username/hash). |
| **429** | Too Many Requests | Rate limit exceeded for the current IP or Token. |
| **500** | Internal Server Error | Generic error indicating a server or database failure. |
| **502** | Bad Gateway | Failure on an upstream dependency (Stellar Network). |

---

## Specific Error Conditions

### Validation Errors (400)
Returned when input constraints are not met:
- Username too short/long.
- Invalid Stellar wallet address format.
- Empty search terms.

### Conflict Errors (409)
Returned during creation:
- **`Creator already exists`**: The provided username is taken.
- **`Transaction already recorded`**: The Stellar `transaction_hash` has been used.

### Rate Limit Errors (429)
Returned when request quotas are exceeded:
- **Write-rate limit**: Higher restrictions on creation endpoints (`/creators`, `/tips`).
- **Read-rate limit**: General limits for browsing and searching.

---

## Best Practices
1. **Handle 500 cleanly**: If your application receives a 500, it should assume a transient failure and can attempt a retry after a short delay.
2. **Follow 429 Retry headers**: Observe the `X-RateLimit-Reset` header to know when your quota will refresh before attempting more requests.
3. **Log Client-side errors**: For 400 and 422 errors, ensure your client application logs the detailed `"error"` message from the response body for debugging.
