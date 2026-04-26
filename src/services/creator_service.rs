use crate::controllers::creator_controller;
use crate::db::connection::AppState;
use crate::errors::AppResult;
use crate::models::creator::{CreateCreatorRequest, Creator};
use std::sync::Arc;

/// Service for managing creator-related business logic.
/// Acts as an abstraction layer over the database controllers.
pub struct CreatorService;

impl CreatorService {
    pub fn new() -> Self {
        Self
    }

    /// Create a new creator and send a welcome email if an email is provided.
    #[tracing::instrument(
        name = "creator_service.create_creator",
        skip(self, state, req),
        fields(creator.username = %req.username)
    )]
    pub async fn create_creator(
        &self,
        state: Arc<AppState>,
        req: CreateCreatorRequest,
    ) -> AppResult<Creator> {
        let creator = creator_controller::create_creator(&state, req).await?;

        tracing::info!(creator.id = %creator.id, "creator created successfully");
        Ok(creator)
    }

    /// Retrieve a creator by their username.
    #[tracing::instrument(
        name = "creator_service.get_creator_by_username",
        skip(self, state),
        fields(creator.username = %username)
    )]
    pub async fn get_creator_by_username(
        &self,
        state: &AppState,
        username: &str,
    ) -> AppResult<Option<Creator>> {
        creator_controller::get_creator_by_username(state, username).await
    }
}
