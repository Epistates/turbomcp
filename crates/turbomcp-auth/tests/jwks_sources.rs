//! The two `JwkSource` implementations.
//!
//! `HttpJwks` is the one a real deployment uses, and it is the one place in the
//! auth path where getting the *caching* wrong is a security problem rather
//! than a performance one: cache too long and a revoked key keeps validating
//! tokens; refresh on every miss and an attacker with an unknown `kid` has a
//! free request amplifier against the authorization server. The rotation path
//! — one forced refresh on a `kid` miss, then give up — is the whole design,
//! so these count fetches rather than only asserting on outcomes.
//!
//! Symmetric (`oct`) JWKs throughout: key *resolution* is what's under test,
//! and it is identical for RSA without needing keygen in-test.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use turbomcp_auth::StaticJwks;

fn jwk(kid: &str) -> Value {
    json!({
        "kty": "oct",
        "k": URL_SAFE_NO_PAD.encode(format!("{kid}-secret-padded-to-32-bytes!!")),
        "alg": "HS256",
        "kid": kid,
    })
}

// ---- StaticJwks --------------------------------------------------------------

#[test]
fn a_lone_key_resolves_without_a_kid() {
    let jwks = StaticJwks::from_json(&json!({ "keys": [jwk("only")] }).to_string()).unwrap();
    futures::executor::block_on(async {
        use turbomcp_auth::JwkSource as _;
        assert!(
            jwks.decoding_key(None).await.is_ok(),
            "one key and no kid is unambiguous"
        );
        assert!(jwks.decoding_key(Some("only")).await.is_ok());
        assert!(
            jwks.decoding_key(Some("other")).await.is_err(),
            "a named kid that isn't present must not fall back to the lone key"
        );
    });
}

/// With more than one key and no `kid`, there is no defensible choice — picking
/// the first would validate a token against a key its issuer never used.
#[test]
fn several_keys_without_a_kid_is_ambiguous_and_refused() {
    let jwks = StaticJwks::from_json(&json!({ "keys": [jwk("a"), jwk("b")] }).to_string()).unwrap();
    futures::executor::block_on(async {
        use turbomcp_auth::JwkSource as _;
        assert!(jwks.decoding_key(None).await.is_err());
        assert!(jwks.decoding_key(Some("a")).await.is_ok());
        assert!(jwks.decoding_key(Some("b")).await.is_ok());
    });
}

#[test]
fn a_malformed_document_is_rejected_at_construction() {
    assert!(StaticJwks::from_json("not json").is_err());
    assert!(
        StaticJwks::from_json(&json!({ "keys": "nope" }).to_string()).is_err(),
        "`keys` must be an array"
    );
}

#[test]
fn an_already_parsed_key_set_can_be_wrapped_directly() {
    let set: jsonwebtoken::jwk::JwkSet =
        serde_json::from_value(json!({ "keys": [jwk("pre")] })).unwrap();
    let jwks = StaticJwks::new(set);
    futures::executor::block_on(async {
        use turbomcp_auth::JwkSource as _;
        assert!(jwks.decoding_key(Some("pre")).await.is_ok());
    });
}

// ---- HttpJwks ----------------------------------------------------------------

#[cfg(feature = "http-jwks")]
mod http {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use turbomcp_auth::{HttpJwks, JwkSource as _};

    /// A JWKS endpoint that counts requests and can swap its key set, so a test
    /// can tell a cache hit from a refetch and simulate a rotation.
    #[derive(Clone)]
    struct Endpoint {
        hits: Arc<AtomicUsize>,
        keys: Arc<std::sync::Mutex<Vec<Value>>>,
    }

