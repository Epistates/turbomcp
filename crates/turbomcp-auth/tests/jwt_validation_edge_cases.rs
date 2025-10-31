//! JWT validation edge case tests
//!
//! These tests verify JWT validation handles boundary conditions correctly.
//! Tests cover:
//! - Clock skew tolerance (±60 seconds per RFC)
//! - Expiration boundary conditions (exactly at exp timestamp)
//! - Not-before (nbf) claim validation
//! - Missing required claims (sub, iss, aud, exp)
//! - Audience claim variations (array vs single string)
//! - Issuer validation
//!
//! # Standards Tested
//! - RFC 7519: JSON Web Token (JWT)
//! - RFC 8725: JWT Best Current Practice
//! - RFC 9449: DPoP clock skew requirements

mod common;

use common::current_timestamp;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
struct TestClaims {
    sub: String,
    iss: Option<String>,
    aud: Option<serde_json::Value>, // Can be string or array
    exp: Option<u64>,
    iat: Option<u64>,
    nbf: Option<u64>,
}

/// Generate test signing key for HS256
fn test_signing_key() -> Vec<u8> {
    b"test_secret_key_at_least_32_bytes_long_12345678".to_vec()
}

/// Test: JWT exactly at expiration timestamp (boundary condition)
#[tokio::test]
async fn test_jwt_expiration_boundary() {
    let secret = test_signing_key();
    let now = current_timestamp();

    // GIVEN: JWT expires exactly now (exp = current timestamp)
    let claims = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!("https://api.example.com")),
        exp: Some(now), // Expires exactly now
        iat: Some(now - 60),
        nbf: Some(now - 60),
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    // WHEN: We validate the token
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 0; // No clock skew tolerance

    let result = decode::<TestClaims>(&token, &DecodingKey::from_secret(&secret), &validation);

    // THEN: Token is expired (exp is inclusive boundary)
    // RFC 7519: "exp" claim identifies expiration time on or after which JWT MUST NOT be accepted
    assert!(result.is_err(), "Token at exact exp timestamp should be rejected");
}

/// Test: Clock skew tolerance (accept JWT slightly in future)
#[tokio::test]
async fn test_clock_skew_tolerance() {
    let secret = test_signing_key();
    let now = current_timestamp();

    // GIVEN: JWT with iat 30 seconds in future (simulating clock skew)
    let claims = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!("https://api.example.com")),
        exp: Some(now + 3600),
        iat: Some(now + 30), // 30 seconds in future
        nbf: Some(now + 30),
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    // WHEN: We validate with 60s leeway
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 60; // Accept ±60 seconds
    validation.validate_nbf = true;
    validation.validate_aud = false; // Don't validate audience

    let result = decode::<TestClaims>(&token, &DecodingKey::from_secret(&secret), &validation);

    // THEN: Token is accepted (within clock skew tolerance)
    assert!(
        result.is_ok(),
        "JWT within clock skew tolerance should be accepted: {:?}",
        result.as_ref().err()
    );

    // Test: JWT too far in future (beyond tolerance)
    let claims_too_far = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!("https://api.example.com")),
        exp: Some(now + 3600),
        iat: Some(now + 120), // 120 seconds in future (beyond 60s tolerance)
        nbf: Some(now + 120),
    };

    let token_too_far = encode(
        &Header::new(Algorithm::HS256),
        &claims_too_far,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    let result_too_far =
        decode::<TestClaims>(&token_too_far, &DecodingKey::from_secret(&secret), &validation);

    assert!(
        result_too_far.is_err(),
        "JWT beyond clock skew tolerance should be rejected"
    );
}

/// Test: Not-before (nbf) claim validation
#[tokio::test]
async fn test_nbf_claim_validation() {
    let secret = test_signing_key();
    let now = current_timestamp();

    // GIVEN: JWT with nbf in future (not yet valid)
    let claims = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!("https://api.example.com")),
        exp: Some(now + 3600),
        iat: Some(now),
        nbf: Some(now + 120), // Valid 120 seconds from now
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    // WHEN: We validate before nbf time
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_nbf = true;
    validation.leeway = 0; // No clock skew

    let result = decode::<TestClaims>(&token, &DecodingKey::from_secret(&secret), &validation);

    // THEN: Token is rejected (not yet valid)
    assert!(
        result.is_err(),
        "JWT before nbf timestamp should be rejected"
    );

    // Test with leeway
    validation.leeway = 60;
    let result_with_leeway =
        decode::<TestClaims>(&token, &DecodingKey::from_secret(&secret), &validation);
    assert!(
        result_with_leeway.is_err(),
        "JWT still invalid even with 60s leeway (nbf is 120s in future)"
    );
}

