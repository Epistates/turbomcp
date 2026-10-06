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
use zeroize::Zeroizing;

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

    /// Whether the transport keeps an authorized request's bearer token
    /// beside it, as a [`SubjectToken`], for a gateway to exchange (RFC 8693)
    /// for a token to an upstream on the caller's behalf. Default `false`:
    /// the service sees the identity, never the token.
    fn retains_token(&self) -> bool {
        false
    }
}

/// The bearer token an authorized request arrived with, kept beside it when
/// its authenticator [retains tokens](HttpAuthenticator::retains_token).
///
/// It is there to be *exchanged*: a gateway presents it to an authorization
/// server as the `subject_token` of an RFC 8693 token exchange and gets back
/// a token issued for its upstream. It is not to be forwarded as is ("MUST
/// NOT pass through the token it received from the MCP client"). Any handler
/// can read a request's facts, so retain tokens only on a server whose
/// handlers are trusted with them. Wiped from memory when the last copy
/// drops; `Debug` never renders it.
#[derive(Clone)]
pub struct SubjectToken(Arc<Zeroizing<String>>);

impl SubjectToken {
    /// `token`, as presented.
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self(Arc::new(Zeroizing::new(token.into())))
    }

    /// The token.
    #[must_use]
    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Debug for SubjectToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SubjectToken(<redacted>)")
    }
}

/// The token of an `Authorization: Bearer …` header value (the scheme is
/// case-insensitive, RFC 7235 §2.1), for retention.
#[must_use]
pub fn bearer_of(authorization: Option<&str>) -> Option<SubjectToken> {
    let value = authorization?.trim();
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then(|| SubjectToken::new(token))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bearer_credential_is_retained() {
        let token = |header| bearer_of(Some(header)).map(|t| t.secret().to_owned());
        assert_eq!(token("Bearer abc").as_deref(), Some("abc"));
        assert_eq!(token("bearer  abc ").as_deref(), Some("abc"));
        assert_eq!(token("Basic abc"), None);
        assert_eq!(token("Bearer "), None);
        assert_eq!(token("Bearer"), None);
        assert_eq!(bearer_of(None).map(|t| t.secret().to_owned()), None);
        assert_eq!(
            format!("{:?}", SubjectToken::new("abc")),
            "SubjectToken(<redacted>)"
        );
    }
}
