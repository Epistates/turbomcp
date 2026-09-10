//! Client registration: the MCP spec's three mechanisms, applied in its
//! priority order — pre-registered credentials, Client ID Metadata Documents,
//! then (deprecated, backwards-compatible) RFC 7591 Dynamic Client
//! Registration with the mandatory `application_type`.

use serde::{Deserialize, Serialize};

use super::OAuthClientError;
use super::discovery::AuthorizationServerMetadata;

/// A client identity usable at one authorization server.
///
/// `Debug` is redacted: `client_secret` is a secret and never renders (only
/// whether one is present does).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCredentials {
    /// The OAuth `client_id`.
    pub client_id: String,
    /// The client secret, when the registration produced a confidential
    /// client. Public clients (the common MCP case) have none.
    pub client_secret: Option<String>,
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
        }
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
        /// The issuer that issued them, when known. Pre-registered
        /// credentials are inherently AS-specific: when set and the
        /// discovered authorization server differs, the flow surfaces an
        /// error instead of silently presenting mismatched credentials
        /// (spec §Authorization Server Binding).
        issuer: Option<String>,
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
    client_secret: Option<String>,
}

/// Resolve credentials at `as_meta`'s server per the spec's priority:
/// pre-registered as given; a Client ID Metadata Document URL when the AS
/// supports it; else RFC 7591 registration at the advertised
/// `registration_endpoint`.
///
/// # Errors
/// [`OAuthClientError::Registration`] when no mechanism is available for this
/// AS or the registration request is rejected.
pub async fn obtain_credentials(
    http: &reqwest::Client,
    as_meta: &AuthorizationServerMetadata,
    strategy: &RegistrationStrategy,
) -> Result<ClientCredentials, OAuthClientError> {
    obtain_credentials_with_policy(http, as_meta, strategy, &crate::NetworkPolicy::default()).await
}
pub(crate) async fn obtain_credentials_with_policy(
    http: &reqwest::Client,
    as_meta: &AuthorizationServerMetadata,
    strategy: &RegistrationStrategy,
    policy: &crate::NetworkPolicy,
) -> Result<ClientCredentials, OAuthClientError> {
    match strategy {
        RegistrationStrategy::Preregistered {
            credentials,
            issuer,
        } => {
            match issuer {
                Some(expected)
                    if expected.trim_end_matches('/') != as_meta.issuer.trim_end_matches('/') =>
                {
                    return Err(OAuthClientError::IssuerChanged {
                        expected: expected.clone(),
                        discovered: as_meta.issuer.clone(),
                    });
                }
                Some(_) => {}
                // No recorded issuer means no Authorization Server Binding check
                // (spec §Authorization Server Binding): the credentials would be
                // presented to whichever AS discovery selected. Warn — set the
                // `issuer` to bind them to a specific server.
                None => tracing::warn!(
                    discovered_issuer = %as_meta.issuer,
                    "pre-registered credentials have no recorded issuer; \
                     presenting them without an authorization-server binding check"
                ),
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
            })
        }
    }
}
