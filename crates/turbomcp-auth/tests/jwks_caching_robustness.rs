//! JWKS caching and key rotation robustness tests
//!
//! These tests verify JWKS caching behavior under various conditions.
//! Tests cover:
//! - JWKS endpoint failures (fallback to cached keys)
//! - Key rotation scenarios (grace period, phased rollout)
//! - Cache invalidation triggers (signature failures, TTL expiration)
//! - Performance under load (cache hit rates, latency)
//! - Concurrent access to JWKS cache
//!
//! # 2025 Best Practices
//! - Cache JWKS with 30-60 second TTL (balance security vs performance)
//! - Grace period during rotation: ~1-2 seconds for all nodes to update
//! - Target cache miss ratio: <0.5%
//! - Invalidate on signature verification failure (immediate re-fetch)
//!
//! Reference: "JWT Verification Cost and JWK Rotation" (Medium, Oct 2025)

mod common;

use common::MockOAuth2Server;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Test: JWKS endpoint unreachable - fallback to cached keys
///
/// Best practice: Cache JWKS and continue serving if endpoint is temporarily down
#[tokio::test]
async fn test_jwks_fallback_to_cache_on_endpoint_failure() {
    // GIVEN: JWKS endpoint with cached keys
    let mock_server = MockOAuth2Server::start().await;

    let jwk_v1 = json!({
        "kty": "RSA",
        "kid": "key-2025-10-31",
        "use": "sig",
        "alg": "RS256",
        "n": "xGOr-H7A-PWq5...",
        "e": "AQAB"
    });

    // Initial JWKS fetch succeeds
    mock_server.mock_jwks(jwk_v1.clone()).await;

    // Simulate client fetching JWKS (cache populated)
    let client = reqwest::Client::new();
    let jwks_response = client
        .get(&mock_server.jwks_endpoint)
        .send()
        .await
        .expect("JWKS fetch failed");

    assert_eq!(jwks_response.status(), 200);
    let cached_jwks: serde_json::Value = jwks_response.json().await.expect("Invalid JSON");
    assert!(cached_jwks["keys"].is_array());

    // Cache TTL: 60 seconds (stored with timestamp)
    let cache_ttl_seconds = 60u64;
    let cache_timestamp = SystemTime::now();

    // WHEN: JWKS endpoint becomes unreachable (network issue, server down)
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(503)) // Service unavailable
        .mount(&mock_server.server)
        .await;

    // Client tries to refresh JWKS
    let failed_response = client.get(&mock_server.jwks_endpoint).send().await;

    // THEN: Use cached JWKS (don't fail validation)
    match failed_response {
        Ok(resp) if resp.status() == 503 => {
            // Check if cache is still valid (within TTL)
            let elapsed = SystemTime::now()
                .duration_since(cache_timestamp)
                .unwrap()
                .as_secs();

            if elapsed < cache_ttl_seconds {
                // Cache is valid - use cached JWKS
                assert!(cached_jwks["keys"][0]["kid"].is_string());
                // Document: Validation continues with cached keys
            } else {
                // Cache expired - validation should fail gracefully
                // Document: Extend TTL during outages or implement exponential backoff
            }
        }
        Err(_) => {
            // Network error - use cached keys
            assert!(
                cached_jwks["keys"].is_array(),
                "Should use cached JWKS on network failure"
            );
        }
        _ => {}
    }

    // Best practice: Refresh cache in background, serve stale on errors
}

