use chrono::Utc;
use data_encoding::BASE32_NOPAD;
use jsonwebtoken::{
    decode, decode_header, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::collections::HashMap;
use totp_lite::{totp_custom, Sha1};
use uuid::Uuid;

use crate::errors::{AppError, AppResult};
use crate::models::auth::{AuthResponse, Claims};

/// Access tokens are short-lived — a stolen one has a tight window before it
/// expires on its own, on top of the jti denylist checked in middleware.
const ACCESS_TOKEN_SECS: i64 = 60 * 15;
const REFRESH_TOKEN_SECS: i64 = 60 * 60 * 24 * 7;

/// Explicit clock-skew allowance applied to `exp`/`nbf` validation.
const CLOCK_SKEW_LEEWAY_SECS: u64 = 30;

const DEFAULT_ISSUER: &str = "stellar-tipjar-backend";
const DEFAULT_AUDIENCE: &str = "stellar-tipjar-api";
const LEGACY_KID: &str = "default";

fn jwt_issuer() -> String {
    std::env::var("JWT_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string())
}

fn jwt_audience() -> String {
    std::env::var("JWT_AUDIENCE").unwrap_or_else(|_| DEFAULT_AUDIENCE.to_string())
}

/// The set of HS256 signing keys known to this instance, keyed by `kid`.
/// Supports overlap during rotation: multiple keys may be present at once,
/// but only `active_kid` is used to *sign* new tokens. See
/// `docs/SESSION_SECURITY.md` for the rotation runbook.
struct KeySet {
    keys: HashMap<String, String>,
    active_kid: String,
}

/// Loads the signing keyset from `JWT_SIGNING_KEYS` (format:
/// `kid1:secret1,kid2:secret2`) plus `JWT_ACTIVE_KID`. Falls back to a single
/// legacy key read from `JWT_SECRET` when `JWT_SIGNING_KEYS` is unset, so
/// existing single-secret deployments keep working unchanged.
fn load_keyset() -> AppResult<KeySet> {
    if let Ok(raw) = std::env::var("JWT_SIGNING_KEYS") {
        let mut keys = HashMap::new();
        for pair in raw.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let mut parts = pair.splitn(2, ':');
            let kid = parts.next().unwrap_or("").trim();
            let secret = parts.next().unwrap_or("").trim();
            if kid.is_empty() || secret.is_empty() {
                continue;
            }
            keys.insert(kid.to_string(), secret.to_string());
        }

        let active_kid = std::env::var("JWT_ACTIVE_KID").map_err(|_| {
            tracing::error!("JWT_ACTIVE_KID must be set when JWT_SIGNING_KEYS is configured");
            AppError::internal()
        })?;

        if !keys.contains_key(&active_kid) {
            tracing::error!(active_kid = %active_kid, "JWT_ACTIVE_KID is not present in JWT_SIGNING_KEYS");
            return Err(AppError::internal());
        }

        Ok(KeySet { keys, active_kid })
    } else {
        let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
        let mut keys = HashMap::new();
        keys.insert(LEGACY_KID.to_string(), secret);
        Ok(KeySet {
            keys,
            active_kid: LEGACY_KID.to_string(),
        })
    }
}

fn sign(claims: &Claims, keyset: &KeySet) -> AppResult<String> {
    let secret = keyset
        .keys
        .get(&keyset.active_kid)
        .ok_or_else(AppError::internal)?;
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(keyset.active_kid.clone());
    encode(
        &header,
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| {
        tracing::error!(error = %e, "Token signing failed");
        AppError::internal()
    })
}

/// Freshly issued access + refresh token pair, along with the identifiers the
/// caller needs to persist the refresh-token family in the database.
pub struct IssuedTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub access_jti: Uuid,
    pub refresh_jti: Uuid,
}

