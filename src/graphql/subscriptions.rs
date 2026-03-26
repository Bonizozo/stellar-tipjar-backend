use async_graphql::{Context, Result, SimpleObject, Subscription};
use async_graphql::futures_util::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use super::context::GraphQLContext;

/// A real-time tip notification delivered over GraphQL subscriptions.
#[derive(SimpleObject, Clone)]
pub struct TipNotification {
    pub creator_username: String,
    pub tipper_id: String,
    /// Amount in stroops (1 XLM = 10_000_000 stroops).
    pub amount: i64,
    pub timestamp: i64,
}

pub struct SubscriptionRoot;

#[Subscription]
impl SubscriptionRoot {
    /// Subscribe to new tips for a specific creator.
    async fn tip_received(
        &self,
        ctx: &Context<'_>,
        creator_username: String,
    ) -> Result<impl Stream<Item = TipNotification>> {
        let gql_ctx = ctx.data::<GraphQLContext>()?;
        let rx = gql_ctx.state.broadcast_tx.subscribe();

        let stream = BroadcastStream::new(rx).filter_map(move |result| {
            let username = creator_username.clone();
            match result {
                Ok(event) if event.creator_id == username => Some(TipNotification {
                    creator_username: event.creator_id,
                    tipper_id: event.tipper_id,
                    amount: event.amount as i64,
                    timestamp: event.timestamp,
                }),
                _ => None,
            }
        });

        Ok(stream)
    }
}
