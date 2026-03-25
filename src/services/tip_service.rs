use anyhow::Result;
use std::sync::Arc;
use crate::controllers::{tip_controller, creator_controller};
use crate::db::connection::AppState;
use crate::models::tip::{RecordTipRequest, Tip};
use crate::email::EmailMessage;
use tera::Context;

/// Service for handling tip-related business logic and notifications.
/// Abstracts the heavy lifting from the controllers.
pub struct TipService;

impl TipService {
    pub fn new() -> Self {
        Self
    }

    /// Record a new tip and trigger a notification email to the creator receiver.
    pub async fn record_tip(&self, state: Arc<AppState>, req: RecordTipRequest) -> Result<Tip> {
        // First record the tip in the database.
        let tip = tip_controller::record_tip(&state, req).await?;
        
        // Asynchronously fetch creator profile to get their email address.
        let state_clone = state.clone();
        let tip_clone = tip.clone();
        
        tokio::spawn(async move {
            if let Ok(Some(creator)) = creator_controller::get_creator_by_username(&state_clone, &tip_clone.creator_username).await {
                if let Some(email_addr) = creator.email {
                    let mut context = Context::new();
                    context.insert("username", &creator.username);
                    context.insert("amount", &tip_clone.amount.to_string());
                    context.insert("transaction_hash", &tip_clone.transaction_hash);

                    let email_msg = EmailMessage {
                        to: email_addr,
                        subject: format!("🚀 New Tip Received: {} XLM!", tip_clone.amount),
                        template_name: "tip_received.html".into(),
                        context,
                    };

                    if let Err(e) = state_clone.email.send(email_msg).await {
                        tracing::error!("Failed to queue tip notification email from service: {}", e);
                    }
                }
            }
        });

        Ok(tip)
    }

    /// Retrieve all tips for a given creator username.
    pub async fn get_tips_for_creator(&self, state: &AppState, username: &str) -> Result<Vec<Tip>> {
        tip_controller::get_tips_for_creator(state, username).await
    }

    /// Process multiple tips in a single atomic database transaction.
    /// Uses SAVEPOINTs to provide error recovery: if one tip fails (e.g. duplicate hash), 
    /// it is rolled back without aborting the entire bulk operation.
    pub async fn bulk_record_tips(&self, state: &AppState, requests: Vec<RecordTipRequest>) -> Result<Vec<Tip>> {
        let mut tx = crate::db::transaction::begin_transaction(&state.db).await?;
        let mut results = Vec::new();

        for (i, req) in requests.into_iter().enumerate() {
            let sp = format!("tip_record_{}", i);
            crate::db::transaction::create_savepoint(&mut tx, &sp).await?;
            
            match tip_controller::record_tip_in_tx(&mut tx, &req).await {
                Ok(tip) => {
                    results.push(tip);
                    crate::db::transaction::release_savepoint(&mut tx, &sp).await?;
                }
                Err(e) => {
                    tracing::error!("Bulk tip record failed for index {}: {}. Rolling back savepoint.", i, e);
                    crate::db::transaction::rollback_savepoint(&mut tx, &sp).await?;
                }
            }
        }

        tx.commit().await?;
        Ok(results)
    }
}
