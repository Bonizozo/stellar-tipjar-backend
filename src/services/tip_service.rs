use crate::controllers::{creator_controller, tip_controller};
use crate::db::connection::AppState;
use crate::db::retry::with_db_retry;
use crate::db::transaction::with_transaction;
use crate::errors::{AppError, AppResult};
use crate::models::tip::{RecordTipRequest, Tip};
use std::sync::Arc;

/// Service for handling tip-related business logic and notifications.
/// Abstracts the heavy lifting from the controllers.
pub struct TipService;

impl TipService {
    pub fn new() -> Self {
        Self
    }

    /// Record a new tip and trigger a notification email to the creator receiver.
    #[tracing::instrument(
        name = "tip_service.record_tip",
        skip(self, state, req),
        fields(
            tip.creator_username = %req.creator_username,
            tip.amount_xlm       = %req.amount_xlm,
        )
    )]
    pub async fn record_tip(&self, state: Arc<AppState>, req: RecordTipRequest) -> AppResult<Tip> {
        let tip = tip_controller::record_tip(&state, req).await?;

        tracing::info!(tip.id = %tip.id, "tip recorded successfully");
        Ok(tip)
    }

    /// Retrieve all tips for a given creator username.
    #[tracing::instrument(
        name = "tip_service.get_tips_for_creator",
        skip(self, state),
        fields(creator.username = %username)
    )]
    pub async fn get_tips_for_creator(
        &self,
        state: &AppState,
        username: &str,
    ) -> AppResult<Vec<Tip>> {
        tip_controller::get_tips_for_creator(state, username).await
    }

    /// Process multiple tips in a single atomic database transaction.
    ///
    /// Uses SAVEPOINTs to provide error recovery: if one tip fails (e.g.
    /// duplicate hash), it is rolled back without aborting the entire bulk
    /// operation.
    #[tracing::instrument(
        name = "tip_service.bulk_record_tips",
        skip(self, state, requests),
        fields(tip.bulk_count = requests.len())
    )]
    pub async fn bulk_record_tips(
        &self,
        state: &AppState,
        requests: Vec<RecordTipRequest>,
    ) -> AppResult<Vec<Tip>> {
        let pool = state.db.clone();
        with_db_retry(&pool, 3, |pool| {
            let requests = requests.clone();
            let state = state.clone();
            Box::pin(async move {
                with_transaction(pool, |tx| {
                    Box::pin(async move {
                        let mut results = Vec::new();
                        for (i, req) in requests.into_iter().enumerate() {
                            let sp = format!("tip_record_{}", i);
                            crate::db::transaction::create_savepoint(tx, &sp)
                                .await
                                .map_err(AppError::from)?;
                            match tip_controller::record_tip_in_tx(&state, tx, &req).await {
                                Ok(tip) => {
                                    results.push(tip);
                                    crate::db::transaction::release_savepoint(tx, &sp)
                                        .await
                                        .map_err(AppError::from)?;
                                }
                                Err(e) => {
                                    tracing::error!(
                                        tip.index = i,
                                        error = %e,
                                        "bulk tip record failed; rolling back savepoint"
                                    );
                                    crate::db::transaction::rollback_savepoint(tx, &sp)
                                        .await
                                        .map_err(AppError::from)?;
                                }
                            }
                        }
                        Ok(results)
                    })
                })
                .await
            })
        })
        .await
    }
}
