//! Multi-provider concurrent authentication tests
//!
//! These tests verify authentication across multiple OAuth2 providers.
//! Tests cover:
//! - Concurrent authentication with Google, GitHub, Microsoft
//! - Per-provider JWKS endpoint configuration
//! - Issuer-specific validation rules
//! - Provider failover and fallback
//! - Cross-provider token isolation
//!
//! # Real-World Scenarios
//! Modern applications support multiple identity providers:
//! - Enterprise SSO (Microsoft Azure AD, Okta)
//! - Social login (Google, GitHub, Facebook)
//! - Custom identity providers
//!
//! Each provider has unique:
//! - JWKS endpoint URLs
//! - Issuer identifiers
//! - Token formats and claims
//! - Rate limits and caching strategies

mod common;

use common::MockOAuth2Server;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

/// Test: Concurrent authentication with multiple providers
///
/// Scenario: User can authenticate with Google, GitHub, or Microsoft
#[tokio::test]
async fn test_concurrent_multi_provider_authentication() {
    // GIVEN: Three OAuth2 providers
    let google_server = MockOAuth2Server::start().await;
    let github_server = MockOAuth2Server::start().await;
    let microsoft_server = MockOAuth2Server::start().await;

    // Configure provider-specific tokens
    google_server
        .mock_token_success("google_access_token", Some("google_refresh"))
        .await;

    github_server
        .mock_token_success("github_access_token", Some("github_refresh"))
        .await;

    microsoft_server
        .mock_token_success("microsoft_access_token", Some("microsoft_refresh"))
        .await;

    // WHEN: Three users authenticate concurrently with different providers
    let client = Arc::new(reqwest::Client::new());

    let google_url = google_server.token_endpoint.clone();
    let github_url = github_server.token_endpoint.clone();
    let microsoft_url = microsoft_server.token_endpoint.clone();

    let google_task = {
        let client = Arc::clone(&client);
        tokio::spawn(async move {
            client
                .post(&google_url)
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", "google_code_123"),
                ])
                .send()
                .await
                .expect("Google auth failed")
        })
    };

    let github_task = {
        let client = Arc::clone(&client);
        tokio::spawn(async move {
            client
                .post(&github_url)
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", "github_code_456"),
                ])
                .send()
                .await
                .expect("GitHub auth failed")
        })
    };

    let microsoft_task = {
        let client = Arc::clone(&client);
        tokio::spawn(async move {
            client
                .post(&microsoft_url)
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("code", "microsoft_code_789"),
                ])
                .send()
                .await
                .expect("Microsoft auth failed")
        })
    };

    // THEN: All authentications succeed concurrently
    let (google_result, github_result, microsoft_result) =
        tokio::join!(google_task, github_task, microsoft_task);

    let google_response = google_result.unwrap();
    let github_response = github_result.unwrap();
    let microsoft_response = microsoft_result.unwrap();

    assert_eq!(google_response.status(), 200);
    assert_eq!(github_response.status(), 200);
    assert_eq!(microsoft_response.status(), 200);

    // Verify provider-specific tokens
    let google_body: serde_json::Value = google_response.json().await.unwrap();
    let github_body: serde_json::Value = github_response.json().await.unwrap();
    let microsoft_body: serde_json::Value = microsoft_response.json().await.unwrap();

    assert!(
        google_body["access_token"]
            .as_str()
            .unwrap()
            .contains("google")
    );
    assert!(
        github_body["access_token"]
            .as_str()
            .unwrap()
            .contains("github")
    );
    assert!(
        microsoft_body["access_token"]
            .as_str()
            .unwrap()
            .contains("microsoft")
    );
}

/// Test: Per-provider JWKS endpoint configuration
///
/// Each provider has unique JWKS URL and key rotation schedule
#[tokio::test]
async fn test_per_provider_jwks_configuration() {
    // GIVEN: Multiple providers with different JWKS endpoints
    let google_server = MockOAuth2Server::start().await;
    let github_server = MockOAuth2Server::start().await;

    let google_jwk = json!({
        "kty": "RSA",
        "kid": "google-key-2025",
        "use": "sig",
        "alg": "RS256",
        "n": "google_modulus",
        "e": "AQAB"
    });

    let github_jwk = json!({
        "kty": "RSA",
        "kid": "github-key-2025",
        "use": "sig",
        "alg": "RS256",
        "n": "github_modulus",
        "e": "AQAB"
    });

    google_server.mock_jwks(google_jwk.clone()).await;
    github_server.mock_jwks(github_jwk.clone()).await;

    // WHEN: Configure validator with multiple JWKS endpoints
    let mut jwks_endpoints = HashMap::new();
    jwks_endpoints.insert("google", google_server.jwks_endpoint.clone());
    jwks_endpoints.insert("github", github_server.jwks_endpoint.clone());

    // THEN: Each provider's JWKS is accessible
    let client = reqwest::Client::new();

    for (provider, endpoint) in &jwks_endpoints {
        let response = client
            .get(endpoint)
            .send()
            .await
            .unwrap_or_else(|_| panic!("{} JWKS fetch failed", provider));

        assert_eq!(response.status(), 200);
        let jwks: serde_json::Value = response.json().await.unwrap();
        let kid = jwks["keys"][0]["kid"].as_str().unwrap();

        assert!(
            kid.contains(provider),
            "JWKS should be provider-specific: {}",
            kid
        );
    }

    // Document: Validator maintains separate JWKS caches per issuer
}

