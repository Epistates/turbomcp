//! Client-side discovery: from a 401 (or a bare server URL) to authorization
//! server endpoints.
//!
//! Implements the two-hop flow the MCP 2025-11-25 spec ("Protected Resource
//! Metadata Discovery Requirements" + "Authorization Server Metadata
//! Discovery") requires an MCP client to run before it can start an OAuth 2.1
//! authorization-code + PKCE flow:
//!
//! 1. **Resource discovery** (RFC 9728): starting from a `WWW-Authenticate`
//!    header's `resource_metadata` param if present, otherwise well-known
//!    path-insertion then well-known at root — fetch and parse Protected
//!    Resource Metadata to find the resource's authorization server(s).
//! 2. **Authorization server discovery** (RFC 8414 / OIDC Discovery, MCP
//!    priority order): resolve the chosen authorization server's issuer URL
//!    to its full metadata via [`super::DiscoveryFetcher`].
//! 3. **PKCE capability check** (AU-15 / spec "Authorization Code
//!    Protection"): refuse to proceed unless the discovered metadata
//!    advertises `S256` in `code_challenge_methods_supported`. Its absence
//!    means the authorization server doesn't support PKCE, which OAuth 2.1
//!    (and therefore MCP) requires.
//!
//! Every fetch goes through [`crate::ssrf::SsrfValidator::fetch`], the same
//! SSRF-protected, DNS-pinned path [`super::DiscoveryFetcher`] uses.

use std::sync::Arc;

use thiserror::Error;

use super::{AuthorizationServerMetadata, DiscoveryFetcher, FetcherError};
use crate::config::ProtectedResourceMetadata;
use crate::ssrf::SsrfValidator;

/// Errors from the resource -> authorization-server discovery flow.
#[derive(Debug, Error)]
pub enum ClientDiscoveryError {
    /// No Protected Resource Metadata candidate URL could be determined
    /// (invalid server URL and no `resource_metadata` challenge param).
    #[error("could not determine a Protected Resource Metadata URL: {0}")]
    NoResourceMetadataCandidate(String),

    /// Every Protected Resource Metadata candidate URL failed.
    #[error("failed to fetch Protected Resource Metadata from any candidate URL: {0:?}")]
    ResourceMetadataFetchFailed(Vec<(String, String)>),

    /// A Protected Resource Metadata response didn't parse as valid JSON
    /// matching the expected shape.
    #[error("Protected Resource Metadata response was invalid: {0}")]
    ResourceMetadataInvalid(String),

    /// The Protected Resource Metadata document named no authorization
    /// server (RFC 9728 §2 makes `authorization_servers` optional, but a
    /// client can't proceed without at least one).
    #[error("Protected Resource Metadata named no authorization server")]
    NoAuthorizationServer,

    /// Authorization server metadata discovery failed (see [`FetcherError`]
    /// for the per-endpoint attempts).
    #[error("authorization server metadata discovery failed: {0}")]
    AuthorizationServerDiscovery(#[from] FetcherError),

    /// AU-15 / spec "Authorization Code Protection": the authorization
    /// server's metadata doesn't advertise S256 PKCE support. MCP clients
    /// MUST refuse to proceed in this case rather than fall back to a
    /// weaker or absent PKCE method.
    #[error(
        "authorization server does not advertise S256 PKCE support (code_challenge_methods_supported); refusing to proceed"
    )]
    PkceNotSupported,
}

/// The subset of a discovered authorization server's metadata an MCP client
/// needs to start the OAuth 2.1 authorization-code + PKCE flow.
///
/// Constructing one (via [`ClientDiscovery::discover_authorization_server`]
/// or [`ClientDiscovery::discover`]) already confirms S256 PKCE support
/// (AU-15) — there's no way to get a value of this type otherwise.
#[derive(Debug, Clone)]
pub struct DiscoveredAuthorizationServer {
    /// The authorization server's issuer identifier
    pub issuer: String,
    /// Authorization endpoint URL
    pub authorization_endpoint: String,
    /// Token endpoint URL (absent only for implicit-flow-only servers, which
    /// OAuth 2.1 doesn't support anyway)
    pub token_endpoint: Option<String>,
    /// Dynamic Client Registration endpoint (RFC 7591), if supported
    pub registration_endpoint: Option<String>,
    /// JWKS URI for verifying tokens this server issues
    pub jwks_uri: Option<String>,
    /// Token revocation endpoint (RFC 7009), if supported
    pub revocation_endpoint: Option<String>,
    /// Token introspection endpoint (RFC 7662), if supported
    pub introspection_endpoint: Option<String>,
    /// Scopes the authorization server advertises
    pub scopes_supported: Option<Vec<String>>,
    /// PKCE code challenge methods the authorization server supports.
    /// Guaranteed to contain `"S256"` — see the type-level doc.
    pub code_challenge_methods_supported: Vec<String>,
}

