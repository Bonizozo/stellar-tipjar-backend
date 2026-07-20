use async_graphql::Schema;
use std::sync::Arc;

use crate::db::connection::AppState;
use super::context::GraphQLContext;
use super::mutations::MutationRoot;
use super::queries::QueryRoot;
use super::subscriptions::SubscriptionRoot;

pub type AppSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;

/// Build the GraphQL schema with application state injected as context data.
pub fn build_schema(state: Arc<AppState>) -> AppSchema {
    Schema::build(QueryRoot, MutationRoot, SubscriptionRoot)
        .data(GraphQLContext::new(state))
        .finish()
}