/// Issues a new access/refresh token pair belonging to refresh-token family
/// `family_id`, stamped with token version `tv` (see `Claims::tv`).
#[tracing::instrument(skip_all, fields(username = %username, role = %role))]
pub fn issue_tokens(
    username: &str,
    role: &str,
    family_id: Uuid,
    tv: i64,
) -> AppResult<IssuedTokens> {
    let keyset = load_keyset()?;
    let now = Utc::now().timestamp() as usize;
    let iss = jwt_issuer();
    let aud = jwt_audience();
    let access_jti = Uuid::new_v4();
    let refresh_jti = Uuid::new_v4();

    let access_claims = Claims {
        sub: username.to_owned(),
        kind: "access".to_owned(),
        role: role.to_owned(),
        exp: now + ACCESS_TOKEN_SECS as usize,
        iat: now,
        nbf: now,
        iss: iss.clone(),
        aud: aud.clone(),
        jti: access_jti.to_string(),
        family: None,
        tv,
    };

    let refresh_claims = Claims {
        sub: username.to_owned(),
        kind: "refresh".to_owned(),
        role: role.to_owned(),
        exp: now + REFRESH_TOKEN_SECS as usize,
        iat: now,
        nbf: now,
        iss,
        aud,
        jti: refresh_jti.to_string(),
        family: Some(family_id.to_string()),
        tv,
    };

    let access_token = sign(&access_claims, &keyset)?;
    let refresh_token = sign(&refresh_claims, &keyset)?;

    tracing::debug!("Tokens generated");
    Ok(IssuedTokens {
        access_token,
        refresh_token,
        access_jti,
        refresh_jti,
    })
}

/// Convenience wrapper for callers that only need the wire response shape
/// (register/login/recover — first token issuance for a brand-new family).
pub fn generate_tokens(
    username: &str,
    role: &str,
    family_id: Uuid,
    tv: i64,
) -> AppResult<AuthResponse> {
    let issued = issue_tokens(username, role, family_id, tv)?;
    Ok(AuthResponse {
        access_token: issued.access_token,
        refresh_token: issued.refresh_token,
        token_type: "Bearer".to_owned(),
    })
}

/// Decodes and validates a token: signature (against the keyset entry named
/// by the token's `kid`), algorithm (pinned to HS256 — rejects `none` and
/// algorithm-confusion attempts), `exp`/`nbf` with explicit clock-skew
/// leeway, and `iss`/`aud`/`kind`.
#[tracing::instrument(skip_all)]
pub fn validate_token(token: &str, expected_kind: &str) -> AppResult<Claims> {
    let unauthorized = || AppError::Unauthorized {
        message: "Invalid or expired token".to_string(),
    };

    let keyset = load_keyset()?;

    let header = decode_header(token).map_err(|e| {
        tracing::warn!(error = %e, "Token header decode failed");
        unauthorized()
    })?;

    // Pin the algorithm allowlist ourselves before even looking up a key —
    // defense in depth against algorithm-confusion (e.g. `alg: none` or a
    // downgrade to a different family) on top of `Validation::algorithms`.
    if header.alg != Algorithm::HS256 {
        tracing::warn!(alg = ?header.alg, "Rejected token with disallowed algorithm");
        return Err(unauthorized());
    }

    let kid = header
        .kid
        .clone()
        .unwrap_or_else(|| keyset.active_kid.clone());
    let secret = keyset.keys.get(&kid).ok_or_else(|| {
        tracing::warn!(kid = %kid, "Rejected token signed with unknown key id");
        unauthorized()
    })?;

    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = CLOCK_SKEW_LEEWAY_SECS;
    validation.validate_nbf = true;
    validation.validate_aud = false; // checked manually below for a clearer error path
    validation.set_required_spec_claims(&["exp", "iat", "nbf"]);

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|e| {
        tracing::warn!(error = %e, "Token validation failed");
        unauthorized()
    })?;

    let claims = token_data.claims;

    if claims.iss != jwt_issuer() || claims.aud != jwt_audience() {
        tracing::warn!("Rejected token with mismatched iss/aud");
        return Err(unauthorized());
    }

    if claims.kind != expected_kind {
        tracing::warn!(expected = %expected_kind, got = %claims.kind, "Wrong token kind");
        return Err(AppError::Unauthorized {
            message: "Invalid token kind".to_string(),
        });
    }

    Ok(claims)
}

#[tracing::instrument(skip_all)]
pub fn hash_password(password: &str) -> AppResult<String> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST).map_err(|e| {
        tracing::error!(error = %e, "Password hashing failed");
        AppError::internal()
    })
}

#[tracing::instrument(skip_all)]
pub fn verify_password(password: &str, hash: &str) -> AppResult<bool> {
    bcrypt::verify(password, hash).map_err(|e| {
        tracing::error!(error = %e, "Password verification failed");
        AppError::internal()
    })
}

#[tracing::instrument(skip_all)]
pub fn generate_totp_secret() -> AppResult<String> {
    let mut secret_bytes = [0u8; 20];
    thread_rng().fill(&mut secret_bytes);
    Ok(BASE32_NOPAD.encode(&secret_bytes))
}

