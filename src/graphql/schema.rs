use async_graphql::Schema;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse, GraphQLSubscription};
use axum::Extension;
use std::sync::Arc;

use crate::db::connection::AppState;
use super::context::GraphQLContext;
use super::mutations::MutationRoot;
use super::queries::QueryRoot;
use super::subscriptions::SubscriptionRoot;

pub type AppSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;

pub fn build_schema(state: Arc<AppState>) -> AppSchema {
    Schema::build(QueryRoot, MutationRoot, SubscriptionRoot)
        .data(GraphQLContext::new(state))
        .finish()
}

pub async fn graphql_handler(
    Extension(schema): Extension<AppSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

pub async fn graphql_ws_handler(
    Extension(schema): Extension<AppSchema>,
    protocol: GraphQLSubscription,
) -> impl axum::response::IntoResponse {
    protocol.on_upgrade(schema)
}
