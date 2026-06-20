pub mod portal;

use utoipa::{
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    Modify, OpenApi,
};

use crate::models::{
    auth::{
        AuthResponse, LoginRequest, RecoverTwoFactorRequest, RefreshRequest, RegisterRequest,
        TwoFactorSetupResponse, VerifyTwoFactorRequest, VerifyTwoFactorResponse,
    },
    creator::{CreateCreatorRequest, CreatorResponse},
    pagination::{PaginatedResponse, PaginationParams},
    tenant::{CreateTenantRequest, TenantResponse, UpdateTenantRequest},
    tip::{RecordTipRequest, ReportMessageRequest, TipFilters, TipResponse, TipSortParams},
};
use crate::tenancy::{TenantAnalytics, TenantUsage};

/// Adds JWT Bearer security scheme to the OpenAPI spec.
struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer_auth",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Stellar Tipjar API",
        version = "1.0.0",
        description = "
## Overview
Backend API for the Stellar Tipjar — create creator profiles and record on-chain tips
verified against the Stellar network.

> **Interactive docs:** [Redoc portal](/docs) | [Swagger UI](/swagger-ui) | [SDK guide](/docs/sdk)

## Authentication
Most write endpoints require a JWT Bearer token obtained via `POST /auth/login`.

Include it in the `Authorization` header:
```
Authorization: Bearer <access_token>
```

Access tokens expire after **15 minutes**. Use `POST /auth/refresh` with your
`refresh_token` to obtain a new pair.

## Rate Limiting
| Tier | Limit | Applies to |
|------|-------|------------|
| Read | 120 req/min per IP | GET endpoints |
| Write | 30 req/min per IP | POST / PUT / DELETE |

HTTP `429 Too Many Requests` is returned when exceeded. Check the `Retry-After` header.

## Multi-Tenancy
Tenant-scoped endpoints require an `X-Tenant-ID` header containing the tenant UUID.

## Versioning
All endpoints are available under `/api/v1` (legacy, deprecated) and `/api/v2` (current).

## Cursor Pagination
Cursor-based pagination uses a composite cursor of `created_at` timestamp and `id`.
The cursor value is a base64-encoded string of the form `\"<timestamp>|<uuid>\"` where
`<timestamp>` is unix seconds and `<uuid>` is the tip's UUID.
Example: `base64(\"1620000000|550e8400-e29b-41d4-a716-446655440000\")`.

## Errors
All errors follow a consistent envelope:
```json
{
  \"error\": \"One or more fields failed validation\",
  \"code\": \"VALIDATION_ERROR\",
  \"status\": 400,
  \"details\": { \"fields\": { \"username\": [\"too short\"] } },
  \"request_id\": \"a1b2c3d4-...\"
}
```
`details` and `request_id` are omitted when not applicable. `request_id` echoes the
`x-request-id` response header — include it when contacting support.

### Error codes
| Code | HTTP status | Meaning |
|------|-------------|---------|
| `VALIDATION_ERROR` | 400 | One or more request fields failed validation |
| `INVALID_JSON` | 400 | Request body is not valid JSON |
| `INVALID_REQUEST` | 400 | Request failed a business-rule precondition |
| `UNAUTHORIZED` | 401 | Missing, invalid, or expired credentials |
| `FORBIDDEN` | 403 | Authenticated but not permitted to perform this action |
| `DB_NOT_FOUND` | 404 | The requested resource does not exist |
| `CREATOR_NOT_FOUND` | 404 | The requested creator username does not exist |
| `DB_UNIQUE_VIOLATION` | 409 | A resource with this unique field already exists |
| `CONFLICT` (or a custom code) | 409 | The request conflicts with current state |
| `STELLAR_TX_NOT_FOUND` | 422 | Stellar transaction not found or unsuccessful |
| `STELLAR_INVALID_TX` | 422 | The submitted Stellar transaction failed validation |
| `STELLAR_NETWORK_UNAVAILABLE` | 502 | Unable to reach the Stellar network |
| `STELLAR_CIRCUIT_OPEN` | 503 | Stellar verification circuit breaker is open |
| `SERVICE_UNAVAILABLE` | 503 | A dependency is temporarily unavailable |
| `RATE_LIMIT_EXCEEDED` | 429 | Too many requests; see `Retry-After` header |
| `DB_QUERY_FAILED` / `INTERNAL_ERROR` | 500 | Unexpected server error (details are never leaked) |
        ",
        contact(
            name = "Stellar Tipjar Team",
            url = "https://github.com/stellar-tipjar"
        ),
        license(name = "MIT")
    ),
    servers(
        (url = "http://localhost:8000", description = "Local development"),
        (url = "https://api.stellar-tipjar.com", description = "Production"),
    ),
    paths(
        // Health
        crate::routes::health::health_check,
        crate::routes::health::readiness_check,
        // Auth
        crate::routes::auth::register,
        crate::routes::auth::login,
        crate::routes::auth::refresh,
        crate::routes::auth::setup_2fa,
        crate::routes::auth::verify_2fa,
        crate::routes::auth::recover,
        // Creators
        crate::routes::creators::create_creator,
        crate::routes::creators::get_creator,
        crate::routes::creators::get_creator_tips,
        crate::routes::creators::search_creators,
        // Tips
        crate::routes::tips::record_tip,
        crate::routes::tips::list_tips,
        crate::routes::tips::report_tip_message,
        // Teams
        crate::routes::teams::create_team,
        crate::routes::teams::list_teams,
        crate::routes::teams::get_team,
        crate::routes::teams::add_member,
        crate::routes::teams::update_member_share,
        crate::routes::teams::remove_member,
        crate::routes::teams::get_team_splits,
        // Tenants
        crate::routes::tenants::create_tenant,
        crate::routes::tenants::list_tenants,
        crate::routes::tenants::get_tenant,
        crate::routes::tenants::update_tenant,
        crate::routes::tenants::delete_tenant,
        crate::routes::tenants::get_analytics,
        crate::routes::tenants::get_usage,
    ),
    components(
        schemas(
            // Auth
            RegisterRequest,
            LoginRequest,
            RefreshRequest,
            AuthResponse,
            TwoFactorSetupResponse,
            VerifyTwoFactorRequest,
            VerifyTwoFactorResponse,
            RecoverTwoFactorRequest,
            // Creators
            CreateCreatorRequest,
            CreatorResponse,
            // Tips
            RecordTipRequest,
            ReportMessageRequest,
            TipResponse,
            TipFilters,
            TipSortParams,
            PaginationParams,
            // Teams
            crate::models::team::CreateTeamRequest,
            crate::models::team::TeamMemberRequest,
            crate::models::team::UpdateMemberShareRequest,
            crate::models::team::TeamResponse,
            crate::models::team::TeamMemberResponse,
            crate::models::team::TipSplitResponse,
            // Tenants
            CreateTenantRequest,
            UpdateTenantRequest,
            TenantResponse,
            TenantAnalytics,
            TenantUsage,
        )
    ),
    modifiers(&BearerAuth),
    tags(
        (name = "health",   description = "Liveness and readiness probes"),
        (name = "auth",     description = "Authentication — register, login, JWT refresh, 2FA"),
        (name = "creators", description = "Creator profile management"),
        (name = "tips",     description = "Tip recording and retrieval"),
        (name = "teams",    description = "Team management and tip-split configuration"),
        (name = "tenants",  description = "Multi-tenant provisioning, configuration, and analytics"),
    ),
    external_docs(
        url = "https://github.com/stellar-tipjar/docs",
        description = "Full developer documentation"
    )
)]
pub struct ApiDoc;
