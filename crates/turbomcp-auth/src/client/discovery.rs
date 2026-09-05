//! Authorization-server discovery: RFC 9728 Protected Resource Metadata and
//! RFC 8414 / OpenID Connect authorization-server metadata, with the MCP
//! spec's mandatory endpoint priority order and validation rules.

use serde::Deserialize;
use url::Url;

use super::OAuthClientError;

/// RFC 9728 Protected Resource Metadata — what an MCP server publishes to
/// point clients at its authorization server(s).
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct ProtectedResourceMetadata {
    /// The resource identifier (the MCP server's canonical URI).
    pub resource: String,
    /// At least one authorization-server issuer URL (MCP MUST).
    #[serde(default)]
    pub authorization_servers: Vec<String>,
    /// The minimal scope set for basic functionality.
    #[serde(default)]
    pub scopes_supported: Option<Vec<String>>,
}

/// RFC 8414 / OIDC authorization-server metadata (the subset MCP flows use).
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct AuthorizationServerMetadata {
    /// The issuer identifier. MUST equal the issuer the document was
    /// discovered for (validated in [`discover_authorization_server`]).
    pub issuer: String,
    /// The authorization endpoint.
    pub authorization_endpoint: String,
    /// The token endpoint.
    pub token_endpoint: String,
    /// RFC 7591 Dynamic Client Registration endpoint, when supported.
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    /// PKCE methods. Its *absence* means no PKCE — MCP clients MUST refuse.
    #[serde(default)]
    pub code_challenge_methods_supported: Option<Vec<String>>,
    /// Whether the AS accepts Client ID Metadata Document client ids.
    #[serde(default)]
    pub client_id_metadata_document_supported: Option<bool>,
    /// RFC 9207: the AS includes `iss` in authorization responses.
    #[serde(default)]
    pub authorization_response_iss_parameter_supported: Option<bool>,
    /// How the AS accepts client credentials at the token endpoint
    /// (`client_secret_basic`, `client_secret_post`, `none`, …). RFC 8414
    /// §2: absent means `client_secret_basic`.
    #[serde(default)]
    pub token_endpoint_auth_methods_supported: Option<Vec<String>>,
    /// Scopes the AS can grant.
    #[serde(default)]
    pub scopes_supported: Option<Vec<String>>,
}

/// Fetch Protected Resource Metadata: from the challenge's
/// `resource_metadata` URL when present, else the RFC 9728 well-known
/// fallbacks in spec order (path-inserted, then root).
///
/// # Errors
/// [`OAuthClientError::Discovery`] when no document can be fetched or parsed.
pub async fn discover_protected_resource(
    http: &reqwest::Client,
    resource_url: &str,
    challenge_metadata_url: Option<&str>,
) -> Result<ProtectedResourceMetadata, OAuthClientError> {
    let candidates: Vec<String> = match challenge_metadata_url {
        Some(url) => vec![url.to_owned()],
        None => protected_resource_wellknown_candidates(resource_url)?,
    };
    let mut last_error = String::from("no candidate URLs");
    for candidate in &candidates {
        match fetch_json::<ProtectedResourceMetadata>(http, candidate).await {
            Ok(meta) => {
                if meta.authorization_servers.is_empty() {
                    return Err(OAuthClientError::Discovery(format!(
                        "protected resource metadata at {candidate} lists no authorization_servers"
                    )));
                }
                // RFC 9728 §3.3: the `resource` in the document MUST be the
                // resource the client is actually accessing. Skipping this
                // check is what turns a hostile or hijacked metadata document
                // into an account-takeover: it can name *any* authorization
                // server, and a client that doesn't compare will happily go
                // authorize against the attacker's and hand over the token it
                // gets back. The `resource` parameter we send is taken from
                // this document too, so an unchecked mismatch also requests a
                // token audienced for something else entirely.
                if !same_resource(&meta.resource, resource_url) {
                    return Err(OAuthClientError::Discovery(format!(
                        "protected resource metadata at {candidate} declares resource {:?}, \
                         which is not the server being accessed ({resource_url}); refusing to \
                         authorize against an authorization server chosen by a document that \
                         is not about this resource",
                        meta.resource,
                    )));
                }
                return Ok(meta);
            }
            Err(e) => last_error = format!("{candidate}: {e}"),
        }
    }
    Err(OAuthClientError::Discovery(format!(
        "protected resource metadata unavailable ({last_error})"
    )))
}

