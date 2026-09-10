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
