use chrono::Utc;
use data_encoding::BASE32_NOPAD;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use oath::HashType;
use rand::{distributions::Alphanumeric, thread_rng, Rng};

use crate::errors::{AppError, AppResult};
use crate::models::auth::{AuthResponse, Claims};

const ACCESS_TOKEN_SECS: i64 = 60 * 60 * 24;
const REFRESH_TOKEN_SECS: i64 = 60 * 60 * 24 * 7;

fn jwt_secret() -> String {
    std::env::var("JWT_SECRET").expect("JWT_SECRET must be set")
}

#[tracing::instrument(skip_all, fields(username = %username))]
pub fn generate_tokens(username: &str) -> AppResult<AuthResponse> {
    let secret = jwt_secret();
    let now = Utc::now().timestamp() as usize;

    let access_claims = Claims {
        sub: username.to_owned(),
        kind: "access".to_owned(),
        exp: now + ACCESS_TOKEN_SECS as usize,
        iat: now,
    };

    let refresh_claims = Claims {
        sub: username.to_owned(),
        kind: "refresh".to_owned(),
        exp: now + REFRESH_TOKEN_SECS as usize,
        iat: now,
    };

    let access_token = encode(
        &Header::default(),
        &access_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| {
        tracing::error!(error = %e, "Token generation failed");
        AppError::internal()
    })?;

    let refresh_token = encode(
        &Header::default(),
        &refresh_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| {
        tracing::error!(error = %e, "Refresh token generation failed");
        AppError::internal()
    })?;

    tracing::debug!("Tokens generated");
    Ok(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_owned(),
    })
}

#[tracing::instrument(skip_all)]
pub fn validate_token(token: &str, expected_kind: &str) -> AppResult<Claims> {
    let secret = jwt_secret();
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| {
        tracing::warn!(error = %e, "Token validation failed");
        AppError::Unauthorized {
            message: "Invalid or expired token".to_string(),
        }
    })?;

    if token_data.claims.kind != expected_kind {
        tracing::warn!(expected = %expected_kind, got = %token_data.claims.kind, "Wrong token kind");
        return Err(AppError::Unauthorized {
            message: "Invalid token kind".to_string(),
        });
    }

    Ok(token_data.claims)
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
    let secret_bytes = BASE32_NOPAD
        .decode(secret.as_bytes())
        .map_err(|e| {
            tracing::error!(error = %e, "Invalid TOTP secret encoding");
            AppError::unauthorized("Invalid two-factor secret")
        })?;

    let now = Utc::now().timestamp() as i64;

    for offset in -1..=1 {
        let timestamp = now.saturating_add(offset as i64 * 30).max(0) as u64;
        let expected = oath::totp_raw_custom(&secret_bytes, 6, 30, 0, &HashType::SHA1, timestamp);
        if code == format!("{:06}", expected) {
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
            rng.sample_iter(&Alphanumeric)
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
pub fn verify_backup_code(code: &str, backup_code_hashes: &[String]) -> AppResult<Option<usize>> {
    for (idx, hash) in backup_code_hashes.iter().enumerate() {
        if bcrypt::verify(code, hash).map_err(|e| {
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
        let secret_bytes = BASE32_NOPAD.decode(secret.as_bytes()).expect("decode secret");
        let timestamp = Utc::now().timestamp() as u64;
        let code = oath::totp_raw_custom(&secret_bytes, 6, 30, 0, &HashType::SHA1, timestamp);
        let code = format!("{:06}", code);

        assert!(validate_totp_code(&secret, &code).expect("validate code"));
    }

    #[test]
    fn backup_code_hash_and_verify() {
        let code = "RECOVERY123";
        let hashed = hash_backup_code(code).expect("hash backup code");
        let index = verify_backup_code(code, &[hashed.clone()]).expect("verify backup code");
        assert_eq!(index, Some(0));
        let missing = verify_backup_code("WRONGCODE", &[hashed]).expect("verify wrong backup code");
        assert_eq!(missing, None);
    }
}
