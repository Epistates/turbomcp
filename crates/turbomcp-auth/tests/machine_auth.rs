//! Machine-to-machine authorization against a mock MCP authorization server:
//! the client credentials grant and RFC 8693 token exchange, their exact
//! parameters (RFC 8707 `resource` included), and the refusals made before
//! anything is sent.

#![cfg(feature = "oauth-client")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde_json::{Value, json};
use turbomcp_auth::NetworkPolicy;
use turbomcp_auth::client::{ClientCredentials, MachineAuthorization, OAuthClientError};

#[derive(Default)]
struct Mock {
    forms: Vec<(HashMap<String, String>, Option<String>)>,
    /// `grant_types_supported`, when the server advertises it.
    grants: Option<Vec<&'static str>>,
    /// The `issued_token_type` a token exchange answers with.
    issued: Option<&'static str>,
}

type Shared = Arc<Mutex<Mock>>;

async fn spawn(mock: Shared) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(resource_metadata),
        )
        .route("/.well-known/oauth-authorization-server", get(as_metadata))
        .route("/token", post(token))
        .with_state((mock, base.clone()));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

async fn resource_metadata(State((_, base)): State<(Shared, String)>) -> impl IntoResponse {
    axum::Json(json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [base],
        "scopes_supported": ["mcp:tools"],
    }))
}

async fn as_metadata(State((mock, base)): State<(Shared, String)>) -> impl IntoResponse {
    let mut meta = json!({
        "issuer": base,
        "token_endpoint": format!("{base}/token"),
    });
    if let Some(grants) = &mock.lock().unwrap().grants {
        meta["grant_types_supported"] = json!(grants);
    }
    axum::Json(meta)
}

async fn token(
    State((mock, _)): State<(Shared, String)>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut mock = mock.lock().unwrap();
    let exchange = form.contains_key("subject_token");
    mock.forms.push((form, auth));
    let mut answer = json!({
        "access_token": if exchange { "exchanged" } else { "machine" },
        "token_type": "Bearer",
        "expires_in": 600,
    });
    if exchange {
        answer["issued_token_type"] = Value::from(
            mock.issued
                .unwrap_or("urn:ietf:params:oauth:token-type:access_token"),
        );
    }
    axum::Json(answer)
}

fn machine(base: &str, secret: Option<&str>) -> MachineAuthorization {
    MachineAuthorization::new(
        format!("{base}/mcp"),
        ClientCredentials {
            client_id: "gateway".into(),
            client_secret: secret.map(|s| s.to_owned().into()),
        },
    )
    // The mock lives on loopback.
    .with_network_policy(NetworkPolicy::default())
    .unwrap()
}

fn scopes() -> Vec<String> {
    vec!["mcp:tools".into()]
}

#[tokio::test]
async fn client_credentials_get_a_token_bound_to_the_server() {
    let mock: Shared = Arc::default();
    let base = spawn(Arc::clone(&mock)).await;
    let machine = machine(&base, Some("s3cret"));
    let discovered = machine.discover(None).await.unwrap();
    let tokens = machine
        .client_credentials(&discovered, &scopes())
        .await
        .unwrap();
    assert_eq!(tokens.access_token.as_str(), "machine");
    assert_eq!(tokens.scopes, scopes(), "the requested scopes stand in");
    assert!(tokens.expires_at_epoch_secs.is_some());

    let mock = mock.lock().unwrap();
    let (form, auth) = &mock.forms[0];
    assert_eq!(form["grant_type"], "client_credentials");
    assert_eq!(form["resource"], format!("{base}/mcp"));
    assert_eq!(form["scope"], "mcp:tools");
    assert!(!form.contains_key("client_secret"), "Basic is the default");
    assert!(auth.as_deref().is_some_and(|a| a.starts_with("Basic ")));
}

#[tokio::test]
async fn a_token_exchange_presents_the_callers_token_as_the_subject() {
    let mock: Shared = Arc::default();
    let base = spawn(Arc::clone(&mock)).await;
    let machine = machine(&base, Some("s3cret"));
    let discovered = machine.discover(None).await.unwrap();
    let tokens = machine
        .exchange(&discovered, &"callers-token".to_owned().into(), &scopes())
        .await
        .unwrap();
    assert_eq!(tokens.access_token.as_str(), "exchanged");

    let mock = mock.lock().unwrap();
    let (form, _) = &mock.forms[0];
    let access = "urn:ietf:params:oauth:token-type:access_token";
    assert_eq!(
        form["grant_type"],
        "urn:ietf:params:oauth:grant-type:token-exchange"
    );
    assert_eq!(form["subject_token"], "callers-token");
    assert_eq!(form["subject_token_type"], access);
    assert_eq!(form["requested_token_type"], access);
    assert_eq!(form["resource"], format!("{base}/mcp"));
}

#[tokio::test]
async fn an_exchange_that_issues_something_else_is_refused() {
    let mock: Shared = Arc::default();
    mock.lock().unwrap().issued = Some("urn:ietf:params:oauth:token-type:refresh_token");
    let base = spawn(Arc::clone(&mock)).await;
    let machine = machine(&base, Some("s3cret"));
    let discovered = machine.discover(None).await.unwrap();
    let err = machine
        .exchange(&discovered, &"callers-token".to_owned().into(), &scopes())
        .await
        .unwrap_err();
    assert!(matches!(err, OAuthClientError::TokenExchange(_)), "{err}");
}

#[tokio::test]
async fn a_public_client_is_refused_before_anything_is_sent() {
    let mock: Shared = Arc::default();
    let base = spawn(Arc::clone(&mock)).await;
    let machine = machine(&base, None);
    let discovered = machine.discover(None).await.unwrap();
    let err = machine
        .client_credentials(&discovered, &scopes())
        .await
        .unwrap_err();
    assert!(matches!(err, OAuthClientError::Discovery(_)), "{err}");
    assert!(mock.lock().unwrap().forms.is_empty());
}

#[tokio::test]
async fn a_grant_the_server_does_not_offer_is_refused_before_asking() {
    let mock: Shared = Arc::default();
    mock.lock().unwrap().grants = Some(vec!["authorization_code"]);
    let base = spawn(Arc::clone(&mock)).await;
    let machine = machine(&base, Some("s3cret"));
    let discovered = machine.discover(None).await.unwrap();
    for err in [
        machine
            .client_credentials(&discovered, &scopes())
            .await
            .unwrap_err(),
        machine
            .exchange(&discovered, &"t".to_owned().into(), &scopes())
            .await
            .unwrap_err(),
    ] {
        assert!(matches!(err, OAuthClientError::Discovery(_)), "{err}");
    }
    assert!(mock.lock().unwrap().forms.is_empty());
}
