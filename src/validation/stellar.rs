use data_encoding::BASE32_NOPAD;
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
    if !address
        .chars()
        .all(|c| c.is_ascii_alphanumeric() && c.is_ascii_uppercase())
    {
        let mut e = ValidationError::new("invalid_stellar_address");
        e.message =
            Some("Stellar address must contain only uppercase alphanumeric characters".into());
        return Err(e);
    }
    Ok(())
}

/// Decode a Stellar public key (StrKey) into raw ed25519 public key bytes.
pub fn decode_stellar_public_key(address: &str) -> Result<[u8; 32], String> {
    let decoded = BASE32_NOPAD
        .decode(address.as_bytes())
        .map_err(|_| "Invalid Stellar public key encoding".to_string())?;

    if decoded.len() != 35 {
        return Err("Invalid Stellar public key length".to_string());
    }

    const ED25519_PUBLIC_KEY_VERSION: u8 = 6 << 3;
    if decoded[0] != ED25519_PUBLIC_KEY_VERSION {
        return Err("Invalid Stellar public key version".to_string());
    }

    let payload = &decoded[..33];
    let checksum = u16::from_le_bytes(decoded[33..35].try_into().unwrap());
    if crc16_xmodem(payload) != checksum {
        return Err("Invalid Stellar public key checksum".to_string());
    }

    let mut public_key = [0u8; 32];
    public_key.copy_from_slice(&decoded[1..33]);
    Ok(public_key)
}

fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}