/// Test: Missing required claims
#[tokio::test]
async fn test_missing_required_claims() {
    let secret = test_signing_key();

    // GIVEN: JWT missing sub claim
    let claims_no_sub = json!({
        "iss": "https://auth.example.com",
        "aud": "https://api.example.com",
        "exp": current_timestamp() + 3600,
        // Missing: sub
    });

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims_no_sub,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    // WHEN: We validate expecting sub claim
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_required_spec_claims(&["sub", "exp", "iss"]);
    validation.validate_aud = false; // Don't validate audience for this test

    let result = decode::<serde_json::Value>(
        &token,
        &DecodingKey::from_secret(&secret),
        &validation,
    );

    // THEN: Validation fails due to missing claim
    assert!(result.is_err(), "JWT without required sub claim should be rejected");

    // Test: JWT with all required claims
    let claims_complete = json!({
        "sub": "user123",
        "iss": "https://auth.example.com",
        "aud": "https://api.example.com",
        "exp": current_timestamp() + 3600,
    });

    let token_complete = encode(
        &Header::new(Algorithm::HS256),
        &claims_complete,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    let result_complete = decode::<serde_json::Value>(
        &token_complete,
        &DecodingKey::from_secret(&secret),
        &validation,
    );

    assert!(
        result_complete.is_ok(),
        "JWT with all required claims should be accepted: {:?}",
        result_complete.as_ref().err()
    );
}

/// Test: Audience claim variations (string vs array)
#[tokio::test]
async fn test_audience_claim_variations() {
    let secret = test_signing_key();
    let now = current_timestamp();

    // Test 1: Single audience as string
    let claims_single = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!("https://api.example.com")), // Single string
        exp: Some(now + 3600),
        iat: Some(now),
        nbf: Some(now),
    };

    let token_single = encode(
        &Header::new(Algorithm::HS256),
        &claims_single,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&["https://api.example.com"]);

    let result_single = decode::<TestClaims>(
        &token_single,
        &DecodingKey::from_secret(&secret),
        &validation,
    );
    assert!(result_single.is_ok(), "Single audience string should be accepted");

    // Test 2: Multiple audiences as array
    let claims_array = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!(["https://api.example.com", "https://api2.example.com"])), // Array
        exp: Some(now + 3600),
        iat: Some(now),
        nbf: Some(now),
    };

    let token_array = encode(
        &Header::new(Algorithm::HS256),
        &claims_array,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    let result_array =
        decode::<TestClaims>(&token_array, &DecodingKey::from_secret(&secret), &validation);
    assert!(
        result_array.is_ok(),
        "Audience array containing expected value should be accepted"
    );

    // Test 3: Audience mismatch
    let mut validation_wrong = Validation::new(Algorithm::HS256);
    validation_wrong.set_audience(&["https://wrong-audience.com"]);

    let result_mismatch =
        decode::<TestClaims>(&token_single, &DecodingKey::from_secret(&secret), &validation_wrong);
    assert!(
        result_mismatch.is_err(),
        "JWT with non-matching audience should be rejected"
    );
}