#[tracing::instrument(skip_all)]
pub fn validate_totp_code(secret: &str, code: &str) -> AppResult<bool> {
    let secret_bytes = BASE32_NOPAD.decode(secret.as_bytes()).map_err(|e| {
        tracing::error!(error = %e, "Invalid TOTP secret encoding");
        AppError::unauthorized("Invalid two-factor secret")
    })?;

    let now = Utc::now().timestamp() as i64;

    for offset in -1i64..=1 {
        let step = (now.saturating_add(offset * 30).max(0) as u64) / 30;
        let expected = totp_custom::<Sha1>(step, 6, &secret_bytes, 30);
        if code == expected {
            return Ok(true);
        }
    }

    Ok(false)
}

#[tracing::instrument(skip_all)]
pub fn generate_backup_codes() -> Vec<String> {
    let mut rng = thread_rng();
    (0..8)
        .map(|_| {
            (&mut rng)
                .sample_iter(&Alphanumeric)
                .take(10)
                .map(char::from)
                .collect()
        })
        .collect()
}

#[tracing::instrument(skip_all)]
pub fn hash_backup_code(code: &str) -> AppResult<String> {
    bcrypt::hash(code, bcrypt::DEFAULT_COST).map_err(|e| {
        tracing::error!(error = %e, "Backup code hashing failed");
        AppError::internal()
    })
}