impl DiscoveredAuthorizationServer {
    fn from_metadata(metadata: &AuthorizationServerMetadata) -> Result<Self, ClientDiscoveryError> {
        if !metadata.supports_pkce_method("S256") {
            return Err(ClientDiscoveryError::PkceNotSupported);
        }

        Ok(Self {
            issuer: metadata.issuer.clone(),
            authorization_endpoint: metadata.authorization_endpoint.clone(),
            token_endpoint: metadata.token_endpoint.clone(),
            registration_endpoint: metadata.registration_endpoint.clone(),
            jwks_uri: metadata.jwks_uri.clone(),
            revocation_endpoint: metadata.revocation_endpoint.clone(),
            introspection_endpoint: metadata.introspection_endpoint.clone(),
            scopes_supported: metadata.scopes_supported.clone(),
            code_challenge_methods_supported: metadata
                .code_challenge_methods_supported
                .clone()
                .unwrap_or_default(),
        })
    }
}

/// Discovers an MCP server's authorization server, end to end, from either a
/// 401 challenge or a bare server URL.
pub struct ClientDiscovery {
    ssrf_validator: Arc<SsrfValidator>,
    as_fetcher: DiscoveryFetcher,
}

impl ClientDiscovery {
    /// Create a new client discovery helper.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying [`DiscoveryFetcher`] can't be
    /// constructed.
    pub fn new(ssrf_validator: SsrfValidator) -> Result<Self, FetcherError> {
        let ssrf_validator = Arc::new(ssrf_validator);
        let as_fetcher = DiscoveryFetcher::new((*ssrf_validator).clone())?;
        Ok(Self {
            ssrf_validator,
            as_fetcher,
        })
    }

    /// Step 1: fetch and parse Protected Resource Metadata (RFC 9728).
    ///
    /// `www_authenticate` is the `WWW-Authenticate` header value from the
    /// 401 response that triggered discovery, if any. When it carries a
    /// `resource_metadata` param (RFC 9728 §5.1), that URL is used
    /// exclusively, per the 2025-11-25 spec's "Protected Resource Metadata
    /// Discovery Requirements": clients "use the resource metadata URL from
    /// the parsed `WWW-Authenticate` headers when present". Otherwise, falls
    /// back to well-known URI probing in the spec's order: path-insertion
    /// (`/.well-known/oauth-protected-resource{path}`) before root
    /// (`/.well-known/oauth-protected-resource`).
    pub async fn discover_resource_metadata(
        &self,
        www_authenticate: Option<&str>,
        server_url: &str,
    ) -> Result<ProtectedResourceMetadata, ClientDiscoveryError> {
        if let Some(header) = www_authenticate
            && let Some(url) = parse_resource_metadata_param(header)
        {
            return self.fetch_resource_metadata_json(&url).await;
        }

        let candidates = well_known_resource_metadata_urls(server_url)
            .map_err(ClientDiscoveryError::NoResourceMetadataCandidate)?;

        let mut attempts = Vec::with_capacity(candidates.len());
        for url in candidates {
            match self.fetch_resource_metadata_json(&url).await {
                Ok(metadata) => return Ok(metadata),
                Err(e) => attempts.push((url, e.to_string())),
            }
        }

        Err(ClientDiscoveryError::ResourceMetadataFetchFailed(attempts))
    }

    /// Steps 2 + 3: resolve the resource's first authorization server to
    /// full metadata (MCP priority order, via [`DiscoveryFetcher`]) and
    /// confirm S256 PKCE support (AU-15).
    ///
    /// Per RFC 9728 §7.6, selecting *which* authorization server to use when
    /// several are listed is a client policy decision; this takes the first
    /// one. Callers with their own selection policy should call
    /// [`super::DiscoveryFetcher::fetch`] directly with their chosen issuer.
    pub async fn discover_authorization_server(
        &self,
        resource: &ProtectedResourceMetadata,
    ) -> Result<DiscoveredAuthorizationServer, ClientDiscoveryError> {
        let issuer = resource
            .authorization_servers
            .first()
            .ok_or(ClientDiscoveryError::NoAuthorizationServer)?;

        let metadata = self.as_fetcher.fetch(issuer).await?;
        DiscoveredAuthorizationServer::from_metadata(metadata.oauth2())
    }

    /// Convenience: run the full 401 -> PRM -> AS metadata flow in one call.
    pub async fn discover(
        &self,
        www_authenticate: Option<&str>,
        server_url: &str,
    ) -> Result<DiscoveredAuthorizationServer, ClientDiscoveryError> {
        let resource = self
            .discover_resource_metadata(www_authenticate, server_url)
            .await?;
        self.discover_authorization_server(&resource).await
    }

    async fn fetch_resource_metadata_json(
        &self,
        url: &str,
    ) -> Result<ProtectedResourceMetadata, ClientDiscoveryError> {
        let bytes = self
            .ssrf_validator
            .fetch(url)
            .await
            .map_err(|e| ClientDiscoveryError::ResourceMetadataInvalid(e.to_string()))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| ClientDiscoveryError::ResourceMetadataInvalid(e.to_string()))
    }
}

