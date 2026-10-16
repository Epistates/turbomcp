//! Per-tool step-up authorization end to end ("Runtime Insufficient Scope
//! Errors" → "Step-Up Authorization Flow"): a `#[tool(scopes(…))]` called
//! with a token lacking its scopes answers `403` with `WWW-Authenticate:
//! Bearer error="insufficient_scope", scope="…"`, the client's credential
//! source steps up, and the retried call goes through.
#![cfg(all(feature = "auth", feature = "http", feature = "client"))]

use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};
use turbomcp::CancellationToken;
use turbomcp::auth::{AuthError, AuthPrincipal, BearerValidator, ResourceMetadata, ResourceServer};
use turbomcp::client::{
    BearerSource, ClientBuilder, ConnectMode, HttpClientTransport, async_trait,
};
use turbomcp::http::{Http, HttpConfig};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Files;

#[server(name = "files", version = "1.0.0")]
impl Files {
    /// Write a file.
    #[tool(scopes("files:read", "files:write"))]
    async fn write_file(&self, path: String) -> String {
        format!("wrote {path}")
    }
}

/// Two tokens: `read` holds `files:read`, `write` both scopes.
struct Tokens;

impl BearerValidator for Tokens {
    fn validate<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<AuthPrincipal, AuthError>> + Send + 'a>> {
        Box::pin(async move {
            let scopes: Vec<String> = match token {
                "files" => vec!["files".into()],
                "read" => vec!["files:read".into()],
                "write" => vec!["files:read".into(), "files:write".into()],
                other => return Err(AuthError::InvalidToken(format!("unknown token {other}"))),
            };
            let mut claims = Map::new();
            claims.insert("scope".into(), Value::String(scopes.join(" ")));
            Ok(AuthPrincipal {
                subject: "alice".into(),
                scopes,
                claims,
            })
        })
    }
}

/// Starts with the read-only token; on a challenge, records it and switches
/// to the broader one, as an OAuth session would after re-authorizing.
struct StepUp {
    token: Mutex<String>,
    challenges: Mutex<Vec<(u16, String)>>,
}

#[async_trait]
impl BearerSource for StepUp {
    async fn bearer(&self) -> Option<turbomcp::client::Zeroizing<String>> {
        Some(self.token.lock().unwrap().clone().into())
    }

    async fn on_challenge(
        &self,
        status: u16,
        header: Option<&str>,
        _rejected: Option<&str>,
    ) -> Result<bool, String> {
        self.challenges
            .lock()
            .unwrap()
            .push((status, header.unwrap_or_default().to_owned()));
        *self.token.lock().unwrap() = "write".into();
        Ok(true)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scoped_tool_steps_the_client_up() {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let resource = format!("http://{addr}/mcp");
    let metadata_url = format!("http://{addr}/.well-known/oauth-protected-resource/mcp");
    let authenticator = ResourceServer::new(
        Tokens,
        ResourceMetadata::new(resource.clone(), ["https://auth.example.com"]),
        metadata_url.clone(),
    );
    let shutdown = CancellationToken::new();
    tokio::spawn(
        Files.into_server().serve(
            Http::listener(listener).config(
                HttpConfig::new()
                    .with_authenticator(Arc::new(authenticator))
                    .with_shutdown(shutdown.clone()),
            ),
        ),
    );
    tokio::time::sleep(Duration::from_millis(100)).await;

    let credentials = Arc::new(StepUp {
        token: Mutex::new("read".into()),
        challenges: Mutex::new(Vec::new()),
    });
    let transport = HttpClientTransport::new(resource)
        .unwrap()
        .with_bearer_source(Arc::clone(&credentials) as Arc<dyn BearerSource>);
    let client = ClientBuilder::new("stepper", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport)
        .await
        .expect("connect with the read-only token");

    let mut args = Map::new();
    args.insert("path".into(), json!("/notes.txt"));
    let result = client
        .call_tool("write_file", args)
        .await
        .expect("the retried call goes through");
    assert!(!result.is_error, "{result:?}");
    match &result.content[0] {
        neutral::Content::Text { text, .. } => assert_eq!(text, "wrote /notes.txt"),
        other => panic!("unexpected content {other:?}"),
    }

    let challenges = credentials.challenges.lock().unwrap().clone();
    assert_eq!(challenges.len(), 1, "{challenges:?}");
    let (status, header) = &challenges[0];
    assert_eq!(*status, 403);
    assert!(header.contains("error=\"insufficient_scope\""), "{header}");
    assert!(
        header.contains("scope=\"files:read files:write\""),
        "{header}"
    );
    assert!(
        header.contains(&format!("resource_metadata=\"{metadata_url}\"")),
        "{header}"
    );
    shutdown.cancel();
}

/// Serve `Files` behind a resource server with an optional scope hierarchy,
/// and call `write_file` with a token holding only `files`; returns whether
/// the call went through without a challenge.
async fn call_with_the_broad_scope(hierarchy: bool) -> (bool, usize) {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let resource = format!("http://{addr}/mcp");
    let mut authenticator = ResourceServer::new(
        Tokens,
        ResourceMetadata::new(resource.clone(), ["https://auth.example.com"]),
        format!("http://{addr}/.well-known/oauth-protected-resource/mcp"),
    );
    if hierarchy {
        authenticator = authenticator.scope_hierarchy(turbomcp::scopes::PrefixHierarchy::default());
    }
    let shutdown = CancellationToken::new();
    tokio::spawn(
        Files.into_server().serve(
            Http::listener(listener).config(
                HttpConfig::new()
                    .with_authenticator(Arc::new(authenticator))
                    .with_shutdown(shutdown.clone()),
            ),
        ),
    );
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Holds `files` and never steps up: a challenge is the end of it.
    struct Broad(Mutex<usize>);
    #[async_trait]
    impl BearerSource for Broad {
        async fn bearer(&self) -> Option<turbomcp::client::Zeroizing<String>> {
            Some("files".to_owned().into())
        }
        async fn on_challenge(
            &self,
            _status: u16,
            _header: Option<&str>,
            _rejected: Option<&str>,
        ) -> Result<bool, String> {
            *self.0.lock().unwrap() += 1;
            Ok(false)
        }
    }
    let credentials = Arc::new(Broad(Mutex::new(0)));
    let transport = HttpClientTransport::new(resource)
        .unwrap()
        .with_bearer_source(Arc::clone(&credentials) as Arc<dyn BearerSource>);
    let client = ClientBuilder::new("broad", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport)
        .await
        .expect("connect");
    let mut args = Map::new();
    args.insert("path".into(), json!("/notes.txt"));
    let went_through = client
        .call_tool("write_file", args)
        .await
        .is_ok_and(|result| !result.is_error);
    let challenges = *credentials.0.lock().unwrap();
    shutdown.cancel();
    (went_through, challenges)
}

/// "Servers MUST account for scope hierarchies, where a broader scope implies
/// narrower ones": under a prefix hierarchy, `files` covers the tool's
/// `files:read` and `files:write`, so the call needs no step-up the
/// authorization server could never grant.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_broader_scope_covers_the_narrower_ones_the_tool_declares() {
    assert_eq!(call_with_the_broad_scope(true).await, (true, 0));
    // Without one, scopes match exactly and the call is challenged.
    let (went_through, challenges) = call_with_the_broad_scope(false).await;
    assert!(!went_through);
    assert_eq!(challenges, 1);
}
