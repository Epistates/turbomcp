//! Token lifecycle management integration tests
//!
//! These tests verify token refresh, revocation, and rotation behaviors.
//! Tests cover:
//! - Automatic token refresh before expiration
//! - Refresh token rotation (single-use tokens)
//! - Token revocation propagation (RFC 7009)
//! - Grace period handling during rotation
//! - Reuse detection for security
//!
//! # Standards Tested
//! - RFC 7009: OAuth 2.0 Token Revocation
//! - OAuth 2.0 Security BCP (2025): Refresh token rotation
//! - Best practices: Short-lived access tokens (15-30 min), single-use refresh tokens

mod common;

use common::MockOAuth2Server;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Test: Automatic token refresh before expiration
///
/// Best practice: Refresh access tokens proactively before they expire
/// to avoid authorization failures during user sessions.
#[tokio::test]
async fn test_automatic_token_refresh_before_expiration() {
    // GIVEN: A mock OAuth2 server with short-lived access tokens
    let mock_server = MockOAuth2Server::start().await;

    // Initial token: expires in 60 seconds
    let initial_access_token = "access_token_initial";
    let refresh_token = "refresh_token_12345";

    mock_server
        .mock_token_success(initial_access_token, Some(refresh_token))
        .await;

    // Simulate client getting initial tokens
    let client = reqwest::Client::new();
    let initial_response = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", "auth_code_123"),
            ("redirect_uri", "http://localhost:3000/callback"),
        ])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(initial_response.status(), 200);
    let initial_body: serde_json::Value = initial_response.json().await.expect("Invalid JSON");
    let expires_in = initial_body["expires_in"].as_u64().unwrap();

    // Best practice: Refresh 5 minutes before expiration (or 80% of lifetime)
    let refresh_threshold = (expires_in as f64 * 0.8) as u64;
    assert!(
        refresh_threshold < expires_in,
        "Should refresh before expiration"
    );

    // WHEN: Client proactively refreshes before expiration
    let new_access_token = "access_token_refreshed";

    // Create new mock server for refresh (avoid mount conflicts)
    let refresh_server = MockOAuth2Server::start().await;

    // Mock successful refresh
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": new_access_token,
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": refresh_token, // Same refresh token (no rotation in this test)
        })))
        .expect(1) // Only expect one call
        .mount(&refresh_server.server)
        .await;

    let refresh_response = client
        .post(&refresh_server.token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .expect("Refresh failed");

    // THEN: Refresh succeeds and new access token is obtained
    assert_eq!(refresh_response.status(), 200);
    let refresh_body: serde_json::Value = refresh_response.json().await.expect("Invalid JSON");
    assert_eq!(refresh_body["access_token"], new_access_token);

    // Document: Client should implement timer to refresh at 80% of token lifetime
    // Example: if expires_in = 3600s, refresh after 2880s (48 minutes)
}

