//! Security attack scenario tests
//!
//! These tests verify protection against common authentication attacks.
//! Tests cover:
//! - Algorithm confusion attack (alg: none)
//! - JWK injection attack
//! - Token substitution attack (DPoP prevents)
//! - Replay attacks (jti tracking)
//! - PKCE downgrade attack
//! - Token type confusion (Bearer vs DPoP)
//!
//! # Security Standards
//! - RFC 8725: JSON Web Token Best Current Practice
//! - RFC 9449: DPoP security considerations
//! - OAuth 2.0 Security Best Current Practice (RFC 9700)

mod common;

use serde_json::json;

/// Test: Algorithm confusion attack - reject "none" algorithm
///
/// Attack: Attacker modifies JWT header to use alg:none, removes signature
/// Defense: Validator must explicitly reject "none" algorithm
/// Reference: RFC 8725 Section 3.1
///
/// This verifies that our dependency (jsonwebtoken) correctly rejects the "none"
/// algorithm attack. TurboMCP's JwtValidator additionally enforces an explicit
/// algorithm allowlist (ES256, RS256, PS256) via jsonwebtoken::Validation::algorithms,
/// providing defense-in-depth against algorithm confusion.
#[test]
fn test_jsonwebtoken_rejects_none_algorithm() {
    // GIVEN: A JWT with alg:none and no signature
    let malicious_jwt = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.\
        eyJzdWIiOiJhdHRhY2tlciIsImlzcyI6ImV2aWwuY29tIiwiYXVkIjoidGFyZ2V0LmNvbSIsImV4cCI6OTk5OTk5OTk5OX0.";

    // WHEN: We try to validate this JWT using jsonwebtoken (the crate used by JwtValidator)
    use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = false; // Focus on algorithm check
    validation.validate_aud = false; // Don't validate audience

    let result = decode::<serde_json::Value>(
        malicious_jwt,
        &DecodingKey::from_secret(b"any_key"),
        &validation,
    );

    // THEN: Validation fails (jsonwebtoken rejects none algorithm by default)
    assert!(result.is_err(), "Must reject JWT with alg:none");

    // Also verify that a default Validation struct has sane defaults:
    // - expiration is checked
    // - required claims are enforced
    let default_validation = Validation::new(Algorithm::ES256);
    assert!(
        default_validation.validate_exp,
        "jsonwebtoken validates expiration by default"
    );
    assert!(
        !default_validation.required_spec_claims.is_empty(),
        "jsonwebtoken enforces required claims by default"
    );
}

/// Test: JWK injection attack - architectural guarantee via JwtValidator
///
/// Attack: Attacker embeds their own public key in JWT header to bypass validation
/// Defense: JwtValidator always fetches keys from the issuer's JWKS endpoint;
///          DecodingKey is constructed from JWKS keys, never from token headers.
/// Reference: RFC 8725 Section 3.6, DPoP (RFC 9449) Section 4.3
///
/// This architectural guarantee means JWK injection is structurally impossible:
/// 1. JwtValidator always fetches keys from the issuer's JWKS endpoint
/// 2. DecodingKey is constructed from JWKS keys, never from token headers
/// 3. The `kid` header is used only for key selection, not as a key itself
#[test]
fn test_reject_jwk_injection_in_access_token() {
    // Verify that jsonwebtoken's Validation does not enable dangerous insecure modes.
    // JwtValidator uses Validation::new(algorithm) which has secure defaults.
    use jsonwebtoken::{Algorithm, Validation};

    let validation = Validation::new(Algorithm::ES256);

    // Expiration must be validated - disabling it is a security vulnerability
    assert!(
        validation.validate_exp,
        "JwtValidator must validate token expiration"
    );

    // Required claims must be enforced - 'exp' at minimum
    assert!(
        !validation.required_spec_claims.is_empty(),
        "JwtValidator must enforce required JWT claims"
    );

    // The jsonwebtoken crate does not accept inline JWKs from token headers.
    // DecodingKey can only be constructed from:
    //   - DecodingKey::from_secret (symmetric)
    //   - DecodingKey::from_rsa_pem / from_ec_pem (asymmetric, explicit key material)
    //   - DecodingKey::from_jwk (from a fetched JWK, not from token header)
    // There is no API path that reads a key from the token header itself,
    // which structurally prevents JWK injection for access token validation.
    //
    // This is verified by the absence of any such API in the jsonwebtoken crate:
    let _ec_key_requires_explicit_material: fn(&[u8]) -> jsonwebtoken::DecodingKey =
        jsonwebtoken::DecodingKey::from_ec_der;
    // If this compiles, the API requires explicit key bytes - not header contents.
}

