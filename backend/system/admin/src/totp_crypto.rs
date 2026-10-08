use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use sha2::{Digest, Sha256};

const CURRENT_KEY_ENV: &str = "TALOS_TOTP_ENCRYPTION_KEY";
const PREVIOUS_KEY_ENV: &str = "TALOS_TOTP_ENCRYPTION_KEY_PREVIOUS";
const NONCE_BYTES: usize = 12;
const ENVELOPE_VERSION: &str = "v1";
const TOTP_DIGITS: u32 = 6;
const TOTP_STEP_SECS: u64 = 30;
const TOTP_SKEW_STEPS: u64 = 1;

pub fn generate_totp_secret() -> Vec<u8> {
    let mut secret = vec![0u8; 32];
    for byte in &mut secret {
        *byte = rand::random::<u8>();
    }
    secret
}

pub fn base32_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut result = String::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for &byte in data {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            result.push(ALPHABET[((buffer >> bits) & 0x1F) as usize] as char);
        }
    }
    if bits > 0 {
        result.push(ALPHABET[((buffer << (5 - bits)) & 0x1F) as usize] as char);
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecryptedTotpSecret {
    pub secret: Vec<u8>,
    pub needs_rewrap: bool,
}

fn decode_key(name: &str, required: bool) -> Result<Option<[u8; 32]>, String> {
    let encoded = match std::env::var(name) {
        Ok(value) => {
            let value = value.trim();
            if value.is_empty() {
                if required {
                    return Err(format!("{name} is required"));
                }
                return Ok(None);
            }
            value.to_string()
        }
        Err(std::env::VarError::NotPresent) => {
            if required {
                return Err(format!("{name} is required"));
            }
            return Ok(None);
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(format!("{name} must be valid UTF-8 hexadecimal text"));
        }
    };
    let decoded =
        hex::decode(&encoded).map_err(|_| format!("{name} must be 64 hexadecimal characters"))?;
    let key: [u8; 32] = decoded
        .try_into()
        .map_err(|_| format!("{name} must decode to exactly 32 bytes"))?;
    Ok(Some(key))
}

fn current_key() -> Result<[u8; 32], String> {
    decode_key(CURRENT_KEY_ENV, true)?.ok_or_else(|| format!("{CURRENT_KEY_ENV} is required"))
}

fn previous_key() -> Result<Option<[u8; 32]>, String> {
    decode_key(PREVIOUS_KEY_ENV, false)
}

fn encode_with_key(secret: &[u8], identity_id: &str, key: &[u8; 32]) -> Result<String, String> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| "invalid TOTP encryption key".to_string())?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: secret,
                aad: identity_id.as_bytes(),
            },
        )
        .map_err(|_| "failed to encrypt TOTP secret".to_string())?;
    let mut envelope = Vec::with_capacity(nonce.len() + ciphertext.len());
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    Ok(format!("{ENVELOPE_VERSION}:{}", hex::encode(envelope)))
}

fn decode_envelope(envelope: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let encoded = envelope
        .strip_prefix("v1:")
        .ok_or_else(|| "unsupported TOTP ciphertext version".to_string())?;

    // Compatibility: an early login-side parser expected
    // `v1:<nonce_hex>:<ciphertext_hex>`, while enrollment persisted
    // `v1:<nonce+ciphertext hex>`. Accept both v1 layouts during the
    // migration window; all new writes use the compact single-blob layout.
    if let Some((nonce_hex, ciphertext_hex)) = encoded.split_once(':') {
        if ciphertext_hex.contains(':') {
            return Err("invalid TOTP ciphertext envelope".to_string());
        }
        let nonce =
            hex::decode(nonce_hex).map_err(|_| "invalid TOTP ciphertext encoding".to_string())?;
        let ciphertext = hex::decode(ciphertext_hex)
            .map_err(|_| "invalid TOTP ciphertext encoding".to_string())?;
        if nonce.len() != NONCE_BYTES || ciphertext.is_empty() {
            return Err("invalid TOTP ciphertext envelope".to_string());
        }
        return Ok((nonce, ciphertext));
    }

    let bytes = hex::decode(encoded).map_err(|_| "invalid TOTP ciphertext encoding".to_string())?;
    if bytes.len() <= NONCE_BYTES {
        return Err("invalid TOTP ciphertext envelope".to_string());
    }
    let (nonce, ciphertext) = bytes.split_at(NONCE_BYTES);
    Ok((nonce.to_vec(), ciphertext.to_vec()))
}

