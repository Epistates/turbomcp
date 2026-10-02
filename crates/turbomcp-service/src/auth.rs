//! The HTTP authentication seam.
//!
//! Auth is **HTTP-transport-level** in MCP: the bearer token rides the
//! `Authorization` header (never `_meta`), and 401/403 + `WWW-Authenticate` +
//! the RFC 9728 Protected Resource Metadata document are HTTP responses. stdio
//! has no auth (the spec says stdio servers retrieve credentials from the
//! environment instead). So this seam lives at the transport boundary, not in
//! the RPC stack: the service sees the validated [`Identity`], never the
//! token.
//!
//! An implementation (e.g. `turbomcp_auth::ResourceServer`) validates the
//! request's `Authorization` header and either authorizes it, yielding the
//! [`Identity`] the transport attaches to the request (and the dispatcher puts
//! in [`RequestContext::identity`](turbomcp_core::RequestContext)), or rejects
//! it with an HTTP challenge. The transport holds it behind an `Arc<dyn …>`, so
//! the trait is dyn-compatible (boxed futures).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use serde_json::Value;
use turbomcp_core::Identity;

/// Boxed future returned by [`HttpAuthenticator::authenticate`] (keeps the
/// trait dyn-compatible).
pub type AuthFuture<'a> = Pin<Box<dyn Future<Output = AuthDecision> + Send + 'a>>;

/// Validates a request's `Authorization` header for the HTTP transport.
pub trait HttpAuthenticator: Send + Sync {
    /// Authenticate one request from its `Authorization` header value (`None`
    /// when the header is absent). JWKS-backed validators may fetch keys, so
    /// this is async.
    fn authenticate<'a>(&'a self, authorization: Option<&'a str>) -> AuthFuture<'a>;

    /// The RFC 9728 Protected Resource Metadata document to serve at
    /// `/.well-known/oauth-protected-resource` (an arbitrary JSON object).
    fn resource_metadata(&self) -> Value;

    /// The `WWW-Authenticate` value for an authenticated request whose
    /// operation needs `scopes` the token lacks: `Bearer
    /// error="insufficient_scope", scope="…", resource_metadata="…"`, which
    /// the transport sends with a `403` ("Runtime Insufficient Scope
    /// Errors"). `None`, the default, answers such a request as usual, with
    /// the error in its body.
    fn insufficient_scope(&self, scopes: &[String]) -> Option<String> {
        let _ = scopes;
        None
    }
}

/// Where a request's handler says which scopes its operation needs and the
/// caller lacks, for the HTTP transport to answer with a step-up challenge
/// ([`HttpAuthenticator::insufficient_scope`]).
///
/// The transport attaches one to each authenticated request's extensions; the
/// dispatcher fills it from an [`McpError::InsufficientScope`](turbomcp_core::McpError)
/// a handler returned. The first demand wins.
#[derive(Clone, Debug, Default)]
pub struct ScopeChallenge(Arc<OnceLock<Vec<String>>>);

impl ScopeChallenge {
    /// The operation needs `scopes`.
    pub fn demand(&self, scopes: &[String]) {
        let _ = self.0.set(scopes.to_vec());
    }

    /// What the operation said it needs, if it said.
    #[must_use]
    pub fn demanded(&self) -> Option<&[String]> {
        self.0.get().map(Vec::as_slice)
    }
}

/// The outcome of authenticating one request.
#[derive(Debug, Clone)]
pub enum AuthDecision {
    /// Authorized, as this identity. The transport attaches it to the request
    /// beside the message, where the client can't forge it.
    Allow(Identity),
    /// Rejected: answer this HTTP status with this `WWW-Authenticate` header
    /// value. 401 for a missing/invalid token, 403 for insufficient scope
    /// (MCP authorization spec §Access).
    Challenge {
        /// HTTP status code (401 or 403).
        status: u16,
        /// The `WWW-Authenticate` response header value.
        www_authenticate: String,
    },
}
