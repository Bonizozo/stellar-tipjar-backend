use crate::errors::app_error::AppError;
use redis::aio::ConnectionManager;

pub struct CursorManager {
    redis: ConnectionManager,
}

impl CursorManager {
    pub fn new(redis: ConnectionManager) -> Self {
        Self { redis }
    }

    pub async fn save_cursor(&self, cursor: &str) -> Result<(), AppError> {
        redis::cmd("SET")
            .arg("indexer:cursor")
            .arg(cursor)
            .query_async::<()>(&mut self.redis.clone())
            .await
            .map_err(|e| AppError::database_error(e.to_string()))
    }

    pub async fn get_cursor(&self) -> Result<Option<String>, AppError> {
        redis::cmd("GET")
            .arg("indexer:cursor")
            .query_async::<Option<String>>(&mut self.redis.clone())
            .await
            .map_err(|e| AppError::database_error(e.to_string()))
    }
}
