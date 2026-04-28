use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Session expired: {0}")]
    SessionExpired(String),
    #[error("Invalid session format: {0}")]
    InvalidSessionFormat(String),
    #[error("Redis operation failed: {0}")]
    RedisError(String),
    #[error("Session creation failed: {0}")]
    CreationFailed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub session_ttl_seconds: u64,
    pub idle_timeout_seconds: u64,
    pub absolute_timeout_seconds: u64,
    pub cleanup_interval_seconds: u64,
    pub max_sessions_per_user: u64,
    pub cookie_name: String,
    pub secure_cookies: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            session_ttl_seconds: 3600, // 1 hour
            idle_timeout_seconds: 1800, // 30 minutes
            absolute_timeout_seconds: 86400, // 24 hours
            cleanup_interval_seconds: 300, // 5 minutes
            max_sessions_per_user: 5,
            cookie_name: "session_id".to_string(),
            secure_cookies: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub session_id: String,
    pub user_id: String,
    pub client_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub is_active: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAnalytics {
    pub total_sessions: u64,
    pub active_sessions: u64,
    pub expired_sessions: u64,
    pub sessions_by_user: HashMap<String, u64>,
    pub sessions_by_client: HashMap<String, u64>,
    pub average_session_duration: Duration,
}

impl SessionData {
    pub fn new(user_id: String, client_id: Option<String>, ip_address: Option<String>, user_agent: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            session_id: Uuid::new_v4().to_string(),
            user_id,
            client_id,
            ip_address,
            user_agent,
            created_at: now,
            last_accessed: now,
            expires_at: now + Duration::hours(1), // Default TTL
            is_active: true,
            metadata: HashMap::new(),
        }
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    pub fn is_idle_expired(&self, idle_timeout: Duration) -> bool {
        let idle_duration = Utc::now() - self.last_accessed;
        idle_duration > idle_timeout
    }

    pub fn touch(&mut self) {
        self.last_accessed = Utc::now();
    }

    pub fn extend_ttl(&mut self, additional_time: Duration) {
        self.expires_at = Utc::now() + additional_time;
    }

    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }
}

pub struct SessionManager {
    redis: Option<Arc<ConnectionManager>>,
    config: SessionConfig,
}

impl SessionManager {
    pub fn new(redis: Option<Arc<ConnectionManager>>, config: SessionConfig) -> Self {
        Self { redis, config }
    }

    pub fn with_redis(redis: Arc<ConnectionManager>) -> Self {
        Self::new(Some(redis), SessionConfig::default())
    }

    pub fn without_redis() -> Self {
        Self::new(None, SessionConfig::default())
    }

    /// Create a new session
    pub async fn create_session(
        &self,
        user_id: &str,
        client_id: Option<&str>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<SessionData, SessionError> {
        let mut session = SessionData::new(
            user_id.to_string(),
            client_id.map(|s| s.to_string()),
            ip_address.map(|s| s.to_string()),
            user_agent.map(|s| s.to_string()),
        );
        
        session.expires_at = Utc::now() + Duration::seconds(self.config.session_ttl_seconds as i64);

        // Check if user has too many sessions
        if let Ok(user_sessions) = self.get_user_sessions(user_id).await {
            if user_sessions.len() as u64 >= self.config.max_sessions_per_user {
                // Remove oldest session
                if let Some(oldest_session) = user_sessions.iter().min_by_key(|s| s.created_at) {
                    self.delete_session(&oldest_session.session_id).await?;
                }
            }
        }

        // Store session
        if let Some(redis) = &self.redis {
            self.store_session_redis(&session).await?;
        }

        Ok(session)
    }

    async fn store_session_redis(&self, session: &SessionData) -> Result<(), SessionError> {
        let redis = self.redis.as_ref().unwrap();
        let mut conn = redis.clone();

        let session_key = format!("session:{}", session.session_id);
        let user_sessions_key = format!("user_sessions:{}", session.user_id);
        
        let serialized = serde_json::to_string(session)
            .map_err(|e| SessionError::CreationFailed(e.to_string()))?;

        // Store session data
        redis::cmd("SETEX")
            .arg(&session_key)
            .arg(self.config.session_ttl_seconds)
            .arg(&serialized)
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| SessionError::RedisError(e.to_string()))?;

        // Add to user sessions set
        redis::cmd("SADD")
            .arg(&user_sessions_key)
            .arg(&session.session_id)
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| SessionError::RedisError(e.to_string()))?;

