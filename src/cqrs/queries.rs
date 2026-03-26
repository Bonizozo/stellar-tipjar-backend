use uuid::Uuid;
use crate::models::{creator::Creator, tip::Tip};
use crate::models::pagination::{PaginatedResponse, PaginationParams};

/// All read-side intents in the system.
#[derive(Debug)]
pub enum Query {
    GetCreator { username: String },
    ListTipsForCreator { username: String, params: PaginationParams },
    GetCreatorTipCount { creator_id: Uuid },
}

/// The result of executing a query.
#[derive(Debug)]
pub enum QueryResult {
    Creator(Option<Creator>),
    Tips(PaginatedResponse<Tip>),
    TipCount(i64),
}
