use anyhow::Result;
use std::sync::Arc;
use crate::db::connection::AppState;
use crate::models::creator::{CreateCreatorRequest, Creator};
use crate::controllers::creator_controller;
use crate::email::EmailMessage;
use tera::Context;

/// Service for managing creator-related business logic.
/// Acts as an abstraction layer over the database controllers.
pub struct CreatorService;

impl CreatorService {
    pub fn new() -> Self {
        Self
    }

    /// Create a new creator and send a welcome email if an email is provided.
    pub async fn create_creator(&self, state: Arc<AppState>, req: CreateCreatorRequest) -> Result<Creator> {
        let creator = creator_controller::create_creator(&state, req).await?;
        
        // If the creator provided an email, send a beautiful welcome message.
        if let Some(email_addr) = &creator.email {
            let mut context = Context::new();
            context.insert("username", &creator.username);

            let email_msg = EmailMessage {
                to: email_addr.clone(),
                subject: "✨ Welcome to Stellar TipJar!".into(),
                template_name: "welcome.html".into(),
                context,
            };

            // Queue the email to avoid blocking the API response.
            let _ = state.email.send(email_msg).await;
        }

        Ok(creator)
    }

    /// Retrieve a creator by their username.
    pub async fn get_creator_by_username(&self, state: &AppState, username: &str) -> Result<Option<Creator>> {
        creator_controller::get_creator_by_username(state, username).await
    }
}
