//! Enterprise-Managed Authorization end to end: a turbomcp server behind a
//! bearer check, a client whose bearer source is an `EnterpriseSession`, and
//! a mock IdP + authorization server between them. The client's first
//! request is challenged, the session trades the user's ID token for an
//! ID-JAG and the ID-JAG for an access token, and the retried call goes
//! through, with no browser anywhere.
#![cfg(all(feature = "auth", feature = "http", feature = "client-oauth"))]

use std::collections::HashMap;
use std::future::Future;
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{Form, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde_json::{Map, Value, json};
use turbomcp::CancellationToken;
use turbomcp::auth::client::{
    ClientCredentials, EnterpriseAuthorization, ID_JAG_PROFILE, IdentityAssertion, IdentityProvider,
};
use turbomcp::auth::{
    AuthError, AuthPrincipal, BearerValidator, NetworkPolicy, ResourceMetadata, ResourceServer,
};
use turbomcp::client::oauth::{AssertionSource, EnterpriseSession};
use turbomcp::client::{
    BearerSource, ClientBuilder, ConnectMode, HttpClientTransport, async_trait,
};
use turbomcp::http::{Http, HttpConfig};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Docs;

#[server(name = "docs", version = "1.0.0")]
impl Docs {
    /// Search the docs.
    #[tool]
    async fn search(&self, query: String) -> String {
        format!("results for {query}")
    }
}

/// The MCP server accepts only the token the authorization server issues.
struct OnlyMcpAccess;

impl BearerValidator for OnlyMcpAccess {
    fn validate<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<AuthPrincipal, AuthError>> + Send + 'a>> {
        Box::pin(async move {
            if token != "mcp-access" {
                return Err(AuthError::InvalidToken("unknown token".into()));
            }
            Ok(AuthPrincipal {
                subject: "alice@corp.example".into(),
                scopes: vec!["mcp:tools".into()],
                claims: Map::new(),
            })
        })
    }
}

/// The IdP's token exchange and the authorization server's JWT bearer grant.
async fn spawn_idp_and_as() -> String {
    async fn as_metadata(State(base): State<String>) -> impl IntoResponse {
        axum::Json(json!({
            "issuer": base,
            "token_endpoint": format!("{base}/token"),
            "authorization_grant_profiles_supported": [ID_JAG_PROFILE],
        }))
    }
    async fn idp_token(Form(form): Form<HashMap<String, String>>) -> impl IntoResponse {
        assert_eq!(form["subject_token"], "alice-id-token");
        axum::Json(json!({
            "access_token": "id-jag-for-alice",
            "issued_token_type": "urn:ietf:params:oauth:token-type:id-jag",
            "token_type": "N_A",
        }))
    }
    async fn as_token(Form(form): Form<HashMap<String, String>>) -> impl IntoResponse {
        assert_eq!(form["assertion"], "id-jag-for-alice");
        axum::Json(
            json!({ "access_token": "mcp-access", "token_type": "Bearer", "expires_in": 3600 }),
        )
    }
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route("/.well-known/oauth-authorization-server", get(as_metadata))
        .route("/idp/token", post(idp_token))
        .route("/token", post(as_token))
        .with_state(base.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

/// Alice is signed in.
struct SignedIn;

#[async_trait]
impl AssertionSource for SignedIn {
    async fn identity_assertion(&self) -> Option<IdentityAssertion> {
        Some(IdentityAssertion::id_token("alice-id-token"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_idp_authorizes_the_client_without_a_browser() {
    let authority = spawn_idp_and_as().await;

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let resource = format!("http://{addr}/mcp");
    let authenticator = ResourceServer::new(
        OnlyMcpAccess,
        ResourceMetadata::new(resource.clone(), [authority.clone()]),
        format!("http://{addr}/.well-known/oauth-protected-resource/mcp"),
    );
    let shutdown = CancellationToken::new();
    tokio::spawn(
        Docs.into_server().serve(
            Http::listener(listener).config(
                HttpConfig::new()
                    .with_authenticator(Arc::new(authenticator))
                    .with_shutdown(shutdown.clone()),
            ),
        ),
    );

    let engine = EnterpriseAuthorization::new(
        resource.clone(),
        IdentityProvider::new(
            format!("{authority}/idp/token"),
            ClientCredentials::public("sso-client"),
        ),
        ClientCredentials::public("mcp-client"),
    )
    .with_network_policy(NetworkPolicy::default())
    .unwrap();
    let session = Arc::new(EnterpriseSession::new(engine, Arc::new(SignedIn)));
    let transport = HttpClientTransport::new(resource)
        .unwrap()
        .with_bearer_source(session as Arc<dyn BearerSource>);
    let client = ClientBuilder::new("corp-agent", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_extension(
            turbomcp::auth::client::enterprise::EXTENSION_ID,
            Value::Object(Map::new()),
        )
        .connect(transport)
        .await
        .expect("connected through the IdP");

    let mut args = Map::new();
    args.insert("query".into(), json!("ID-JAG"));
    let result = client
        .call_tool("search", args)
        .await
        .expect("authorized call");
    assert_eq!(result.text_content().as_deref(), Some("results for ID-JAG"));
    shutdown.cancel();
}
