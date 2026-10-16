//! Client registration: the MCP spec's three mechanisms, applied in its
//! priority order — pre-registered credentials, Client ID Metadata Documents,
//! then (deprecated, backwards-compatible) RFC 7591 Dynamic Client
//! Registration with the mandatory `application_type`.

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::OAuthClientError;
use super::discovery::AuthorizationServerMetadata;

/// A client identity usable at one authorization server.
///
/// `Debug` is redacted: `client_secret` is a secret and never renders (only
/// whether one is present does).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClientCredentials {
    /// The OAuth `client_id`.
    pub client_id: String,
    /// The client secret, when the registration produced a confidential
    /// client. Public clients (the common MCP case) have none. Wiped from
    /// memory when dropped.
    pub client_secret: Option<Zeroizing<String>>,
    /// When the secret stops working (seconds since the epoch), as dynamic
    /// registration reported it (`client_secret_expires_at`, RFC 7591
    /// §3.2.1); `None` (or `0` on the wire) for never. Stored credentials
    /// past it are registered again rather than presented.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_expires_at: Option<u64>,
}

impl std::fmt::Debug for ClientCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientCredentials")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl ClientCredentials {
    /// Public-client credentials (no secret).
    #[must_use]
    pub fn public(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: None,
            client_secret_expires_at: None,
        }
    }

    /// Confidential-client credentials: `client_id` and its secret.
    #[must_use]
    pub fn confidential(client_id: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: Some(Zeroizing::new(secret.into())),
            client_secret_expires_at: None,
        }
    }

    /// Whether the secret has expired (a public client's never does).
    #[must_use]
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.client_secret_expires_at.is_some_and(|at| at <= now)
    }
}

/// OIDC `application_type` for Dynamic Client Registration. MCP clients MUST
/// send one: omitting it defaults to `web` under OIDC, which rejects
/// native-style (`localhost`) redirect URIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApplicationType {
    /// Desktop / CLI / mobile / locally-hosted (localhost redirect) clients.
    Native,
    /// Remote browser-based clients.
    Web,
}

/// How this client obtains its `client_id` at an authorization server, in the
/// spec's own priority order.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum RegistrationStrategy {
    /// Pre-registered credentials for a known authorization server (highest
    /// priority when available).
    Preregistered {
        /// The credentials issued at registration time.
        credentials: ClientCredentials,
        /// The issuer that issued them. Pre-registered credentials belong to
        /// one authorization server ("clients MUST associate those
        /// credentials with the specific authorization server that issued
        /// them, keyed by the authorization server's `issuer`"): when the
        /// discovered server differs, the flow fails with
        /// [`OAuthClientError::IssuerChanged`] instead of presenting them to
        /// a server the resource chose.
        issuer: String,
    },
    /// OAuth Client ID Metadata Documents: the HTTPS URL of a metadata
    /// document this client hosts, used verbatim as the `client_id`. Usable
    /// when the AS advertises `client_id_metadata_document_supported`.
    MetadataDocument {
        /// The HTTPS URL (must have a path component) that serves the JSON
        /// metadata document; becomes the `client_id`.
        client_id_url: String,
    },
    /// RFC 7591 Dynamic Client Registration (deprecated fallback).
    Dynamic(DynamicRegistration),
    /// Each strategy in turn, the first that applies to the authorization
    /// server winning: "Clients supporting all options SHOULD use the
    /// following priority order: pre-registered, Client ID Metadata
    /// Documents, Dynamic Client Registration". A strategy that doesn't apply
    /// here (pre-registered at another issuer, a metadata document the server
    /// doesn't support, no registration endpoint) moves on to the next;
    /// one that applies and fails is the answer.
    Chain(Vec<RegistrationStrategy>),
}