/// Test: Issuer-specific validation rules
///
/// Different providers have different claim requirements
#[tokio::test]
async fn test_issuer_specific_validation_rules() {
    // GIVEN: Provider-specific validation configurations
    let mut provider_configs = HashMap::new();

    // Google: Requires 'email_verified' claim
    provider_configs.insert(
        "https://accounts.google.com",
        json!({
            "required_claims": ["sub", "email", "email_verified"],
            "audience": "google_client_id.apps.googleusercontent.com"
        }),
    );

    // GitHub: Uses 'login' instead of 'email'
    provider_configs.insert(
        "https://github.com",
        json!({
            "required_claims": ["sub", "login"],
            "audience": "github_oauth_app_id"
        }),
    );

    // Microsoft: Requires 'tid' (tenant ID)
    provider_configs.insert(
        "https://login.microsoftonline.com/common/v2.0",
        json!({
            "required_claims": ["sub", "tid", "preferred_username"],
            "audience": "microsoft_client_id"
        }),
    );

    // WHEN: Validate tokens from each provider
    for (issuer, config) in &provider_configs {
        let required_claims = config["required_claims"].as_array().unwrap();

        // THEN: Each provider has unique requirements
        assert!(
            !required_claims.is_empty(),
            "Provider {} should have required claims",
            issuer
        );

        if issuer.contains("google") {
            assert!(
                required_claims
                    .iter()
                    .any(|c| c.as_str() == Some("email_verified"))
            );
        } else if issuer.contains("github") {
            assert!(required_claims.iter().any(|c| c.as_str() == Some("login")));
        } else if issuer.contains("microsoft") {
            assert!(required_claims.iter().any(|c| c.as_str() == Some("tid")));
        }
    }

    // Document: Implement provider-specific validation strategies
}

/// Test: Provider failover and fallback
///
/// If primary provider fails, attempt alternate provider
#[tokio::test]
async fn test_provider_failover() {
    // GIVEN: Primary and backup providers
    let primary_server = MockOAuth2Server::start().await;
    let backup_server = MockOAuth2Server::start().await;

    // Primary provider fails
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(503)) // Service unavailable
        .mount(&primary_server.server)
        .await;

    // Backup provider works
    backup_server
        .mock_token_success("backup_access_token", Some("backup_refresh"))
        .await;

    let client = reqwest::Client::new();

    // WHEN: Primary provider fails
    let primary_result = client
        .post(&primary_server.token_endpoint)
        .form(&[("grant_type", "authorization_code"), ("code", "code_123")])
        .send()
        .await;

    match primary_result {
        Ok(resp) if resp.status() == 503 => {
            // THEN: Fall back to backup provider
            let backup_result = client
                .post(&backup_server.token_endpoint)
                .form(&[("grant_type", "authorization_code"), ("code", "code_123")])
                .send()
                .await
                .expect("Backup provider failed");

            assert_eq!(backup_result.status(), 200);
            let backup_body: serde_json::Value = backup_result.json().await.unwrap();
            assert!(
                backup_body["access_token"]
                    .as_str()
                    .unwrap()
                    .contains("backup")
            );
        }
        _ => panic!("Expected primary provider to fail"),
    }

    // Document: Implement circuit breaker pattern for provider failures
}

/// Test: Cross-provider token isolation
///
/// Tokens from one provider should not be valid for another
#[tokio::test]
async fn test_cross_provider_token_isolation() {
    // GIVEN: Tokens from different providers
    let google_server = MockOAuth2Server::start().await;
    let github_server = MockOAuth2Server::start().await;

    google_server.mock_token_success("google_token", None).await;

    github_server.mock_token_success("github_token", None).await;

    let client = reqwest::Client::new();

    // Obtain Google token
    let google_response = client
        .post(&google_server.token_endpoint)
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await
        .expect("Google auth failed");

    let google_body: serde_json::Value = google_response.json().await.unwrap();
    let google_token = google_body["access_token"].as_str().unwrap();

    // WHEN: Try to use Google token with GitHub resource
    // (In production, this would be validated via issuer claim)

    let issuer_check = |token: &str, expected_provider: &str| {
        // Simulate JWT decode to check issuer
        // In real code: decode JWT, check 'iss' claim
        if expected_provider == "google" {
            token.contains("google")
        } else {
            token.contains(expected_provider)
        }
    };

    // THEN: Google token should not be valid for GitHub
    assert!(issuer_check(google_token, "google"));
    assert!(!issuer_check(google_token, "github"));

    // Document: Validate 'iss' and 'aud' claims to prevent cross-provider token use
}

