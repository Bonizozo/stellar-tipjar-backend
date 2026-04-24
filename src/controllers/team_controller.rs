use rust_decimal::Decimal;
use std::collections::HashSet;
use uuid::Uuid;

use crate::db::connection::AppState;
use crate::db::transaction;
use crate::errors::{AppError, AppResult};
use crate::models::team::{CreateTeamRequest, Team, TeamMember, TeamMemberRequest, TeamResponse, TipSplit, TipSplitResponse};

#[tracing::instrument(skip(state), fields(team_name = %req.name, owner = %req.owner_username))]
pub async fn create_team(state: &AppState, req: CreateTeamRequest) -> AppResult<TeamResponse> {
    let members = req.members.unwrap_or_default();
    let mut member_requests = members;

    if member_requests.is_empty() {
        member_requests.push(TeamMemberRequest {
            creator_username: req.owner_username.clone(),
            share_percentage: 100,
        });
    }

    let mut seen = HashSet::new();
    let mut total_share: i32 = 0;
    for member in &member_requests {
        if !seen.insert(member.creator_username.clone()) {
            return Err(AppError::Validation(crate::errors::ValidationError::InvalidRequest {
                message: "Duplicate team member username".to_string(),
            }));
        }
        total_share += member.share_percentage;
    }
    if total_share <= 0 {
        return Err(AppError::Validation(crate::errors::ValidationError::InvalidRequest {
            message: "Team member share percentages must total more than zero".to_string(),
        }));
    }

    let mut tx = transaction::begin_transaction(&state.db).await.map_err(AppError::from)?;

    let team = sqlx::query_as::<_, Team>(
        r#"INSERT INTO teams (id, name, owner_username, created_at)
        VALUES ($1, $2, $3, NOW())
        RETURNING id, name, owner_username, created_at"#,
    )
    .bind(Uuid::new_v4())
    .bind(&req.name)
    .bind(&req.owner_username)
    .fetch_one(&mut *tx)
    .await?;

    let mut members_rows = Vec::new();
    for member in member_requests {
        let row = sqlx::query_as::<_, TeamMember>(
            r#"INSERT INTO team_members (team_id, creator_username, share_percentage, added_at)
            VALUES ($1, $2, $3, NOW())
            RETURNING team_id, creator_username, share_percentage, added_at"#,
        )
        .bind(team.id)
        .bind(&member.creator_username)
        .bind(member.share_percentage)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(db_err) = &e {
                if db_err.code().as_deref() == Some("23505") {
                    return AppError::Conflict {
                        code: "TEAM_MEMBER_CONFLICT",
                        message: "A team member already belongs to another team or this member is already added".to_string(),
                    };
                }
            }
            AppError::from(e)
        })?;
        members_rows.push(row);
    }

    tx.commit().await?;

    Ok(TeamResponse::from((team, members_rows)))
}

#[tracing::instrument(skip(state), fields(team_id = %team_id))]
pub async fn get_team(state: &AppState, team_id: Uuid) -> AppResult<TeamResponse> {
    let team = sqlx::query_as::<_, Team>(
        "SELECT id, name, owner_username, created_at FROM teams WHERE id = $1",
    )
    .bind(team_id)
    .fetch_one(&state.db)
    .await?;

    let members = sqlx::query_as::<_, TeamMember>(
        "SELECT team_id, creator_username, share_percentage, added_at FROM team_members WHERE team_id = $1 ORDER BY creator_username ASC",
    )
    .bind(team_id)
    .fetch_all(&state.db)
    .await?;

    Ok(TeamResponse::from((team, members)))
}

#[tracing::instrument(skip(state))]
pub async fn list_teams(state: &AppState) -> AppResult<Vec<TeamResponse>> {
    let teams = sqlx::query_as::<_, Team>(
        "SELECT id, name, owner_username, created_at FROM teams ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;

    let mut results = Vec::with_capacity(teams.len());
    for team in teams {
        let members = sqlx::query_as::<_, TeamMember>(
            "SELECT team_id, creator_username, share_percentage, added_at FROM team_members WHERE team_id = $1 ORDER BY creator_username ASC",
        )
        .bind(team.id)
        .fetch_all(&state.db)
        .await?;
        results.push(TeamResponse::from((team, members)));
    }

    Ok(results)
}

#[tracing::instrument(skip(state), fields(team_id = %team_id, username = %member_username))]
pub async fn add_member(
    state: &AppState,
    team_id: Uuid,
    member: TeamMemberRequest,
) -> AppResult<TeamMember> {
    let row = sqlx::query_as::<_, TeamMember>(
        r#"INSERT INTO team_members (team_id, creator_username, share_percentage, added_at)
        VALUES ($1, $2, $3, NOW())
        RETURNING team_id, creator_username, share_percentage, added_at"#,
    )
    .bind(team_id)
    .bind(&member.creator_username)
    .bind(member.share_percentage)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db_err) = &e {
            if db_err.code().as_deref() == Some("23505") {
                return AppError::Conflict {
                    code: "TEAM_MEMBER_CONFLICT",
                    message: "A team member already belongs to another team or is already a member of this team".to_string(),
                };
            }
        }
        AppError::from(e)
    })?;

    Ok(row)
}