impl RegistrationStrategy {
    /// The spec's priority chain over whichever of the three this client has.
    #[must_use]
    pub fn in_spec_order(
        preregistered: Option<(ClientCredentials, String)>,
        client_id_url: Option<String>,
        dynamic: Option<DynamicRegistration>,
    ) -> Self {
        let mut chain = Vec::new();
        if let Some((credentials, issuer)) = preregistered {
            chain.push(Self::Preregistered {
                credentials,
                issuer,
            });
        }
        if let Some(client_id_url) = client_id_url {
            chain.push(Self::MetadataDocument { client_id_url });
        }
        if let Some(request) = dynamic {
            chain.push(Self::Dynamic(request));
        }
        Self::Chain(chain)
    }

    /// The issuers this strategy holds pre-registered credentials for, which
    /// discovery prefers among a resource's authorization servers.
    pub(crate) fn preregistered_issuers(&self) -> Vec<&str> {
        match self {
            Self::Preregistered { issuer, .. } => vec![issuer.as_str()],
            Self::Chain(chain) => chain.iter().flat_map(Self::preregistered_issuers).collect(),
            _ => Vec::new(),
        }
    }

    /// The redirect URIs dynamic registration asks for, when this strategy
    /// registers dynamically (the first such, in a chain).
    pub(crate) fn dynamic_redirect_uris(&self) -> Option<&[String]> {
        match self {
            Self::Dynamic(request) => Some(&request.redirect_uris),
            Self::Chain(chain) => chain.iter().find_map(Self::dynamic_redirect_uris),
            _ => None,
        }
    }

    /// Whether any strategy in this one is pre-registered credentials.
    pub(crate) fn is_preregistered(&self) -> bool {
        !self.preregistered_issuers().is_empty()
    }
}

/// The RFC 7591 registration request this client sends when falling back to
/// Dynamic Client Registration.
#[derive(Debug, Clone, Serialize)]
pub struct DynamicRegistration {
    /// Human-readable client name shown on consent screens.
    pub client_name: String,
    /// Exact redirect URIs to register.
    pub redirect_uris: Vec<String>,
    /// OIDC application type (MCP MUST send one; see [`ApplicationType`]).
    pub application_type: ApplicationType,
    /// Grant types; defaults to `authorization_code` + `refresh_token`.
    pub grant_types: Vec<String>,
    /// Response types; defaults to `code`.
    pub response_types: Vec<String>,
    /// Token-endpoint auth; defaults to `none` (public client + PKCE).
    pub token_endpoint_auth_method: String,
}

impl DynamicRegistration {
    /// A native public client (localhost redirect, PKCE, no secret) — the
    /// common MCP desktop/CLI shape.
    #[must_use]
    pub fn native(client_name: impl Into<String>, redirect_uris: Vec<String>) -> Self {
        Self {
            client_name: client_name.into(),
            redirect_uris,
            application_type: ApplicationType::Native,
            grant_types: vec!["authorization_code".into(), "refresh_token".into()],
            response_types: vec!["code".into()],
            token_endpoint_auth_method: "none".into(),
        }
    }

    /// A web application client (HTTPS redirect).
    #[must_use]
    pub fn web(client_name: impl Into<String>, redirect_uris: Vec<String>) -> Self {
        Self {
            application_type: ApplicationType::Web,
            ..Self::native(client_name, redirect_uris)
        }
    }
}

#[derive(Deserialize)]
struct RegistrationResponse {
    client_id: String,
    #[serde(default)]
    client_secret: Option<Zeroizing<String>>,
    /// `0` means the secret never expires (RFC 7591 §3.2.1).
    #[serde(default)]
    client_secret_expires_at: Option<u64>,
}

