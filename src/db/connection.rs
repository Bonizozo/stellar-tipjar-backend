use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;

use super::performance::PerformanceMonitor;
use crate::email::sender::EmailSender;
use crate::services::creator_service::CreatorService;
use crate::services::stellar_service::StellarService;
use crate::services::tip_service::TipService;
use crate::ws::TipEvent;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub stellar: StellarService,
    pub performance: Arc<PerformanceMonitor>,
    pub redis: Option<ConnectionManager>,
    pub broadcast_tx: broadcast::Sender<TipEvent>,
    pub moderation: Arc<ModerationService>,
}
