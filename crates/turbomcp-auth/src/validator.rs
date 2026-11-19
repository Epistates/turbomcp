//! Bearer-token validation.
//!
//! [`BearerValidator`] is the pluggable seam: JWTs (`JwtValidator`, feature
//! `jwt`), RFC 7662 introspection (`IntrospectionValidator`), or anything
//! else that turns a token into an [`AuthPrincipal`].

use futures::future::BoxFuture;
use serde_json::{Map, Value};

use crate::error::AuthError;

/// A validated bearer principal: who the caller is, plus their scopes and the
/// full claim set. Serialized into the request's identity by the transport.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuthPrincipal {
    /// The `sub` claim.
    pub subject: String,
    /// Granted scopes (parsed from the `scope` string or `scp` array claim).
    pub scopes: Vec<String>,
    /// The full validated claim set.
    pub claims: Map<String, Value>,
}

impl AuthPrincipal {
    /// The principal `subject` with the validated `claims`. Its scopes are
    /// read from the claims' `scope` string or `scp` array, the same way the
    /// bundled validators and the per-tool scope check read them.
    #[must_use]
    pub fn new(subject: impl Into<String>, claims: Map<String, Value>) -> Self {
        Self {
            subject: subject.into(),
            scopes: turbomcp_core::scopes::granted_by(|claim| claims.get(claim)),
            claims,
        }
    }

    /// Whether the principal holds every scope in `required`.
    #[must_use]
    pub fn has_scopes(&self, required: &[String]) -> bool {
        required.iter().all(|r| self.scopes.iter().any(|s| s == r))
    }
}

/// Validates a bearer token string, yielding an [`AuthPrincipal`].
pub trait BearerValidator: Send + Sync {
    /// Validate `token` (the raw value after `Bearer `).
    fn validate<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<AuthPrincipal, AuthError>>;

    /// The audiences this validator accepts, when it knows them. A
    /// [`ResourceServer`](crate::ResourceServer) checks its metadata's
    /// `resource` is among them. The default, empty, means unknown.
    fn audiences(&self) -> Vec<String> {
        Vec::new()
    }
}
