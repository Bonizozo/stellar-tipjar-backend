use async_graphql::{Context, InputObject, Object, Result};
use validator::Validate;

use super::context::GraphQLContext;
use super::queries::{GqlCreator, GqlTip};
use crate::controllers::creator_controller;
use crate::models::creator::CreateCreatorRequest;
use crate::models::tip::RecordTipRequest;
use crate::services::tip_service::TipService;

#[derive(InputObject)]
pub struct CreateCreatorInput {
    pub username: String,
    pub wallet_address: String,
    pub email: Option<String>,
}

#[derive(InputObject)]
pub struct RecordTipInput {
    pub username: String,
    pub amount: String,
    pub transaction_hash: String,
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    async fn create_creator(
        &self,
        ctx: &Context<'_>,
        input: CreateCreatorInput,
    ) -> Result<GqlCreator> {
        let gql_ctx = ctx.data::<GraphQLContext>()?;
        let req = CreateCreatorRequest {
            username: input.username,
            wallet_address: input.wallet_address,
            email: input.email,
        };
        req.validate()
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        let creator = creator_controller::create_creator(&gql_ctx.state, req)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(GqlCreator::from(creator))
    }

    async fn record_tip(&self, ctx: &Context<'_>, input: RecordTipInput) -> Result<GqlTip> {
        let gql_ctx = ctx.data::<GraphQLContext>()?;

        gql_ctx
            .state
            .stellar
            .verify_transaction(&input.transaction_hash)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let req = RecordTipRequest {
            username: input.username,
            amount: input.amount,
            transaction_hash: input.transaction_hash,
            message: None,
        };
        req.validate()
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;

        let tip = TipService::new()
            .record_tip(gql_ctx.state.clone(), req)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(GqlTip::from(tip))
    }
}
