//! Rate limiting wired into the HTTP transport: an over-budget request gets
//! `429` + `Retry-After` before any dispatch, anonymous requests share a bucket
//! (no peer IP under oneshot), and authenticated requests are bucketed
//! per-subject so one principal can't exhaust another's budget.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::json;
use tower::ServiceExt;
use turbomcp_auth::{JwtValidator, ResourceMetadata, ResourceServer, StaticJwks};
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, VersionDispatcher, WithTools,
};
use turbomcp_service::GovernorRateLimiter;
use turbomcp_transport_http::{HttpConfig, router};

const SECRET: &[u8] = b"test-hmac-secret-key-thirty-two!";
const KID: &str = "test-1";
const RESOURCE: &str = "https://mcp.example.com";
const ISSUER: &str = "https://auth.example.com";
const METADATA_URL: &str = "https://mcp.example.com/.well-known/oauth-protected-resource";

/// A trivial server with one no-op tool — enough to exercise the boundary.
#[derive(Clone)]
struct Tiny;

impl McpServerCore for Tiny {
    fn server_info(&self) -> Implementation {
        Implementation::new("tiny", "0.1.0")
    }
}

impl WithTools for Tiny {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

fn nz(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).unwrap()
}

fn dispatcher() -> VersionDispatcher<Tiny> {
    VersionDispatcher::new(Tiny, MethodRouter::new().with_tools())
}

/// An open (no-auth) endpoint with the given limiter.
fn open_app(limiter: GovernorRateLimiter) -> axum::Router {
    router(
        dispatcher(),
        HttpConfig::new().with_rate_limiter(Arc::new(limiter)),
    )
}

/// An authenticated endpoint with the given limiter (per-subject keying).
fn authed_app(limiter: GovernorRateLimiter) -> axum::Router {
    let k = URL_SAFE_NO_PAD.encode(SECRET);
    let jwks = StaticJwks::from_json(
        &json!({ "keys": [ { "kty": "oct", "k": k, "alg": "HS256", "kid": KID } ]}).to_string(),
    )
    .unwrap();
    let validator = JwtValidator::new(jwks, RESOURCE, ISSUER).algorithms(vec![Algorithm::HS256]);
    let metadata = ResourceMetadata::new(RESOURCE, [ISSUER]);
    let rs = ResourceServer::new(validator, metadata, METADATA_URL);
    router(
        dispatcher(),
        HttpConfig::new()
            .with_authenticator(Arc::new(rs))
            .with_rate_limiter(Arc::new(limiter)),
    )
}

fn token(sub: &str) -> String {
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(KID.to_owned());
    let claims = json!({
        "sub": sub, "aud": RESOURCE, "iss": ISSUER, "exp": 4_102_444_800i64,
    });
    encode(&header, &claims, &EncodingKey::from_secret(SECRET)).unwrap()
}

fn call_request(auth: Option<&str>) -> Request<Body> {
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {
            "name": "noop", "arguments": {},
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        }
    });
    let mut req = Request::builder()
        .method("POST")
        .header("accept", "application/json, text/event-stream")
        .uri("/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        // The draft envelope requires the mirrored request-metadata headers.
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/call")
        .header("Mcp-Name", "noop");
    if let Some(auth) = auth {
        req = req.header(header::AUTHORIZATION, auth);
    }
    req.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn anonymous_requests_share_a_bucket_and_429_over_budget() {
    // rate 1/s, burst 2: two pass, the third is over budget. (Oneshot has no
    // peer IP, so anonymous requests all key on RateKey::Global.)
    let app = open_app(GovernorRateLimiter::per_second_burst(nz(1), nz(2)));

    let r1 = app.clone().oneshot(call_request(None)).await.unwrap();
    assert_eq!(r1.status(), StatusCode::OK);
    let r2 = app.clone().oneshot(call_request(None)).await.unwrap();
    assert_eq!(r2.status(), StatusCode::OK);

    let r3 = app.clone().oneshot(call_request(None)).await.unwrap();
    assert_eq!(r3.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry = r3.headers()[header::RETRY_AFTER].to_str().unwrap();
    assert!(retry.parse::<u64>().unwrap() >= 1);
    // The 429 short-circuits before dispatch: a JSON-RPC error body, no result.
    let bytes = r3.into_body().collect().await.unwrap().to_bytes();
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(doc["error"].is_object());
}

#[tokio::test]
async fn authenticated_requests_are_bucketed_per_subject() {
    // burst 1: each subject gets exactly one before refill.
    let app = authed_app(GovernorRateLimiter::per_second_burst(nz(1), nz(1)));
    let alice = format!("Bearer {}", token("alice"));
    let bob = format!("Bearer {}", token("bob"));

    // alice spends her single token.
    let a1 = app
        .clone()
        .oneshot(call_request(Some(&alice)))
        .await
        .unwrap();
    assert_eq!(a1.status(), StatusCode::OK);
    let a2 = app
        .clone()
        .oneshot(call_request(Some(&alice)))
        .await
        .unwrap();
    assert_eq!(a2.status(), StatusCode::TOO_MANY_REQUESTS);

    // bob has his own bucket — alice exhausting hers doesn't touch him.
    let b1 = app.clone().oneshot(call_request(Some(&bob))).await.unwrap();
    assert_eq!(b1.status(), StatusCode::OK);
}

#[tokio::test]
async fn unauthenticated_request_is_challenged_before_rate_limit() {
    // With auth configured, a tokenless request is 401 (auth runs first); it is
    // not consumed from any rate bucket.
    let app = authed_app(GovernorRateLimiter::per_second_burst(nz(1), nz(1)));
    let resp = app.oneshot(call_request(None)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