        // Set expiration on user sessions set
        redis::cmd("EXPIRE")
            .arg(&user_sessions_key)
            .arg(self.config.absolute_timeout_seconds)
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| SessionError::RedisError(e.to_string()))?;

        Ok(())
    }

    /// Get a session by ID
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionData>, SessionError> {
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let session_key = format!("session:{}", session_id);

            let value: Option<String> = redis::cmd("GET")
                .arg(&session_key)
                .query_async(&mut conn)
                .await
                .map_err(|e| SessionError::RedisError(e.to_string()))?;

            if let Some(value) = value {
                let session: SessionData = serde_json::from_str(&value)
                    .map_err(|e| SessionError::InvalidSessionFormat(e.to_string()))?;

                if session.is_expired() {
                    self.delete_session(session_id).await?;
                    return Ok(None);
                }

                Ok(Some(session))
            } else {
                Ok(None)
            }
        } else {
            Ok(None) // Return None if Redis is not available
        }
    }

    /// Update session access time
    pub async fn touch_session(&self, session_id: &str) -> Result<Option<SessionData>, SessionError> {
        if let Some(mut session) = self.get_session(session_id).await? {
            session.touch();
            
            if let Some(redis) = &self.redis {
                self.update_session_redis(&session).await?;
            }

            Ok(Some(session))
        } else {
            Ok(None)
        }
    }

    async fn update_session_redis(&self, session: &SessionData) -> Result<(), SessionError> {
        let redis = self.redis.as_ref().unwrap();
        let mut conn = redis.clone();

        let session_key = format!("session:{}", session.session_id);
        let serialized = serde_json::to_string(session)
            .map_err(|e| SessionError::RedisError(e.to_string()))?;

        redis::cmd("SETEX")
            .arg(&session_key)
            .arg(self.config.session_ttl_seconds)
            .arg(&serialized)
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| SessionError::RedisError(e.to_string()))?;

        Ok(())
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> Result<(), SessionError> {
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let session_key = format!("session:{}", session_id);

            // Get session data to update user sessions
            if let Ok(Some(session)) = self.get_session(session_id).await {
                let user_sessions_key = format!("user_sessions:{}", session.user_id);
                
                // Remove from user sessions set
                redis::cmd("SREM")
                    .arg(&user_sessions_key)
                    .arg(session_id)
                    .query_async::<_, ()>(&mut conn)
                    .await
                    .map_err(|e| SessionError::RedisError(e.to_string()))?;
            }

            // Delete session data
            redis::cmd("DEL")
                .arg(&session_key)
                .query_async::<_, ()>(&mut conn)
                .await
                .map_err(|e| SessionError::RedisError(e.to_string()))?;
        }

        Ok(())
    }

    /// Get all sessions for a user
    pub async fn get_user_sessions(&self, user_id: &str) -> Result<Vec<SessionData>, SessionError> {
        let mut sessions = Vec::new();

        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let user_sessions_key = format!("user_sessions:{}", user_id);

            let session_ids: Vec<String> = redis::cmd("SMEMBERS")
                .arg(&user_sessions_key)
                .query_async(&mut conn)
                .await
                .map_err(|e| SessionError::RedisError(e.to_string()))?;

            for session_id in session_ids {
                if let Ok(Some(session)) = self.get_session(&session_id).await {
                    sessions.push(session);
                }
            }
        }

        Ok(sessions)
    }

    /// Delete all sessions for a user
    pub async fn delete_user_sessions(&self, user_id: &str) -> Result<u64, SessionError> {
        let sessions = self.get_user_sessions(user_id).await?;
        let mut deleted_count = 0;

        for session in sessions {
            self.delete_session(&session.session_id).await?;
            deleted_count += 1;
        }

        Ok(deleted_count)
    }

    /// Refresh session (extend TTL)
    pub async fn refresh_session(&self, session_id: &str, additional_time: Option<Duration>) -> Result<Option<SessionData>, SessionError> {
        if let Some(mut session) = self.get_session(session_id).await? {
            let additional = additional_time.unwrap_or(Duration::seconds(self.config.session_ttl_seconds as i64));
            session.extend_ttl(additional);
            session.touch();

            if let Some(redis) = &self.redis {
                self.update_session_redis(&session).await?;
            }

            Ok(Some(session))
        } else {
            Ok(None)
        }
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self) -> Result<u64, SessionError> {
        let mut cleaned_count = 0;

        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let pattern = "session:*";

            let keys: Vec<String> = redis::cmd("KEYS")
                .arg(pattern)
                .query_async(&mut conn)
                .await
                .map_err(|e| SessionError::RedisError(e.to_string()))?;

            for key in keys {
                if let Ok(Some(session)) = self.get_session(&key.strip_prefix("session:").unwrap_or(&key)).await {
                    if session.is_expired() || session.is_idle_expired(Duration::seconds(self.config.idle_timeout_seconds as i64)) {
                        self.delete_session(&session.session_id).await?;
                        cleaned_count += 1;
                    }
                }
            }
        }

        Ok(cleaned_count)
    }

    /// Get session analytics
    pub async fn get_session_analytics(&self) -> Result<SessionAnalytics, SessionError> {
        let mut analytics = SessionAnalytics {
            total_sessions: 0,
            active_sessions: 0,
            expired_sessions: 0,
            sessions_by_user: HashMap::new(),
            sessions_by_client: HashMap::new(),
            average_session_duration: Duration::zero(),
        };

        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let pattern = "session:*";

            let keys: Vec<String> = redis::cmd("KEYS")
                .arg(pattern)
                .query_async(&mut conn)
                .await
                .map_err(|e| SessionError::RedisError(e.to_string()))?;

            let mut total_duration = Duration::zero();
            let mut valid_sessions = 0;

            for key in keys {
                if let Ok(Some(session)) = self.get_session(&key.strip_prefix("session:").unwrap_or(&key)).await {
                    analytics.total_sessions += 1;

                    if session.is_active && !session.is_expired() {
                        analytics.active_sessions += 1;
                        valid_sessions += 1;

                        // Count by user
                        *analytics.sessions_by_user.entry(session.user_id.clone()).or_insert(0) += 1;

                        // Count by client
                        if let Some(ref client_id) = session.client_id {
                            *analytics.sessions_by_client.entry(client_id.clone()).or_insert(0) += 1;
                        }

                        // Calculate duration
                        let duration = session.last_accessed - session.created_at;
                        total_duration = total_duration + duration;
                    } else {
                        analytics.expired_sessions += 1;
                    }
                }
            }

            if valid_sessions > 0 {
                analytics.average_session_duration = total_duration / valid_sessions as i32;
            }
        }

        Ok(analytics)
    }

    /// Validate session and update access time
    pub async fn validate_session(&self, session_id: &str) -> Result<Option<SessionData>, SessionError> {
        if let Some(session) = self.get_session(session_id).await? {
            if session.is_expired() {
                self.delete_session(session_id).await?;
                return Err(SessionError::SessionExpired(session_id.to_string()));
            }

            if session.is_idle_expired(Duration::seconds(self.config.idle_timeout_seconds as i64)) {
                self.delete_session(session_id).await?;
                return Err(SessionError::SessionExpired(session_id.to_string()));
            }

            // Update access time
            let updated_session = self.touch_session(session_id).await?;
            Ok(updated_session)
        } else {
            Err(SessionError::SessionNotFound(session_id.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_session_data_creation() {
        let session = SessionData::new(
            "user123".to_string(),
            Some("client1".to_string()),
            Some("127.0.0.1".to_string()),
            Some("Mozilla/5.0".to_string()),
        );

        assert_eq!(session.user_id, "user123");
        assert_eq!(session.client_id, Some("client1".to_string()));
        assert!(session.is_active);
        assert!(!session.is_expired());
    }

    #[test]
    fn test_session_expiration() {
        let mut session = SessionData::new(
            "user123".to_string(),
            None,
            None,
            None,
        );

        // Set expiration to past
        session.expires_at = Utc::now() - Duration::hours(1);
        assert!(session.is_expired());

        // Set expiration to future
        session.expires_at = Utc::now() + Duration::hours(1);
        assert!(!session.is_expired());
    }

    #[test]
    fn test_session_touch() {
        let mut session = SessionData::new(
            "user123".to_string(),
            None,
            None,
            None,
        );

        let original_last_accessed = session.last_accessed;
        
        // Small delay to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));
        
        session.touch();
        assert!(session.last_accessed > original_last_accessed);
    }

    #[tokio::test]
    async fn test_session_manager_without_redis() {
        let manager = SessionManager::without_redis();
        
        let session = manager.create_session("user123", Some("client1"), None, None).await.unwrap();
        assert_eq!(session.user_id, "user123");
        
        // Should return None without Redis
        let retrieved = manager.get_session(&session.session_id).await.unwrap();
        assert!(retrieved.is_none());
    }
}
