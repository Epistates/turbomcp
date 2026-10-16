#![cfg(feature = "http-jwks")]
use axum::{Json, Router, routing::get};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use turbomcp_auth::{HttpJwks, JwkSource};
#[tokio::test]
async fn jwt_rejects_token_before_nbf() {
    use jsonwebtoken::{Algorithm, EncodingKey, Header};
    use turbomcp_auth::{BearerValidator, JwtValidator, StaticJwks};
    let secret = b"audit-secret-32-bytes-long-1234567";
    // URL-safe base64 encoding of the same ASCII secret.
    let jwks = serde_json::json!({"keys":[{"kty":"oct","k":"YXVkaXQtc2VjcmV0LTMyLWJ5dGVzLWxvbmctMTIzNDU2Nw","kid":"test"}]}).to_string();
    let source = StaticJwks::from_json(&jwks).unwrap();
    let validator = JwtValidator::new(source, "mcp", "issuer").algorithms(vec![Algorithm::HS256]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("test".into());
    let token = jsonwebtoken::encode(&header, &serde_json::json!({"sub":"alice","aud":"mcp","iss":"issuer","exp":now+7200,"nbf":now+3600}), &EncodingKey::from_secret(secret)).unwrap();
    assert!(validator.validate(&token).await.is_err());
}
#[tokio::test]
async fn concurrent_jwks_refresh_is_coalesced() {
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let app = Router::new().route(
        "/jwks",
        get(move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(100)).await;
                Json(json!({"keys":[]}))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/jwks", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let source = Arc::new(HttpJwks::new(url, Duration::from_secs(3600)));
    let barrier = Arc::new(tokio::sync::Barrier::new(20));
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..20 {
        let source = source.clone();
        let barrier = barrier.clone();
        jobs.spawn(async move {
            barrier.wait().await;
            let _ = source.decoding_key(Some("unknown")).await;
        });
    }
    while let Some(j) = jobs.join_next().await {
        j.unwrap();
    }
    server.abort();
    let n = count.load(Ordering::SeqCst);
    assert_eq!(n, 1, "concurrent misses must join one refresh");
}

#[tokio::test]
async fn failed_jwks_refresh_establishes_backoff() {
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let app = Router::new().route(
        "/jwks",
        get(move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/jwks", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let source = HttpJwks::new(url, Duration::from_secs(3600));
    for _ in 0..20 {
        assert!(source.decoding_key(Some("unknown")).await.is_err());
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    server.abort();
}

/// An issuer outage after the TTL keeps the last good key set serving (for
/// the stale-if-error grace) instead of failing every token at once, and an
/// encryption key never verifies a signature.
#[tokio::test]
async fn an_outage_serves_the_last_good_key_set() {
    let up = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let serving = up.clone();
    let app = Router::new().route(
        "/jwks",
        get(move || {
            let serving = serving.clone();
            async move {
                if serving.load(Ordering::SeqCst) {
                    Ok(Json(json!({ "keys": [
                        { "kty": "oct", "k": "c2VjcmV0LXNpZ25pbmcta2V5LTMyLWJ5dGVzLWxvbmc", "kid": "sig" },
                        { "kty": "oct", "k": "c2VjcmV0LXNpZ25pbmcta2V5LTMyLWJ5dGVzLWxvbmc", "kid": "enc", "use": "enc" },
                    ]})))
                } else {
                    Err(axum::http::StatusCode::SERVICE_UNAVAILABLE)
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/jwks", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let source = HttpJwks::new(url, Duration::from_millis(50));
    assert!(source.decoding_key(Some("sig")).await.is_ok());
    assert!(
        source.decoding_key(Some("enc")).await.is_err(),
        "a key published for encryption verifies nothing"
    );

    up.store(false, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        source.decoding_key(Some("sig")).await.is_ok(),
        "past its TTL with the issuer down, the last good set still serves"
    );

    let strict = HttpJwks::new(
        format!("http://127.0.0.1:{}/jwks", 9),
        Duration::from_millis(50),
    )
    .stale_if_error(Duration::ZERO);
    assert!(strict.decoding_key(Some("sig")).await.is_err());
    server.abort();
}
