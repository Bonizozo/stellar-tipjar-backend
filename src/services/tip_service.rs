use std::sync::Arc;
use crate::controllers::{tip_controller, creator_controller};
use crate::db::connection::AppState;
use crate::errors::{AppError, AppResult};
use crate::models::tip::{RecordTipRequest, Tip};

/// Service for handling tip-related business logic and notifications.
/// Abstracts the heavy lifting from the controllers.
pub struct TipService;

impl TipService {
    pub fn new() -> Self {
        Self
    }

    /// Record a new tip and trigger a notification email to the creator receiver.
    pub async fn record_tip(&self, state: Arc<AppState>, req: RecordTipRequest) -> AppResult<Tip> {
        // First record the tip in the database.
        let tip = tip_controller::record_tip(&state, req).await?;

        Ok(tip)
    }

    /// Retrieve all tips for a given creator username.
    pub async fn get_tips_for_creator(&self, state: &AppState, username: &str) -> AppResult<Vec<Tip>> {
        tip_controller::get_tips_for_creator(state, username).await
    }

    /// Process multiple tips in a single atomic database transaction.
    /// Uses SAVEPOINTs to provide error recovery: if one tip fails (e.g. duplicate hash), 
    /// it is rolled back without aborting the entire bulk operation.
    pub async fn bulk_record_tips(&self, state: &AppState, requests: Vec<RecordTipRequest>) -> AppResult<Vec<Tip>> {
        let mut tx = crate::db::transaction::begin_transaction(&state.db)
            .await
            .map_err(AppError::from)?;
        let mut results = Vec::new();

        for (i, req) in requests.into_iter().enumerate() {
            let sp = format!("tip_record_{}", i);
            crate::db::transaction::create_savepoint(&mut tx, &sp)
                .await
                .map_err(AppError::from)?;

            match tip_controller::record_tip_in_tx(&mut tx, &req).await {
                Ok(tip) => {
                    results.push(tip);
                    crate::db::transaction::release_savepoint(&mut tx, &sp)
                        .await
                        .map_err(AppError::from)?;
                }
                Err(e) => {
                    tracing::error!("Bulk tip record failed for index {}: {}. Rolling back savepoint.", i, e);
                    crate::db::transaction::rollback_savepoint(&mut tx, &sp)
                        .await
                        .map_err(AppError::from)?;
                }
            }
        }

        tx.commit().await?;
        Ok(results)
    }
}