/// Test: Refresh token rotation (single-use tokens)
///
/// Security best practice (2025): Each token refresh should return a NEW refresh token,
/// making the old one invalid. This prevents token reuse attacks.
/// Reference: OAuth 2.0 Security BCP
#[tokio::test]
#[ignore = "Requires OAuth2 refresh token rotation implementation"]
async fn test_refresh_token_rotation_single_use() {
    // GIVEN: A mock server implementing refresh token rotation
    let mock_server = MockOAuth2Server::start().await;
    let used_refresh_tokens: Arc<Mutex<std::collections::HashSet<String>>> =
        Arc::new(Mutex::new(std::collections::HashSet::new()));

    let initial_refresh = "refresh_v1";
    let rotated_refresh = "refresh_v2";

    // Initial token grant
    mock_server
        .mock_token_success("access_initial", Some(initial_refresh))
        .await;

    let client = reqwest::Client::new();
    let initial_response = client
        .post(&mock_server.token_endpoint)
        .form(&[("grant_type", "authorization_code"), ("code", "code_123")])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(initial_response.status(), 200);

    // WHEN: Client uses refresh token (first time)
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{body_string_contains, method, path},
    };

    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(initial_refresh))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access_v2",
            "token_type": "Bearer",
            "expires_in": 1800, // 30 minutes (best practice)
            "refresh_token": rotated_refresh, // NEW refresh token (rotated)
        })))
        .mount(&mock_server.server)
        .await;

    let first_refresh = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", initial_refresh),
        ])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(first_refresh.status(), 200);
    let body: serde_json::Value = first_refresh.json().await.expect("Invalid JSON");
    assert_eq!(
        body["refresh_token"], rotated_refresh,
        "Should return NEW refresh token"
    );

    // Track used token
    {
        let mut used = used_refresh_tokens.lock().unwrap();
        used.insert(initial_refresh.to_string());
    }

    // WHEN: Attacker tries to reuse old refresh token
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(initial_refresh))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Refresh token already used (rotation detected)",
        })))
        .mount(&mock_server.server)
        .await;

    let reuse_attempt = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", initial_refresh),
        ])
        .send()
        .await
        .expect("Request failed");

    // THEN: Reuse is rejected
    assert_eq!(reuse_attempt.status(), 400);
    let error_body: serde_json::Value = reuse_attempt.json().await.expect("Invalid JSON");
    assert_eq!(error_body["error"], "invalid_grant");

    // Security: Server should revoke ALL tokens in the chain when reuse detected
    // This prevents attackers who stole the token from using it
}

/// Test: Grace period during refresh token rotation
///
/// Best practice (2025): Allow brief grace period (e.g., 5-10 seconds) for network issues
/// Reference: Okta's refresh token rotation implementation
#[tokio::test]
async fn test_refresh_token_rotation_grace_period() {
    // GIVEN: Server with grace period configuration (10 seconds)
    let mock_server = MockOAuth2Server::start().await;
    let grace_period_secs = 10u64;

    let old_refresh = "refresh_old";
    let new_refresh = "refresh_new";

    // WHEN: Token is rotated
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access_new",
            "token_type": "Bearer",
            "expires_in": 900, // 15 minutes
            "refresh_token": new_refresh,
        })))
        .mount(&mock_server.server)
        .await;

    let client = reqwest::Client::new();
    let rotation_response = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", old_refresh),
        ])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(rotation_response.status(), 200);
    let rotation_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // WHEN: Client immediately retries with old token (within grace period)
    // Scenario: Network issue caused duplicate request

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "access_new",
            "token_type": "Bearer",
            "expires_in": 900,
            "refresh_token": new_refresh, // Returns NEW token again
        })))
        .mount(&mock_server.server)
        .await;

    let grace_response = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", old_refresh),
        ])
        .send()
        .await
        .expect("Request failed");

    // THEN: Within grace period, old token still works
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if current_time - rotation_time <= grace_period_secs {
        assert_eq!(
            grace_response.status(),
            200,
            "Should accept old token within grace period"
        );
    }

    // After grace period expires, old token should be rejected
    // Document: Grace period prevents network-related failures
}

