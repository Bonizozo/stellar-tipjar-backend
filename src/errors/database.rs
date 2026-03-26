use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
	#[error("{entity} not found")]
	NotFound { entity: &'static str, identifier: String },
	#[error("resource already exists")]
	UniqueViolation { field: String },
	#[error("database query failed")]
	QueryFailed,
}

impl DatabaseError {
	pub fn from_sqlx(err: &sqlx::Error) -> Self {
		match err {
			sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23505") => {
				let field = db_err
					.constraint()
					.map(|name| name.to_string())
					.unwrap_or_else(|| "unknown".to_string());
				Self::UniqueViolation { field }
			}
			_ => Self::QueryFailed,
		}
	}

	pub fn code(&self) -> &'static str {
		match self {
			Self::NotFound { .. } => "DB_NOT_FOUND",
			Self::UniqueViolation { .. } => "DB_UNIQUE_VIOLATION",
			Self::QueryFailed => "DB_QUERY_FAILED",
		}
	}

	pub fn message(&self) -> String {
		match self {
			Self::NotFound { entity, .. } => format!("{} not found", entity),
			Self::UniqueViolation { field } => {
				format!("A record with this {} already exists", field)
			}
			Self::QueryFailed => "Unable to complete database operation".to_string(),
		}
	}

	pub fn details(&self) -> serde_json::Value {
		match self {
			Self::NotFound { entity, identifier } => {
				json!({ "entity": entity, "identifier": identifier })
			}
			Self::UniqueViolation { field } => json!({ "field": field }),
			Self::QueryFailed => json!({}),
		}
	}
}