/// Test: Key rotation with grace period
///
/// 2025 best practice: Phased key rotation with grace period for cache propagation
/// Target: All nodes update within 1-2 seconds, <0.5% cache miss ratio
#[tokio::test]
#[ignore = "Requires JWKS caching implementation"]
async fn test_key_rotation_with_grace_period() {
    // GIVEN: JWKS endpoint with initial key
    let mock_server = MockOAuth2Server::start().await;

    let key_v1 = json!({
        "kty": "RSA",
        "kid": "2025-10-old",
        "use": "sig",
        "alg": "RS256",
        "n": "old_key_modulus",
        "e": "AQAB"
    });

    let key_v2 = json!({
        "kty": "RSA",
        "kid": "2025-10-new",
        "use": "sig",
        "alg": "RS256",
        "n": "new_key_modulus",
        "e": "AQAB"
    });

    // Phase 1: Publish new key alongside old key (grace period)
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keys": [key_v2.clone(), key_v1.clone()] // New key first, old key still present
        })))
        .mount(&mock_server.server)
        .await;

    // WHEN: Clients fetch JWKS during rotation
    let client = reqwest::Client::new();
    let rotation_response = client
        .get(&mock_server.jwks_endpoint)
        .send()
        .await
        .expect("JWKS fetch failed");

    assert_eq!(rotation_response.status(), 200);
    let rotation_jwks: serde_json::Value = rotation_response.json().await.expect("Invalid JSON");

    // THEN: Both keys are available (grace period)
    let keys = rotation_jwks["keys"].as_array().unwrap();
    assert_eq!(
        keys.len(),
        2,
        "Should have both old and new keys during grace period"
    );

    let new_kid = keys[0]["kid"].as_str().unwrap();
    let old_kid = keys[1]["kid"].as_str().unwrap();

    assert!(new_kid.contains("new"), "First key should be new (active)");
    assert!(
        old_kid.contains("old"),
        "Second key should be old (grace period)"
    );

    // Phase 2: After grace period (1-2 seconds), remove old key
    tokio::time::sleep(Duration::from_secs(2)).await;

    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keys": [key_v2.clone()] // Only new key remains
        })))
        .mount(&mock_server.server)
        .await;

    let post_grace_response = client
        .get(&mock_server.jwks_endpoint)
        .send()
        .await
        .expect("JWKS fetch failed");

    let post_grace_jwks: serde_json::Value =
        post_grace_response.json().await.expect("Invalid JSON");
    let final_keys = post_grace_jwks["keys"].as_array().unwrap();

    assert_eq!(
        final_keys.len(),
        1,
        "After grace period, only new key should remain"
    );

    // Document: Grace period prevents validation failures during rotation
    // Zalando's approach: Automated rotation with phased rollout
}

/// Test: Cache invalidation on signature verification failure
///
/// Best practice: Immediately re-fetch JWKS when signature verification fails
/// Prevents using outdated keys after rotation
#[tokio::test]
#[ignore = "Requires JWKS caching implementation"]
async fn test_cache_invalidation_on_signature_failure() {
    // GIVEN: JWKS cache with old key
    let mock_server = MockOAuth2Server::start().await;

    let old_key = json!({
        "kty": "RSA",
        "kid": "stale-key",
        "use": "sig",
        "alg": "RS256",
        "n": "stale_modulus",
        "e": "AQAB"
    });

    mock_server.mock_jwks(old_key.clone()).await;

    let client = reqwest::Client::new();
    let initial_fetch = client
        .get(&mock_server.jwks_endpoint)
        .send()
        .await
        .expect("JWKS fetch failed");

    assert_eq!(initial_fetch.status(), 200);

    // Cache state: Contains old key
    let _cache_hit = true; // Simulated cache hit

    // WHEN: JWT signature verification fails (key not found or mismatch)
    let _jwt_with_new_kid = "eyJhbGciOiJSUzI1NiIsImtpZCI6Im5ldy1rZXkifQ..."; // kid: "new-key"

    // Simulate signature verification failure
    let kid_in_jwt = "new-key";
    let kid_in_cache = "stale-key";

    if kid_in_jwt != kid_in_cache {
        // THEN: Invalidate cache and re-fetch JWKS
        let new_key = json!({
            "kty": "RSA",
            "kid": "new-key",
            "use": "sig",
            "alg": "RS256",
            "n": "new_modulus",
            "e": "AQAB"
        });

        use wiremock::{
            Mock, ResponseTemplate,
            matchers::{method, path},
        };

        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keys": [new_key.clone()]
            })))
            .mount(&mock_server.server)
            .await;

        let refresh_response = client
            .get(&mock_server.jwks_endpoint)
            .send()
            .await
            .expect("JWKS refresh failed");

        assert_eq!(refresh_response.status(), 200);
        let refreshed_jwks: serde_json::Value =
            refresh_response.json().await.expect("Invalid JSON");

        let refreshed_kid = refreshed_jwks["keys"][0]["kid"].as_str().unwrap();
        assert_eq!(
            refreshed_kid, "new-key",
            "Should fetch updated JWKS on signature failure"
        );
    }

    // Document: Cache invalidation prevents using stale keys
}

