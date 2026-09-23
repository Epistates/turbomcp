//! MCP authorization for the Streamable HTTP transport.
//!
//! MCP's authorization spec makes an HTTP server that requires authorization
//! an OAuth 2.1 *protected resource*. Configuring [`HttpAuthorization`] on a
//! [`ServerConfig`](crate::ServerConfig) turns that on:
//!
//! - the server publishes its Protected Resource Metadata (RFC 9728) at
//!   `/.well-known/oauth-protected-resource` — path-inserted for a server
//!   whose URL has a path — naming the authorization servers a client should
//!   use;
//! - every MCP request must carry `Authorization: Bearer <token>`. A missing
//!   or invalid token is answered `401` with a `WWW-Authenticate` challenge
//!   pointing at that metadata, which is how a client discovers where to get
//!   a token; a token lacking a required scope gets `403`
//!   `insufficient_scope`;
//! - the authenticated [`Principal`] reaches handlers through the request
//!   context, and a session is bound to the principal that created it.
//!
//! Token validation is delegated to a [`BearerTokenValidator`]. The spec
//! requires it to accept only tokens issued for this server — audience
//! validation — and never to pass a client's token on to another service.
//! `turbomcp_auth::server::JwtBearerValidator` (feature `mcp-http-server`;
//! `turbomcp::auth::server::JwtBearerValidator` with the facade's `auth` and
//! `http` features) implements it over `turbomcp-auth`'s JWT validator.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use turbomcp_core::auth::Principal;

/// Why a bearer token was refused.
#[derive(Debug, Clone)]
pub enum BearerRejection {
    /// Missing from the request, malformed, expired, not issued for this
    /// server, or otherwise invalid. Answered `401`.
    InvalidToken(String),
    /// Valid, but without a scope this server requires. Answered `403`.
    InsufficientScope {
        /// The scopes that would have been sufficient.
        required: Vec<String>,
    },
}

impl fmt::Display for BearerRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidToken(reason) => write!(f, "invalid token: {reason}"),
            Self::InsufficientScope { required } => {
                write!(f, "insufficient scope: requires {}", required.join(" "))
            }
        }
    }
}

/// Future returned by [`BearerTokenValidator::validate`].
pub type ValidationFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Principal, BearerRejection>> + Send + 'a>>;

/// Validates the bearer tokens presented to an [`HttpAuthorization`]-enabled
/// server.
///
/// An implementation MUST reject a token that was not issued for this
/// server: MCP's authorization spec requires audience validation, and a
/// token accepted regardless of audience is one issued for any other service
/// that trusts the same authorization server.
pub trait BearerTokenValidator: Send + Sync + 'static {
    /// Validate `token` and return who presented it.
    fn validate<'a>(&'a self, token: &'a str) -> ValidationFuture<'a>;
}

/// Protected-resource configuration for the Streamable HTTP transport.
///
/// ```rust,ignore
/// use turbomcp_server::{HttpAuthorization, ServerConfig};
///
/// let config = ServerConfig::builder()
///     .authorization(HttpAuthorization::new(
///         "https://mcp.example.com/mcp",
///         "https://auth.example.com",
///         validator,
///     ))
///     .build();
/// ```
#[derive(Clone)]
pub struct HttpAuthorization {
    resource: String,
    authorization_servers: Vec<String>,
    scopes_supported: Vec<String>,
    validator: Arc<dyn BearerTokenValidator>,
}

impl fmt::Debug for HttpAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpAuthorization")
            .field("resource", &self.resource)
            .field("authorization_servers", &self.authorization_servers)
            .field("scopes_supported", &self.scopes_supported)
            .finish_non_exhaustive()
    }
}

impl HttpAuthorization {
    /// Protect the server at `resource` — its canonical URL, which is what
    /// tokens must name as their audience — with tokens from
    /// `authorization_server`, checked by `validator`.
    pub fn new(
        resource: impl Into<String>,
        authorization_server: impl Into<String>,
        validator: impl BearerTokenValidator,
    ) -> Self {
        Self {
            resource: resource.into(),
            authorization_servers: vec![authorization_server.into()],
            scopes_supported: Vec::new(),
            validator: Arc::new(validator),
        }
    }

    /// Advertise another authorization server this resource accepts tokens
    /// from.
    #[must_use]
    pub fn with_authorization_server(mut self, authorization_server: impl Into<String>) -> Self {
        self.authorization_servers.push(authorization_server.into());
        self
    }

