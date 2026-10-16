//! The OAuth 2.1 **resource server**: the [`HttpAuthenticator`] the HTTP
//! transport wires in.
//!
//! It validates the request's `Authorization: Bearer` token (via a
//! [`BearerValidator`]), enforces a baseline required-scope set, and — on
//! failure — produces the spec's challenges: 401 for a missing/invalid token,
//! 403 for insufficient scope, each carrying `WWW-Authenticate: Bearer …,
//! resource_metadata="…"` so the client can discover how to authenticate
//! (MCP authorization spec §Access).

use serde_json::Value;
use turbomcp_service::{AuthDecision, AuthFuture, HttpAuthenticator};

use crate::error::AuthError;
use crate::metadata::ResourceMetadata;
use crate::validator::BearerValidator;

/// `Retry-After` on a `503` for an unreachable key or introspection backend:
/// long enough not to hammer it, short next to a token's life.
const UNAVAILABLE_RETRY_AFTER_SECS: u64 = 5;

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
    retain_token: bool,
    scope_rules: Option<turbomcp_core::ScopeRules>,
}

impl<V: BearerValidator> ResourceServer<V> {
    /// A resource server validating tokens with `validator`, advertising
    /// `metadata`, whose metadata document is served at `metadata_url` (the
    /// absolute `…/.well-known/oauth-protected-resource` URL, echoed in the
    /// `WWW-Authenticate` challenge so clients can fetch it).
    ///
    /// # Panics
    /// If the validator names its audiences and the metadata's `resource` is
    /// not one of them. Clients request tokens for exactly that `resource`
    /// (RFC 8707), so such a server would refuse every token issued for it:
    /// an endpoint URL used as one and the origin as the other, say.
    #[must_use]
    pub fn new(validator: V, metadata: ResourceMetadata, metadata_url: impl Into<String>) -> Self {
        let audiences = validator.audiences();
        assert!(
            audiences.is_empty() || audiences.contains(&metadata.resource),
            "ResourceServer: the metadata's resource `{}` is not an audience the validator \
             accepts ({}); clients request tokens for that resource, so every one would be refused",
            metadata.resource,
            audiences.join(", ")
        );
        Self {
            validator,
            metadata,
            metadata_url: metadata_url.into(),
            required_scopes: Vec::new(),
            retain_token: false,
            scope_rules: None,
        }
    }

    /// What this authorization server's scopes imply: "a broader scope
    /// implies narrower ones" (`files` covering `files:read`, say, with
    /// [`PrefixHierarchy`](turbomcp_core::PrefixHierarchy), or a table with
    /// [`ImpliedScopes`](turbomcp_core::ImpliedScopes)). Applied to the
    /// baseline [`required_scopes`](Self::required_scopes) here and, through
    /// the transport, to every per-tool and visibility check of the requests
    /// this authorizes. Default: scopes match exactly.
    #[must_use]
    pub fn scope_hierarchy(
        mut self,
        hierarchy: impl turbomcp_core::ScopeHierarchy + 'static,
    ) -> Self {
        self.scope_rules = Some(turbomcp_core::ScopeRules(std::sync::Arc::new(hierarchy)));
        self
    }

    /// Keep each authorized request's bearer token beside it, as a
    /// `SubjectToken`, for a gateway to exchange (RFC 8693) for a token to
    /// an upstream on the caller's behalf (default off). Every handler can
    /// read it then: turn this on only for a server whose handlers are
    /// trusted with its callers' tokens.
    #[must_use]
    pub fn retain_token(mut self, retain: bool) -> Self {
        self.retain_token = retain;
        self
    }

    /// Require every request to carry these scopes (a baseline gate). A token
    /// missing any of them is answered 403 `insufficient_scope`. Per-tool
    /// scopes (`#[tool(scopes(…))]`) are checked when the tool is called, and
    /// answered with the same challenge naming the tool's scopes.
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