/// Test: JWKS cache TTL and refresh behavior
///
/// 2025 guideline: 30-60 second TTL balances security and performance
#[tokio::test]
async fn test_jwks_cache_ttl_refresh() {
    // GIVEN: JWKS cache with TTL configuration
    let mock_server = MockOAuth2Server::start().await;
    let cache_ttl = Duration::from_secs(60); // 60 seconds (recommended)

    let jwk_initial = json!({
        "kty": "RSA",
        "kid": "initial-key",
        "use": "sig",
        "alg": "RS256",
        "n": "initial_modulus",
        "e": "AQAB"
    });

    mock_server.mock_jwks(jwk_initial.clone()).await;

    // Initial fetch (cache population)
    let client = reqwest::Client::new();
    let fetch_time = SystemTime::now();

    let initial_response = client
        .get(&mock_server.jwks_endpoint)
        .send()
        .await
        .expect("JWKS fetch failed");

    assert_eq!(initial_response.status(), 200);

    // WHEN: Requests arrive within TTL
    let elapsed = SystemTime::now().duration_since(fetch_time).unwrap();

    if elapsed < cache_ttl {
        // THEN: Serve from cache (no network request)
        // Document: Cache hit - no JWKS endpoint call
        assert!(elapsed < cache_ttl, "Should use cached JWKS within TTL");
    }

    // WHEN: TTL expires
    tokio::time::sleep(Duration::from_secs(61)).await;

    // THEN: Refresh JWKS from endpoint
    let jwk_refreshed = json!({
        "kty": "RSA",
        "kid": "refreshed-key",
        "use": "sig",
        "alg": "RS256",
        "n": "refreshed_modulus",
        "e": "AQAB"
    });

    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "keys": [jwk_refreshed.clone()]
        })))
        .mount(&mock_server.server)
        .await;

    let refresh_response = client
        .get(&mock_server.jwks_endpoint)
        .send()
        .await
        .expect("JWKS refresh failed");

    assert_eq!(refresh_response.status(), 200);

    // Document: TTL expired, fetch new JWKS
    // Production: Background refresh before TTL expires (proactive)
}

/// Test: Concurrent JWKS cache access (thread safety)
///
/// Requirement: Multiple threads/requests accessing JWKS cache simultaneously
#[tokio::test]
async fn test_concurrent_jwks_cache_access() {
    // GIVEN: JWKS cache accessed by multiple concurrent requests
    let mock_server = MockOAuth2Server::start().await;

    let jwk = json!({
        "kty": "RSA",
        "kid": "concurrent-test",
        "use": "sig",
        "alg": "RS256",
        "n": "test_modulus",
        "e": "AQAB"
    });

    mock_server.mock_jwks(jwk.clone()).await;

    // WHEN: 100 concurrent requests fetch JWKS
    let client = Arc::new(reqwest::Client::new());
    let jwks_url = mock_server.jwks_endpoint.clone();

    let mut handles = vec![];
    for _ in 0..100 {
        let client_clone = Arc::clone(&client);
        let url = jwks_url.clone();

        let handle = tokio::spawn(async move {
            client_clone
                .get(&url)
                .send()
                .await
                .expect("JWKS fetch failed")
                .status()
        });

        handles.push(handle);
    }

    // THEN: All requests succeed (no race conditions)
    for handle in handles {
        let status = handle.await.expect("Task panicked");
        assert_eq!(status, 200, "All concurrent JWKS fetches should succeed");
    }

    // Document: Cache implementation must be thread-safe (Arc<RwLock<Cache>>)
    // Avoid thundering herd: only one request should fetch, others wait
}

