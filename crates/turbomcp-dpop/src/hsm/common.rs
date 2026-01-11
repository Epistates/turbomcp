//! Common HSM utilities and shared functionality
//!
//! This module provides shared implementations used across HSM backends to
//! eliminate code duplication while maintaining proven quality.

use super::super::{DpopAlgorithm, DpopError, DpopPublicKey, Result};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio;
use tracing::trace;

/// Compute RFC 7638 compliant JWK thumbprint for any public key
///
/// This implements the canonical JWK thumbprint computation as specified in RFC 7638,
/// using the exact same logic for both PKCS#11 and YubiHSM backends.
///
/// Only supports ES256 (ECDSA P-256) as of TurboMCP v3.0+
///
/// # RFC 7638 Compliance
///
/// - Fields are included in canonical (alphabetical) order
/// - Base64URL encoding without padding is used
/// - SHA-256 hash is computed over UTF-8 bytes of canonical JSON
///
/// # Arguments
///
/// * `public_key` - The public key to compute thumbprint for
/// * `algorithm` - The DPoP algorithm being used (must be ES256)
/// * `backend_name` - Name of HSM backend for tracing
///
/// # Returns
///
/// Base64URL-encoded (no padding) SHA-256 hash of canonical JWK
pub fn compute_jwk_thumbprint(
    public_key: &DpopPublicKey,
    algorithm: DpopAlgorithm,
    backend_name: &str,
) -> Result<String> {
    // RFC 7638: Build canonical JWK JSON with required fields only, alphabetically ordered
    // Only ES256 (ECDSA P-256) is supported
    let canonical_jwk = match (algorithm, public_key) {
        (DpopAlgorithm::ES256, DpopPublicKey::EcdsaP256 { x, y }) => {
            // RFC 7638 Section 3.1: Required fields for EC keys: crv, kty, x, y
            let x_b64 =
                base64::prelude::Engine::encode(&base64::prelude::BASE64_URL_SAFE_NO_PAD, x);
            let y_b64 =
                base64::prelude::Engine::encode(&base64::prelude::BASE64_URL_SAFE_NO_PAD, y);

            format!(
                r#"{{"crv":"P-256","kty":"EC","x":"{}","y":"{}"}}"#,
                x_b64, y_b64
            )
        }
    };

    // RFC 7638 Section 3: Compute SHA-256 hash of canonical JWK UTF-8 bytes
    let mut hasher = Sha256::new();
    hasher.update(canonical_jwk.as_bytes());
    let thumbprint_bytes = hasher.finalize();

    // RFC 7638 Section 3: Return base64url-encoded thumbprint (no padding)
    let thumbprint =
        base64::prelude::Engine::encode(&base64::prelude::BASE64_URL_SAFE_NO_PAD, thumbprint_bytes);

    trace!(
        "Computed RFC 7638 JWK thumbprint for {}: {}",
        backend_name, thumbprint
    );
    Ok(thumbprint)
}

/// Execute an operation with exponential backoff retry logic
///
/// This provides proven retry functionality with exponential backoff
/// for HSM operations that may fail due to temporary network issues or
/// HSM availability problems.
///
/// # Arguments
///
/// * `operation` - Async closure that performs the operation
/// * `max_attempts` - Maximum number of retry attempts (default: 3)
/// * `initial_backoff_ms` - Initial backoff time in milliseconds (default: 100)
/// * `operation_name` - Name of operation for logging
///
/// # Returns
///
/// Result of the operation, or the last error encountered
pub async fn retry_with_exponential_backoff<F, Fut, T, E>(
    mut operation: F,
    max_attempts: usize,
    initial_backoff_ms: u64,
    operation_name: &str,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display + Into<DpopError>,
{
    let mut backoff_ms = initial_backoff_ms;
    let mut last_error = None;

    for attempt in 1..=max_attempts {
        match operation().await {
            Ok(result) => {
                if attempt > 1 {
                    trace!(
                        "{} succeeded on attempt {}/{}",
                        operation_name, attempt, max_attempts
                    );
                }
                return Ok(result);
            }
            Err(error) => {
                last_error = Some(error.into());

                if attempt < max_attempts {
                    trace!(
                        "{} failed on attempt {}/{}, retrying in {}ms: {}",
                        operation_name,
                        attempt,
                        max_attempts,
                        backoff_ms,
                        last_error.as_ref().unwrap()
                    );

                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms *= 2; // Exponential backoff
                } else {
                    trace!(
                        "{} failed on final attempt {}/{}",
                        operation_name, attempt, max_attempts
                    );
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| DpopError::InternalError {
        reason: format!("{} failed after {} attempts", operation_name, max_attempts),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_retry_succeeds_immediately() {
        let result = retry_with_exponential_backoff(
            || async { Ok::<i32, DpopError>(42) },
            3,
            100,
            "test_operation",
        )
        .await
        .unwrap();

        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn test_retry_succeeds_on_second_attempt() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let attempts = AtomicU32::new(0);
        let result = retry_with_exponential_backoff(
            || async {
                let current_attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                if current_attempt == 1 {
                    Err(DpopError::InternalError {
                        reason: "temporary failure".to_string(),
                    })
                } else {
                    Ok::<i32, DpopError>(42)
                }
            },
            3,
            100,
            "test_operation",
        )
        .await
        .unwrap();

        assert_eq!(result, 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_fails_after_max_attempts() {
        let result = retry_with_exponential_backoff(
            || async {
                Err::<i32, DpopError>(DpopError::InternalError {
                    reason: "persistent failure".to_string(),
                })
            },
            2,
            50,
            "test_operation",
        )
        .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_jwk_thumbprint_ec_p256() {
        let mut x = [0u8; 32];
        let mut y = [0u8; 32];
        x[0..3].copy_from_slice(&[0x01, 0x02, 0x03]);
        y[0..3].copy_from_slice(&[0x04, 0x05, 0x06]);

        let public_key = DpopPublicKey::EcdsaP256 { x, y };

        let thumbprint = compute_jwk_thumbprint(&public_key, DpopAlgorithm::ES256, "test").unwrap();
        assert!(!thumbprint.is_empty());
        // Thumbprint should be base64url encoded (no padding)
        assert!(!thumbprint.contains('='));
    }
}
