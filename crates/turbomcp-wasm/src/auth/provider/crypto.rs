//! Cryptographic utilities for OAuth 2.1 provider.
//!
//! This module provides cryptographic functions using the Web Crypto API:
//! - Token generation (random bytes)
//! - Token hashing (for storage)
//! - PKCE code verifier validation
//! - JWT signing

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// Errors that can occur during cryptographic operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    /// Web Crypto API not available
    CryptoUnavailable(String),
    /// Random generation failed
    RandomError(String),
    /// Hashing failed
    HashError(String),
    /// Signing failed
    SigningError(String),
    /// Base64 encoding/decoding error
    Base64Error(String),
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CryptoUnavailable(msg) => write!(f, "Crypto unavailable: {}", msg),
            Self::RandomError(msg) => write!(f, "Random error: {}", msg),
            Self::HashError(msg) => write!(f, "Hash error: {}", msg),
            Self::SigningError(msg) => write!(f, "Signing error: {}", msg),
            Self::Base64Error(msg) => write!(f, "Base64 error: {}", msg),
        }
    }
}

impl std::error::Error for CryptoError {
    // CryptoError wraps string messages (from JsValue), not source errors,
    // so source() returns None. The error messages contain sufficient context.
}

/// Result type for crypto operations.
pub type CryptoResult<T> = Result<T, CryptoError>;

/// Generate cryptographically secure random bytes.
///
/// Uses Web Crypto API's `getRandomValues`.
pub fn generate_random_bytes(length: usize) -> CryptoResult<Vec<u8>> {
    let crypto = web_sys::window()
        .ok_or_else(|| CryptoError::CryptoUnavailable("No window object".to_string()))?
        .crypto()
        .map_err(|_| CryptoError::CryptoUnavailable("No crypto object".to_string()))?;

    let mut bytes = vec![0u8; length];
    let array = js_sys::Uint8Array::new_with_length(length as u32);

    crypto
        .get_random_values_with_u8_array(&mut bytes)
        .map_err(|e| CryptoError::RandomError(format!("{:?}", e)))?;

    // Copy from TypedArray to Vec
    array.copy_from(&bytes);

    Ok(bytes)
}

/// Generate a random token (URL-safe base64 encoded).
///
/// # Arguments
///
/// * `byte_length` - Number of random bytes (token will be ~4/3 this length after encoding)
pub fn generate_token(byte_length: usize) -> CryptoResult<String> {
    let bytes = generate_random_bytes(byte_length)?;
    Ok(URL_SAFE_NO_PAD.encode(&bytes))
}

/// Generate an authorization code.
///
/// Returns a 32-byte (256-bit) random token, URL-safe base64 encoded.
pub fn generate_authorization_code() -> CryptoResult<String> {
    generate_token(32)
}

/// Generate a refresh token.
///
/// Returns a 32-byte (256-bit) random token, URL-safe base64 encoded.
pub fn generate_refresh_token() -> CryptoResult<String> {
    generate_token(32)
}

/// Generate a token family ID.
///
/// Returns a 16-byte (128-bit) random ID, URL-safe base64 encoded.
pub fn generate_family_id() -> CryptoResult<String> {
    generate_token(16)
}

/// Hash a token using SHA-256.
///
/// Returns the URL-safe base64 encoded hash.
pub async fn hash_token(token: &str) -> CryptoResult<String> {
    let crypto = web_sys::window()
        .ok_or_else(|| CryptoError::CryptoUnavailable("No window object".to_string()))?
        .crypto()
        .map_err(|_| CryptoError::CryptoUnavailable("No crypto object".to_string()))?;

    let subtle = crypto.subtle();

    // Convert token to bytes
    let data = js_sys::Uint8Array::from(token.as_bytes());

    // Hash using SHA-256
    let promise = subtle
        .digest_with_str_and_buffer_source("SHA-256", &data)
        .map_err(|e| CryptoError::HashError(format!("{:?}", e)))?;

    let result = JsFuture::from(promise)
        .await
        .map_err(|e| CryptoError::HashError(format!("{:?}", e)))?;

    // Convert ArrayBuffer to Vec<u8>
    let array = js_sys::Uint8Array::new(&result);
    let mut hash = vec![0u8; array.length() as usize];
    array.copy_to(&mut hash);

    Ok(URL_SAFE_NO_PAD.encode(&hash))
}

/// Verify a PKCE code verifier against a code challenge.
///
/// # Arguments
///
/// * `code_verifier` - The verifier sent by the client
/// * `code_challenge` - The challenge stored during authorization
/// * `method` - The challenge method ("S256" or "plain")
///
/// # Returns
///
/// `true` if the verifier matches the challenge, `false` otherwise.
pub async fn verify_pkce(
    code_verifier: &str,
    code_challenge: &str,
    method: &str,
) -> CryptoResult<bool> {
    match method {
        "S256" => {
            // SHA-256 hash the verifier and compare to challenge
            let verifier_challenge = generate_code_challenge(code_verifier).await?;
            Ok(constant_time_compare(&verifier_challenge, code_challenge))
        }
        "plain" => {
            // Direct comparison (not recommended, but supported for compatibility)
            Ok(constant_time_compare(code_verifier, code_challenge))
        }
        _ => Err(CryptoError::HashError(format!(
            "Unknown PKCE method: {}",
            method
        ))),
    }
}

