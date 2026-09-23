//! Authentication middleware for WASM MCP servers.
//!
//! Provides a wrapper that adds authentication to any MCP handler.
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::{McpServer, WithAuth};
//! use turbomcp_wasm::auth::{CloudflareAccessAuthenticator};
//!
//! let server = McpServer::builder("my-server", "1.0.0")
//!     .tool_with_ctx("whoami", "Who am I", |ctx: Arc<RequestContext>, _args: Value| async move {
//!         ctx.subject().unwrap_or("anonymous").to_string()
//!     })
//!     .build();
//!
//! // Wrap with Cloudflare Access authentication
//! let auth = CloudflareAccessAuthenticator::new("my-team", "my-aud");
//! let protected_server = WithAuth::new(server, auth)
//!     .with_resource_metadata("https://mcp.example.com/.well-known/oauth-protected-resource");
//!
//! // Handle requests (authentication happens automatically)
//! protected_server.handle(request).await
//! ```

use turbomcp_core::auth::{AuthError, Authenticator, CredentialExtractor, HeaderExtractor};
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::McpError;
use turbomcp_core::handler::McpHandler;
use turbomcp_core::jsonrpc::JsonRpcOutgoing;
use worker::{Request, Response};

use super::endpoint::{self, EndpointConfig, Reply};
use super::server::McpServer;

/// Authentication-enabled MCP handler wrapper.
///
/// Wraps an [`McpHandler`] — an [`McpServer`] by default — with an
/// [`Authenticator`] to require authentication for all requests. The
/// authenticated [`Principal`](turbomcp_core::auth::Principal) is attached to
/// the [`RequestContext`] of the request it authenticated, where handlers read
/// it with `ctx.principal()` / `ctx.subject()`. Nothing is stored on the
/// wrapper itself, so concurrent requests cannot see each other's identity.
///
/// # Example
///
/// ```ignore
/// use turbomcp_wasm::wasm_server::{McpServer, WithAuth};
/// use turbomcp_wasm::auth::CloudflareAccessAuthenticator;
///
/// let server = McpServer::builder("my-server", "1.0.0")
///     .tool("hello", "Say hello", handler)
///     .build();
///
/// let auth = CloudflareAccessAuthenticator::new("my-team", "my-aud");
/// let protected = WithAuth::new(server, auth);
///
/// // In your fetch handler:
/// protected.handle(request).await
/// ```
pub struct WithAuth<A, E = HeaderExtractor, H = McpServer>
where
    A: Authenticator<Error = AuthError> + Clone + 'static,
    E: CredentialExtractor + 'static,
    H: McpHandler,
{
    handler: H,
    authenticator: A,
    extractor: E,
    /// Skip authentication for certain methods
    skip_auth_methods: Vec<String>,
    /// Protected Resource Metadata URL advertised in `WWW-Authenticate`
    resource_metadata: Option<String>,
    /// HTTP-level policy for the endpoint
    endpoint: EndpointConfig,
}

/// Methods allowed without credentials unless configured otherwise.
fn default_skip_auth_methods() -> Vec<String> {
    vec![
        "initialize".to_string(),
        "notifications/initialized".to_string(),
        "ping".to_string(),
    ]
}

impl<A, H> WithAuth<A, HeaderExtractor, H>
where
    A: Authenticator<Error = AuthError> + Clone + 'static,
    H: McpHandler,
{
    /// Create a new authenticated server wrapper.
    ///
    /// Uses the default [`HeaderExtractor`] to extract credentials from
    /// the Authorization header.
    pub fn new(handler: H, authenticator: A) -> Self {
        Self::with_extractor(handler, authenticator, HeaderExtractor)
    }
}

