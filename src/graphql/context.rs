use std::sync::Arc;
use async_graphql::dataloader::DataLoader;
use crate::db::connection::AppState;
use super::dataloaders::{CreatorLoader, TipLoader};

pub struct GraphQLContext {
    pub state: Arc<AppState>,
    pub creator_loader: DataLoader<CreatorLoader>,
    pub tip_loader: DataLoader<TipLoader>,
}

impl GraphQLContext {
    pub fn new(state: Arc<AppState>) -> Self {
        let creator_loader = DataLoader::new(
            CreatorLoader { pool: state.db.clone() },
            tokio::spawn,
        );
        let tip_loader = DataLoader::new(
            TipLoader { pool: state.db.clone() },
            tokio::spawn,
        );
        Self { state, creator_loader, tip_loader }
    }
}