fn decrypt_with_key(envelope: &str, identity_id: &str, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let (nonce, ciphertext) = decode_envelope(envelope)?;
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| "invalid TOTP encryption key".to_string())?;
    cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: identity_id.as_bytes(),
            },
        )
        .map_err(|_| "failed to decrypt TOTP secret".to_string())
}

pub fn encrypt_totp_secret(secret: &[u8], identity_id: &str) -> Result<String, String> {
    encode_with_key(secret, identity_id, &current_key()?)
}

pub fn decrypt_totp_secret(
    envelope: &str,
    identity_id: &str,
) -> Result<DecryptedTotpSecret, String> {
    let current = current_key()?;
    if let Ok(secret) = decrypt_with_key(envelope, identity_id, &current) {
        return Ok(DecryptedTotpSecret {
            secret,
            needs_rewrap: false,
        });
    }

    if let Some(previous) = previous_key()? {
        if previous != current {
            if let Ok(secret) = decrypt_with_key(envelope, identity_id, &previous) {
                return Ok(DecryptedTotpSecret {
                    secret,
                    needs_rewrap: true,
                });
            }
        }
    }

    Err("failed to decrypt TOTP secret".to_string())
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hash = Sha256::digest(key);
        key_block[..32].copy_from_slice(&hash);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5Cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    let result = outer.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

fn generate_totp_code(secret: &[u8], time_step: u64) -> u32 {
    let hmac = hmac_sha256(secret, &time_step.to_be_bytes());
    let offset = (hmac[31] & 0x0F) as usize;
    let binary = ((hmac[offset] as u32 & 0x7F) << 24)
        | ((hmac[offset + 1] as u32) << 16)
        | ((hmac[offset + 2] as u32) << 8)
        | (hmac[offset + 3] as u32);
    binary % 10u32.pow(TOTP_DIGITS)
}

pub fn verify_totp_code_at(secret: &[u8], code: &str, unix_seconds: u64) -> bool {
    if code.len() != TOTP_DIGITS as usize || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let Ok(user_code) = code.parse::<u32>() else {
        return false;
    };
    let current_step = unix_seconds / TOTP_STEP_SECS;
    for step in
        current_step.saturating_sub(TOTP_SKEW_STEPS)..=current_step.saturating_add(TOTP_SKEW_STEPS)
    {
        if generate_totp_code(secret, step) == user_code {
            return true;
        }
    }
    false
}

pub fn verify_totp_code(secret: &[u8], code: &str) -> bool {
    let unix_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    verify_totp_code_at(secret, code, unix_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_and_legacy_split_v1_layouts_decrypt_with_same_key() {
        let key = [7_u8; 32];
        let secret = [42_u8; 32];
        let compact = encode_with_key(&secret, "identity-a", &key).unwrap();
        assert!(compact.starts_with("v1:"));
        assert!(!compact.contains(&hex::encode(secret)));
        assert_eq!(
            decrypt_with_key(&compact, "identity-a", &key).unwrap(),
            secret
        );

        let encoded = compact.strip_prefix("v1:").unwrap();
        let split = format!(
            "v1:{}:{}",
            &encoded[..NONCE_BYTES * 2],
            &encoded[NONCE_BYTES * 2..]
        );
        assert_eq!(
            decrypt_with_key(&split, "identity-a", &key).unwrap(),
            secret
        );
        assert!(decrypt_with_key(&compact, "identity-b", &key).is_err());
        assert!(decrypt_with_key(&compact, "identity-a", &[8_u8; 32]).is_err());
    }

    #[test]
    fn malformed_envelopes_fail_closed() {
        for envelope in ["", "v2:abcd", "v1:", "v1:aa:bb", "v1:not-hex"] {
            assert!(decode_envelope(envelope).is_err(), "{envelope}");
        }
    }

    #[test]
    fn totp_verification_is_deterministic_and_skew_bounded() {
        let secret = b"test-secret-material";
        let unix_seconds = 1_700_000_000_u64;
        let step = unix_seconds / TOTP_STEP_SECS;
        let code = format!("{:06}", generate_totp_code(secret, step));
        assert!(verify_totp_code_at(secret, &code, unix_seconds));
        assert!(verify_totp_code_at(
            secret,
            &code,
            unix_seconds.saturating_add(TOTP_STEP_SECS)
        ));
        assert!(!verify_totp_code_at(
            secret,
            &code,
            unix_seconds.saturating_add(TOTP_STEP_SECS * 3)
        ));
        assert!(!verify_totp_code_at(secret, "12345", unix_seconds));
        assert!(!verify_totp_code_at(secret, "abcdef", unix_seconds));
    }
}
