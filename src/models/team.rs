use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Team {
    pub id: Uuid,
    pub name: String,
    pub owner_username: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TeamMember {
    pub team_id: Uuid,
    pub creator_username: String,
    pub share_percentage: i32,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TipSplit {
    pub id: Uuid,
    pub tip_id: Uuid,
    pub team_id: Uuid,
    pub member_username: String,
    pub amount: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct TeamMemberRequest {
    #[validate(length(min = 3, max = 30, message = "Username must be between 3 and 30 characters"))]
    pub creator_username: String,

    #[validate(range(min = 1, message = "Share percentage must be greater than 0"))]
    pub share_percentage: i32,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateTeamRequest {
    #[validate(length(min = 3, max = 50, message = "Team name must be between 3 and 50 characters"))]
    pub name: String,

    #[validate(length(min = 3, max = 30, message = "Owner username must be between 3 and 30 characters"))]
    pub owner_username: String,

    #[validate]
    pub members: Option<Vec<TeamMemberRequest>>,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateMemberShareRequest {
    #[validate(range(min = 1, message = "Share percentage must be greater than 0"))]
    pub share_percentage: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TeamMemberResponse {
    pub creator_username: String,
    pub share_percentage: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TeamResponse {
    pub id: Uuid,
    pub name: String,
    pub owner_username: String,
    pub members: Vec<TeamMemberResponse>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TipSplitResponse {
    pub id: Uuid,
    pub tip_id: Uuid,
    pub member_username: String,
    pub amount: String,
    pub created_at: DateTime<Utc>,
}

impl From<TeamMember> for TeamMemberResponse {
    fn from(member: TeamMember) -> Self {
        Self {
            creator_username: member.creator_username,
            share_percentage: member.share_percentage,
        }
    }
}

impl From<(Team, Vec<TeamMember>)> for TeamResponse {
    fn from((team, members): (Team, Vec<TeamMember>)) -> Self {
        Self {
            id: team.id,
            name: team.name,
            owner_username: team.owner_username,
            members: members.into_iter().map(TeamMemberResponse::from).collect(),
            created_at: team.created_at,
        }
    }
}

impl From<TipSplit> for TipSplitResponse {
    fn from(split: TipSplit) -> Self {
        Self {
            id: split.id,
            tip_id: split.tip_id,
            member_username: split.member_username,
            amount: split.amount,
            created_at: split.created_at,
        }
    }
}
