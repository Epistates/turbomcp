//! RFC 7662 introspection against a local authorization server: what is
//! accepted (active, for this audience, from a required issuer, in its
//! validity window), how the resource server authenticates, that answers are
//! cached by token and not past `exp`, and that every failure closes.
#![cfg(feature = "introspection")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine as _;
use serde_json::{Value, json};
use turbomcp_auth::{AuthError, BearerValidator, ClientAuth, IntrospectionValidator};

const RESOURCE: &str = "https://mcp.example.com";
const ISSUER: &str = "https://auth.example.com";

/// One introspection request as the endpoint saw it.
#[derive(Clone, Debug)]
struct Seen {
    authorization: Option<String>,
    form: HashMap<String, String>,
}

#[derive(Clone, Default)]
struct Endpoint {
    /// Token → the answer to give for it (or a status to fail with).
    answers: Arc<Mutex<HashMap<String, Result<Value, u16>>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Endpoint {
    fn answer(&self, token: &str, answer: Value) {
        self.answers
            .lock()
            .unwrap()
            .insert(token.into(), Ok(answer));
    }

    fn fail(&self, token: &str, status: u16) {
        self.answers
            .lock()
            .unwrap()
            .insert(token.into(), Err(status));
    }

    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

async fn introspect(
    State(endpoint): State<Endpoint>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    endpoint.seen.lock().unwrap().push(Seen {
        authorization: headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        form: form.clone(),
    });
    let token = form.get("token").cloned().unwrap_or_default();
    match endpoint.answers.lock().unwrap().get(&token).cloned() {
        Some(Ok(answer)) => (StatusCode::OK, Json(answer)),
        Some(Err(status)) => (
            StatusCode::from_u16(status).unwrap(),
            Json(json!({ "error": "server_error" })),
        ),
        None => (StatusCode::OK, Json(json!({ "active": false }))),
    }
}

async fn serve() -> (String, Endpoint) {
    let endpoint = Endpoint::default();
    let app = Router::new()
        .route("/introspect", post(introspect))
        .with_state(endpoint.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/introspect", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, endpoint)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn active(extra: Value) -> Value {
    let mut answer = json!({
        "active": true,
        "sub": "alice",
        "aud": RESOURCE,
        "iss": ISSUER,
        "scope": "files:read files:write",
        "exp": now() + 3600,
    });
    for (k, v) in extra.as_object().unwrap() {
        if v.is_null() {
            answer.as_object_mut().unwrap().remove(k);
        } else {
            answer[k] = v.clone();
        }
    }
    answer
}

fn validator(url: &str) -> IntrospectionValidator {
    IntrospectionValidator::new(url, RESOURCE, ClientAuth::basic("mcp server", "s3cr+t"))
}

#[tokio::test]
async fn an_active_token_for_this_resource_is_accepted_and_cached() {
    let (url, endpoint) = serve().await;
    endpoint.answer("opaque-1", active(json!({})));
    let v = validator(&url).require_issuer(ISSUER);

    let principal = v.validate("opaque-1").await.expect("accepted");
    assert_eq!(principal.subject, "alice");
    assert_eq!(principal.scopes, ["files:read", "files:write"]);
    assert_eq!(principal.claims["iss"], ISSUER);

    // The second request is answered from the cache.
    v.validate("opaque-1").await.expect("accepted again");
    assert_eq!(endpoint.calls(), 1);

    // How it asked: the token and its hint in the form, and Basic auth with
    // each part form-encoded first (RFC 6749 §2.3.1).
    let seen = endpoint.seen.lock().unwrap()[0].clone();
    assert_eq!(seen.form["token"], "opaque-1");
    assert_eq!(seen.form["token_type_hint"], "access_token");
    let expected = base64::engine::general_purpose::STANDARD.encode("mcp+server:s3cr%2Bt");
    assert_eq!(seen.authorization, Some(format!("Basic {expected}")));
}

#[tokio::test]
async fn post_auth_carries_the_credentials_in_the_body() {
    let (url, endpoint) = serve().await;
    endpoint.answer("opaque", active(json!({})));
    IntrospectionValidator::new(&url, RESOURCE, ClientAuth::post("rs", "secret"))
        .validate("opaque")
        .await
        .unwrap();
    let seen = endpoint.seen.lock().unwrap()[0].clone();
    assert_eq!(seen.authorization, None);
    assert_eq!(seen.form["client_id"], "rs");
    assert_eq!(seen.form["client_secret"], "secret");
}

#[tokio::test]
async fn what_is_refused() {
    let (url, endpoint) = serve().await;
    let cases = [
        ("inactive", json!({ "active": false })),
        ("not-boolean", active(json!({ "active": "true" }))),
        (
            "other-audience",
            active(json!({ "aud": "https://other.example.com" })),
        ),
        ("no-audience", active(json!({ "aud": null }))),
        ("expired", active(json!({ "exp": now() - 3600 }))),
        ("not-yet", active(json!({ "nbf": now() + 3600 }))),
        (
            "untrusted-issuer",
            active(json!({ "iss": "https://evil.example.com" })),
        ),
        ("no-issuer", active(json!({ "iss": null }))),
        ("nobody", active(json!({ "sub": null }))),
    ];
    let v = validator(&url).require_issuer(ISSUER);
    for (token, answer) in cases {
        endpoint.answer(token, answer);
        match v.validate(token).await {
            Err(AuthError::InvalidToken(_)) => {}
            other => panic!("{token}: expected InvalidToken, got {other:?}"),
        }
    }
    // An audience among several is fine, and so is a client-credentials
    // token naming only its client.
    endpoint.answer(
        "many",
        active(json!({ "aud": ["https://other.example.com", RESOURCE] })),
    );
    v.validate("many").await.expect("one of its audiences");
    endpoint.answer(
        "machine",
        active(json!({ "sub": null, "client_id": "ci-runner" })),
    );
    assert_eq!(v.validate("machine").await.unwrap().subject, "ci-runner");
}

#[tokio::test]
async fn an_unreachable_or_failing_endpoint_closes() {
    let (url, endpoint) = serve().await;
    endpoint.fail("boom", 500);
    assert!(matches!(
        validator(&url).validate("boom").await,
        Err(AuthError::KeyUnavailable(_))
    ));
    // Nothing listening there.
    let gone = IntrospectionValidator::new(
        "http://127.0.0.1:1/introspect",
        RESOURCE,
        ClientAuth::bearer("rs-token"),
    );
    assert!(matches!(
        gone.validate("any").await,
        Err(AuthError::KeyUnavailable(_))
    ));
}

#[tokio::test]
async fn without_a_cache_every_request_asks() {
    let (url, endpoint) = serve().await;
    endpoint.answer("opaque", active(json!({})));
    let v = validator(&url).cache_ttl(Duration::ZERO);
    v.validate("opaque").await.unwrap();
    v.validate("opaque").await.unwrap();
    assert_eq!(endpoint.calls(), 2);
}

/// A cached answer is never used past the token's own `exp`, whatever the
/// cache's TTL.
#[tokio::test]
async fn a_cached_answer_ends_with_the_token() {
    let (url, endpoint) = serve().await;
    endpoint.answer("short", active(json!({ "exp": now() + 1 })));
    let v = validator(&url)
        .leeway(0)
        .cache_ttl(Duration::from_secs(600));
    v.validate("short").await.expect("valid for a second");
    tokio::time::sleep(Duration::from_millis(2100)).await;
    // Expired now: the cache doesn't answer, and the endpoint's (stale)
    // answer is refused on its `exp`.
    assert!(matches!(
        v.validate("short").await,
        Err(AuthError::InvalidToken(_))
    ));
    assert_eq!(endpoint.calls(), 2);
}
