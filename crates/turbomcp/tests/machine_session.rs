//! Machine-to-machine authorization end to end: a turbomcp server behind a
//! bearer check, a client whose bearer source is a `MachineSession`, and a
//! mock authorization server. The first request is challenged and the
//! session gets a token with the client credentials grant; a tool needing
//! more is answered `403 insufficient_scope`, and the session re-grants
//! with the scopes it had plus the ones named, with no user anywhere.
#![cfg(all(feature = "auth", feature = "http", feature = "client-oauth"))]

use std::collections::HashMap;
use std::future::Future;
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use axum::extract::{Form, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde_json::{Map, Value, json};
use turbomcp::CancellationToken;
use turbomcp::auth::client::{ClientCredentials, MachineAuthorization};
use turbomcp::auth::{
    AuthError, AuthPrincipal, BearerValidator, NetworkPolicy, ResourceMetadata, ResourceServer,
};
use turbomcp::client::oauth::MachineSession;
use turbomcp::client::{BearerSource, ClientBuilder, HttpClientTransport};
use turbomcp::http::{Http, HttpConfig};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Ops;

#[server(name = "ops", version = "1.0.0")]
impl Ops {
    /// Read the status.
    #[tool]
    async fn status(&self) -> String {
        "green".into()
    }

    /// Restart the service.
    #[tool(scopes("ops:admin"))]
    async fn restart(&self) -> String {
        "restarted".into()
    }
}

/// `basic` holds `ops:read`; `admin` holds `ops:read` and `ops:admin`.
struct Tokens;

impl BearerValidator for Tokens {
    fn validate<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<AuthPrincipal, AuthError>> + Send + 'a>> {
        Box::pin(async move {
            let scopes: Vec<String> = match token {
                "basic" => vec!["ops:read".into()],
                "admin" => vec!["ops:read".into(), "ops:admin".into()],
                other => return Err(AuthError::InvalidToken(format!("unknown token {other}"))),
            };
            let mut claims = Map::new();
            claims.insert("scope".into(), Value::String(scopes.join(" ")));
            Ok(AuthPrincipal {
                subject: "deploy-bot".into(),
                scopes,
                claims,
            })
        })
    }
}

type Forms = Arc<Mutex<Vec<HashMap<String, String>>>>;

/// The authorization server: a token per requested scope set.
async fn spawn_authority(forms: Forms) -> String {
    async fn metadata(State((base, _)): State<(String, Forms)>) -> impl IntoResponse {
        axum::Json(json!({
            "issuer": base,
            "token_endpoint": format!("{base}/token"),
            "grant_types_supported": ["client_credentials"],
        }))
    }
    async fn token(
        State((_, forms)): State<(String, Forms)>,
        Form(form): Form<HashMap<String, String>>,
    ) -> impl IntoResponse {
        let scope = form.get("scope").cloned().unwrap_or_default();
        forms.lock().unwrap().push(form);
        let access = if scope.split(' ').any(|s| s == "ops:admin") {
            "admin"
        } else {
            "basic"
        };
        axum::Json(json!({
            "access_token": access,
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": scope,
        }))
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

/// `Ops` behind a bearer check that trusts `authority`: its MCP endpoint.
async fn spawn_ops(authority: &str, shutdown: &CancellationToken) -> String {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let resource = format!("http://{addr}/mcp");
    let authenticator = ResourceServer::new(
        Tokens,
        ResourceMetadata::new(resource.clone(), [authority.to_owned()])
            .scopes_supported(["ops:read"]),
        format!("http://{addr}/.well-known/oauth-protected-resource/mcp"),
    );
    tokio::spawn(
        Ops.into_server().serve(
            Http::listener(listener).config(
                HttpConfig::new()
                    .with_authenticator(Arc::new(authenticator))
                    .with_shutdown(shutdown.clone()),
            ),
        ),
    );
    resource
}

/// The grants the authorization server saw were a client credentials grant
/// for `ops:read`, then one adding `ops:admin` to it.
fn assert_stepped_up(forms: &Forms) {
    let forms = forms.lock().unwrap();
    assert_eq!(forms[0]["grant_type"], "client_credentials");
    assert_eq!(forms[0]["scope"], "ops:read");
    let stepped = forms.last().unwrap();
    let scopes: Vec<&str> = stepped["scope"].split(' ').collect();
    assert!(
        scopes.contains(&"ops:read") && scopes.contains(&"ops:admin"),
        "the step-up keeps what was granted: {scopes:?}"
    );
}

async fn status_then_restart(client: &turbomcp::client::Client) {
    let status = client.call_tool("status", Map::new()).await.unwrap();
    assert_eq!(status.text_content().as_deref(), Some("green"));
    let restart = client.call_tool("restart", Map::new()).await.unwrap();
    assert_eq!(restart.text_content().as_deref(), Some("restarted"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_service_authorizes_itself_and_steps_up() {
    let forms = Forms::default();
    let authority = spawn_authority(Arc::clone(&forms)).await;
    let shutdown = CancellationToken::new();
    let resource = spawn_ops(&authority, &shutdown).await;

    let engine = MachineAuthorization::new(
        resource.clone(),
        ClientCredentials {
            client_id: "deploy-bot".into(),
            client_secret: Some("s3cret".to_owned().into()),
        },
    )
    .with_network_policy(NetworkPolicy::default())
    .unwrap();
    let session = Arc::new(MachineSession::client_credentials(engine));
    let transport = HttpClientTransport::new(resource)
        .unwrap()
        .with_bearer_source(session as Arc<dyn BearerSource>);
    let client = ClientBuilder::new("deploy-bot", "1.0.0")
        .connect(transport)
        .await
        .expect("connected as the service");
    status_then_restart(&client).await;
    assert_stepped_up(&forms);
    shutdown.cancel();
}

/// The gateway calls its upstream as itself: the downstream caller's
/// credentials never reach the upstream, and its own step up there.
#[cfg(feature = "proxy")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_gateway_authorizes_itself_at_its_upstream() {
    use turbomcp::proxy::{OutboundAuth, RemoteServer, ServiceAccount, Upstream};
    let forms = Forms::default();
    let authority = spawn_authority(Arc::clone(&forms)).await;
    let shutdown = CancellationToken::new();
    let resource = spawn_ops(&authority, &shutdown).await;

    let remote = RemoteServer::builder(Upstream::http(resource))
        .auth(OutboundAuth::ClientCredentials(ServiceAccount::new(
            "gateway", "s3cret",
        )))
        .network_policy(NetworkPolicy::default())
        .connect()
        .await
        .expect("the gateway authorized itself");
    let client = turbomcp::testing::connect(remote.into_server(), ClientBuilder::new("agent", "1"))
        .await
        .unwrap();
    status_then_restart(&client).await;
    assert_stepped_up(&forms);
    shutdown.cancel();
}
