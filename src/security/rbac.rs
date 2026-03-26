use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use super::permissions::{Permission, Role};

pub struct RBACSystem {
    pub pool: PgPool,
}

impl RBACSystem {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn assign_role(&self, user_id: Uuid, role: &Role) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO user_roles (user_id, role) VALUES ($1, $2)
             ON CONFLICT (user_id) DO UPDATE SET role = $2, assigned_at = NOW()",
        )
        .bind(user_id)
        .bind(role.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_role(&self, user_id: Uuid) -> AppResult<Role> {
        let role_str: Option<String> = sqlx::query_scalar(
            "SELECT role FROM user_roles WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        let role = role_str
            .as_deref()
            .and_then(|s| Role::try_from(s).ok())
            .unwrap_or(Role::Supporter);

        Ok(role)
    }

    pub async fn has_permission(&self, user_id: Uuid, permission: &Permission) -> AppResult<bool> {
        let role = self.get_role(user_id).await?;
        Ok(permission.allowed_for(&role))
    }

    pub async fn require_permission(&self, user_id: Uuid, permission: &Permission) -> AppResult<()> {
        if !self.has_permission(user_id, permission).await? {
            return Err(AppError::forbidden("Insufficient permissions"));
        }
        Ok(())
    }
}