impl<A, E, H> WithAuth<A, E, H>
where
    A: Authenticator<Error = AuthError> + Clone + 'static,
    E: CredentialExtractor + 'static,
    H: McpHandler,
{
    /// Create with a custom credential extractor.
    pub fn with_extractor(handler: H, authenticator: A, extractor: E) -> Self {
        Self {
            handler,
            authenticator,
            extractor,
            skip_auth_methods: default_skip_auth_methods(),
            resource_metadata: None,
            endpoint: EndpointConfig::default(),
        }
    }

    /// Configure methods that don't require authentication.
    ///
    /// By default, `initialize`, `notifications/initialized`, and `ping`
    /// are allowed without authentication.
    pub fn skip_auth_for(mut self, methods: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.skip_auth_methods = methods.into_iter().map(Into::into).collect();
        self
    }

    /// Add a method to the skip list.
    pub fn also_skip_auth_for(mut self, method: impl Into<String>) -> Self {
        self.skip_auth_methods.push(method.into());
        self
    }

    /// Advertise this server's OAuth Protected Resource Metadata (RFC 9728).
    ///
    /// The URL is sent as `resource_metadata` in the `WWW-Authenticate`
    /// challenge of every `401`, which is how an MCP client discovers the
    /// authorization server to obtain a token from. Without it the challenge
    /// is a bare `Bearer`, and a spec-following client has nowhere to go.
    #[must_use]
    pub fn with_resource_metadata(mut self, url: impl Into<String>) -> Self {
        self.resource_metadata = Some(url.into());
        self
    }

    /// Set the HTTP-level policy (allowed origins, body limit).
    #[must_use]
    pub fn with_endpoint_config(mut self, config: EndpointConfig) -> Self {
        self.endpoint = config;
        self
    }

    /// Handle an incoming request with authentication.
    ///
    /// Extracts credentials, validates them, and then dispatches the request
    /// with the authenticated principal on its context. Returns HTTP 401 if
    /// authentication fails.
    ///
    /// When no credential is supplied, the request is allowed through only
    /// if its JSON-RPC method appears in `skip_auth_methods` (default:
    /// `initialize`, `notifications/initialized`, `ping`). Any other method
    /// is rejected with HTTP 401 and a `WWW-Authenticate: Bearer` challenge.
    pub async fn handle(&self, req: Request) -> worker::Result<Response> {
        endpoint::serve(
            &self.handler,
            req,
            &self.endpoint,
            |ctx| ctx,
            |method, ctx| self.admit(method, ctx),
        )
        .await
    }

    /// Authenticate one request, once its JSON-RPC method is known.
    ///
    /// `method` is `None` for a JSON-RPC response from the client, which is
    /// never exempt.
    async fn admit(
        &self,
        method: Option<String>,
        ctx: RequestContext,
    ) -> Result<RequestContext, Reply> {
        let credential = self
            .extractor
            .extract(|name| ctx.header(name).map(str::to_string));

        match credential {
            Some(credential) => match self.authenticator.authenticate(&credential).await {
                Ok(principal) => Ok(ctx.with_principal(principal)),
                Err(error) => Err(self.unauthorized(error.to_string(), true)),
            },
            None if method.is_some_and(|m| self.skip_auth_methods.contains(&m)) => Ok(ctx),
            None => Err(self.unauthorized("Authentication required".to_string(), false)),
        }
    }

    /// The `401` for a missing or rejected credential.
    ///
    /// The challenge follows RFC 6750 §3: `error="invalid_token"` only when a
    /// token was presented and refused, and — per the MCP authorization spec —
    /// `resource_metadata` pointing at the Protected Resource Metadata.
    fn unauthorized(&self, detail: String, token_rejected: bool) -> Reply {
        let mut challenge = String::from("Bearer");
        let mut params = Vec::new();
        if let Some(url) = &self.resource_metadata {
            params.push(format!("resource_metadata=\"{url}\""));
        }
        if token_rejected {
            params.push("error=\"invalid_token\"".to_string());
        }
        if !params.is_empty() {
            challenge.push(' ');
            challenge.push_str(&params.join(", "));
        }

        let body = JsonRpcOutgoing::error(None, McpError::authentication(detail));
        Reply::rpc(401, &body).with_header("WWW-Authenticate", challenge)
    }
}

/// Extension trait for adding authentication to any [`McpHandler`].
pub trait AuthExt: McpHandler {
    /// Wrap this handler with authentication.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use turbomcp_wasm::wasm_server::{McpServer, AuthExt};
    /// use turbomcp_wasm::auth::CloudflareAccessAuthenticator;
    ///
    /// let server = McpServer::builder("my-server", "1.0.0")
    ///     .tool("hello", "Say hello", handler)
    ///     .build()
    ///     .with_auth(CloudflareAccessAuthenticator::new("team", "aud"));
    /// ```
    fn with_auth<A>(self, authenticator: A) -> WithAuth<A, HeaderExtractor, Self>
    where
        A: Authenticator<Error = AuthError> + Clone + 'static;

