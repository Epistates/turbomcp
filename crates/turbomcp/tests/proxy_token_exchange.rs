//! A gateway acting for its callers by token exchange (RFC 8693): each
//! caller's token, retained by the gateway's authenticator, is exchanged at
//! the upstream's authorization server for one issued for the upstream, so
//! the upstream sees who is calling and never sees the caller's own token.
#![cfg(all(
    feature = "auth",
    feature = "http",
    feature = "client-oauth",
    feature = "proxy"
))]

use std::collections::HashMap;
use std::future::Future;
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde_json::{Map, json};
use turbomcp::CancellationToken;
use turbomcp::auth::{
    AuthError, AuthPrincipal, BearerValidator, NetworkPolicy, ResourceMetadata, ResourceServer,
};
use turbomcp::client::{Client, ClientBuilder, HttpClientTransport};
use turbomcp::http::{Http, HttpConfig};
use turbomcp::prelude::*;
use turbomcp::proxy::{OutboundAuth, RemoteServer, ServiceAccount, Upstream};

#[derive(Clone)]
struct Upstairs;

#[server(name = "upstairs", version = "1.0.0")]
impl Upstairs {
    /// Who the upstream thinks is calling.
    #[tool]
    async fn whoami(&self, ctx: &CallToolContext) -> String {
        ctx.base.identity.subject().unwrap_or("nobody").to_owned()
    }
}

/// Accepts the tokens in its table, recording every token it is shown.
struct Table {
    tokens: &'static [(&'static str, &'static str)],
    seen: Arc<Mutex<Vec<String>>>,
}

impl BearerValidator for Table {
    fn validate<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<AuthPrincipal, AuthError>> + Send + 'a>> {
        Box::pin(async move {
            self.seen.lock().unwrap().push(token.to_owned());
            let subject = self
                .tokens
                .iter()
                .find(|(t, _)| *t == token)
                .map(|(_, s)| *s)
                .ok_or_else(|| AuthError::InvalidToken("unknown token".into()))?;
            Ok(AuthPrincipal {
                subject: subject.into(),
                scopes: vec![],
                claims: Map::new(),
            })
        })
    }
}

type Forms = Arc<Mutex<Vec<HashMap<String, String>>>>;

