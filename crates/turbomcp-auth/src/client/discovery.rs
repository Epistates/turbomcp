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
    discover_protected_resource_with_policy(
        http,
        resource_url,
        challenge_metadata_url,
        &crate::NetworkPolicy::default(),
    )
    .await
}

pub(crate) async fn discover_protected_resource_with_policy(
    http: &reqwest::Client,
    resource_url: &str,
    challenge_metadata_url: Option<&str>,
    policy: &crate::NetworkPolicy,
) -> Result<ProtectedResourceMetadata, OAuthClientError> {
    let candidates: Vec<String> = match challenge_metadata_url {
        Some(url) => vec![url.to_owned()],
        None => protected_resource_wellknown_candidates(resource_url)?,
    };
    let mut last_error = String::from("no candidate URLs");
    for candidate in &candidates {
        match fetch_json::<ProtectedResourceMetadata>(http, candidate, policy).await {
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
                // The document names where the user will be sent to authorize.
                // A plaintext entry here is refused now, so the caller never
                // gets a metadata document it might act on.
                for issuer in &meta.authorization_servers {
                    require_secure_url(issuer, "an advertised authorization server")?;
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

/// Refuse a URL the flow would carry credentials over unless it is HTTPS, or
/// plaintext to a loopback host.
///
/// The authorization spec makes this a MUST twice over: "all authorization
/// server endpoints MUST be served over HTTPS", and "all redirect URIs MUST be
/// either `localhost` or use HTTPS". Without it, a hostile or hijacked metadata
/// document can name a plaintext authorization server and the whole exchange —
/// the authorization request, the PKCE verifier, the code, the client secret,
/// and the issued token — crosses the network in the clear.
///
/// **Loopback is the one deviation**, and it is deliberate. The spec grants it
/// for redirect URIs, RFC 8252 builds native-app flows on it, and every local
/// authorization server (including the conformance harness's) is plaintext
/// `127.0.0.1`. Extending it to the endpoint rules keeps development and
/// testing working while remote plaintext stays refused, which is where the
/// attack actually lives.
pub(crate) fn require_secure_url(url: &str, what: &str) -> Result<(), OAuthClientError> {
    let insecure = || OAuthClientError::InsecureUrl {
        what: what.to_owned(),
        url: url.to_owned(),
    };
    let parsed = Url::parse(url).map_err(|_| insecure())?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if is_loopback(&parsed) => Ok(()),
        _ => Err(insecure()),
    }
}

/// Whether `url`'s host is the loopback interface. `localhost` and anything
/// under it are reserved for loopback by RFC 6761, alongside the literal
/// addresses.
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
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
    discover_authorization_server_with_policy(http, issuer, &crate::NetworkPolicy::default()).await
}

pub(crate) async fn discover_authorization_server_with_policy(
    http: &reqwest::Client,
    issuer: &str,
    policy: &crate::NetworkPolicy,
) -> Result<AuthorizationServerMetadata, OAuthClientError> {
    // Before anything is fetched: an issuer we would talk to in the clear is
    // refused outright, rather than after its metadata has been read and
    // trusted.
    require_secure_url(issuer, "the authorization server issuer")?;
    let candidates = authorization_server_wellknown_candidates(issuer)?;
    let mut last_error = String::from("no candidate URLs");
    for candidate in &candidates {
        match fetch_json::<AuthorizationServerMetadata>(http, candidate, policy).await {
            Ok(meta) => {
                // RFC 8414 §3.3 / OIDC Discovery §4.3: reject impersonation.
                if meta.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
                    return Err(OAuthClientError::Discovery(format!(
                        "authorization server metadata issuer mismatch: document says {}, expected {issuer}",
                        meta.issuer
                    )));
                }
                // An HTTPS issuer can still hand back plaintext endpoints, and
                // those are where the credentials actually go — so each is
                // checked on its own rather than inferred from the issuer.
                require_secure_url(&meta.authorization_endpoint, "the authorization endpoint")?;
                require_secure_url(&meta.token_endpoint, "the token endpoint")?;
                if let Some(registration) = &meta.registration_endpoint {
                    require_secure_url(registration, "the registration endpoint")?;
                }
                // MCP MUST: no advertised PKCE ⇒ refuse to proceed.
                let has_pkce = meta
                    .code_challenge_methods_supported
                    .as_ref()
                    .is_some_and(|m| m.iter().any(|method| method == "S256"));
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
    policy: &crate::NetworkPolicy,
) -> Result<T, String> {
    let resp = policy
        .send(http, http.get(url).header("accept", "application/json"))
        .await?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    serde_json::from_slice(&policy.body(resp).await?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point: a remote plaintext endpoint is refused. Without this,
    /// a hijacked metadata document can name `http://evil.example` and the
    /// authorization request, PKCE verifier, code, client secret, and issued
    /// token all cross the network readable.
    #[test]
    fn remote_plaintext_urls_are_refused() {
        for url in [
            "http://as.example.com",
            "http://as.example.com/token",
            "http://192.0.2.10/token",
            "http://[2001:db8::1]/token",
            // A loopback-looking name that is not loopback.
            "http://localhost.evil.example/token",
            "http://notlocalhost/token",
        ] {
            assert!(
                matches!(
                    require_secure_url(url, "the token endpoint"),
                    Err(OAuthClientError::InsecureUrl { .. })
                ),
                "{url} should have been refused"
            );
        }
    }

    /// HTTPS anywhere, and plaintext only to the loopback interface — the
    /// deviation that keeps native-app flows (RFC 8252) and every local
    /// authorization server working.
    #[test]
    fn https_and_loopback_are_allowed() {
        for url in [
            "https://as.example.com/token",
            "https://192.0.2.10/token",
            "http://localhost:7777/cb",
            "http://localhost./cb",
            "http://LOCALHOST:7777/cb",
            "http://app.localhost/cb",
            "http://127.0.0.1:3456/cb",
            "http://127.9.9.9/cb",
            "http://[::1]:7777/cb",
        ] {
            assert!(
                require_secure_url(url, "the redirect URI").is_ok(),
                "{url} should have been allowed"
            );
        }
    }

    /// A non-HTTP scheme is not a loophole. These parse fine and would
    /// otherwise fall through to whatever the caller does with the URL — the
    /// best-practices document names `javascript:`, `data:`, `file:`, and
    /// `vbscript:` specifically, because an authorization URL is one a client
    /// is expected to hand to a browser. Allowlisting `https` (plus loopback
    /// `http`) refuses all of them and anything else invented later, which is
    /// why the check is shaped that way rather than as a blocklist.
    #[test]
    fn other_schemes_and_junk_are_refused() {
        for url in [
            "javascript:alert(1)",
            "vbscript:msgbox(1)",
            "file:///etc/passwd",
            "ftp://as.example.com/",
            "data:text/plain,hi",
            "not a url",
            "",
        ] {
            assert!(
                require_secure_url(url, "the issuer").is_err(),
                "{url:?} should have been refused"
            );
        }
    }

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
