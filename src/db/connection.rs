use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;

use crate::services::stellar_service::StellarService;
use super::performance::PerformanceMonitor;

pub struct AppState {
    pub db: PgPool,
    pub stellar: StellarService,
    pub performance: Arc<PerformanceMonitor>,
    pub redis: Option<ConnectionManager>,
}