    async fn spawn(keys: Vec<Value>) -> (String, Endpoint) {
        let state = Endpoint {
            hits: Arc::new(AtomicUsize::new(0)),
            keys: Arc::new(std::sync::Mutex::new(keys)),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = format!("http://{}/jwks", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/jwks",
                axum::routing::get(
                    |axum::extract::State(s): axum::extract::State<Endpoint>| async move {
                        s.hits.fetch_add(1, Ordering::SeqCst);
                        let keys = s.keys.lock().unwrap().clone();
                        axum::Json(json!({ "keys": keys }))
                    },
                ),
            )
            .with_state(state.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (uri, state)
    }

    #[tokio::test]
    async fn a_fresh_cache_is_served_without_refetching() {
        let (uri, ep) = spawn(vec![jwk("k1")]).await;
        let source = HttpJwks::new(uri, Duration::from_secs(3600));

        assert!(source.decoding_key(Some("k1")).await.is_ok());
        assert_eq!(ep.hits.load(Ordering::SeqCst), 1, "first call fetches");

        for _ in 0..5 {
            assert!(source.decoding_key(Some("k1")).await.is_ok());
        }
        assert_eq!(
            ep.hits.load(Ordering::SeqCst),
            1,
            "a hit within the TTL must not touch the authorization server"
        );
    }

    #[tokio::test]
    async fn an_expired_cache_refetches() {
        let (uri, ep) = spawn(vec![jwk("k1")]).await;
        let source = HttpJwks::new(uri, Duration::from_millis(50));

        assert!(source.decoding_key(Some("k1")).await.is_ok());
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(source.decoding_key(Some("k1")).await.is_ok());
        assert_eq!(ep.hits.load(Ordering::SeqCst), 2);
    }

    /// Rotation: the cache holds the old key, the token names the new one. One
    /// forced refresh must pick it up — otherwise every token signed with a
    /// freshly rotated key is rejected until the TTL happens to lapse.
    #[tokio::test]
    async fn a_kid_miss_forces_one_refresh_and_picks_up_a_rotated_key() {
        let (uri, ep) = spawn(vec![jwk("old")]).await;
        // Zero cooldown: this test is about the rotation path, not the rate
        // limit on it — those are exercised separately below.
        let source = HttpJwks::new(uri, Duration::from_secs(3600)).refresh_cooldown(Duration::ZERO);

        assert!(source.decoding_key(Some("old")).await.is_ok());
        assert_eq!(ep.hits.load(Ordering::SeqCst), 1);

        *ep.keys.lock().unwrap() = vec![jwk("new")];
        assert!(
            source.decoding_key(Some("new")).await.is_ok(),
            "a kid absent from a still-fresh cache must trigger a refresh"
        );
        assert_eq!(ep.hits.load(Ordering::SeqCst), 2);
    }

    /// The forced refresh is rate-limited.
    ///
    /// A `kid` is attacker-chosen — it comes out of the token header, which is
    /// read *before* any signature is verified — so refreshing on every miss
    /// turns any unauthenticated caller into a traffic amplifier pointed at the
    /// authorization server. Within the cooldown, a miss must cost zero
    /// upstream requests.
    #[tokio::test]
    async fn an_unknown_kid_cannot_drive_unbounded_upstream_fetches() {
        let (uri, ep) = spawn(vec![jwk("k1")]).await;
        let source = HttpJwks::new(uri, Duration::from_secs(3600));

        assert!(source.decoding_key(Some("nope")).await.is_err());
        assert_eq!(
            ep.hits.load(Ordering::SeqCst),
            1,
            "the cold-cache fetch is itself inside the cooldown, so the forced \
             retry re-reads what was just fetched instead of asking again"
        );

        for _ in 0..20 {
            assert!(source.decoding_key(Some("nope")).await.is_err());
        }
        assert_eq!(
            ep.hits.load(Ordering::SeqCst),
            1,
            "misses inside the cooldown must not reach the authorization server"
        );
    }

    /// …and the cooldown expires, so a rotation is still picked up.
    #[tokio::test]
    async fn the_cooldown_expires_so_rotation_still_converges() {
        let (uri, ep) = spawn(vec![jwk("old")]).await;
        let source = HttpJwks::new(uri, Duration::from_secs(3600))
            .refresh_cooldown(Duration::from_millis(50));

        assert!(source.decoding_key(Some("old")).await.is_ok());
        *ep.keys.lock().unwrap() = vec![jwk("new")];

        assert!(
            source.decoding_key(Some("new")).await.is_err(),
            "inside the cooldown the stale cache still answers"
        );
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(
            source.decoding_key(Some("new")).await.is_ok(),
            "once the cooldown lapses the rotation is picked up"
        );
    }

    #[tokio::test]
    async fn a_lone_fetched_key_resolves_without_a_kid() {
        let (uri, _) = spawn(vec![jwk("solo")]).await;
        let source = HttpJwks::new(uri, Duration::from_secs(3600));
        assert!(source.decoding_key(None).await.is_ok());

        let (uri, _) = spawn(vec![jwk("a"), jwk("b")]).await;
        let source = HttpJwks::new(uri, Duration::from_secs(3600));
        assert!(
            source.decoding_key(None).await.is_err(),
            "two fetched keys and no kid is as ambiguous as it is for StaticJwks"
        );
    }

    /// An unreachable authorization server must surface as `KeyUnavailable`
    /// with the cause attached — this is what an operator reads at 3am.
    #[tokio::test]
    async fn an_unreachable_endpoint_reports_key_unavailable() {
        // Port 1 on loopback: reserved, nothing binds it.
        let source = HttpJwks::new("http://127.0.0.1:1/jwks", Duration::from_secs(60));
        let err = source
            .decoding_key(Some("k1"))
            .await
            .expect_err("no server");
        let msg = err.to_string();
        assert!(
            msg.contains("JWKS fetch failed"),
            "unhelpful message: {msg}"
        );
    }

    #[tokio::test]
    async fn a_non_jwks_body_reports_a_decode_failure() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = format!("http://{}/jwks", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/jwks",
            axum::routing::get(|| async { axum::Json(json!({ "keys": "not-an-array" })) }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let source = HttpJwks::new(uri, Duration::from_secs(60));
        let err = source.decoding_key(Some("k1")).await.expect_err("bad body");
        let msg = err.to_string();
        assert!(
            msg.contains("JWKS decode failed"),
            "unhelpful message: {msg}"
        );
    }
}