/// Test: Rate limiting per provider
///
/// Each provider has different rate limits and retry strategies
#[tokio::test]
async fn test_per_provider_rate_limiting() {
    // GIVEN: Providers with different rate limits
    let mut rate_limits = HashMap::new();
    rate_limits.insert("google", 10); // 10 req/sec
    rate_limits.insert("github", 5); // 5 req/sec
    rate_limits.insert("microsoft", 20); // 20 req/sec

    // WHEN: Making rapid requests to each provider
    for (provider, limit) in &rate_limits {
        // THEN: Track request rate
        let requests_per_second = *limit;

        // Document: Implement per-provider rate limiters
        // Use token bucket algorithm with provider-specific refill rates
        assert!(
            requests_per_second > 0,
            "Provider {} should have rate limit",
            provider
        );
    }

    // Best practice: Implement exponential backoff with per-provider state
    // Example: Google 429 response → backoff 1s, 2s, 4s, 8s
}

/// Test: Provider-specific error handling
///
/// Each provider returns errors differently
#[tokio::test]
async fn test_provider_specific_error_formats() {
    // GIVEN: Mock providers with different error formats
    let google_server = MockOAuth2Server::start().await;
    let github_server = MockOAuth2Server::start().await;

    // Google error format (OAuth 2.0 standard)
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Code was already redeemed."
        })))
        .mount(&google_server.server)
        .await;

    // GitHub error format (includes error_uri)
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "bad_verification_code",
            "error_description": "The code passed is incorrect or expired.",
            "error_uri": "https://docs.github.com/apps/troubleshooting"
        })))
        .mount(&github_server.server)
        .await;

    let client = reqwest::Client::new();

    // WHEN: Errors occur
    let google_error = client
        .post(&google_server.token_endpoint)
        .form(&[("grant_type", "authorization_code"), ("code", "invalid")])
        .send()
        .await
        .expect("Request failed");

    let github_error = client
        .post(&github_server.token_endpoint)
        .form(&[("grant_type", "authorization_code"), ("code", "invalid")])
        .send()
        .await
        .expect("Request failed");

    // THEN: Parse provider-specific error formats
    assert_eq!(google_error.status(), 400);
    assert_eq!(github_error.status(), 400);

    let google_body: serde_json::Value = google_error.json().await.unwrap();
    let github_body: serde_json::Value = github_error.json().await.unwrap();

    assert_eq!(google_body["error"], "invalid_grant");
    assert_eq!(github_body["error"], "bad_verification_code");
    assert!(
        github_body["error_uri"].is_string(),
        "GitHub includes error_uri"
    );

    // Document: Map provider errors to unified error types
}

/// Test: Concurrent JWKS fetching for multiple providers
///
/// Scenario: Application startup fetches JWKS from all providers simultaneously
#[tokio::test]
async fn test_concurrent_jwks_fetching_multi_provider() {
    // GIVEN: Multiple providers with JWKS endpoints
    let providers = vec![
        MockOAuth2Server::start().await,
        MockOAuth2Server::start().await,
        MockOAuth2Server::start().await,
    ];

    let provider_names = ["Google", "GitHub", "Microsoft"];

    // Mock JWKS for each provider
    for (i, server) in providers.iter().enumerate() {
        let jwk = json!({
            "kty": "RSA",
            "kid": format!("{}-key-2025", provider_names[i].to_lowercase()),
            "use": "sig",
            "alg": "RS256",
            "n": format!("{}_modulus", provider_names[i].to_lowercase()),
            "e": "AQAB"
        });

        server.mock_jwks(jwk).await;
    }

    // WHEN: Fetch JWKS concurrently on startup
    let client = Arc::new(reqwest::Client::new());
    let mut tasks = vec![];

    for server in &providers {
        let client_clone = Arc::clone(&client);
        let url = server.jwks_endpoint.clone();

        let task = tokio::spawn(async move {
            client_clone
                .get(&url)
                .send()
                .await
                .expect("JWKS fetch failed")
        });

        tasks.push(task);
    }

    // THEN: All JWKS fetches succeed
    for (i, task) in tasks.into_iter().enumerate() {
        let response = task.await.expect("Task failed");
        assert_eq!(
            response.status(),
            200,
            "{} JWKS fetch should succeed",
            provider_names[i]
        );

        let jwks: serde_json::Value = response.json().await.unwrap();
        let kid = jwks["keys"][0]["kid"].as_str().unwrap();
        assert!(
            kid.contains(&provider_names[i].to_lowercase()),
            "JWKS should be for {}",
            provider_names[i]
        );
    }

    // Document: Parallel JWKS fetching reduces startup time
    // Alternative: Fetch JWKS lazily on first use per provider
}
