use super::queries::{Query, QueryResult};
use crate::controllers::{creator_controller, tip_controller};
use crate::db::connection::AppState;
use crate::errors::AppResult;
use std::sync::Arc;

use super::projections::CqrsProjection;

/// Executes read-side queries against the (optionally separate) read pool.
///
/// In this deployment both read and write share the same `AppState` pool.
/// To point reads at a replica, swap `state.db` for a replica `PgPool` here.
pub struct QueryBus {
    state: Arc<AppState>,
    projection: Option<Arc<CqrsProjection>>,
}

impl QueryBus {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            projection: None,
        }
    }

    pub fn with_projection(state: Arc<AppState>, projection: Arc<CqrsProjection>) -> Self {
        Self {
            state,
            projection: Some(projection),
        }
    }

    pub async fn execute(&self, query: Query) -> AppResult<QueryResult> {
        match query {
            Query::GetCreator { username } => {
                let creator =
                    creator_controller::get_creator_by_username(&self.state, &username).await?;
                Ok(QueryResult::Creator(creator))
            }

            Query::ListTipsForCreator { username, params } => {
                let page =
                    tip_controller::get_tips_paginated(&self.state, &username, params).await?;
                Ok(QueryResult::Tips(page))
            }

            Query::GetCreatorTipCount { creator_id } => {
                let count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM tips t \
                     JOIN creators c ON c.username = t.creator_username \
                     WHERE c.id = $1",
                )
                .bind(creator_id)
                .fetch_one(&self.state.db)
                .await?;
                Ok(QueryResult::TipCount(count))
            }
            Query::GetCreatorSummary { username } => {
                if let Some(projection) = &self.projection {
                    let summary = projection
                        .get_creator_summary_by_username(&username)
                        .await?;
                    Ok(QueryResult::CreatorSummary(summary))
                } else {
                    Ok(QueryResult::CreatorSummary(None))
                }
            }
        }
    }
}
