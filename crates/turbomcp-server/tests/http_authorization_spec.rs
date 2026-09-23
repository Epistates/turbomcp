//! MCP authorization on the Streamable HTTP transport, over the wire.
//!
//! A server configured with `HttpAuthorization` is an OAuth 2.1 protected
//! resource: it publishes RFC 9728 metadata, refuses requests without a valid
//! bearer token with a challenge that points at that metadata, and binds each
//! session to the principal that created it.

use std::time::Duration;

use reqwest::{Client, StatusCode, header};
use serde_json::json;
use tokio::net::TcpListener;
use turbomcp_core::auth::Principal;
use turbomcp_core::context::RequestContext as CoreRequestContext;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_server::transport::http;
use turbomcp_server::{
    BearerRejection, BearerTokenValidator, HttpAuthorization, McpHandler, ServerConfig,
    ValidationFuture,
};
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
};

/// Answers `whoami` with the authenticated subject.
#[derive(Clone)]
struct WhoAmI;

impl McpHandler for WhoAmI {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("protected", "1.0.0")
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![Tool::new("whoami", "Who is calling")]
    }

    fn list_resources(&self) -> Vec<Resource> {
        Vec::new()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        Vec::new()
    }

    async fn call_tool(
        &self,
        _name: &str,
        _args: serde_json::Value,
        ctx: &CoreRequestContext,
    ) -> McpResult<ToolResult> {
        let subject = ctx
            .principal()
            .map(|principal| principal.subject.clone())
            .unwrap_or_default();
        Ok(ToolResult::text(subject))
    }

    async fn read_resource(
        &self,
        uri: &str,
        _ctx: &CoreRequestContext,
    ) -> McpResult<ResourceResult> {
        Err(McpError::resource_not_found(uri))
    }

    async fn get_prompt(
        &self,
        name: &str,
        _args: Option<serde_json::Value>,
        _ctx: &CoreRequestContext,
    ) -> McpResult<PromptResult> {
        Err(McpError::prompt_not_found(name))
    }
}

/// `alice` and `bob` are valid; `narrow` is valid without the needed scope.
struct Tokens;

impl BearerTokenValidator for Tokens {
    fn validate<'a>(&'a self, token: &'a str) -> ValidationFuture<'a> {
        Box::pin(async move {
            match token {
                "alice" | "bob" => Ok(Principal::new(token)),
                "narrow" => Err(BearerRejection::InsufficientScope {
                    required: vec!["files:write".into()],
                }),
                _ => Err(BearerRejection::InvalidToken("unknown token".into())),
            }
        })
    }
}

async fn spawn_protected() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let base = format!("http://{addr}");

    let config = ServerConfig::builder()
        .authorization(
            HttpAuthorization::new(format!("{base}/mcp"), "https://auth.example.com", Tokens)
                .with_scopes_supported(["files:read", "files:write"]),
        )
        .build();
    let addr = addr.to_string();
    tokio::spawn(async move {
        http::run_with_config(&WhoAmI, &addr, &config)
            .await
            .unwrap();
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    base
}

fn initialize() -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "clientInfo": { "name": "auth-test", "version": "1.0.0" },
            "capabilities": {}
        }
    })
}

async fn post(
    client: &Client,
    base: &str,
    token: Option<&str>,
    session: Option<&str>,
    body: serde_json::Value,
) -> reqwest::Response {
    let mut request = client
        .post(format!("{base}/mcp"))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .json(&body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(session) = session {
        request = request
            .header("mcp-session-id", session)
            .header("mcp-protocol-version", "2025-11-25");
    }
    request.send().await.unwrap()
}

fn challenge(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .expect("a refusal carries a challenge")
        .to_str()
        .unwrap()
        .to_owned()
}

/// RFC 9728, path-inserted for a server at `/mcp`, with the root location
/// served too — the order a 2025-11-25 client tries them in.
#[tokio::test]
async fn protected_resource_metadata_is_published() {
    let base = spawn_protected().await;
    let client = Client::new();

    for path in [
        "/.well-known/oauth-protected-resource/mcp",
        "/.well-known/oauth-protected-resource",
    ] {
        let response = client.get(format!("{base}{path}")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let document: serde_json::Value = response.json().await.unwrap();
        assert_eq!(document["resource"], format!("{base}/mcp"));
        assert_eq!(
            document["authorization_servers"],
            json!(["https://auth.example.com"])
        );
    }
}

/// With no credentials the challenge is bare — RFC 6750 reports no error —
/// but names the metadata, which is how a client discovers where to get a
/// token.
#[tokio::test]
async fn a_request_without_a_token_is_challenged() {
    let base = spawn_protected().await;
    let response = post(&Client::new(), &base, None, None, initialize()).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let challenge = challenge(&response);
    assert!(
        challenge.contains(&format!(
            "resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\""
        )),
        "{challenge}"
    );
    assert!(
        challenge.contains("scope=\"files:read files:write\""),
        "{challenge}"
    );
    assert!(!challenge.contains("error="), "{challenge}");
}

#[tokio::test]
async fn an_invalid_token_is_refused() {
    let base = spawn_protected().await;
    let response = post(&Client::new(), &base, Some("mallory"), None, initialize()).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(challenge(&response).contains("error=\"invalid_token\""));
}

#[tokio::test]
async fn a_token_without_the_scope_is_forbidden() {
    let base = spawn_protected().await;
    let response = post(&Client::new(), &base, Some("narrow"), None, initialize()).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let challenge = challenge(&response);
    assert!(
        challenge.contains("error=\"insufficient_scope\""),
        "{challenge}"
    );
    assert!(challenge.contains("scope=\"files:write\""), "{challenge}");
}

/// The spec forbids access tokens in the URI query string.
#[tokio::test]
async fn a_token_in_the_query_string_is_ignored() {
    let base = spawn_protected().await;
    let response = Client::new()
        .post(format!("{base}/mcp?access_token=alice"))
        .header(header::ACCEPT, "application/json, text/event-stream")
        .json(&initialize())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// The principal reaches the handler; and a session is its creator's alone,
/// so another user holding the id finds nothing there.
#[tokio::test]
async fn a_session_belongs_to_the_principal_that_created_it() {
    let base = spawn_protected().await;
    let client = Client::new();

    let response = post(&client, &base, Some("alice"), None, initialize()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let session = response
        .headers()
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();

    let call = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "whoami", "arguments": {} }
    });
    let response = post(&client, &base, Some("alice"), Some(&session), call.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["result"]["content"][0]["text"], "alice", "{body}");

    let response = post(&client, &base, Some("bob"), Some(&session), call).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