/// Extract the `resource_metadata` auth-param from a `WWW-Authenticate`
/// header value (RFC 9728 §5.1): `Bearer resource_metadata="...", ...`.
///
/// This is a minimal parser for the one param this flow needs, not a
/// general RFC 7235 auth-param parser — it assumes the URL doesn't itself
/// contain a `"` (URLs don't), which is the only case that matters here.
fn parse_resource_metadata_param(header_value: &str) -> Option<String> {
    const KEY: &str = "resource_metadata=\"";
    let start = header_value.find(KEY)? + KEY.len();
    let rest = &header_value[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Build the well-known Protected Resource Metadata candidate URLs for
/// `server_url`, in the 2025-11-25 spec's fallback order: path-insertion
/// before root. Per the spec's example, `https://example.com/public/mcp`
/// probes `https://example.com/.well-known/oauth-protected-resource/public/mcp`
/// before `https://example.com/.well-known/oauth-protected-resource`.
fn well_known_resource_metadata_urls(server_url: &str) -> Result<Vec<String>, String> {
    let parsed = url::Url::parse(server_url)
        .map_err(|e| format!("invalid server URL '{server_url}': {e}"))?;

    let mut urls = Vec::with_capacity(2);
    let path = parsed.path().trim_matches('/');
    if !path.is_empty() {
        let mut with_path = parsed.clone();
        with_path.set_path(&format!("/.well-known/oauth-protected-resource/{path}"));
        urls.push(with_path.to_string());
    }

    let mut root = parsed;
    root.set_path("/.well-known/oauth-protected-resource");
    urls.push(root.to_string());

    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_resource_metadata_param() {
        let header = "Bearer resource_metadata=\"https://mcp.example.com/.well-known/oauth-protected-resource\", scope=\"files:read\"";
        assert_eq!(
            parse_resource_metadata_param(header),
            Some("https://mcp.example.com/.well-known/oauth-protected-resource".to_string())
        );
    }

    #[test]
    fn test_parse_resource_metadata_param_absent() {
        let header = "Bearer error=\"invalid_token\"";
        assert_eq!(parse_resource_metadata_param(header), None);
    }

    #[test]
    fn test_well_known_urls_no_path() {
        let urls = well_known_resource_metadata_urls("https://mcp.example.com").unwrap();
        assert_eq!(
            urls,
            vec!["https://mcp.example.com/.well-known/oauth-protected-resource"]
        );
    }

    /// Spec example: `https://example.com/public/mcp` tries the
    /// path-insertion form before the root form.
    #[test]
    fn test_well_known_urls_with_path_priority_order() {
        let urls = well_known_resource_metadata_urls("https://example.com/public/mcp").unwrap();
        assert_eq!(
            urls,
            vec![
                "https://example.com/.well-known/oauth-protected-resource/public/mcp",
                "https://example.com/.well-known/oauth-protected-resource",
            ]
        );
    }

    /// AU-15: metadata missing `code_challenge_methods_supported` (or
    /// lacking S256) must be refused, not silently accepted.
    #[test]
    fn test_discovered_authorization_server_requires_s256_pkce() {
        let mut metadata = sample_as_metadata();
        metadata.code_challenge_methods_supported = None;
        assert!(matches!(
            DiscoveredAuthorizationServer::from_metadata(&metadata),
            Err(ClientDiscoveryError::PkceNotSupported)
        ));

        metadata.code_challenge_methods_supported = Some(vec!["plain".to_string()]);
        assert!(matches!(
            DiscoveredAuthorizationServer::from_metadata(&metadata),
            Err(ClientDiscoveryError::PkceNotSupported)
        ));

        metadata.code_challenge_methods_supported = Some(vec!["S256".to_string()]);
        assert!(DiscoveredAuthorizationServer::from_metadata(&metadata).is_ok());
    }

    fn sample_as_metadata() -> AuthorizationServerMetadata {
        AuthorizationServerMetadata {
            issuer: "https://auth.example.com".to_string(),
            authorization_endpoint: "https://auth.example.com/authorize".to_string(),
            token_endpoint: Some("https://auth.example.com/token".to_string()),
            jwks_uri: None,
            registration_endpoint: None,
            scopes_supported: None,
            response_types_supported: vec!["code".to_string()],
            response_modes_supported: None,
            grant_types_supported: None,
            token_endpoint_auth_methods_supported: None,
            token_endpoint_auth_signing_alg_values_supported: None,
            service_documentation: None,
            ui_locales_supported: None,
            op_policy_uri: None,
            op_tos_uri: None,
            revocation_endpoint: None,
            revocation_endpoint_auth_methods_supported: None,
            introspection_endpoint: None,
            introspection_endpoint_auth_methods_supported: None,
            code_challenge_methods_supported: Some(vec!["S256".to_string()]),
            additional_fields: std::collections::HashMap::new(),
        }
    }
}