/// Generate a PKCE code challenge from a verifier (S256 method).
///
/// This is the operation: `BASE64URL(SHA256(code_verifier))`
pub async fn generate_code_challenge(code_verifier: &str) -> CryptoResult<String> {
    let crypto = web_sys::window()
        .ok_or_else(|| CryptoError::CryptoUnavailable("No window object".to_string()))?
        .crypto()
        .map_err(|_| CryptoError::CryptoUnavailable("No crypto object".to_string()))?;

    let subtle = crypto.subtle();

    // Convert verifier to bytes (ASCII)
    let data = js_sys::Uint8Array::from(code_verifier.as_bytes());

    // Hash using SHA-256
    let promise = subtle
        .digest_with_str_and_buffer_source("SHA-256", &data)
        .map_err(|e| CryptoError::HashError(format!("{:?}", e)))?;

    let result = JsFuture::from(promise)
        .await
        .map_err(|e| CryptoError::HashError(format!("{:?}", e)))?;

    // Convert ArrayBuffer to Vec<u8>
    let array = js_sys::Uint8Array::new(&result);
    let mut hash = vec![0u8; array.length() as usize];
    array.copy_to(&mut hash);

    // Base64url encode (no padding, per RFC 7636)
    Ok(URL_SAFE_NO_PAD.encode(&hash))
}

/// Constant-time string comparison to prevent timing attacks.
///
/// Returns `true` if the strings are equal, `false` otherwise.
/// Takes constant time regardless of where the strings differ or their lengths.
///
/// # Security
///
/// This function is designed to prevent timing attacks by:
/// - Using branchless bitwise operations for length comparison
/// - Always iterating over the maximum length of both strings
/// - Using bitwise OR to accumulate differences without short-circuiting
///
/// # Implementation Note
///
/// The length difference check uses `(len_diff | len_diff.wrapping_neg()) >> (BITS-1)`
/// which extracts the sign bit without branching. This is a standard constant-time
/// idiom for checking if a value is non-zero.
pub fn constant_time_compare(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    // Compute length difference using wrapping arithmetic
    let len_diff = (a_bytes.len() as isize).wrapping_sub(b_bytes.len() as isize);

    // Convert non-zero to 1 without branching:
    // For any non-zero value x: (x | -x) has its sign bit set
    // Arithmetic right shift by (BITS-1) gives all 1s (-1) or all 0s (0)
    // We want 1 for non-zero, 0 for zero, so we negate and mask
    let len_ne = ((len_diff | len_diff.wrapping_neg()) >> (isize::BITS - 1)) as u8;
    let mut result = len_ne & 1;

    // Always iterate over the maximum length to maintain constant time
    let max_len = a_bytes.len().max(b_bytes.len());

    for i in 0..max_len {
        // Use get() to safely handle out-of-bounds, defaulting to 0
        // The XOR with default 0 will contribute to result if lengths differ
        let x = a_bytes.get(i).copied().unwrap_or(0);
        let y = b_bytes.get(i).copied().unwrap_or(0);
        result |= x ^ y;
    }

    result == 0
}

/// Validate a PKCE code verifier format.
///
/// Per RFC 7636, the code verifier must be:
/// - Between 43 and 128 characters
/// - Only contain unreserved characters: [A-Z] / [a-z] / [0-9] / "-" / "." / "_" / "~"
pub fn validate_code_verifier(verifier: &str) -> bool {
    let len = verifier.len();
    if !(43..=128).contains(&len) {
        return false;
    }

    verifier
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '~')
}

/// Get current Unix timestamp in seconds.
pub fn now_secs() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare("hello", "hello"));
        assert!(!constant_time_compare("hello", "world"));
        assert!(!constant_time_compare("hello", "hell"));
        assert!(!constant_time_compare("", "hello"));
    }

    #[test]
    fn test_validate_code_verifier() {
        // Valid verifiers
        assert!(validate_code_verifier(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        )); // 43 chars
        assert!(validate_code_verifier(&"a".repeat(43)));
        assert!(validate_code_verifier(&"a".repeat(128)));

        // Invalid - too short
        assert!(!validate_code_verifier(&"a".repeat(42)));

        // Invalid - too long
        assert!(!validate_code_verifier(&"a".repeat(129)));

        // Invalid - bad characters
        assert!(!validate_code_verifier(&format!("{}!", "a".repeat(42))));
        assert!(!validate_code_verifier(&format!("{}@", "a".repeat(42))));
        assert!(!validate_code_verifier(&format!("{} ", "a".repeat(42))));

        // Valid - all allowed special chars
        assert!(validate_code_verifier(&format!(
            "abc-._~{}",
            "x".repeat(39)
        )));
    }

    #[test]
    fn test_base64_url_encoding() {
        // Test that we're using URL-safe base64
        let data = vec![0xfb, 0xff, 0xfe]; // Would be +//+ in standard base64
        let encoded = URL_SAFE_NO_PAD.encode(&data);
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
    }
}
