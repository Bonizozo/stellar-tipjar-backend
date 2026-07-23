# Session Security (#345)

This document covers the session/token security model implemented for
`src/services/auth_service.rs`, `src/middleware/auth.rs`,
`src/security/token_revocation.rs`, and `src/services/token_family_service.rs`:
refresh-token rotation with reuse detection, access-token revocation,
kid-based key rotation, and claims hygiene.

## Token shapes

Both access and refresh tokens are HS256 JWTs carrying:

| claim  | meaning |
|--------|---------|
| `sub`  | username |
| `kind` | `"access"` or `"refresh"` |
| `role` | role string used for authorization |
| `iat`/`nbf`/`exp` | issued-at / not-before / expiry, `nbf == iat` |
| `iss`/`aud` | issuer/audience, checked against `JWT_ISSUER`/`JWT_AUDIENCE` |
| `jti`  | unique token id |
| `family` | refresh-token family id (refresh tokens only) |
| `tv`   | token version — the user's revocation epoch at issuance time |

Access tokens expire after **15 minutes**. Refresh tokens expire after **7
days** (also the absolute lifetime of their token family, enforced
server-side regardless of rotation).

Validation (`auth_service::validate_token`) pins the algorithm to HS256,
applies a **30 second** clock-skew leeway to `exp`/`nbf`, and rejects any
token whose `iss`/`aud`/`kind` don't match expectations.

## Refresh-token rotation & reuse detection

Every refresh token belongs to a **family**, created at login/register/2FA
recovery and persisted in `refresh_token_families` (migration `0071`) with
`current_jti`, `revoked`, device metadata (`user_agent`, `ip_address`), and
`expires_at`.

On `POST /auth/refresh`:

1. The presented refresh token is validated normally.
2. Its `family` id and `jti` are compared against the family row (locked
   `FOR UPDATE` for the duration of the check) via
   `token_family_service::rotate_or_detect_reuse`.
3. If the presented `jti` matches `current_jti`, the family is advanced to a
   freshly minted `jti` and a new token pair is returned.
4. If it does **not** match — the token was already rotated away — this is
   treated as theft: the entire family is marked `revoked`, and the request
   is rejected. All tokens for that family (old and new) stop working;
   the user must re-authenticate.

The rotation decision is a pure function
(`token_family_service::decide_rotation`), unit-tested directly without a
database — see `reuse_of_already_rotated_token_is_detected_and_kills_family`
in `src/services/token_family_service.rs`.

## Access-token revocation

`src/security/token_revocation.rs` implements two Redis-backed signals,
checked together in **one pipelined round trip**
(`token_revocation::check_not_revoked`) from both `middleware::auth::require_auth`
(routes mounted directly, e.g. `/auth/*`) and `gateway::authentication::gateway_auth`
(the `/api/v1`, `/api/v2` surface) — every JWT-authenticated entry point enforces it:

- **jti denylist** (`revoked_jti:<jti>`) — set on logout, TTL'd to the
  token's remaining lifetime. Revokes exactly one access token.
- **per-user token epoch** (`token_epoch:<username>`) — incremented on
  password change or admin revocation. Every access token embeds the epoch
  that was current when it was minted (`tv`); any token whose `tv` is behind
  the current epoch is rejected. This invalidates *every* outstanding access
  token for a user without having to track individual jtis.

**Fail-closed policy**: if Redis is configured but the round trip errors,
the request is rejected rather than allowed through. If Redis is not
configured at all, revocation enforcement is unavailable and a warning is
logged — **Redis is required in production** for logout / password-change /
admin-revocation to actually take effect; without it, tokens remain valid
until natural expiry (15 minutes for access tokens).

## kid-based key rotation

Signing keys are configured via `JWT_SIGNING_KEYS` (comma-separated
`kid:secret` pairs) and `JWT_ACTIVE_KID`. When `JWT_SIGNING_KEYS` is unset,
a single legacy key is read from `JWT_SECRET` (kid `"default"`) so existing
single-secret deployments keep working unchanged.

Every signed token carries its signing `kid` in the JWT header. Verification
looks up the matching key from the full configured set — not just the
active one — so keys can overlap during rotation.

### Rotation runbook

1. Generate a new secret: `openssl rand -hex 32`.
2. Add it to `JWT_SIGNING_KEYS` under a new `kid` **alongside** the current
   key (do not remove the old one yet):
   ```
   JWT_SIGNING_KEYS=k1:<old-secret>,k2:<new-secret>
   JWT_ACTIVE_KID=k1
   ```
3. Deploy. New tokens still sign with `k1`; both `k1`- and `k2`-signed
   tokens verify.
4. Flip `JWT_ACTIVE_KID=k2` and redeploy. New tokens now sign with `k2`.
   Tokens still in flight that were signed with `k1` keep verifying — **zero
   forced global logout**.
5. Wait out the maximum token lifetime (7 days, the refresh-token/family
   TTL) so every `k1`-signed token has naturally expired.
6. Remove `k1` from `JWT_SIGNING_KEYS` entirely. Tokens signed with a key no
   longer in the set are rejected.

## Session management endpoints

All under `/auth`, protected by `require_auth` unless noted:

| Endpoint | Effect |
|---|---|
| `POST /auth/logout` | Denylists the current access token's `jti`; optionally revokes the refresh-token family if `refresh_token` is supplied in the body |
| `POST /auth/password/change` | Verifies the old password, updates the hash, bumps the token epoch, revokes all refresh-token families |
| `GET /auth/sessions` | Lists the caller's active sessions (family id, device metadata, timestamps) |
| `DELETE /auth/sessions/:family_id` | Revokes one of the caller's own sessions |
| `DELETE /auth/sessions` | Revokes all of the caller's sessions and bumps their token epoch |
| `POST /admin/sessions/:username/revoke` (admin, `X-Admin-Key`) | Revokes all of a user's sessions and bumps their token epoch — account-compromise response |

## Threat model summary

| Threat | Mitigation |
|---|---|
| Stolen refresh token replayed alongside the legitimate client | Reuse detection kills the whole family on the first replay |
| Stolen access token used after logout/password change | jti denylist / token-epoch check in `require_auth`, fail-closed |
| Compromised signing key | kid rotation with overlap; old key removed once in-flight tokens expire |
| `alg: none` / algorithm-confusion downgrade | Algorithm allowlist pinned to HS256 both explicitly and via `Validation::algorithms` |
| Token replayed across environments/services | `iss`/`aud` checked on every decode |
| Minor clock drift between hosts | Explicit 30s leeway on `exp`/`nbf`, not open-ended |
