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
}

/// Scopes from `scope` (space-delimited string) or `scp` (array of strings).
#[cfg(any(feature = "jwt", feature = "introspection"))]
pub(crate) fn extract_scopes(claims: &Map<String, Value>) -> Vec<String> {
    if let Some(scope) = claims.get("scope").and_then(Value::as_str) {
        return scope.split_whitespace().map(str::to_owned).collect();
    }
    if let Some(arr) = claims.get("scp").and_then(Value::as_array) {
        return arr
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    Vec::new()
}