    /// The 400 for a malformed Bearer `Authorization` header (RFC 6750 §3.1
    /// `invalid_request`).
    fn bad_request(&self) -> AuthDecision {
        AuthDecision::Challenge {
            status: 400,
            www_authenticate: www_authenticate(&[
                ("error".to_owned(), "invalid_request".to_owned()),
                ("resource_metadata".to_owned(), self.metadata_url.clone()),
            ]),
        }
    }

    /// The 403 challenge for a valid token lacking a required scope
    /// (RFC 6750 §3.1: `error="insufficient_scope"`).
    fn forbidden(&self) -> AuthDecision {
        AuthDecision::Challenge {
            status: 403,
            www_authenticate: self.insufficient_scope_challenge(&self.required_scopes),
        }
    }

    fn insufficient_scope_challenge(&self, scopes: &[String]) -> String {
        www_authenticate(&[
            ("error".to_owned(), "insufficient_scope".to_owned()),
            ("scope".to_owned(), scopes.join(" ")),
            ("resource_metadata".to_owned(), self.metadata_url.clone()),
        ])
    }
}

impl<V: BearerValidator> HttpAuthenticator for ResourceServer<V> {
    fn authenticate<'a>(&'a self, authorization: Option<&'a str>) -> AuthFuture<'a> {
        Box::pin(async move {
            let token = match bearer_token(authorization) {
                Ok(token) => token,
                // No credentials, or another scheme's ("attempted using an
                // unsupported authentication method"): the bare challenge,
                // with no error code (RFC 6750 §3.1).
                Err(AuthError::MissingToken) => return self.unauthorized(false),
                Err(_) if !authorization.is_some_and(names_bearer) => {
                    return self.unauthorized(false);
                }
                // A Bearer header that isn't one: `400 invalid_request`.
                Err(_) => return self.bad_request(),
            };
            let principal = match self.validator.validate(token).await {
                Ok(principal) => principal,
                Err(AuthError::Unavailable(e)) => {
                    tracing::warn!(error = %e, "bearer token undecided: backend unavailable");
                    return AuthDecision::Unavailable {
                        retry_after_secs: UNAVAILABLE_RETRY_AFTER_SECS,
                    };
                }
                Err(e) => {
                    tracing::debug!(error = %e, "bearer token rejected");
                    return self.unauthorized(true);
                }
            };
            let required: Vec<&str> = self.required_scopes.iter().map(String::as_str).collect();
            if !turbomcp_core::scopes::satisfies(
                self.scope_rules.as_ref().map(|rules| rules.0.as_ref()),
                &principal.scopes,
                &required,
            ) {
                return self.forbidden();
            }
            AuthDecision::Allow(turbomcp_core::Identity::Bearer {
                sub: principal.subject,
                claims: principal.claims,
            })
        })
    }

    fn retains_token(&self) -> bool {
        self.retain_token
    }

    fn scope_rules(&self) -> Option<turbomcp_core::ScopeRules> {
        self.scope_rules.clone()
    }

    fn resource_metadata(&self) -> Value {
        serde_json::to_value(&self.metadata).unwrap_or(Value::Null)
    }

    /// A tool (or any handler) said it needs `scopes`: the same challenge as
    /// the baseline gate's, naming them.
    fn insufficient_scope(&self, scopes: &[String]) -> Option<String> {
        Some(self.insufficient_scope_challenge(scopes))
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
    // Quoted-string escaping (RFC 9110 §5.6.4): a `"` or `\\` in a value
    // would otherwise end it early, and the header then fails to parse, or
    // to become a header value at all, losing `insufficient_scope`.
    let quote = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
    let body = params
        .iter()
        .map(|(k, v)| format!("{k}=\"{}\"", quote(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Bearer {body}")
}

/// Whether an `Authorization` value is Bearer's (however malformed).
fn names_bearer(header: &str) -> bool {
    header
        .split_whitespace()
        .next()
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("bearer"))
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
    fn challenge_values_are_escaped() {
        let h = www_authenticate(&[("scope".to_owned(), r#"a"b\c"#.to_owned())]);
        assert_eq!(h, r#"Bearer scope="a\"b\\c""#);
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
