use crate::models::pagination::{PaginatedResponse, PaginationParams};
use crate::models::{creator::Creator, tip::Tip};
use uuid::Uuid;

use super::projections::CreatorSummaryView;

/// All read-side intents in the system.
#[derive(Debug)]
pub enum Query {
    GetCreator {
        username: String,
    },
    ListTipsForCreator {
        username: String,
        params: PaginationParams,
    },
    GetCreatorTipCount {
        creator_id: Uuid,
    },
    GetCreatorSummary {
        username: String,
    },
}

/// The result of executing a query.
#[derive(Debug)]
pub enum QueryResult {
    Creator(Option<Creator>),
    Tips(PaginatedResponse<Tip>),
    TipCount(i64),
    CreatorSummary(Option<CreatorSummaryView>),
}
