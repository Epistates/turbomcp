//! Bearer-token validation.
//!
//! [`BearerValidator`] is the pluggable seam (JWT here; token introspection or
//! PASETO could implement it too). [`JwtValidator`] is the JWT implementation:
//! it verifies the signature against a [`JwkSource`](crate::JwkSource), and —
//! per the MCP authorization spec's MUSTs — binds the `aud` claim to this
//! resource and checks `iss` and `exp`.

use futures::future::BoxFuture;
use jsonwebtoken::{Algorithm, TokenData, Validation, decode, decode_header};
use serde_json::{Map, Value};

use crate::error::AuthError;
use crate::jwks::JwkSource;

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

/// A JWT resource-server validator: signature (via JWKS) + `aud` binding +
/// `iss` + `exp`, per the MCP authorization spec.
#[derive(Debug)]
pub struct JwtValidator<S> {
    source: S,
    audiences: Vec<String>,
    issuers: Vec<String>,
    algorithms: Vec<Algorithm>,
    leeway: u64,
}

impl<S: JwkSource> JwtValidator<S> {
    /// A validator keyed by `source`, requiring tokens whose `aud` includes
    /// `audience` (this resource's canonical URI) and whose `iss` is `issuer`.
    /// Defaults to RS256 with 60s clock-skew leeway.
    #[must_use]
    pub fn new(source: S, audience: impl Into<String>, issuer: impl Into<String>) -> Self {
        Self {
            source,
            audiences: vec![audience.into()],
            issuers: vec![issuer.into()],
            algorithms: vec![Algorithm::RS256],
            leeway: 60,
        }
    }

    /// Accept additional audiences (e.g. a legacy URI alongside the canonical).
    #[must_use]
    pub fn add_audience(mut self, audience: impl Into<String>) -> Self {
        self.audiences.push(audience.into());
        self
    }

    /// Accept additional issuers.
    #[must_use]
    pub fn add_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuers.push(issuer.into());
        self
    }

    /// Set the accepted signature algorithms (default `[RS256]`).
    #[must_use]
    pub fn algorithms(mut self, algorithms: Vec<Algorithm>) -> Self {
        self.algorithms = algorithms;
        self
    }

    /// Set the clock-skew leeway in seconds (default 60).
    #[must_use]
    pub fn leeway(mut self, seconds: u64) -> Self {
        self.leeway = seconds;
        self
    }

    async fn validate_inner(&self, token: &str) -> Result<AuthPrincipal, AuthError> {
        let header = decode_header(token)
            .map_err(|e| AuthError::InvalidToken(format!("bad header: {e}")))?;
        if !self.algorithms.contains(&header.alg) {
            return Err(AuthError::InvalidToken(format!(
                "algorithm {:?} not accepted",
                header.alg
            )));
        }
        let key = self.source.decoding_key(header.kid.as_deref()).await?;

        let mut validation = Validation::new(header.alg);
        validation.set_audience(&self.audiences);
        validation.set_issuer(&self.issuers);
        validation.leeway = self.leeway;
        // `exp` is required and validated by default; demand it explicitly so a
        // token without it is rejected rather than treated as non-expiring.
        validation.set_required_spec_claims(&["exp", "aud", "iss"]);

        let data: TokenData<Map<String, Value>> = decode(token, &key, &validation)
            .map_err(|e| AuthError::InvalidToken(format!("verification failed: {e}")))?;
        principal_from_claims(data.claims)
    }
}

impl<S: JwkSource> BearerValidator for JwtValidator<S> {
    fn validate<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<AuthPrincipal, AuthError>> {
        Box::pin(self.validate_inner(token))
    }
}

/// Build a principal from a validated claim set: `sub` is required; scopes come
/// from the space-delimited `scope` string (RFC 8693) or the `scp` array.
fn principal_from_claims(claims: Map<String, Value>) -> Result<AuthPrincipal, AuthError> {
    let subject = claims
        .get("sub")
        .and_then(Value::as_str)
        .ok_or_else(|| AuthError::InvalidToken("token has no `sub` claim".to_owned()))?
        .to_owned();
    let scopes = extract_scopes(&claims);
    Ok(AuthPrincipal {
        subject,
        scopes,
        claims,
    })
}

/// Scopes from `scope` (space-delimited string) or `scp` (array of strings).
fn extract_scopes(claims: &Map<String, Value>) -> Vec<String> {
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
