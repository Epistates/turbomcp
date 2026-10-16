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

/// Scopes from `scope` and `scp`, each a space-delimited string or an array
/// of strings (Microsoft Entra ID issues `scp` as a string). Read the same
/// way as [`Identity::granted_scopes`](turbomcp_core::Identity::granted_scopes),
/// so the baseline gate and the per-tool check agree on what a token grants.
#[cfg(any(feature = "jwt", feature = "introspection"))]
pub(crate) fn extract_scopes(claims: &Map<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    for claim in ["scope", "scp"] {
        match claims.get(claim) {
            Some(Value::String(s)) => out.extend(s.split_whitespace().map(str::to_owned)),
            Some(Value::Array(arr)) => {
                out.extend(arr.iter().filter_map(Value::as_str).map(str::to_owned));
            }
            _ => {}
        }
    }
    out
}
