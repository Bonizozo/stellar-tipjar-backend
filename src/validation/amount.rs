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

/// Convert an XLM amount string (e.g. "10.5" or "10.5000000") to stroops (i64).
///
/// This is an integer-only computation — no f64 used anywhere.
///
/// Stellar uses 7 decimal places: 1 XLM = 10_000_000 stroops.
pub fn xlm_to_stroops_str(amount: &str) -> Result<i64, String> {
    let amount = amount.trim();

    // Split into whole and fractional parts.
    let (whole_str, frac_str) = match amount.split_once('.') {
        Some((w, f)) => (w, f),
        None => (amount, ""),
    };

    // Validate
    if whole_str.is_empty() {
        return Err(format!("Invalid XLM amount: '{}'", amount));
    }

    let whole: i64 = whole_str
        .parse()
        .map_err(|_| format!("Cannot parse whole part '{}' of amount '{}'", whole_str, amount))?;

    if whole < 0 {
        return Err(format!("Amount must not be negative: '{}'", amount));
    }

    // Ensure at most 7 decimal digits; pad to exactly 7.
    if frac_str.len() > 7 {
        return Err(format!(
            "Amount '{}' has more than 7 decimal places",
            amount
        ));
    }

    let padded = format!("{:0<7}", frac_str); // left-align, pad right with zeros
    let fractional: i64 = padded[..7]
        .parse()
        .map_err(|_| format!("Cannot parse fractional part of amount '{}'", amount))?;

    Ok(whole * 10_000_000 + fractional)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xlm_to_stroops_exact() {
        assert_eq!(xlm_to_stroops_str("10.0").unwrap(), 100_000_000);
        assert_eq!(xlm_to_stroops_str("1.0000000").unwrap(), 10_000_000);
        assert_eq!(xlm_to_stroops_str("0.0000001").unwrap(), 1);
        assert_eq!(xlm_to_stroops_str("10.5").unwrap(), 105_000_000);
    }

    #[test]
    fn test_xlm_to_stroops_no_fraction() {
        assert_eq!(xlm_to_stroops_str("100").unwrap(), 1_000_000_000);
    }

    #[test]
    fn test_xlm_to_stroops_too_many_decimals() {
        assert!(xlm_to_stroops_str("1.12345678").is_err());
    }
}