#[tracing::instrument(skip(state), fields(team_id = %team_id, username = %member_username))]
pub async fn update_member_share(
    state: &AppState,
    team_id: Uuid,
    member_username: String,
    share_percentage: i32,
) -> AppResult<TeamMember> {
    let row = sqlx::query_as::<_, TeamMember>(
        r#"UPDATE team_members
        SET share_percentage = $1
        WHERE team_id = $2 AND creator_username = $3
        RETURNING team_id, creator_username, share_percentage, added_at"#,
    )
    .bind(share_percentage)
    .bind(team_id)
    .bind(&member_username)
    .fetch_one(&state.db)
    .await?;

    Ok(row)
}

#[tracing::instrument(skip(state), fields(team_id = %team_id, username = %member_username))]
pub async fn remove_member(
    state: &AppState,
    team_id: Uuid,
    member_username: String,
) -> AppResult<()> {
    sqlx::query(
        "DELETE FROM team_members WHERE team_id = $1 AND creator_username = $2",
    )
    .bind(team_id)
    .bind(&member_username)
    .execute(&state.db)
    .await?;
    Ok(())
}

#[tracing::instrument(skip(state), fields(team_id = %team_id))]
pub async fn get_split_history(state: &AppState, team_id: Uuid) -> AppResult<Vec<TipSplitResponse>> {
    let splits = sqlx::query_as::<_, TipSplit>(
        "SELECT id, tip_id, team_id, member_username, amount, created_at FROM tip_splits WHERE team_id = $1 ORDER BY created_at DESC",
    )
    .bind(team_id)
    .fetch_all(&state.db)
    .await?;

    Ok(splits.into_iter().map(TipSplitResponse::from).collect())
}

#[tracing::instrument(skip(state), fields(tip_id = %tip_id, amount = %amount))]
pub async fn record_tip_splits(
    state: &AppState,
    tip_id: Uuid,
    recipient_username: &str,
    amount: &str,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> AppResult<()> {
    let team_id = sqlx::query_scalar(
        "SELECT team_id FROM team_members WHERE creator_username = $1",
    )
    .bind(recipient_username)
    .fetch_optional(&mut *tx)
    .await?;

    let team_id = if let Some(team_id) = team_id {
        team_id
    } else {
        return Ok(());
    };

    let members = sqlx::query_as::<_, TeamMember>(
        "SELECT team_id, creator_username, share_percentage, added_at FROM team_members WHERE team_id = $1 ORDER BY creator_username ASC",
    )
    .bind(team_id)
    .fetch_all(&mut *tx)
    .await?;

    if members.is_empty() {
        return Ok(());
    }

    let amount_decimal: Decimal = amount.parse().map_err(|_| AppError::Validation(crate::errors::ValidationError::InvalidRequest {
        message: "Invalid tip amount for split calculation".to_string(),
    }))?;

    let total_shares: i32 = members.iter().map(|m| m.share_percentage).sum();
    if total_shares <= 0 {
        return Ok(());
    }

    let mut accumulated = Decimal::ZERO;
    for (idx, member) in members.iter().enumerate() {
        let member_amount = if idx == members.len() - 1 {
            amount_decimal - accumulated
        } else {
            let share = Decimal::from(member.share_percentage);
            let percent = share / Decimal::from(total_shares);
            let split = (amount_decimal * percent).round_dp(7);
            accumulated += split;
            split
        };

        sqlx::query(
            "INSERT INTO tip_splits (id, tip_id, team_id, member_username, amount, created_at) VALUES ($1, $2, $3, $4, $5, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(tip_id)
        .bind(team_id)
        .bind(&member.creator_username)
        .bind(member_amount.to_string())
        .execute(&mut *tx)
        .await?;
    }

    Ok(())
}