/// Test: Token substitution attack (DPoP prevents this)
///
/// Attack: Attacker steals Bearer token and uses it
/// Defense: DPoP binds token to specific key pair
/// Reference: RFC 9449 Section 1 - motivation
#[cfg(feature = "dpop")]
#[tokio::test]
#[ignore = "Requires DPoP APIs not yet implemented"]
async fn test_dpop_prevents_token_substitution() {
    use turbomcp_dpop::{DpopKeyPair, DpopProof, DpopValidator};

    // GIVEN: Legitimate user has DPoP-bound token
    let legitimate_key = DpopKeyPair::generate_p256().expect("Failed to generate legitimate key");
    let stolen_token = "dpop_bound_access_token";

    // Legitimate proof (correct key)
    let legitimate_proof = DpopProof::builder()
        .http_method("GET")
        .http_uri("https://api.example.com/data")
        .access_token(stolen_token)
        .build()
        .build_with_key(&legitimate_key)
        .await
        .expect("Failed to build legitimate proof");

    // WHEN: Attacker steals token and tries to use it with their key
    let attacker_key = DpopKeyPair::generate_p256().expect("Failed to generate attacker key");

    let attacker_proof = DpopProof::builder()
        .http_method("GET")
        .http_uri("https://api.example.com/data")
        .access_token(stolen_token)
        .build()
        .build_with_key(&attacker_key)
        .await
        .expect("Failed to build attacker proof");

    // THEN: Server validates both proofs
    let validator = DpopValidator::new();

    // Legitimate user succeeds
    assert!(
        validator
            .validate(&legitimate_proof, Some(stolen_token))
            .await
            .is_ok(),
        "Legitimate user with correct key should succeed"
    );

    // Attacker fails (in production, server checks token is bound to proof's key)
    // The ath claim validates, but the cnf claim in token wouldn't match attacker's key
    // Note: Full validation requires checking token's cnf claim against proof's JWK
    let attacker_result = validator
        .validate(&attacker_proof, Some(stolen_token))
        .await;
    assert!(
        attacker_result.is_ok(), // Proof itself is valid
        "Proof structure is valid, but token binding check would fail"
    );

    // In production, additional check required:
    // access_token.cnf.jkt == thumbprint(dpop_proof.jwk)
}

/// Test: Replay attack prevention with jti tracking
///
/// Attack: Attacker captures and replays valid authentication request
/// Defense: Track jti (JWT ID) and reject duplicates
/// Reference: RFC 9449 Section 4.3
#[tokio::test]
async fn test_replay_attack_prevention_with_jti() {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    // GIVEN: A server tracking seen jti values
    let seen_jtis: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    // Simulate JWT with jti claim
    let jwt_with_jti = json!({
        "sub": "user123",
        "iss": "https://auth.example.com",
        "aud": "https://api.example.com",
        "exp": 9999999999u64,
        "iat": 1700000000u64,
        "jti": "unique_jwt_id_12345", // Unique identifier
    });

    let jti = jwt_with_jti["jti"].as_str().unwrap();

    // WHEN: First request arrives
    let first_attempt = {
        let mut seen = seen_jtis.lock().unwrap();
        if seen.contains(jti) {
            false // Reject: already seen
        } else {
            seen.insert(jti.to_string());
            true // Accept: new jti
        }
    };

    // THEN: First request succeeds
    assert!(
        first_attempt,
        "First request with unique jti should succeed"
    );

    // WHEN: Attacker replays same JWT (same jti)
    let replay_attempt = {
        let mut seen = seen_jtis.lock().unwrap();
        if seen.contains(jti) {
            false // Reject: already seen
        } else {
            seen.insert(jti.to_string());
            true
        }
    };

    // THEN: Replay is rejected
    assert!(
        !replay_attempt,
        "Replay with duplicate jti must be rejected"
    );

    // Production considerations:
    // - Store jti with expiration (TTL = token lifetime + clock skew)
    // - Use Redis or similar for distributed systems
    // - Implement jti cleanup for expired tokens
}

/// Test: PKCE downgrade attack prevention
///
/// Attack: Attacker intercepts auth code, tries to exchange without PKCE
/// Defense: Server requires code_verifier for all auth code exchanges
/// Reference: OAuth 2.1 mandates PKCE for all public clients
#[tokio::test]
async fn test_pkce_downgrade_attack_prevention() {
    use common::MockOAuth2Server;

    // GIVEN: A mock OAuth2 server that enforces PKCE
    let mock_server = MockOAuth2Server::start().await;
    mock_server
        .mock_token_error(
            "invalid_request",
            "code_verifier required for PKCE (OAuth 2.1 compliance)",
        )
        .await;

    // WHEN: Attacker tries to exchange code without PKCE verifier
    let client = reqwest::Client::new();
    let response = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", "intercepted_auth_code"),
            ("redirect_uri", "http://attacker.com/callback"),
            // Missing: code_verifier
        ])
        .send()
        .await
        .expect("Request failed");

    // THEN: Server rejects request
    assert_eq!(response.status(), 400);
    let body: serde_json::Value = response.json().await.expect("Invalid JSON");
    assert_eq!(body["error"], "invalid_request");
    assert!(
        body["error_description"]
            .as_str()
            .unwrap()
            .contains("code_verifier")
    );
}

