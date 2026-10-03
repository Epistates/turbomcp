//! Enterprise-Managed Authorization (ID-JAG) end to end against a mock IdP
//! and a mock MCP authorization server: discovery and the ID-JAG profile
//! check, the RFC 8693 token exchange at the IdP (its exact parameters), the
//! RFC 7523 JWT bearer grant at the authorization server, and refusals.

#![cfg(feature = "oauth-client")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde_json::json;
use turbomcp_auth::NetworkPolicy;
use turbomcp_auth::client::{
    ClientCredentials, EnterpriseAuthorization, ID_JAG_PROFILE, IdentityAssertion,
    IdentityProvider, OAuthClientError,
};

#[derive(Default)]
struct Mock {
    idp_forms: Vec<(HashMap<String, String>, Option<String>)>,
    as_forms: Vec<HashMap<String, String>>,
    /// Leave the ID-JAG profile out of the authorization server's metadata.
    no_profile: bool,
    /// The IdP's policy refuses the exchange.
    idp_denies: bool,
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
        .route("/idp/token", post(idp_token))
        .route("/token", post(as_token))
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
        "grant_types_supported": ["urn:ietf:params:oauth:grant-type:jwt-bearer"],
        "token_endpoint_auth_methods_supported": ["client_secret_post"],
    });
    if !mock.lock().unwrap().no_profile {
        meta["authorization_grant_profiles_supported"] = json!([ID_JAG_PROFILE]);
    }
    axum::Json(meta)
}

async fn idp_token(
    State((mock, _)): State<(Shared, String)>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut mock = mock.lock().unwrap();
    mock.idp_forms.push((form, auth));
    if mock.idp_denies {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(
                json!({ "error": "access_denied", "error_description": "not in the MCP group" }),
            ),
        )
            .into_response();
    }
    axum::Json(json!({
        "access_token": "the-id-jag",
        "issued_token_type": "urn:ietf:params:oauth:token-type:id-jag",
        "token_type": "N_A",
        "scope": "mcp:tools",
        "expires_in": 300,
    }))
    .into_response()
}

async fn as_token(
    State((mock, _)): State<(Shared, String)>,
    Form(form): Form<HashMap<String, String>>,
) -> impl IntoResponse {
    mock.lock().unwrap().as_forms.push(form);
    axum::Json(json!({
        "access_token": "mcp-access",
        "token_type": "Bearer",
        "expires_in": 3600,
        "scope": "mcp:tools",
    }))
}

fn flow(base: &str) -> EnterpriseAuthorization {
    EnterpriseAuthorization::new(
        format!("{base}/mcp"),
        IdentityProvider::new(
            format!("{base}/idp/token"),
            ClientCredentials {
                client_id: "sso-client".into(),
                client_secret: Some("sso-secret".to_owned().into()),
            },
        ),
        ClientCredentials {
            client_id: "mcp-client".into(),
            client_secret: Some("mcp-secret".to_owned().into()),
        },
    )
    // The mocks live on loopback.
    .with_network_policy(NetworkPolicy::default())
    .unwrap()
}

#[tokio::test]
async fn an_id_token_becomes_an_access_token_through_the_idp() {
    let mock: Shared = Arc::default();
    let base = spawn(Arc::clone(&mock)).await;
    let tokens = flow(&base)
        .authorize(&IdentityAssertion::id_token("the-id-token"), None)
        .await
        .expect("authorized");
    assert_eq!(tokens.access_token.as_str(), "mcp-access");
    assert_eq!(tokens.scopes, ["mcp:tools"]);
    assert!(tokens.expires_at_epoch_secs.is_some());

    let mock = mock.lock().unwrap();
    // RFC 8693 at the IdP, exactly as the extension words it.
    let (form, auth) = &mock.idp_forms[0];
    assert_eq!(
        form["grant_type"],
        "urn:ietf:params:oauth:grant-type:token-exchange"
    );
    assert_eq!(
        form["requested_token_type"],
        "urn:ietf:params:oauth:token-type:id-jag"
    );
    assert_eq!(form["audience"], base, "the authorization server's issuer");
    assert_eq!(form["resource"], format!("{base}/mcp"));
    assert_eq!(form["subject_token"], "the-id-token");
    assert_eq!(
        form["subject_token_type"],
        "urn:ietf:params:oauth:token-type:id_token"
    );
    assert_eq!(form["scope"], "mcp:tools");
    assert!(
        auth.as_deref().is_some_and(|a| a.starts_with("Basic ")),
        "the SSO client authenticates at the IdP"
    );
    // RFC 7523 at the MCP authorization server, the secret where its
    // metadata asks for it.
    let form = &mock.as_forms[0];
    assert_eq!(
        form["grant_type"],
        "urn:ietf:params:oauth:grant-type:jwt-bearer"
    );
    assert_eq!(form["assertion"], "the-id-jag");
    assert_eq!(form["client_id"], "mcp-client");
    assert_eq!(form["client_secret"], "mcp-secret");
}

#[tokio::test]
async fn a_server_without_the_profile_is_refused_before_anything_is_sent() {
    let mock: Shared = Arc::default();
    mock.lock().unwrap().no_profile = true;
    let base = spawn(Arc::clone(&mock)).await;
    let err = flow(&base)
        .authorize(&IdentityAssertion::id_token("t"), None)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, OAuthClientError::Discovery(m) if m.contains("ID-JAG")),
        "{err}"
    );
    assert!(
        mock.lock().unwrap().idp_forms.is_empty(),
        "the IdP was never asked"
    );
}

#[tokio::test]
async fn the_idps_refusal_is_reported() {
    let mock: Shared = Arc::default();
    mock.lock().unwrap().idp_denies = true;
    let base = spawn(Arc::clone(&mock)).await;
    let err = flow(&base)
        .authorize(&IdentityAssertion::id_token("t"), None)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, OAuthClientError::TokenExchange(m) if m.contains("access_denied") && m.contains("not in the MCP group")),
        "{err}"
    );
    assert!(mock.lock().unwrap().as_forms.is_empty());
}

#[test]
fn assertions_never_print() {
    let assertion = IdentityAssertion::id_token("secret-id-token");
    assert!(!format!("{assertion:?}").contains("secret"));
}
