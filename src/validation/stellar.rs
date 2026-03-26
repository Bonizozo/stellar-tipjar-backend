use validator::ValidationError;

/// Stellar public keys are base32-encoded, start with 'G', and are exactly 56 chars.
pub fn validate_stellar_address(address: &str) -> Result<(), ValidationError> {
    if address.len() != 56 {
        let mut e = ValidationError::new("invalid_stellar_address");
        e.message = Some("Stellar address must be exactly 56 characters".into());
        return Err(e);
    }
    if !address.starts_with('G') {
        let mut e = ValidationError::new("invalid_stellar_address");
        e.message = Some("Stellar address must start with 'G'".into());
        return Err(e);
    }
    if !address.chars().all(|c| c.is_ascii_alphanumeric() && c.is_ascii_uppercase()) {
        let mut e = ValidationError::new("invalid_stellar_address");
        e.message = Some("Stellar address must contain only uppercase alphanumeric characters".into());
        return Err(e);
    }
    Ok(())
}