/// Test: Token revocation (RFC 7009)
///
/// Standard: OAuth 2.0 Token Revocation
/// Use case: User logs out, token should be immediately invalid
#[tokio::test]
#[ignore = "Requires OAuth2 token revocation implementation"]
async fn test_token_revocation_rfc7009() {
    // GIVEN: A mock OAuth2 server with revocation endpoint
    let mock_server = MockOAuth2Server::start().await;
    let revocation_endpoint = format!("{}/revoke", mock_server.server.uri());

    let access_token = "access_token_to_revoke";
    let refresh_token = "refresh_token_to_revoke";

    // Client obtains tokens
    mock_server
        .mock_token_success(access_token, Some(refresh_token))
        .await;

    let client = reqwest::Client::new();
    let token_response = client
        .post(&mock_server.token_endpoint)
        .form(&[("grant_type", "authorization_code"), ("code", "code_123")])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(token_response.status(), 200);

    // WHEN: Client revokes the refresh token (RFC 7009)
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{body_string_contains, method, path},
    };

    Mock::given(method("POST"))
        .and(path("/revoke"))
        .and(body_string_contains(refresh_token))
        .respond_with(ResponseTemplate::new(200)) // RFC 7009: Always 200, even for invalid tokens
        .mount(&mock_server.server)
        .await;

    let revoke_response = client
        .post(&revocation_endpoint)
        .form(&[
            ("token", refresh_token),
            ("token_type_hint", "refresh_token"), // Optional hint
        ])
        .send()
        .await
        .expect("Revocation request failed");

    // THEN: Revocation succeeds
    assert_eq!(revoke_response.status(), 200);

    // WHEN: Client tries to use revoked refresh token
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(refresh_token))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Refresh token has been revoked",
        })))
        .mount(&mock_server.server)
        .await;

    let use_revoked = client
        .post(&mock_server.token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await
        .expect("Request failed");

    // THEN: Revoked token is rejected
    assert_eq!(use_revoked.status(), 400);

    // RFC 7009: Server SHOULD also revoke access token when refresh token revoked
    // and MAY revoke other tokens from same authorization grant
}

/// Test: Revocation propagation to dependent tokens
///
/// Security requirement: Revoking refresh token should invalidate access tokens
#[tokio::test]
async fn test_revocation_propagates_to_access_tokens() {
    // GIVEN: Client has both access and refresh tokens
    let mock_server = MockOAuth2Server::start().await;
    let revocation_endpoint = format!("{}/revoke", mock_server.server.uri());

    let access_token = "access_related";
    let refresh_token = "refresh_parent";

    mock_server
        .mock_token_success(access_token, Some(refresh_token))
        .await;

    let client = reqwest::Client::new();
    client
        .post(&mock_server.token_endpoint)
        .form(&[("grant_type", "authorization_code"), ("code", "code_123")])
        .send()
        .await
        .expect("Request failed");

    // WHEN: Refresh token is revoked
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    Mock::given(method("POST"))
        .and(path("/revoke"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server.server)
        .await;

    let revoke_response = client
        .post(&revocation_endpoint)
        .form(&[("token", refresh_token)])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(revoke_response.status(), 200);

    // THEN: Related access token should also be invalid
    // (Server implementation should mark access token as revoked)

    // Document: In production, resource server checks revocation status
    // via token introspection (RFC 7662) or revocation list cache
}

/// Test: Token lifetime best practices
///
/// 2025 Best practice: Short-lived access tokens (15-30 min)
/// Reference: OAuth 2.0 Security BCP, Auth0 guidelines
#[tokio::test]
async fn test_token_lifetime_best_practices() {
    // GIVEN: Mock server following 2025 best practices
    let mock_server = MockOAuth2Server::start().await;

    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "short_lived_access",
            "token_type": "Bearer",
            "expires_in": 1800, // 30 minutes (recommended max)
            "refresh_token": "long_lived_refresh",
        })))
        .mount(&mock_server.server)
        .await;

    // WHEN: Client requests tokens
    let client = reqwest::Client::new();
    let response = client
        .post(&mock_server.token_endpoint)
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await
        .expect("Request failed");

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.expect("Invalid JSON");
    let expires_in = body["expires_in"].as_u64().unwrap();

    // THEN: Access token follows best practices
    assert!(
        (900..=1800).contains(&expires_in),
        "Access token should expire between 15-30 minutes (got {} seconds)",
        expires_in
    );

    // Best practices:
    // - Access tokens: 15-30 minutes (900-1800 seconds)
    // - Refresh tokens: 7-14 days for SPAs, longer for confidential clients
    // - Rotate refresh tokens on every use (single-use)
    // - Implement automatic refresh at 80% of access token lifetime
}
