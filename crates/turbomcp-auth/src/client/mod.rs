//! The OAuth 2.1 **client** half (feature `oauth-client`): everything an MCP
//! client needs to obtain and maintain tokens for a protected MCP server, per
//! the MCP authorization spec.
//!
//! - [`challenge`] — `WWW-Authenticate` Bearer parsing (401/403, step-up)
//! - [`discovery`] — RFC 9728 Protected Resource Metadata + RFC 8414/OIDC
//!   authorization-server metadata, in the spec's mandatory endpoint order,
//!   with issuer validation and the PKCE MUST-refuse rule
//! - [`registration`] — pre-registered / Client ID Metadata Documents /
//!   RFC 7591 Dynamic Client Registration (with `application_type`)
//! - [`store`] — the issuer-keyed [`CredentialStore`] (spec §Authorization
//!   Server Binding)
//! - [`flow`] — [`OAuthClient`]: PKCE authorization-code flow, RFC 8707
//!   `resource` on every request, RFC 9207 `iss` validation, refresh,
//!   scope step-up
//!
//! The interactive step is the embedding application's: [`OAuthClient::begin`]
//! returns the URL to open; the redirect callback goes to
//! [`OAuthClient::complete`].

pub mod challenge;
pub mod discovery;
pub mod flow;
pub mod registration;
pub mod store;

use serde::{Deserialize, Serialize};

pub use challenge::{BearerChallenge, parse_bearer_challenge};
pub use discovery::{AuthorizationServerMetadata, ProtectedResourceMetadata};
pub use flow::{CallbackParams, Discovered, OAuthClient, PendingAuthorization};
pub use registration::{
    ApplicationType, ClientCredentials, DynamicRegistration, RegistrationStrategy,
};
pub use store::{CredentialStore, MemoryCredentialStore};

/// The tokens one authorization produced.
///
/// `Debug` is redacted: the access and refresh tokens are secrets, so they
/// never render (only their presence and the non-secret fields do). Use the
/// public fields directly when you deliberately need the values.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TokenSet {
    /// The bearer access token presented to the MCP server.
    pub access_token: String,
    /// The refresh token, when the AS issued one.
    pub refresh_token: Option<String>,
    /// Access-token expiry as Unix epoch seconds, when known.
    pub expires_at_epoch_secs: Option<u64>,
    /// The scopes this token set was granted (or requested, when the AS
    /// didn't echo a grant).
    pub scopes: Vec<String>,
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at_epoch_secs", &self.expires_at_epoch_secs)
            .field("scopes", &self.scopes)
            .finish()
    }
}

impl TokenSet {
    /// Whether the access token is expired (or expires within `skew`).
    #[must_use]
    pub fn expires_within(&self, skew: std::time::Duration) -> bool {
        let Some(expires_at) = self.expires_at_epoch_secs else {
            return false; // unknown expiry: assume live until the server 401s
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now + skew.as_secs() >= expires_at
    }
}

/// Failures across the OAuth client flow.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OAuthClientError {
    /// Metadata discovery/validation failed.
    #[error("discovery failed: {0}")]
    Discovery(String),
    /// The authorization server does not advertise PKCE
    /// (`code_challenge_methods_supported`) — MCP clients MUST refuse.
    #[error("authorization server does not advertise PKCE support; refusing to proceed")]
    PkceUnsupported,
    /// A URL the flow would have used is plaintext `http://` to a non-loopback
    /// host (authorization security §Communication Security: authorization
    /// server endpoints MUST be HTTPS, and redirect URIs MUST be `localhost`
    /// or HTTPS).
    #[error("{what} must use https (or a loopback host); refusing plaintext {url}")]
    InsecureUrl {
        /// Which URL this is — the issuer, an endpoint name, the redirect URI.
        what: String,
        /// The offending URL.
        url: String,
    },
    /// Client registration failed or no mechanism applies to this AS.
    #[error("client registration failed: {0}")]
    Registration(String),
    /// Stored/pre-registered credentials belong to a different authorization
    /// server than the one discovery produced (spec §Authorization Server
    /// Binding: never present mismatched credentials).
    #[error(
        "authorization server changed: credentials are for {expected}, discovered {discovered}"
    )]
    IssuerChanged {
        /// The issuer the credentials were registered with.
        expected: String,
        /// The issuer discovery produced.
        discovered: String,
    },
    /// The authorization response failed validation (state mismatch, missing
    /// code, or an AS error response).
    #[error("authorization failed: {0}")]
    Authorization(String),
    /// RFC 9207: the response's `iss` does not identify the recorded
    /// authorization server (possible mix-up attack). The response's error
    /// parameters were not trusted.
    #[error("issuer mismatch in authorization response: expected {expected}, got {got}")]
    IssuerMismatch {
        /// The issuer recorded before redirecting.
        expected: String,
        /// The `iss` the response carried.
        got: String,
    },
    /// The token endpoint rejected the exchange/refresh.
    #[error("token exchange failed: {0}")]
    TokenExchange(String),
}
