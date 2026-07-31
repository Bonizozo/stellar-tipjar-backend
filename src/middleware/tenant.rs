use crate::errors::app_error::AppError;
use crate::tenancy::context::TenantContext;
use axum::{extract::Request, middleware::Next, response::Response};
use std::sync::Arc;

pub async fn tenant_middleware(
    axum::extract::State(resolver): axum::extract::State<Arc<crate::tenancy::TenantResolver>>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let tenant = resolver.resolve_from_request(&req).await?;
    req.extensions_mut().insert(tenant);
    Ok(next.run(req).await)
}