    /// Wrap this handler with authentication using a custom extractor.
    fn with_auth_extractor<A, E>(self, authenticator: A, extractor: E) -> WithAuth<A, E, Self>
    where
        A: Authenticator<Error = AuthError> + Clone + 'static,
        E: CredentialExtractor + 'static;
}

impl<H: McpHandler> AuthExt for H {
    fn with_auth<A>(self, authenticator: A) -> WithAuth<A, HeaderExtractor, Self>
    where
        A: Authenticator<Error = AuthError> + Clone + 'static,
    {
        WithAuth::new(self, authenticator)
    }

    fn with_auth_extractor<A, E>(self, authenticator: A, extractor: E) -> WithAuth<A, E, Self>
    where
        A: Authenticator<Error = AuthError> + Clone + 'static,
        E: CredentialExtractor + 'static,
    {
        WithAuth::with_extractor(self, authenticator, extractor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use turbomcp_core::auth::{Credential, Principal};
    use turbomcp_core::marker::MaybeSend;

    /// Accepts `Bearer good-<subject>`.
    #[derive(Clone)]
    struct TokenAuth;

    impl Authenticator for TokenAuth {
        type Error = AuthError;

        fn authenticate(
            &self,
            credential: &Credential,
        ) -> impl std::future::Future<Output = Result<Principal, AuthError>> + MaybeSend {
            let result = match credential {
                Credential::Bearer(token) => token
                    .strip_prefix("good-")
                    .map(Principal::new)
                    .ok_or(AuthError::InvalidSignature),
                _ => Err(AuthError::InvalidSignature),
            };
            async move { result }
        }
    }

    fn protected() -> WithAuth<TokenAuth> {
        let server = McpServer::builder("auth", "1.0.0")
            .tool_with_ctx_raw(
                "whoami",
                "Who am I",
                |ctx: Arc<RequestContext>, _args: serde_json::Value| async move {
                    ctx.subject().unwrap_or("anonymous").to_string()
                },
            )
            .build();
        WithAuth::new(server, TokenAuth)
            .with_resource_metadata("https://mcp.example.com/.well-known/oauth-protected-resource")
    }

    async fn post(auth: &WithAuth<TokenAuth>, body: &str, token: Option<&str>) -> Reply {
        let mut headers = endpoint::HeaderMap::new();
        headers.insert("content-type".into(), "application/json".into());
        if let Some(token) = token {
            headers.insert("authorization".into(), format!("Bearer {token}"));
        }
        let ctx = endpoint::request_context(&headers);
        endpoint::answer_post(&auth.handler, body, &headers, ctx, |method, ctx| {
            auth.admit(method, ctx)
        })
        .await
    }

    const WHOAMI: &str =
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"whoami"}}"#;

    #[tokio::test]
    async fn principal_reaches_the_handler_through_the_context() {
        let auth = protected();
        for subject in ["alice", "bob"] {
            let reply = post(&auth, WHOAMI, Some(&format!("good-{subject}"))).await;
            let body: serde_json::Value = serde_json::from_str(&reply.body).unwrap();
            assert_eq!(body["result"]["content"][0]["text"], subject);
        }
    }

    #[tokio::test]
    async fn missing_credential_gets_a_challenge_naming_the_resource_metadata() {
        let reply = post(&protected(), WHOAMI, None).await;
        assert_eq!(reply.status, 401);
        let challenge = reply
            .headers
            .iter()
            .find(|(name, _)| *name == "WWW-Authenticate")
            .map(|(_, value)| value.as_str())
            .unwrap();
        assert_eq!(
            challenge,
            r#"Bearer resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource""#
        );
        let body: serde_json::Value = serde_json::from_str(&reply.body).unwrap();
        assert!(body["id"].is_null());
    }

    #[tokio::test]
    async fn rejected_token_is_flagged_invalid_token() {
        let reply = post(&protected(), WHOAMI, Some("forged")).await;
        assert_eq!(reply.status, 401);
        assert!(
            reply
                .headers
                .iter()
                .any(|(name, value)| *name == "WWW-Authenticate"
                    && value.contains("error=\"invalid_token\""))
        );
    }

    #[tokio::test]
    async fn skip_list_admits_initialize_without_credentials() {
        let reply = post(
            &protected(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
            None,
        )
        .await;
        assert_eq!(reply.status, 200);
    }
}
