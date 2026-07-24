# Rate Limiting Guide

The API implements distributed request rate limiting to ensure platform
stability and protect against abuse and Denial-of-Service attacks.

Rate limiting has two layers:

1. **`tower_governor`** — a per-process, in-memory token bucket. It runs on
   every replica independently and acts purely as a cheap, local backstop.
2. **Redis-backed GCRA (Generic Cell Rate Algorithm)** — the primary,
   authoritative limiter (`src/gateway/rate_limiter.rs` +
   `src/gateway/rate_limit_script.rs`). Every replica evaluates the same
   Lua script against the same Redis instance, so the limit is shared
   correctly across all replicas — it does not multiply with `N_replicas`
   the way a purely in-memory limiter would, and it does not reset on
   restart/deploy.

---

## How the distributed limiter works

Each request is checked with a single atomic Redis Lua script (`EVALSHA`,
with automatic `EVAL` fallback) that implements GCRA: it stores one
"theoretical arrival time" value per client key and updates it with one
`GET`+`SET` executed as an indivisible unit inside Redis. There is no
read-modify-write race window — two concurrent requests for the same key can
never both be admitted past capacity, because Redis serializes script
execution.

GCRA naturally expresses both a sustained rate and a burst allowance in one
value, replacing the previous two-round-trip approach (a separate burst
counter plus an approximate two-bucket sliding window).

## Layered policies

- **Per-tier** (`CallerTier`): `anonymous` / `free` / `premium` / `admin`,
  resolved from the authenticated `GatewayIdentity` (JWT role or API key
  permissions). Overridable via `RATE_LIMIT_<TIER>_RPM` / `_BURST` env vars.
- **Per-identity key**: authenticated callers are keyed by JWT subject or API
  key (not by IP), so limits follow the caller rather than their network
  address. Anonymous callers are keyed by client IP.
- **Per-route-class override**: `ROUTE_RL_<SEGMENT>_RPM` / `_BURST` (e.g.
  `ROUTE_RL_TIPS_RPM=30`) overrides the tier default for a specific route,
  independent of caller tier — e.g. tighter limits on `/auth/login`, looser
  limits on public reads.

## Client IP extraction behind a proxy

Naively trusting `X-Forwarded-For` lets a client rate-limit anyone (or
nobody) by forging the header. Set `TRUSTED_PROXY_DEPTH` to the number of
reverse proxies between the internet and this service that each append their
observed peer to `X-Forwarded-For` (`1` for a single load balancer, `2` for
an LB in front of an ingress gateway, etc.). With depth `N`, only the `N`-th
entry from the right is trusted as the client's real IP; `TRUSTED_PROXY_DEPTH`
defaults to `0`, which ignores `X-Forwarded-For` entirely and uses the raw
TCP peer address — the only safe default when the topology isn't known.

## Fail-open vs. fail-closed

When Redis is unreachable the limiter cannot verify whether a caller is over
their limit. The decision is an explicit per-route policy
(`src/gateway/rate_limit_policy.rs`), not a single global default:

- **Fail-closed** (reject with `503` rather than risk unlimited throughput):
  auth, registration, password reset, MFA, and API key endpoints, by default.
- **Fail-open** (allow through, `tower_governor` remains a backstop):
  everything else, by default — mainly public reads.

Override per route with `RATE_LIMIT_FAIL_POLICY_<SEGMENT>=open|closed`, or
globally with `RATE_LIMIT_FAIL_POLICY_DEFAULT=open|closed`. Every degraded
decision increments the `gateway_rate_limit_degraded_total{tier,policy,outcome}`
Prometheus counter so on-call can see how often (and where) the fallback
path is being taken.

---

## Response Headers

Both the legacy headers and the standard IETF headers
([draft-ietf-httpapi-ratelimit-headers](https://datatracker.ietf.org/doc/draft-ietf-httpapi-ratelimit-headers/))
are returned on every limited route:

| Header | Meaning |
| :--- | :--- |
| **`RateLimit-Limit`** / `X-RateLimit-Limit` | The maximum number of requests allowed within the current window. |
| **`RateLimit-Remaining`** / `X-RateLimit-Remaining` | The number of requests remaining for this client. |
| **`RateLimit-Reset`** / `X-RateLimit-Reset` | Seconds until the window resets. |
| **`X-RateLimit-Tier`** | The resolved caller tier (`anonymous`/`free`/`premium`/`admin`). |
| **`X-RateLimit-Degraded`** | Present (`true`) only when the request was served in degraded (Redis-unreachable, fail-open) mode. |

## Rate Limit Errors

**`429 Too Many Requests`** — the caller is genuinely over their limit:

```json
{
  "error": "Rate limit exceeded. Please slow down.",
  "code": "RATE_LIMIT_EXCEEDED",
  "status": 429,
  "details": { "tier": "anonymous", "limit_rpm": 30, "retry_after_secs": 4, "reset_at_secs": 12 }
}
```

`Retry-After` is set on every `429`.

**`503 Service Unavailable`** — Redis is unreachable and this route's fail
policy is fail-closed, so the request could not be safely admitted:

```json
{
  "error": "Rate limiting is temporarily unavailable and this endpoint fails closed for safety. Please retry shortly.",
  "code": "RATE_LIMIT_DEGRADED",
  "status": 503
}
```

---

## Load testing the multi-replica guarantee

`scripts/rate_limit_multi_replica_test.sh` spins up one Redis instance and
two application replicas via Docker Compose, fires concurrent traffic split
across both replicas, and asserts the combined number of admitted requests
matches the configured limit (± the burst tolerance) — proving the limit is
shared, not per-replica. See the script header for usage and
`scripts/compose.rate-limit-demo.yml` for the topology.

---

## Best Practices
1. **Pacing**: Use client-side throttling to ensure requests stay within the allowed rate.
2. **Handle 429**: Implement an exponential backoff strategy if your application receives a 429 error.
3. **Respect Reset Headers**: Do not retry until `RateLimit-Reset` (or `Retry-After` on a 429) has elapsed.
4. **Caching**: Utilize local caching for read-only data (like creator profiles and tip lists) to minimize redundant requests.
5. **Background Processing**: If your integration requires high-volume writes, process them through a background queue at a controlled rate.