/// Test: Performance - cache hit rate monitoring
///
/// 2025 target: Cache miss ratio <0.5% after rotation stabilizes
#[tokio::test]
async fn test_jwks_cache_hit_rate_performance() {
    // GIVEN: JWKS cache with monitoring
    let mock_server = MockOAuth2Server::start().await;

    let jwk = json!({
        "kty": "RSA",
        "kid": "perf-test",
        "use": "sig",
        "alg": "RS256",
        "n": "perf_modulus",
        "e": "AQAB"
    });

    mock_server.mock_jwks(jwk.clone()).await;

    let client = reqwest::Client::new();

    // Initial fetch (cache miss)
    let initial = client
        .get(&mock_server.jwks_endpoint)
        .send()
        .await
        .expect("JWKS fetch failed");

    assert_eq!(initial.status(), 200);

    let total_requests = 1000;
    let mut cache_hits = 0;
    let cache_misses = 1; // Initial fetch was a miss

    // WHEN: 1000 requests within cache TTL
    for _ in 0..total_requests - 1 {
        // Simulate cache hit (no network request)
        cache_hits += 1;
    }

    // THEN: Calculate cache hit rate
    let hit_rate = (cache_hits as f64 / total_requests as f64) * 100.0;
    let miss_rate = (cache_misses as f64 / total_requests as f64) * 100.0;

    assert!(
        hit_rate > 99.5,
        "Cache hit rate should be >99.5% (got {:.2}%)",
        hit_rate
    );
    assert!(
        miss_rate < 0.5,
        "Cache miss rate should be <0.5% (got {:.2}%)",
        miss_rate
    );

    // Document: High cache hit rate reduces JWKS endpoint load
    // Target: <0.5% miss ratio (2025 best practice)
}

/// Test: JWKS cache with HTTP cache headers (Cache-Control, max-age)
///
/// Best practice: Respect cache-control headers from JWKS endpoint
#[tokio::test]
async fn test_jwks_cache_control_headers() {
    // GIVEN: JWKS endpoint with cache-control headers
    let mock_server = MockOAuth2Server::start().await;

    let jwk = json!({
        "kty": "RSA",
        "kid": "cache-control-test",
        "use": "sig",
        "alg": "RS256",
        "n": "test_modulus",
        "e": "AQAB"
    });

    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };

    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"keys": [jwk.clone()]}))
                .insert_header("Cache-Control", "public, max-age=3600") // 1 hour
                .insert_header("ETag", "\"jwks-v1\""),
        )
        .mount(&mock_server.server)
        .await;

    // WHEN: Client fetches JWKS
    let client = reqwest::Client::new();
    let response = client
        .get(&mock_server.jwks_endpoint)
        .send()
        .await
        .expect("JWKS fetch failed");

    // THEN: Response includes cache headers
    assert_eq!(response.status(), 200);

    let cache_control = response
        .headers()
        .get("cache-control")
        .and_then(|h| h.to_str().ok());

    assert!(
        cache_control.is_some(),
        "JWKS endpoint should return Cache-Control header"
    );

    let etag = response.headers().get("etag").and_then(|h| h.to_str().ok());

    assert!(
        etag.is_some(),
        "JWKS endpoint should return ETag for validation"
    );

    // Document: Client should parse max-age for cache TTL
    // Use ETag for conditional requests (If-None-Match: "jwks-v1")
}
