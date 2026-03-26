/// All roles in the system, ordered from least to most privileged.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "text")]
pub enum Role {
    Guest,
    Supporter,
    Creator,
    Moderator,
    Admin,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Guest => "guest",
            Role::Supporter => "supporter",
            Role::Creator => "creator",
            Role::Moderator => "moderator",
            Role::Admin => "admin",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for Role {
    type Error = ();
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "guest" => Ok(Role::Guest),
            "supporter" => Ok(Role::Supporter),
            "creator" => Ok(Role::Creator),
            "moderator" => Ok(Role::Moderator),
            "admin" => Ok(Role::Admin),
            _ => Err(()),
        }
    }
}

/// Fine-grained permissions checked at the handler level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    CreateCreator,
    UpdateCreator,
    DeleteCreator,
    SendTip,
    WithdrawFunds,
    ViewAnalytics,
    ManageUsers,
    ModerateContent,
}

impl Permission {
    /// Returns true if `role` is allowed this permission.
    pub fn allowed_for(&self, role: &Role) -> bool {
        match role {
            Role::Admin => true,
            Role::Moderator => matches!(
                self,
                Permission::ModerateContent | Permission::ViewAnalytics
            ),
            Role::Creator => matches!(
                self,
                Permission::CreateCreator | Permission::UpdateCreator | Permission::WithdrawFunds | Permission::ViewAnalytics
            ),
            Role::Supporter => matches!(self, Permission::SendTip),
            Role::Guest => false,
        }
    }
}
