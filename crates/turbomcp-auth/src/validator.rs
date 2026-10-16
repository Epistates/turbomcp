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
pub struct AuthPrincipal {
    /// The `sub` claim.
    pub subject: String,
    /// Granted scopes (parsed from the `scope` string or `scp` array claim).
    pub scopes: Vec<String>,
    /// The full validated claim set.
    pub claims: Map<String, Value>,
}

impl AuthPrincipal {
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

/// Scopes from `scope` and `scp`, read by
/// [`turbomcp_core::scopes::granted_by`], as
/// [`Identity::granted_scopes`](turbomcp_core::Identity::granted_scopes) reads
/// them: the baseline gate and the per-tool check agree on what a token grants.
#[cfg(any(feature = "jwt", feature = "introspection"))]
pub(crate) fn extract_scopes(claims: &Map<String, Value>) -> Vec<String> {
    turbomcp_core::scopes::granted_by(|claim| claims.get(claim))
}
