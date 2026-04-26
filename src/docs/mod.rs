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
    tip::{RecordTipRequest, TipFilters, TipResponse, TipSortParams},
};

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

## Authentication
Most write endpoints require a JWT Bearer token obtained via `POST /auth/login`.

Include it in the `Authorization` header:
```
Authorization: Bearer <access_token>
```

Access tokens expire after **15 minutes**. Use `POST /auth/refresh` with your
`refresh_token` to obtain a new pair.

## Rate Limiting
- **Read endpoints**: 120 req/min per IP
- **Write endpoints**: 30 req/min per IP

## Versioning
All endpoints are available under `/api/v1` and `/api/v2`.
        ",
        contact(
            name = "Stellar Tipjar Team",
            url = "https://github.com/stellar-tipjar"
        ),
        license(name = "MIT")
    ),
    paths(
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
            TipResponse,
            TipFilters,
            TipSortParams,
            PaginationParams,
        )
    ),
    modifiers(&BearerAuth),
    tags(
        (name = "auth",     description = "Authentication — register, login, JWT refresh, 2FA"),
        (name = "creators", description = "Creator profile management"),
        (name = "tips",     description = "Tip recording and retrieval")
    ),
    external_docs(
        url = "https://github.com/stellar-tipjar/docs",
        description = "Full developer documentation"
    )
)]
pub struct ApiDoc;