/// The upstream's authorization server: the gateway's own token for client
/// credentials, and a per-user token for each exchange.
async fn spawn_authority(forms: Forms) -> String {
    async fn metadata(State((base, _)): State<(String, Forms)>) -> impl IntoResponse {
        axum::Json(json!({
            "issuer": base,
            "token_endpoint": format!("{base}/token"),
            "grant_types_supported": [
                "client_credentials",
                "urn:ietf:params:oauth:grant-type:token-exchange",
            ],
        }))
    }
    async fn token(
        State((_, forms)): State<(String, Forms)>,
        Form(form): Form<HashMap<String, String>>,
    ) -> impl IntoResponse {
        forms.lock().unwrap().push(form.clone());
        let (token, typed) = match form.get("subject_token").map(String::as_str) {
            None => ("gateway-own", false),
            Some("alice-token") => ("upstream-alice", true),
            Some("bob-token") => ("upstream-bob", true),
            Some(_) => return axum::http::StatusCode::BAD_REQUEST.into_response(),
        };
        let mut answer =
            json!({ "access_token": token, "token_type": "Bearer", "expires_in": 3600 });
        if typed {
            answer["issued_token_type"] = json!("urn:ietf:params:oauth:token-type:access_token");
        }
        axum::Json(answer).into_response()
    }
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route("/.well-known/oauth-authorization-server", get(metadata))
        .route("/token", post(token))
        .with_state((base.clone(), forms));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

/// An HTTP endpoint on loopback serving `server` behind `validator`.
async fn serve_protected<S>(
    server: turbomcp::ServerBuilder<S>,
    validator: Table,
    authority: &str,
    retain: bool,
    shutdown: &CancellationToken,
) -> String
where
    S: turbomcp::McpServerCore + Send + Sync + 'static,
{
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let resource = format!("http://{addr}/mcp");
    let authenticator = ResourceServer::new(
        validator,
        ResourceMetadata::new(resource.clone(), [authority.to_owned()]),
        format!("http://{addr}/.well-known/oauth-protected-resource/mcp"),
    )
    .retain_token(retain);
    tokio::spawn(
        server.serve(
            Http::listener(listener).config(
                HttpConfig::new()
                    .with_authenticator(Arc::new(authenticator))
                    .with_shutdown(shutdown.clone()),
            ),
        ),
    );
    resource
}

async fn as_caller(gateway: &str, token: &str) -> Client {
    ClientBuilder::new("agent", "1.0.0")
        .connect(
            HttpClientTransport::new(gateway)
                .unwrap()
                .with_bearer(token),
        )
        .await
        .unwrap()
}

/// A gateway over `Upstairs` that exchanges its callers' tokens, retaining
/// them (or not) at its own authenticator; its endpoint, and what the
/// upstream and the authorization server saw.
async fn gateway(
    retain: bool,
    shutdown: &CancellationToken,
) -> (String, Arc<Mutex<Vec<String>>>, Forms) {
    let forms = Forms::default();
    let authority = spawn_authority(Arc::clone(&forms)).await;
    let upstream_seen = Arc::new(Mutex::new(Vec::new()));
    let upstream = serve_protected(
        Upstairs.into_server(),
        Table {
            tokens: &[
                ("gateway-own", "gateway"),
                ("upstream-alice", "alice"),
                ("upstream-bob", "bob"),
            ],
            seen: Arc::clone(&upstream_seen),
        },
        &authority,
        false,
        shutdown,
    )
    .await;
    let remote = RemoteServer::builder(Upstream::http(upstream))
        .auth(OutboundAuth::TokenExchange(ServiceAccount::new(
            "gateway", "s3cret",
        )))
        .network_policy(NetworkPolicy::default())
        .connect()
        .await
        .expect("the gateway learned what the upstream serves");
    let gateway = serve_protected(
        remote.into_server(),
        Table {
            tokens: &[("alice-token", "alice"), ("bob-token", "bob")],
            seen: Arc::default(),
        },
        // The gateway's callers authenticate elsewhere; any issuer will do.
        &authority,
        retain,
        shutdown,
    )
    .await;
    (gateway, upstream_seen, forms)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_caller_reaches_the_upstream_as_themselves() {
    let shutdown = CancellationToken::new();
    let (gateway, upstream_seen, forms) = gateway(true, &shutdown).await;

    let alice = as_caller(&gateway, "alice-token").await;
    let bob = as_caller(&gateway, "bob-token").await;
    let whoami = |client: Client| async move {
        client
            .call_tool("whoami", Map::new())
            .await
            .unwrap()
            .text_content()
            .unwrap()
    };
    assert_eq!(whoami(alice).await, "alice");
    assert_eq!(whoami(bob).await, "bob");

    // The callers' own tokens went to the authorization server, as subject
    // tokens, and never to the upstream.
    let subjects: Vec<String> = forms
        .lock()
        .unwrap()
        .iter()
        .filter_map(|f| f.get("subject_token").cloned())
        .collect();
    assert!(subjects.contains(&"alice-token".to_owned()), "{subjects:?}");
    assert!(subjects.contains(&"bob-token".to_owned()), "{subjects:?}");
    let seen = upstream_seen.lock().unwrap();
    assert!(
        !seen.iter().any(|t| t.ends_with("-token")),
        "a caller's token reached the upstream: {seen:?}"
    );
    shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_retained_token_the_call_fails_and_says_why() {
    let shutdown = CancellationToken::new();
    let (gateway, upstream_seen, _) = gateway(false, &shutdown).await;
    let alice = as_caller(&gateway, "alice-token").await;
    let failed = alice.call_tool("whoami", Map::new()).await;
    let message = match failed {
        Ok(result) => {
            assert!(result.is_error, "{result:?}");
            result.text_content().unwrap_or_default()
        }
        Err(e) => e.to_string(),
    };
    assert!(message.contains("no token"), "{message}");
    // Only the gateway's own startup connection reached the upstream.
    assert!(
        upstream_seen
            .lock()
            .unwrap()
            .iter()
            .all(|t| t == "gateway-own"),
        "{:?}",
        upstream_seen.lock().unwrap()
    );
    shutdown.cancel();
}