/// Whether a metadata document's `resource` covers the server at `url`.
///
/// Origin must match exactly — scheme, host, port — because that is what stops
/// a document from pointing at somebody else's authorization server. The path
/// is a *containment* check rather than equality: RFC 9728's fallback order
/// tries the path-inserted well-known URL and then the root one, and the root
/// document legitimately declares the origin (`https://host`) as the resource
/// while the endpoint being accessed is `https://host/mcp`. Requiring equality
/// there would reject a correctly configured server for using the fallback the
/// RFC defines.
///
/// Containment is checked at a segment boundary, so a document for
/// `https://host/mcp` does not vouch for `https://host/mcp-evil`.
fn same_resource(declared: &str, url: &str) -> bool {
    let (Ok(declared), Ok(url)) = (Url::parse(declared), Url::parse(url)) else {
        // An unparseable identifier is not something to wave through.
        return false;
    };
    let origin = |u: &Url| {
        (
            u.scheme().to_ascii_lowercase(),
            u.host_str().unwrap_or_default().to_ascii_lowercase(),
            u.port_or_known_default(),
        )
    };
    if origin(&declared) != origin(&url) {
        return false;
    }
    let declared_path = declared.path().trim_end_matches('/');
    let url_path = url.path().trim_end_matches('/');
    declared_path.is_empty()
        || url_path == declared_path
        || url_path
            .strip_prefix(declared_path)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The RFC 9728 well-known URLs for `resource_url`, in the spec's fallback
/// order: path-inserted (`/.well-known/oauth-protected-resource/<path>`)
/// first when the resource has a path, then the root document.
fn protected_resource_wellknown_candidates(
    resource_url: &str,
) -> Result<Vec<String>, OAuthClientError> {
    let url = Url::parse(resource_url)
        .map_err(|e| OAuthClientError::Discovery(format!("invalid resource URL: {e}")))?;
    let origin = format!(
        "{}://{}",
        url.scheme(),
        url.host_str().map_or_else(String::new, |h| {
            url.port()
                .map_or_else(|| h.to_owned(), |p| format!("{h}:{p}"))
        })
    );
    let mut out = Vec::new();
    let path = url.path().trim_end_matches('/');
    if !path.is_empty() {
        out.push(format!(
            "{origin}/.well-known/oauth-protected-resource{path}"
        ));
    }
    out.push(format!("{origin}/.well-known/oauth-protected-resource"));
    Ok(out)
}

/// Discover authorization-server metadata for `issuer`, trying the spec's
/// endpoint priority order, and validate it (document `issuer` MUST equal the
/// issuer used to build the URL; PKCE support MUST be advertised).
///
/// # Errors
/// [`OAuthClientError::Discovery`] when no valid document is found;
/// [`OAuthClientError::PkceUnsupported`] when the metadata omits
/// `code_challenge_methods_supported` (the MCP MUST-refuse rule).
pub async fn discover_authorization_server(
    http: &reqwest::Client,
    issuer: &str,
) -> Result<AuthorizationServerMetadata, OAuthClientError> {
    let candidates = authorization_server_wellknown_candidates(issuer)?;
    let mut last_error = String::from("no candidate URLs");
    for candidate in &candidates {
        match fetch_json::<AuthorizationServerMetadata>(http, candidate).await {
            Ok(meta) => {
                // RFC 8414 §3.3 / OIDC Discovery §4.3: reject impersonation.
                if meta.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
                    return Err(OAuthClientError::Discovery(format!(
                        "authorization server metadata issuer mismatch: document says {}, expected {issuer}",
                        meta.issuer
                    )));
                }
                // MCP MUST: no advertised PKCE ⇒ refuse to proceed.
                let has_pkce = meta
                    .code_challenge_methods_supported
                    .as_ref()
                    .is_some_and(|m| !m.is_empty());
                if !has_pkce {
                    return Err(OAuthClientError::PkceUnsupported);
                }
                return Ok(meta);
            }
            Err(e) => last_error = format!("{candidate}: {e}"),
        }
    }
    Err(OAuthClientError::Discovery(format!(
        "authorization server metadata unavailable ({last_error})"
    )))
}

/// The metadata endpoints for `issuer` in the MCP-mandated priority order.
///
/// With a path component: OAuth path-insertion, OIDC path-insertion, OIDC
/// path-appending. Without: OAuth, then OIDC.
fn authorization_server_wellknown_candidates(
    issuer: &str,
) -> Result<Vec<String>, OAuthClientError> {
    let url = Url::parse(issuer)
        .map_err(|e| OAuthClientError::Discovery(format!("invalid issuer URL: {e}")))?;
    let origin = format!(
        "{}://{}",
        url.scheme(),
        url.host_str().map_or_else(String::new, |h| {
            url.port()
                .map_or_else(|| h.to_owned(), |p| format!("{h}:{p}"))
        })
    );
    let path = url.path().trim_end_matches('/');
    Ok(if path.is_empty() {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server"),
            format!("{origin}/.well-known/openid-configuration"),
        ]
    } else {
        vec![
            format!("{origin}/.well-known/oauth-authorization-server{path}"),
            format!("{origin}/.well-known/openid-configuration{path}"),
            format!("{origin}{path}/.well-known/openid-configuration"),
        ]
    })
}

async fn fetch_json<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
) -> Result<T, String> {
    let resp = http
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<T>().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_wellknown_order_prefers_path_insertion() {
        let c = protected_resource_wellknown_candidates("https://example.com/public/mcp").unwrap();
        assert_eq!(
            c,
            vec![
                "https://example.com/.well-known/oauth-protected-resource/public/mcp",
                "https://example.com/.well-known/oauth-protected-resource",
            ]
        );
        let c = protected_resource_wellknown_candidates("https://example.com").unwrap();
        assert_eq!(
            c,
            vec!["https://example.com/.well-known/oauth-protected-resource"]
        );
    }

    #[test]
    fn as_wellknown_order_matches_the_spec() {
        // With a path component: OAuth insertion, OIDC insertion, OIDC append.
        let c =
            authorization_server_wellknown_candidates("https://auth.example.com/tenant1").unwrap();
        assert_eq!(
            c,
            vec![
                "https://auth.example.com/.well-known/oauth-authorization-server/tenant1",
                "https://auth.example.com/.well-known/openid-configuration/tenant1",
                "https://auth.example.com/tenant1/.well-known/openid-configuration",
            ]
        );
        // Without: OAuth, OIDC.
        let c = authorization_server_wellknown_candidates("https://auth.example.com").unwrap();
        assert_eq!(
            c,
            vec![
                "https://auth.example.com/.well-known/oauth-authorization-server",
                "https://auth.example.com/.well-known/openid-configuration",
            ]
        );
    }

    #[test]
    fn ports_survive_candidate_construction() {
        let c = authorization_server_wellknown_candidates("http://127.0.0.1:3456").unwrap();
        assert_eq!(
            c[0],
            "http://127.0.0.1:3456/.well-known/oauth-authorization-server"
        );
    }
}