/// Resolve credentials at `as_meta`'s server per the spec's priority:
/// pre-registered as given; a Client ID Metadata Document URL when the AS
/// supports it; else RFC 7591 registration at the advertised
/// `registration_endpoint`.
///
/// # Errors
/// [`OAuthClientError::Registration`] when no mechanism is available for this
/// AS or the registration request is rejected.
///
/// Under [`NetworkPolicy::public_only`](crate::NetworkPolicy::public_only),
/// as the engines default to.
pub async fn obtain_credentials(
    http: &reqwest::Client,
    as_meta: &AuthorizationServerMetadata,
    strategy: &RegistrationStrategy,
) -> Result<ClientCredentials, OAuthClientError> {
    obtain_credentials_with_policy(
        http,
        as_meta,
        strategy,
        &crate::NetworkPolicy::public_only(),
    )
    .await
}
pub(crate) fn obtain_credentials_with_policy<'a>(
    http: &'a reqwest::Client,
    as_meta: &'a AuthorizationServerMetadata,
    strategy: &'a RegistrationStrategy,
    policy: &'a crate::NetworkPolicy,
) -> futures::future::BoxFuture<'a, Result<ClientCredentials, OAuthClientError>> {
    Box::pin(obtain_one(http, as_meta, strategy, policy))
}