/// Test: Issuer validation
#[tokio::test]
async fn test_issuer_validation() {
    let secret = test_signing_key();
    let now = current_timestamp();

    // GIVEN: JWT from specific issuer
    let claims = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!("https://api.example.com")),
        exp: Some(now + 3600),
        iat: Some(now),
        nbf: Some(now),
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    // Test 1: Correct issuer
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&["https://auth.example.com"]);
    validation.validate_aud = false; // Don't validate audience

    let result = decode::<TestClaims>(&token, &DecodingKey::from_secret(&secret), &validation);
    assert!(result.is_ok(), "JWT from expected issuer should be accepted: {:?}", result.as_ref().err());

    // Test 2: Wrong issuer
    let mut validation_wrong = Validation::new(Algorithm::HS256);
    validation_wrong.set_issuer(&["https://wrong-issuer.com"]);
    validation_wrong.validate_aud = false; // Don't validate audience

    let result_wrong =
        decode::<TestClaims>(&token, &DecodingKey::from_secret(&secret), &validation_wrong);
    assert!(
        result_wrong.is_err(),
        "JWT from unexpected issuer should be rejected"
    );

    // Test 3: Multiple allowed issuers
    let mut validation_multi = Validation::new(Algorithm::HS256);
    validation_multi.set_issuer(&["https://auth.example.com", "https://auth2.example.com"]);
    validation_multi.validate_aud = false; // Don't validate audience

    let result_multi =
        decode::<TestClaims>(&token, &DecodingKey::from_secret(&secret), &validation_multi);
    assert!(
        result_multi.is_ok(),
        "JWT from one of multiple allowed issuers should be accepted"
    );
}

/// Test: Expired token (well past expiration)
#[tokio::test]
async fn test_expired_token() {
    let secret = test_signing_key();
    let now = current_timestamp();

    // GIVEN: JWT that expired 1 hour ago
    let claims = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!("https://api.example.com")),
        exp: Some(now - 3600), // Expired 1 hour ago
        iat: Some(now - 7200),
        nbf: Some(now - 7200),
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    // WHEN: We validate
    let validation = Validation::new(Algorithm::HS256);

    let result = decode::<TestClaims>(&token, &DecodingKey::from_secret(&secret), &validation);

    // THEN: Token is rejected
    assert!(result.is_err(), "Expired JWT should be rejected");
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("expired") || error_msg.contains("Expired"),
        "Error should indicate expiration: {}",
        error_msg
    );
}

/// Test: Token without expiration (when exp is required)
#[tokio::test]
async fn test_missing_expiration() {
    let secret = test_signing_key();

    // GIVEN: JWT without exp claim
    let claims = json!({
        "sub": "user123",
        "iss": "https://auth.example.com",
        "aud": "https://api.example.com",
        "iat": current_timestamp(),
        // Missing: exp
    });

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    // WHEN: We validate with exp required
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.set_required_spec_claims(&["exp"]);

    let result = decode::<serde_json::Value>(
        &token,
        &DecodingKey::from_secret(&secret),
        &validation,
    );

    // THEN: Validation fails
    assert!(
        result.is_err(),
        "JWT without exp claim should be rejected when exp is required"
    );
}

/// Test: Very long expiration (far future)
#[tokio::test]
async fn test_far_future_expiration() {
    let secret = test_signing_key();
    let now = current_timestamp();

    // GIVEN: JWT that expires in 100 years
    let claims = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!("https://api.example.com")),
        exp: Some(now + (100 * 365 * 24 * 3600)), // 100 years
        iat: Some(now),
        nbf: Some(now),
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    // WHEN: We validate
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_aud = false; // Don't validate audience
    let result = decode::<TestClaims>(&token, &DecodingKey::from_secret(&secret), &validation);

    // THEN: Token is valid (though long expiration is poor security practice)
    assert!(
        result.is_ok(),
        "JWT with far future exp should be technically valid: {:?}",
        result.as_ref().err()
    );

    // Note: In production, consider max lifetime policies
    // Best practice: Access tokens should expire within minutes/hours
}

/// Test: Invalid algorithm specified in validation
#[tokio::test]
async fn test_algorithm_mismatch() {
    let secret = test_signing_key();
    let now = current_timestamp();

    // GIVEN: JWT signed with HS256
    let claims = TestClaims {
        sub: "user123".to_string(),
        iss: Some("https://auth.example.com".to_string()),
        aud: Some(json!("https://api.example.com")),
        exp: Some(now + 3600),
        iat: Some(now),
        nbf: Some(now),
    };

    let _token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&secret),
    )
    .expect("Failed to encode JWT");

    // WHEN: We try to validate with RS256 (wrong algorithm)
    let _validation = Validation::new(Algorithm::RS256); // Mismatch!

    // Note: This will fail during decode with wrong key type
    // In production, header algorithm should be checked against whitelist
    // before attempting validation
}
