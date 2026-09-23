//! OAuth 2.1 Validation Functions
//!
//! This module provides validation functions for OAuth 2.1 flows:
//! - OAuth state parameter validation (CSRF protection)
//!
//! RFC 8707 resource-URI canonicalization lives in [`super::resource::validate_resource_uri`]
//! — this module used to duplicate it with a stricter, non-normalizing
//! variant (`validate_canonical_resource_uri`) that rejected anything not
//! already in canonical form instead of normalizing it, and disagreed with
//! `validate_resource_uri` on the root-path trailing slash. It had no
//! callers in this workspace and was removed (AU-17): normalize with
//! `validate_resource_uri` rather than validating-then-rejecting.

use turbomcp_protocol::{Error as McpError, Result as McpResult};

/// Constant-time OAuth state parameter validation
///
/// This function validates OAuth 2.1 state parameters using constant-time comparison
/// to prevent timing attacks that could leak state values (CSRF tokens).
///
/// # Security
/// The state parameter is used for CSRF protection in OAuth flows. If an attacker
/// can use timing attacks to determine valid state values, they could potentially
/// forge OAuth callbacks. This function uses constant-time comparison to prevent
/// such timing attacks.
///
/// # Arguments
/// * `expected_state` - The state value stored in the session/database
/// * `received_state` - The state value received from the OAuth callback
///
/// # Returns
/// * `Ok(())` if states match
/// * `Err(McpError)` if states don't match or are invalid
///
/// # Example
/// ```ignore
/// // In OAuth callback handler
/// let stored_state = session.get("oauth_state")?;
/// let callback_state = request.query_param("state")?;
/// validate_oauth_state(&stored_state, &callback_state)?;
/// ```
pub fn validate_oauth_state(expected_state: &str, received_state: &str) -> McpResult<()> {
    use sha2::{Digest, Sha256};
    use subtle::ConstantTimeEq;

    // Validate state is not empty (security requirement)
    if expected_state.is_empty() || received_state.is_empty() {
        return Err(McpError::invalid_params(
            "OAuth state parameter cannot be empty".to_string(),
        ));
    }

    // Hash both inputs to fixed length before comparing. `ct_eq` short-circuits
    // when input lengths differ — comparing raw strings would leak the expected
    // state's length through timing. Comparing 32-byte SHA-256 digests gives a
    // truly constant-time comparison regardless of input length.
    let expected_hash = Sha256::digest(expected_state.as_bytes());
    let received_hash = Sha256::digest(received_state.as_bytes());
    let is_equal = expected_hash.ct_eq(&received_hash);

    if bool::from(is_equal) {
        Ok(())
    } else {
        Err(McpError::invalid_params(
            "OAuth state parameter mismatch - possible CSRF attack".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_state_validation_success() {
        let state = "random-csrf-token-123";
        assert!(validate_oauth_state(state, state).is_ok());
    }

    #[test]
    fn test_oauth_state_validation_mismatch() {
        let expected = "state-abc123";
        let received = "state-xyz789";
        let result = validate_oauth_state(expected, received);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("state parameter mismatch")
        );
    }

    #[test]
    fn test_oauth_state_validation_empty_expected() {
        let result = validate_oauth_state("", "some-state");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_oauth_state_validation_empty_received() {
        let result = validate_oauth_state("some-state", "");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_oauth_state_validation_case_sensitive() {
        let result = validate_oauth_state("State123", "state123");
        assert!(result.is_err());
    }
}
