# Rate Limiting Guide

The Nova Launch API implements request rate limiting to ensure platform stability and protect against Denial-of-Service attacks.

---

## Rate Limit Quotas
Our rate limits are applied per **IP Address**.

### Read Quota (General)
For list, search, and retrieval endpoints (`GET` methods):
- **Limit**: 10 requests per second.
- **Burst**: 20 requests.

### Write Quota (Strict)
For data-modifying endpoints (`POST` methods like `/creators` and `/tips`):
- **Limit**: 2 requests per second.
- **Burst**: 5 requests.

---

## Response Headers
All requests return the following headers to assist with client-side management:

| Header | Meaning |
| :--- | :--- |
| **`X-RateLimit-Limit`** | The maximum number of requests allowed within the current window. |
| **`X-RateLimit-Remaining`** | The number of requests remaining for the current IP. |
| **`X-RateLimit-Reset`** | The time in seconds until the current window resets. |

---

## Rate Limit Errors (429)
When a limit is reached, the API returns a **429 Too Many Requests** status code:

```json
{
  "error": "Too many requests, please try again later."
}
```

---

## Best Practices
1. **Pacing**: Use client-side throttling to ensure requests stay within the allowed rate.
2. **Handle 429**: Implement an exponential backoff strategy if your application receives a 429 error.
3. **Respect Reset Headers**: Do not attempt to retry until the time specified in `X-RateLimit-Reset` has passed.
4. **Caching**: Utilize local caching for read-only data (like creator profiles and tip lists) to minimize redundant requests.
5. **Background Processing**: If your integration requires high-volume writes, process them through a background queue at a controlled rate to stay within the 2 req/sec write quota.
