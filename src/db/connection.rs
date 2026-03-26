use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::services::stellar_service::StellarService;
use crate::services::tip_service::TipService;
use crate::services::creator_service::CreatorService;
use crate::email::sender::EmailSender;
use crate::ws::TipEvent;
use super::performance::PerformanceMonitor;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub stellar: StellarService,
    pub performance: Arc<PerformanceMonitor>,
    pub redis: Option<ConnectionManager>,
    pub tip_service: Arc<TipService>,
    pub creator_service: Arc<CreatorService>,
    pub email_sender: Arc<EmailSender>,
    pub broadcast_tx: broadcast::Sender<TipEvent>,
}