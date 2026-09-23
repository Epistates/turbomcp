//! DPoP auto-attach integration: when a `DpopBinding` is wired to
//! `OAuth2Client`, every token-endpoint request must carry a fresh `DPoP`
//! header per RFC 9449. Also covers the `use_dpop_nonce` retry of §8.

#![cfg(feature = "dpop")]

mod common;

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
use turbomcp_auth::oauth2::{DpopBinding, OAuth2Client};
use turbomcp_auth::{OAuth2Config, OAuth2FlowType, ProviderType};
use turbomcp_dpop::DpopProofGenerator;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use common::MockOAuth2Server;

fn config_for(token_url: &str, auth_url: &str) -> OAuth2Config {
    OAuth2Config {
        client_id: "client-id".to_string(),
        client_secret: secrecy::SecretString::new("secret".to_string().into()),
        auth_url: auth_url.to_string(),
        token_url: token_url.to_string(),
        revocation_url: None,
        redirect_uri: "http://127.0.0.1:8080/cb".to_string(),
        scopes: vec!["read".to_string()],
        flow_type: OAuth2FlowType::AuthorizationCode,
        additional_params: Default::default(),
        security_level: turbomcp_auth::SecurityLevel::Enhanced,
        dpop_config: None,
        mcp_resource_uri: None,
        auto_resource_indicators: false,
        allow_custom_scheme_redirect: false,
    }
}

fn decode_jwt_payload(jwt: &str) -> Value {
    let payload_b64 = jwt.split('.').nth(1).expect("payload segment");
    let bytes = URL_SAFE_NO_PAD.decode(payload_b64).expect("base64");
    serde_json::from_slice(&bytes).expect("payload json")
}

#[tokio::test]
async fn token_request_carries_dpop_header_when_binding_configured() {
    let mock = MockOAuth2Server::start().await;
    mock.mock_token_with_dpop("at-123", None).await;

    let cfg = config_for(&mock.token_endpoint, &mock.authorize_endpoint);
    let generator = DpopProofGenerator::new_simple()
        .await
        .expect("generator init");
    let binding = DpopBinding::new(Arc::new(generator));

    let client = OAuth2Client::new(&cfg, ProviderType::Generic)
        .expect("client")
        .with_dpop_binding(binding);

    let token = client
        .exchange_code_for_token(
            "auth-code".to_string(),
            "verifier-1234567890123456789012345".to_string(),
        )
        .await
        .expect("token exchange");
    assert_eq!(token.access_token, "at-123");

    // Inspect the recorded request to confirm a DPoP proof was attached and
    // that its `htm`/`htu` claims match the token endpoint call.
    let received = mock.server.received_requests().await.expect("requests");
    let token_req = received
        .into_iter()
        .find(|r| r.url.path() == "/token")
        .expect("/token request");
    let dpop_header = token_req
        .headers
        .get("dpop")
        .or_else(|| token_req.headers.get("DPoP"))
        .expect("DPoP header present");
    let jwt = dpop_header.to_str().expect("ascii");
    let payload = decode_jwt_payload(jwt);
    assert_eq!(payload["htm"].as_str(), Some("POST"));
    assert!(
        payload["htu"]
            .as_str()
            .unwrap_or_default()
            .ends_with("/token"),
        "htu should reference token endpoint, got: {payload}"
    );
    // Token-endpoint proofs MUST NOT carry an `ath` claim — there is no
    // access token at this point.
    assert!(
        payload.get("ath").map(|v| v.is_null()).unwrap_or(true),
        "token-endpoint proof should not carry `ath`, got: {payload}"
    );
}

#[tokio::test]
async fn use_dpop_nonce_challenge_triggers_retry_with_nonce() {
    // First response: 400 use_dpop_nonce + DPoP-Nonce header.
    // Second response: 200 with token.
    // wiremock's `up_to_n_times` lets us stage two responses on the same path.
    let server = MockServer::start().await;
    let nonce = "nonce-from-as-xyz";
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(400)
                .insert_header("DPoP-Nonce", nonce)
                .set_body_json(serde_json::json!({
                    "error": "use_dpop_nonce",
                    "error_description": "use the supplied nonce",
                })),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "at-after-retry",
            "token_type": "DPoP",
            "expires_in": 3600,
        })))
        .mount(&server)
        .await;

    let token_url = format!("{}/token", server.uri());
    let auth_url = format!("{}/authorize", server.uri());
    let cfg = config_for(&token_url, &auth_url);
    let generator = DpopProofGenerator::new_simple()
        .await
        .expect("generator init");
    let binding = DpopBinding::new(Arc::new(generator));

    let client = OAuth2Client::new(&cfg, ProviderType::Generic)
        .expect("client")
        .with_dpop_binding(binding);

    let token = client
        .exchange_code_for_token(
            "auth-code".to_string(),
            "verifier-1234567890123456789012345".to_string(),
        )
        .await
        .expect("token exchange after nonce retry");
    assert_eq!(token.access_token, "at-after-retry");

    // Both attempts should have carried a DPoP header; the second must include
    // the server-provided nonce in its payload.
    let mut received = server.received_requests().await.expect("requests");
    received.retain(|r: &Request| r.url.path() == "/token");
    assert_eq!(received.len(), 2, "expected two POSTs to /token");

    let second_jwt = received[1]
        .headers
        .get("dpop")
        .or_else(|| received[1].headers.get("DPoP"))
        .expect("retry DPoP header")
        .to_str()
        .expect("ascii");
    let second_payload = decode_jwt_payload(second_jwt);
    assert_eq!(
        second_payload["nonce"].as_str(),
        Some(nonce),
        "retry proof must carry server nonce, got: {second_payload}"
    );
}
