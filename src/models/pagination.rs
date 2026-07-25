use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, TimeZone, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::errors::AppError;

type HmacSha256 = Hmac<Sha256>;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;
const DEFAULT_MAX_OFFSET: i64 = 10_000;

fn default_limit() -> i64 {
    DEFAULT_LIMIT
}
fn default_max_offset() -> i64 {
    std::env::var("PAGINATION_MAX_OFFSET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_OFFSET)
}
fn cursor_secret() -> String {
    std::env::var("PAGINATION_CURSOR_SECRET")
        .or_else(|_| std::env::var("JWT_SECRET"))
        .unwrap_or_else(|_| "development-pagination-secret-change-me".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, IntoParams, ToSchema)]
pub struct PaginationParams {
    /// Page size, max 100 (default: 20).
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Opaque signed cursor returned as `next_cursor` or `prev_cursor`.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Cursor after which to fetch results. Alias for `cursor`.
    #[serde(default)]
    pub after: Option<String>,
    /// Cursor before which to fetch results.
    #[serde(default)]
    pub before: Option<String>,
    /// Deprecated offset page number retained only for compatibility.
    #[serde(default)]
    pub page: Option<i64>,
    /// Deprecated raw offset retained only for compatibility.
    #[serde(default)]
    pub offset: Option<i64>,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            cursor: None,
            after: None,
            before: None,
            page: None,
            offset: None,
        }
    }
}

impl PaginationParams {
    pub fn validated(mut self) -> Self {
        self.limit = self.limit.clamp(1, MAX_LIMIT);
        self
    }
    pub fn direction(&self) -> CursorDirection {
        if self.before.is_some() {
            CursorDirection::Before
        } else {
            CursorDirection::After
        }
    }
    pub fn active_cursor(&self) -> Option<&str> {
        self.before
            .as_deref()
            .or(self.after.as_deref())
            .or(self.cursor.as_deref())
    }
    pub fn uses_offset(&self) -> bool {
        self.page.is_some() || self.offset.is_some()
    }
    pub fn offset(&self) -> i64 {
        let requested = self
            .offset
            .unwrap_or_else(|| self.page.map(|p| (p.max(1) - 1) * self.limit).unwrap_or(0));
        requested.clamp(0, default_max_offset())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorDirection {
    After,
    Before,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeysetCursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

impl KeysetCursor {
    pub fn new(created_at: DateTime<Utc>, id: Uuid) -> Self {
        Self { created_at, id }
    }

    pub fn encode(&self) -> String {
        let payload = format!("{}|{}", self.created_at.timestamp_micros(), self.id);
        let mut mac = HmacSha256::new_from_slice(cursor_secret().as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(payload.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());
        URL_SAFE_NO_PAD.encode(format!("{payload}|{sig}"))
    }

    pub fn decode(token: &str) -> Result<Self, AppError> {
        let decoded = URL_SAFE_NO_PAD
            .decode(token)
            .map_err(|_| AppError::bad_request("Invalid cursor format"))?;
        let s = String::from_utf8(decoded)
            .map_err(|_| AppError::bad_request("Invalid cursor encoding"))?;
        let parts: Vec<&str> = s.split('|').collect();
        if parts.len() != 3 {
            return Err(AppError::bad_request("Invalid cursor structure"));
        }
        let payload = format!("{}|{}", parts[0], parts[1]);
        let mut mac = HmacSha256::new_from_slice(cursor_secret().as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(payload.as_bytes());
        let expected = hex::encode(mac.finalize().into_bytes());
        if expected != parts[2] {
            return Err(AppError::bad_request("Invalid cursor signature"));
        }
        let micros = parts[0]
            .parse::<i64>()
            .map_err(|_| AppError::bad_request("Invalid cursor timestamp"))?;
        let id =
            Uuid::parse_str(parts[1]).map_err(|_| AppError::bad_request("Invalid cursor id"))?;
        let created_at = Utc
            .timestamp_micros(micros)
            .single()
            .ok_or_else(|| AppError::bad_request("Invalid cursor timestamp"))?;
        Ok(Self { created_at, id })
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
    pub prev_cursor: Option<String>,
    pub has_more: bool,
}

impl<T: Serialize> PaginatedResponse<T> {
    pub fn keyset(
        mut items: Vec<T>,
        limit: i64,
        mut cursor_for: impl FnMut(&T) -> KeysetCursor,
    ) -> Self {
        let has_more = items.len() > limit as usize;
        if has_more {
            items.truncate(limit as usize);
        }
        let next_cursor = items.last().map(&mut cursor_for).map(|c| c.encode());
        let prev_cursor = items.first().map(&mut cursor_for).map(|c| c.encode());
        Self {
            items,
            next_cursor,
            prev_cursor,
            has_more,
        }
    }
    pub fn map<U: Serialize, F: Fn(T) -> U>(self, f: F) -> PaginatedResponse<U> {
        PaginatedResponse {
            items: self.items.into_iter().map(f).collect(),
            next_cursor: self.next_cursor,
            prev_cursor: self.prev_cursor,
            has_more: self.has_more,
        }
    }
}
