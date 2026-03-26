use validator::ValidationError;

/// Validates an XLM amount string: must be a positive decimal with up to 7 decimal places.
pub fn validate_xlm_amount(amount: &str) -> Result<(), ValidationError> {
    let parsed: f64 = amount.parse().map_err(|_| {
        let mut e = ValidationError::new("invalid_amount");
        e.message = Some("Amount must be a valid number".into());
        e
    })?;

    if parsed <= 0.0 {
        let mut e = ValidationError::new("invalid_amount");
        e.message = Some("Amount must be greater than zero".into());
        return Err(e);
    }

    // Enforce max 7 decimal places (Stellar's stroops precision).
    if let Some(decimals) = amount.split('.').nth(1) {
        if decimals.len() > 7 {
            let mut e = ValidationError::new("invalid_amount");
            e.message = Some("Amount must have at most 7 decimal places".into());
            return Err(e);
        }
    }

    Ok(())
}