    /// Advertise the scopes this server understands, in the metadata and in
    /// `401` challenges — which is how a client learns what to ask for.
    #[must_use]
    pub fn with_scopes_supported<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes_supported = scopes.into_iter().map(Into::into).collect();
        self
    }

    /// The canonical URL of the protected server.
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Path at which the metadata document is served: RFC 9728's
    /// well-known path, with the resource's own path inserted after it.
    pub(crate) fn metadata_path(&self) -> String {
        let path = self
            .resource
            .parse::<http::Uri>()
            .map(|uri| uri.path().trim_end_matches('/').to_owned())
            .unwrap_or_default();
        format!("/.well-known/oauth-protected-resource{path}")
    }

    /// Absolute URL of the metadata document, for `resource_metadata`.
    pub(crate) fn metadata_url(&self) -> String {
        let origin = self
            .resource
            .parse::<http::Uri>()
            .ok()
            .and_then(|uri| {
                Some(format!(
                    "{}://{}",
                    uri.scheme_str()?,
                    uri.authority()?.as_str()
                ))
            })
            .unwrap_or_default();
        format!("{origin}{}", self.metadata_path())
    }

    /// The RFC 9728 Protected Resource Metadata document.
    pub(crate) fn metadata_document(&self) -> serde_json::Value {
        let mut document = serde_json::json!({
            "resource": self.resource,
            "authorization_servers": self.authorization_servers,
            // Tokens are accepted in the Authorization header only; the spec
            // forbids them in the query string.
            "bearer_methods_supported": ["header"],
        });
        if !self.scopes_supported.is_empty() {
            document["scopes_supported"] = serde_json::json!(self.scopes_supported);
        }
        document
    }

    /// Authenticate a request from its `Authorization` header.
    pub(crate) async fn authorize(
        &self,
        authorization: Option<&str>,
    ) -> Result<Principal, BearerRejection> {
        let token = authorization
            .and_then(|value| {
                let (scheme, token) = value.split_once(' ')?;
                scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
            })
            .filter(|token| !token.is_empty())
            .ok_or_else(|| BearerRejection::InvalidToken("no bearer token".into()))?;
        self.validator.validate(token).await
    }

    /// The `WWW-Authenticate` challenge for a refusal.
    ///
    /// A request with no credentials at all gets a bare challenge — RFC 6750
    /// says not to report an error then — while a rejected token gets
    /// `invalid_token` and a missing scope `insufficient_scope`. Every
    /// challenge names the metadata document, which is how a client that has
    /// never seen this server discovers where to get a token.
    pub(crate) fn challenge(&self, rejection: &BearerRejection, had_credentials: bool) -> String {
        let mut challenge = format!(
            "Bearer resource_metadata=\"{}\"",
            quoted(&self.metadata_url())
        );
        match rejection {
            BearerRejection::InvalidToken(reason) => {
                if !self.scopes_supported.is_empty() {
                    challenge.push_str(&format!(
                        ", scope=\"{}\"",
                        quoted(&self.scopes_supported.join(" "))
                    ));
                }
                if had_credentials {
                    challenge.push_str(&format!(
                        ", error=\"invalid_token\", error_description=\"{}\"",
                        quoted(reason)
                    ));
                }
            }
            BearerRejection::InsufficientScope { required } => {
                challenge.push_str(&format!(
                    ", error=\"insufficient_scope\", scope=\"{}\"",
                    quoted(&required.join(" "))
                ));
            }
        }
        challenge
    }
}

/// Make `value` safe inside a quoted-string (RFC 9110 §5.6.4).
fn quoted(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            c => vec![c],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Result<Principal, BearerRejection>);

    impl BearerTokenValidator for Fixed {
        fn validate<'a>(&'a self, _token: &'a str) -> ValidationFuture<'a> {
            let outcome = self.0.clone();
            Box::pin(async move { outcome })
        }
    }

    fn auth(resource: &str) -> HttpAuthorization {
        HttpAuthorization::new(
            resource,
            "https://auth.example.com",
            Fixed(Ok(Principal::new("u"))),
        )
    }

    #[test]
    fn metadata_is_path_inserted_for_a_server_with_a_path() {
        let auth = auth("https://mcp.example.com/mcp");
        assert_eq!(
            auth.metadata_path(),
            "/.well-known/oauth-protected-resource/mcp"
        );
        assert_eq!(
            auth.metadata_url(),
            "https://mcp.example.com/.well-known/oauth-protected-resource/mcp"
        );

        let root = super::HttpAuthorization::new(
            "https://mcp.example.com",
            "https://auth.example.com",
            Fixed(Ok(Principal::new("u"))),
        );
        assert_eq!(
            root.metadata_path(),
            "/.well-known/oauth-protected-resource"
        );
    }

    #[test]
    fn the_metadata_document_follows_rfc_9728() {
        let document = auth("https://mcp.example.com/mcp")
            .with_scopes_supported(["files:read"])
            .metadata_document();
        assert_eq!(document["resource"], "https://mcp.example.com/mcp");
        assert_eq!(
            document["authorization_servers"],
            serde_json::json!(["https://auth.example.com"])
        );
        assert_eq!(document["scopes_supported"][0], "files:read");
    }

    #[tokio::test]
    async fn only_the_authorization_header_carries_a_token() {
        let auth = auth("https://mcp.example.com/mcp");
        assert!(auth.authorize(None).await.is_err());
        assert!(auth.authorize(Some("Basic abc")).await.is_err());
        assert!(auth.authorize(Some("Bearer ")).await.is_err());
        assert!(auth.authorize(Some("bearer abc")).await.is_ok());
    }

    #[test]
    fn challenges_name_the_metadata_and_escape_their_values() {
        let auth = auth("https://mcp.example.com/mcp");
        let missing = auth.challenge(&BearerRejection::InvalidToken("none".into()), false);
        assert_eq!(
            missing,
            "Bearer resource_metadata=\"https://mcp.example.com/.well-known/oauth-protected-resource/mcp\""
        );

        let invalid = auth.challenge(&BearerRejection::InvalidToken("bad \"sig\"\n".into()), true);
        assert!(invalid.contains("error=\"invalid_token\""), "{invalid}");
        assert!(invalid.contains("bad \\\"sig\\\""), "{invalid}");
        assert!(!invalid.contains('\n'), "{invalid}");

        let scope = auth.challenge(
            &BearerRejection::InsufficientScope {
                required: vec!["files:write".into()],
            },
            true,
        );
        assert!(scope.contains("error=\"insufficient_scope\""), "{scope}");
        assert!(scope.contains("scope=\"files:write\""), "{scope}");
    }
}