async fn obtain_one(
    http: &reqwest::Client,
    as_meta: &AuthorizationServerMetadata,
    strategy: &RegistrationStrategy,
    policy: &crate::NetworkPolicy,
) -> Result<ClientCredentials, OAuthClientError> {
    match strategy {
        RegistrationStrategy::Chain(chain) => {
            let mut skipped = Vec::new();
            for strategy in chain {
                match obtain_credentials_with_policy(http, as_meta, strategy, policy).await {
                    Ok(credentials) => return Ok(credentials),
                    // Doesn't apply to this server: the next one might.
                    Err(
                        e @ (OAuthClientError::IssuerChanged { .. }
                        | OAuthClientError::Registration(_)),
                    ) if !applies(strategy, as_meta) => {
                        skipped.push(e.to_string());
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(OAuthClientError::Registration(format!(
                "no registration strategy applies to {}: {}",
                as_meta.issuer,
                if skipped.is_empty() {
                    "none configured".to_owned()
                } else {
                    skipped.join("; ")
                }
            )))
        }
        RegistrationStrategy::Preregistered {
            credentials,
            issuer,
        } => {
            if !super::discovery::same_issuer(issuer, &as_meta.issuer) {
                return Err(OAuthClientError::IssuerChanged {
                    expected: issuer.clone(),
                    discovered: as_meta.issuer.clone(),
                });
            }
            Ok(credentials.clone())
        }
        RegistrationStrategy::MetadataDocument { client_id_url } => {
            if as_meta.client_id_metadata_document_supported != Some(true) {
                return Err(OAuthClientError::Registration(format!(
                    "authorization server {} does not advertise client_id_metadata_document_supported",
                    as_meta.issuer
                )));
            }
            require_metadata_document_url(client_id_url)?;
            Ok(ClientCredentials::public(client_id_url.clone()))
        }
        RegistrationStrategy::Dynamic(request) => {
            let Some(endpoint) = &as_meta.registration_endpoint else {
                return Err(OAuthClientError::Registration(format!(
                    "authorization server {} advertises no registration_endpoint",
                    as_meta.issuer
                )));
            };
            for redirect in &request.redirect_uris {
                super::discovery::require_secure_url(redirect, "the registration redirect URI")?;
            }
            let resp = policy
                .send(http, http.post(endpoint).json(request))
                .await
                .map_err(OAuthClientError::Registration)?;
            let status = resp.status();
            if !status.is_success() {
                // MCP: surface registration failures meaningfully (the AS may
                // enforce application_type/redirect constraints).
                let body = String::from_utf8_lossy(&policy.body(resp).await.unwrap_or_default())
                    .into_owned();
                return Err(OAuthClientError::Registration(format!(
                    "dynamic registration rejected (HTTP {status}): {body}"
                )));
            }
            let registered: RegistrationResponse = serde_json::from_slice(
                &policy
                    .body(resp)
                    .await
                    .map_err(OAuthClientError::Registration)?,
            )
            .map_err(|e| OAuthClientError::Registration(format!("invalid response: {e}")))?;
            Ok(ClientCredentials {
                client_id: registered.client_id,
                client_secret: registered.client_secret,
                client_secret_expires_at: registered.client_secret_expires_at.filter(|at| *at != 0),
            })
        }
    }
}

/// Whether `strategy` is one this authorization server can take at all, as
/// opposed to one it took and refused.
fn applies(strategy: &RegistrationStrategy, as_meta: &AuthorizationServerMetadata) -> bool {
    match strategy {
        RegistrationStrategy::Preregistered { issuer, .. } => {
            super::discovery::same_issuer(issuer, &as_meta.issuer)
        }
        RegistrationStrategy::MetadataDocument { .. } => {
            as_meta.client_id_metadata_document_supported == Some(true)
        }
        RegistrationStrategy::Dynamic(_) => as_meta.registration_endpoint.is_some(),
        RegistrationStrategy::Chain(chain) => chain.iter().any(|s| applies(s, as_meta)),
    }
}

/// "The `client_id` URL MUST use the `https` scheme and contain a path
/// component, e.g. `https://example.com/client.json`." No loopback exception,
/// unlike redirect URIs: the authorization server fetches this document, and
/// a URL it can't fetch as HTTPS isn't a client identity at all.
fn require_metadata_document_url(url: &str) -> Result<(), OAuthClientError> {
    let parsed = url::Url::parse(url)
        .map_err(|_| OAuthClientError::Registration(format!("invalid client_id_url: {url:?}")))?;
    if parsed.scheme() != "https" || matches!(parsed.path(), "" | "/") {
        return Err(OAuthClientError::Registration(format!(
            "client_id_url must be an https URL with a path component (e.g. \
             https://example.com/client.json), got {url:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod metadata_document_tests {
    use super::*;

    #[test]
    fn a_client_id_url_needs_https_and_a_path() {
        assert!(require_metadata_document_url("https://example.com/client.json").is_ok());
        for bad in [
            "https://example.com",
            "https://example.com/",
            "http://example.com/client.json",
            "http://localhost/client.json",
            "not a url",
        ] {
            assert!(require_metadata_document_url(bad).is_err(), "{bad}");
        }
    }
}

#[cfg(test)]
mod chain_tests {
    use super::*;

    fn server(meta: serde_json::Value) -> AuthorizationServerMetadata {
        serde_json::from_value(meta).unwrap()
    }

    /// A chain moves past what this server can't take (a metadata document
    /// it doesn't support) to what it can (credentials issued by it), and
    /// says so when nothing applies.
    #[tokio::test]
    async fn a_chain_takes_the_first_strategy_that_applies() {
        let as_meta = server(serde_json::json!({
            "issuer": "https://as.example",
            "authorization_endpoint": "https://as.example/authorize",
            "token_endpoint": "https://as.example/token",
        }));
        let http = reqwest::Client::new();
        let chain = RegistrationStrategy::in_spec_order(
            Some((
                ClientCredentials::public("pre"),
                "https://as.example".into(),
            )),
            Some("https://client.example/meta.json".into()),
            None,
        );
        let got = obtain_credentials(&http, &as_meta, &chain).await.unwrap();
        assert_eq!(got.client_id, "pre");

        let elsewhere = RegistrationStrategy::Chain(vec![
            RegistrationStrategy::Preregistered {
                credentials: ClientCredentials::public("pre"),
                issuer: "https://other.example".into(),
            },
            RegistrationStrategy::MetadataDocument {
                client_id_url: "https://client.example/meta.json".into(),
            },
        ]);
        let err = obtain_credentials(&http, &as_meta, &elsewhere)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("no registration strategy applies"),
            "{err}"
        );
    }

    #[test]
    fn an_expired_secret_is_expired() {
        let mut credentials = ClientCredentials::confidential("c", "s");
        assert!(!credentials.is_expired());
        credentials.client_secret_expires_at = Some(1);
        assert!(credentials.is_expired());
    }
}