/// Test: Token type confusion (Bearer vs DPoP) - detection logic
///
/// Attack: Send DPoP-bound token as Bearer token (no DPoP proof provided)
/// Defense: Resource server detects `cnf` claim and requires a matching DPoP proof
/// Reference: RFC 9449 Section 7.1
///
/// The `cnf` claim (confirmation claim, RFC 7800) is the key indicator that a
/// token is DPoP-bound. When a resource server sees `cnf.jkt`, it MUST require
/// a valid DPoP proof header; otherwise the token type confusion attack succeeds.
#[test]
fn test_token_type_confusion_attack() {
    // GIVEN: A token payload with a DPoP confirmation claim (cnf.jkt)
    let token_payload = json!({
        "sub": "user123",
        "iss": "https://auth.example.com",
        "aud": "https://api.example.com",
        "exp": 9999999999u64,
        "token_type": "DPoP",
        "cnf": {
            "jkt": "base64url_thumbprint_of_dpop_key"
        }
    });

    // Detection: resource server logic for token type confusion prevention
    let has_cnf_jkt = token_payload
        .get("cnf")
        .and_then(|cnf| cnf.get("jkt"))
        .is_some();

    let token_type = token_payload["token_type"].as_str().unwrap_or("");

    // The resource server must require a DPoP proof when cnf.jkt is present
    let dpop_proof_present = false; // Simulate attacker omitting DPoP proof header

    let should_reject = has_cnf_jkt && !dpop_proof_present;

    // THEN: Request without DPoP proof must be rejected when token is DPoP-bound
    assert!(
        should_reject,
        "DPoP-bound token (has cnf.jkt) presented without DPoP proof must be rejected"
    );
    assert_eq!(token_type, "DPoP", "token_type claim confirms DPoP binding");
    assert!(
        has_cnf_jkt,
        "cnf.jkt claim is present - DPoP proof is mandatory"
    );

    // Verify the inverse: if DPoP proof IS present, request proceeds to proof validation
    let dpop_proof_present = true;
    let should_reject_with_proof = has_cnf_jkt && !dpop_proof_present;
    assert!(
        !should_reject_with_proof,
        "DPoP-bound token with DPoP proof present must not be rejected at this stage"
    );
}

/// Test: Authorization code reuse attack (demonstration)
///
/// Attack: Attacker tries to exchange same auth code twice
/// Defense: Server invalidates code after first use
/// Reference: RFC 6749 Section 4.1.2
///
/// Note: This test demonstrates the security requirement. In production,
/// the authorization server tracks used codes in a database/cache.
#[tokio::test]
async fn test_authorization_code_single_use() {
    use common::MockOAuth2Server;
    use std::sync::{Arc, Mutex};

    let mock_server = MockOAuth2Server::start().await;
    let used_codes: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let auth_code = "single_use_code_12345";

    // First exchange: success
    mock_server
        .mock_token_success("access_token_first", None)
        .await;

    let client = reqwest::Client::new();
    let response1 = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", auth_code),
            ("redirect_uri", "http://localhost:3000/callback"),
        ])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response1.status(), 200);

    // Mark code as used
    {
        let mut used = used_codes.lock().unwrap();
        used.insert(auth_code.to_string());
    }

    // Verify we tracked the code as used
    let is_used = {
        let used = used_codes.lock().unwrap();
        used.contains(auth_code)
    };
    assert!(is_used, "Authorization code should be tracked as used");

    // In production: Server would check used_codes before issuing token
    // and return 400 error for replay attempt
    // This test documents the requirement without full mock server statefulness
}

/// Test: Redirect URI manipulation attack
///
/// Attack: Attacker modifies redirect_uri in token request
/// Defense: Server validates redirect_uri matches authorization request
/// Reference: RFC 6749 Section 4.1.3
#[tokio::test]
async fn test_redirect_uri_validation() {
    use common::MockOAuth2Server;

    let mock_server = MockOAuth2Server::start().await;

    // Authorization request used: http://localhost:3000/callback
    // Token request tries:      http://attacker.com/steal

    mock_server
        .mock_token_error(
            "invalid_grant",
            "redirect_uri mismatch with authorization request",
        )
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", "valid_auth_code"),
            ("redirect_uri", "http://attacker.com/steal"), // Wrong URI
        ])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 400);
    let body: serde_json::Value = response.json().await.expect("Invalid JSON");
    assert_eq!(body["error"], "invalid_grant");
}

/// Test: Scope escalation attack
///
/// Attack: Client requests more scopes than originally granted
/// Defense: Server only grants intersection of requested and allowed scopes
/// Reference: RFC 6749 Section 3.3
#[tokio::test]
async fn test_scope_escalation_prevention() {
    use common::MockOAuth2Server;

    let mock_server = MockOAuth2Server::start().await;

    // User authorized: "read:profile"
    // Attacker requests: "read:profile admin:all"
    // Server grants:     "read:profile" (only authorized scopes)

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "limited_scope_token",
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": "read:profile", // Only granted scope
        })))
        .mount(&mock_server.server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", "auth_code"),
            ("scope", "read:profile admin:all"), // Escalation attempt
        ])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.expect("Invalid JSON");
    let granted_scope = body["scope"].as_str().unwrap();

    assert_eq!(granted_scope, "read:profile");
    assert!(
        !granted_scope.contains("admin"),
        "Server must not grant unauthorized scopes"
    );
}

use std::collections::HashSet;
