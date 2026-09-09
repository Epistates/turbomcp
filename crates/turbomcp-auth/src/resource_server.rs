//! The OAuth 2.1 **resource server**: the [`HttpAuthenticator`] the HTTP
//! transport wires in.
//!
//! It validates the request's `Authorization: Bearer` token (via a
//! [`BearerValidator`]), enforces a baseline required-scope set, and — on
//! failure — produces the spec's challenges: 401 for a missing/invalid token,
//! 403 for insufficient scope, each carrying `WWW-Authenticate: Bearer …,
//! resource_metadata="…"` so the client can discover how to authenticate
//! (MCP authorization spec §Access).

use serde_json::{Value, json};
use turbomcp_service::{AuthDecision, AuthFuture, HttpAuthenticator};

use crate::error::AuthError;
use crate::metadata::ResourceMetadata;
use crate::validator::BearerValidator;

/// An MCP server acting as an OAuth 2.1 resource server.
///
/// Construct one with a [`BearerValidator`] and the resource's
/// [`ResourceMetadata`], then hand it to the HTTP transport
/// (`HttpConfig::with_authenticator`).
#[derive(Debug)]
pub struct ResourceServer<V> {
    validator: V,
    metadata: ResourceMetadata,
    metadata_url: String,
    required_scopes: Vec<String>,
}

impl<V: BearerValidator> ResourceServer<V> {
    /// A resource server validating tokens with `validator`, advertising
    /// `metadata`, whose metadata document is served at `metadata_url` (the
    /// absolute `…/.well-known/oauth-protected-resource` URL, echoed in the
    /// `WWW-Authenticate` challenge so clients can fetch it).
    #[must_use]
    pub fn new(validator: V, metadata: ResourceMetadata, metadata_url: impl Into<String>) -> Self {
        Self {
            validator,
            metadata,
            metadata_url: metadata_url.into(),
            required_scopes: Vec::new(),
        }
    }

    /// Require every request to carry these scopes (a baseline gate; per-tool
    /// scope policy is a separate, later concern). A token missing any of them
    /// is answered 403 `insufficient_scope`.
    #[must_use]
    pub fn required_scopes(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.required_scopes = scopes.into_iter().map(Into::into).collect();
        self
    }

    /// The 401 challenge for a missing or invalid token. `error` distinguishes
    /// "no token at all" (bare challenge) from "token present but bad"
    /// (`error="invalid_token"`, RFC 6750 §3).
    fn unauthorized(&self, invalid: bool) -> AuthDecision {
        let mut params = Vec::new();
        if invalid {
            params.push(("error".to_owned(), "invalid_token".to_owned()));
        }
        if !self.required_scopes.is_empty() {
            params.push(("scope".to_owned(), self.required_scopes.join(" ")));
        }
        params.push(("resource_metadata".to_owned(), self.metadata_url.clone()));
        AuthDecision::Challenge {
            status: 401,
            www_authenticate: www_authenticate(&params),
        }
    }

    /// The 403 challenge for a valid token lacking a required scope
    /// (RFC 6750 §3.1: `error="insufficient_scope"`).
    fn forbidden(&self) -> AuthDecision {
        let params = vec![
            ("error".to_owned(), "insufficient_scope".to_owned()),
            ("scope".to_owned(), self.required_scopes.join(" ")),
            ("resource_metadata".to_owned(), self.metadata_url.clone()),
        ];
        AuthDecision::Challenge {
            status: 403,
            www_authenticate: www_authenticate(&params),
        }
    }
}

impl<V: BearerValidator> HttpAuthenticator for ResourceServer<V> {
    fn authenticate<'a>(&'a self, authorization: Option<&'a str>) -> AuthFuture<'a> {
        Box::pin(async move {
            let token = match bearer_token(authorization) {
                Ok(token) => token,
                // No header → bare 401; malformed header → invalid_token 401.
                Err(AuthError::MissingToken) => return self.unauthorized(false),
                Err(_) => return self.unauthorized(true),
            };
            let principal = match self.validator.validate(token).await {
                Ok(principal) => principal,
                Err(e) => {
                    tracing::debug!(error = %e, "bearer token rejected");
                    return self.unauthorized(true);
                }
            };
            if !principal.has_scopes(&self.required_scopes) {
                return self.forbidden();
            }
            AuthDecision::Allow(json!({
                "sub": principal.subject,
                "claims": principal.claims,
            }))
        })
    }

    fn resource_metadata(&self) -> Value {
        serde_json::to_value(&self.metadata).unwrap_or(Value::Null)
    }
}

/// Extract the token from an `Authorization: Bearer <token>` header value. The
/// scheme is matched case-insensitively (RFC 7235); the token is taken verbatim.
fn bearer_token(authorization: Option<&str>) -> Result<&str, AuthError> {
    let header = authorization.ok_or(AuthError::MissingToken)?;
    let (scheme, token) = header.split_once(' ').ok_or(AuthError::MalformedHeader)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AuthError::MalformedHeader);
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(AuthError::MalformedHeader);
    }
    Ok(token)
}

/// Build a `Bearer <k>="<v>", …` header value. Values are quoted; the spec
/// values here (errors, scopes, URLs) don't contain quotes, so no escaping is
/// needed.
fn www_authenticate(params: &[(String, String)]) -> String {
    let body = params
        .iter()
        .map(|(k, v)| format!("{k}=\"{v}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Bearer {body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bearer_case_insensitively() {
        assert_eq!(bearer_token(Some("Bearer abc")).unwrap(), "abc");
        assert_eq!(bearer_token(Some("bearer abc")).unwrap(), "abc");
        assert!(matches!(bearer_token(None), Err(AuthError::MissingToken)));
        assert!(matches!(
            bearer_token(Some("Basic abc")),
            Err(AuthError::MalformedHeader)
        ));
        assert!(matches!(
            bearer_token(Some("Bearer ")),
            Err(AuthError::MalformedHeader)
        ));
    }

    #[test]
    fn www_authenticate_shape() {
        let h = www_authenticate(&[
            ("error".to_owned(), "invalid_token".to_owned()),
            (
                "resource_metadata".to_owned(),
                "https://x/.well-known".to_owned(),
            ),
        ]);
        assert_eq!(
            h,
            r#"Bearer error="invalid_token", resource_metadata="https://x/.well-known""#
        );
    }
}