#[tracing::instrument(skip_all)]
pub fn verify_backup_code(
    code: &str,
    backup_code_hashes: &[crate::crypto::encryption::EncryptedString],
) -> AppResult<Option<usize>> {
    for (idx, hash) in backup_code_hashes.iter().enumerate() {
        if bcrypt::verify(code, hash.as_str()).map_err(|e| {
            tracing::error!(error = %e, "Backup code verification failed");
            AppError::internal()
        })? {
            return Ok(Some(idx));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn totp_secret_generation_and_validation() {
        let secret = generate_totp_secret().expect("generate secret");
        let secret_bytes = BASE32_NOPAD
            .decode(secret.as_bytes())
            .expect("decode secret");
        let timestamp = Utc::now().timestamp() as u64;
        let step = timestamp / 30;
        let code = totp_custom::<Sha1>(step, 6, &secret_bytes, 30);

        assert!(validate_totp_code(&secret, &code).expect("validate code"));
    }

    #[test]
    fn backup_code_hash_and_verify() {
        let code = "RECOVERY123";
        let hashed = hash_backup_code(code).expect("hash backup code");
        let index = verify_backup_code(
            code,
            &[crate::crypto::encryption::EncryptedString::new(
                hashed.clone(),
            )],
        )
        .expect("verify backup code");
        assert_eq!(index, Some(0));
        let missing = verify_backup_code(
            "WRONGCODE",
            &[crate::crypto::encryption::EncryptedString::new(hashed)],
        )
        .expect("verify wrong backup code");
        assert_eq!(missing, None);
    }

    // JWT-related tests mutate process-wide env vars (`JWT_SIGNING_KEYS` /
    // `JWT_ACTIVE_KID`), so they're serialized on this lock to avoid racing
    // each other. They deliberately never touch `JWT_SECRET`, so they can't
    // race the pagination cursor tests, which fall back to that var.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn set_keyset_env(pairs: &str, active_kid: &str) {
        std::env::set_var("JWT_SIGNING_KEYS", pairs);
        std::env::set_var("JWT_ACTIVE_KID", active_kid);
    }

    fn clear_keyset_env() {
        std::env::remove_var("JWT_SIGNING_KEYS");
        std::env::remove_var("JWT_ACTIVE_KID");
    }

    #[test]
    fn issue_and_validate_round_trip_embeds_tv_and_family() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_keyset_env("rt:rt-secret-hhhhhhhhhhhhhhhhhhhh", "rt");

        let family = Uuid::new_v4();
        let issued = issue_tokens("dave", "creator", family, 3).expect("issue tokens");

        let access_claims =
            validate_token(&issued.access_token, "access").expect("valid access token");
        assert_eq!(access_claims.tv, 3);
        assert_eq!(access_claims.jti, issued.access_jti.to_string());
        assert!(access_claims.family.is_none());

        let refresh_claims =
            validate_token(&issued.refresh_token, "refresh").expect("valid refresh token");
        assert_eq!(refresh_claims.family, Some(family.to_string()));
        assert_eq!(refresh_claims.jti, issued.refresh_jti.to_string());

        clear_keyset_env();
    }

    /// Key rotation with overlap: tokens signed before rotation must keep
    /// verifying after the active signing key changes, as long as the old
    /// kid is still present in the keyset — zero forced global logout.
    #[test]
    fn key_rotation_overlap_verifies_old_and_new_kid_tokens() {
        let _guard = ENV_LOCK.lock().unwrap();
        let keys = "old:old-secret-aaaaaaaaaaaaaaaaaaaa,new:new-secret-bbbbbbbbbbbbbbbbbbbb";
        set_keyset_env(keys, "old");
        let family = Uuid::new_v4();
        let issued_old =
            issue_tokens("alice", "creator", family, 0).expect("issue with old active key");

        // Rotate: `new` becomes active, `old` stays in the set for overlap.
        set_keyset_env(keys, "new");
        let issued_new =
            issue_tokens("alice", "creator", family, 0).expect("issue with new active key");

        let old_claims = validate_token(&issued_old.access_token, "access")
            .expect("pre-rotation token must still verify during overlap");
        let new_claims = validate_token(&issued_new.access_token, "access")
            .expect("post-rotation token must verify");
        assert_eq!(old_claims.sub, "alice");
        assert_eq!(new_claims.sub, "alice");

        clear_keyset_env();
    }

    #[test]
    fn token_signed_with_fully_retired_key_is_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_keyset_env("retiring:retiring-secret-cccccccccccccccccc", "retiring");
        let family = Uuid::new_v4();
        let issued = issue_tokens("bob", "creator", family, 0).expect("issue token");

        // Admin finishes the rotation runbook: the retiring kid is dropped entirely.
        set_keyset_env("fresh:fresh-secret-dddddddddddddddddddd", "fresh");
        let result = validate_token(&issued.access_token, "access");
        assert!(
            result.is_err(),
            "token signed with a fully-retired key must be rejected"
        );

        clear_keyset_env();
    }

    #[test]
    fn clock_skew_boundary_within_leeway_accepted_beyond_leeway_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_keyset_env("skew:skew-secret-eeeeeeeeeeeeeeeeeeee", "skew");
        let keyset = load_keyset().expect("keyset");
        let now = Utc::now().timestamp() as usize;

        let base = Claims {
            sub: "carol".to_string(),
            kind: "access".to_string(),
            role: "creator".to_string(),
            exp: now - 10,
            iat: now - 100,
            nbf: now - 100,
            iss: jwt_issuer(),
            aud: jwt_audience(),
            jti: Uuid::new_v4().to_string(),
            family: None,
            tv: 0,
        };

        // Expired 10s ago: within the 30s leeway, still accepted.
        let within = sign(&base, &keyset).expect("sign");
        assert!(validate_token(&within, "access").is_ok());

        // Expired 60s ago: beyond the 30s leeway, rejected.
        let mut beyond = base;
        beyond.exp = now - 60;
        let beyond_token = sign(&beyond, &keyset).expect("sign");
        assert!(validate_token(&beyond_token, "access").is_err());

        clear_keyset_env();
    }

    #[test]
    fn algorithm_confusion_attempt_is_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_keyset_env("alg:alg-secret-ffffffffffffffffffff", "alg");
        let keyset = load_keyset().expect("keyset");
        let secret = keyset.keys.get("alg").unwrap();
        let now = Utc::now().timestamp() as usize;

        let claims = Claims {
            sub: "mallory".to_string(),
            kind: "access".to_string(),
            role: "admin".to_string(),
            exp: now + 900,
            iat: now,
            nbf: now,
            iss: jwt_issuer(),
            aud: jwt_audience(),
            jti: Uuid::new_v4().to_string(),
            family: None,
            tv: 0,
        };

        // Forge a token using HS384 instead of the pinned HS256.
        let mut header = Header::new(Algorithm::HS384);
        header.kid = Some("alg".to_string());
        let forged = encode(
            &header,
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("encode forged token");

        assert!(
            validate_token(&forged, "access").is_err(),
            "token signed with a non-allowlisted algorithm must be rejected"
        );

        clear_keyset_env();
    }

    #[test]
    fn mismatched_audience_is_rejected() {
        let _guard = ENV_LOCK.lock().unwrap();
        set_keyset_env("aud:aud-secret-gggggggggggggggggggg", "aud");
        let keyset = load_keyset().expect("keyset");
        let now = Utc::now().timestamp() as usize;

        let claims = Claims {
            sub: "eve".to_string(),
            kind: "access".to_string(),
            role: "creator".to_string(),
            exp: now + 900,
            iat: now,
            nbf: now,
            iss: jwt_issuer(),
            aud: "some-other-service".to_string(),
            jti: Uuid::new_v4().to_string(),
            family: None,
            tv: 0,
        };
        let token = sign(&claims, &keyset).expect("sign");
        assert!(validate_token(&token, "access").is_err());

        clear_keyset_env();
    }
}
