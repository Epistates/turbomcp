//! Origin header validation for DNS rebinding protection
//!
//! This module implements critical origin validation required by MCP specification
//! to prevent DNS rebinding attacks. It provides flexible configuration for
//! development, staging, and production environments.

use super::errors::SecurityError;
use super::utils::get_header_case_insensitive;
use crate::security::SecurityHeaders;
use std::collections::HashSet;
use std::net::Ipv4Addr;
use url::{Host, Url};

/// Canonicalize an origin string into the comparable triple
/// `(scheme, host, effective_port)`.
///
/// - Schemes are lowercased.
/// - Hostnames are converted via the URL parser, which already handles
///   Punycode/IDN canonicalization and case folding (`Example.com` →
///   `example.com`).
/// - Ports are filled in from the scheme's default if absent (`https://h` →
///   port 443) so `https://example.com` matches `https://example.com:443`.
///
/// Returns `None` for inputs that don't parse as a URL or aren't true origins
/// (no host, or path/query/fragment present — RFC 6454 origins are bare).
pub(crate) fn canonicalize_origin(input: &str) -> Option<(String, String, u16)> {
    let url = Url::parse(input.trim()).ok()?;
    // Reject origins that carry path / query / fragment / userinfo —
    // `Url::parse("http://localhost.evil.com")` is the kind of input the old
    // `starts_with` check accepted. We want only `<scheme>://<host>[:port]`.
    if url.path() != "/" && !url.path().is_empty() {
        return None;
    }
    if url.query().is_some() || url.fragment().is_some() || !url.username().is_empty() {
        return None;
    }
    let scheme = url.scheme().to_ascii_lowercase();
    let host = match url.host()? {
        Host::Domain(d) => d.to_ascii_lowercase(),
        Host::Ipv4(ip) => ip.to_string(),
        Host::Ipv6(ip) => format!("[{}]", ip),
    };
    let port = url.port_or_known_default()?;
    Some((scheme, host, port))
}

/// True iff a *parsed* origin refers to a loopback host on http/https.
fn is_loopback_origin_parsed(scheme: &str, host: &str) -> bool {
    if scheme != "http" && scheme != "https" {
        return false;
    }
    if host == "localhost" {
        return true;
    }
    if let Ok(v4) = host.parse::<Ipv4Addr>() {
        return v4.is_loopback();
    }
    if host.starts_with('[')
        && host.ends_with(']')
        && let Ok(v6) = host[1..host.len() - 1].parse::<std::net::Ipv6Addr>()
    {
        return v6.is_loopback();
    }
    false
}

/// Origin validation configuration.
///
/// # Production deployments must override the default
///
/// `Default` is **dev-only**: it sets `allow_localhost = true`. In production,
/// always supply an explicit `OriginConfig` with `allow_localhost: false` and an
/// explicit `allowed_origins` allowlist (the public origins the browser-facing
/// client will use). Behind a reverse proxy, also ensure your `client_ip`
/// extraction reflects the *real* client and not the proxy hop, otherwise the
/// loopback heuristic can be satisfied by the proxy's local connection.
#[derive(Clone, Debug)]
pub struct OriginConfig {
    /// Allowed origins for CORS
    pub allowed_origins: HashSet<String>,
    /// Whether to allow localhost origins (for development).
    ///
    /// **Production should set this to `false`** and rely on `allowed_origins`.
    pub allow_localhost: bool,
    /// Whether to allow any origin (DANGEROUS - only for testing)
    pub allow_any: bool,
    /// Whether to accept a request that carries no `Origin` header from a
    /// non-loopback client.
    ///
    /// Browsers attach `Origin` to the cross-origin and state-changing
    /// requests DNS rebinding depends on; a CLI, an agent runtime, or another
    /// server calling over the network sends none. Without this, the only way
    /// to admit such a client was `allow_any`, which also stops checking the
    /// browsers. A request that *does* carry an `Origin` is still validated
    /// against the rest of this policy — this relaxes absence, never a bad
    /// value. Pair it with authentication.
    pub allow_missing: bool,
}

impl Default for OriginConfig {
    /// Returns a development-friendly configuration: `allow_localhost = true`,
    /// no public origins allowlisted. **Do not use this directly in production**
    /// — provide an explicit allowlist instead.
    fn default() -> Self {
        Self {
            allowed_origins: HashSet::new(),
            allow_localhost: true,
            allow_any: false,
            allow_missing: false,
        }
    }
}

impl OriginConfig {
    /// Create a new origin configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Production-safe constructor: requires an explicit allowlist and disables
    /// the localhost fallback. Use this instead of [`Self::default`] / [`Self::new`]
    /// when building servers that face the public internet.
    pub fn production(allowed_origins: impl IntoIterator<Item = String>) -> Self {
        Self {
            allowed_origins: allowed_origins.into_iter().collect(),
            allow_localhost: false,
            allow_any: false,
            allow_missing: false,
        }
    }

    /// Returns `true` when this config has no real allowlist and is therefore
    /// only safe for local development. Servers that observe `is_dev_permissive`
    /// at startup should refuse to bind to a non-loopback address (or at least
    /// log a `WARN`).
    pub fn is_dev_permissive(&self) -> bool {
        !self.allow_any && self.allow_localhost && self.allowed_origins.is_empty()
    }

    /// Add an allowed origin
    pub fn add_origin(&mut self, origin: String) {
        self.allowed_origins.insert(origin);
    }

    /// Add multiple allowed origins
    pub fn add_origins(&mut self, origins: Vec<String>) {
        self.allowed_origins.extend(origins);
    }

    /// Set whether to allow localhost origins
    pub fn set_allow_localhost(&mut self, allow: bool) {
        self.allow_localhost = allow;
    }

    /// Set whether to allow any origin (use with extreme caution)
    pub fn set_allow_any(&mut self, allow: bool) {
        self.allow_any = allow;
    }
}

/// Check one `Origin` value against the policy.
///
/// The single statement of what this policy accepts, shared by
/// [`validate_origin`] and by anything that has to answer the same question
/// for a browser — a CORS layer, say — so the two can never disagree about
/// which origins are trusted.
pub fn validate_origin_value(config: &OriginConfig, origin: &str) -> Result<(), SecurityError> {
    if config.allow_any {
        return Ok(());
    }

    // Parse + canonicalize the inbound origin. Anything that doesn't
    // round-trip through the URL parser as a bare origin is rejected
    // outright — that catches `http://localhost.evil.com`,
    // `http://localhost@evil.com`, paths, queries, and userinfo.
    let canonical = canonicalize_origin(origin).ok_or_else(|| {
        SecurityError::InvalidOrigin(format!("Origin '{}' is not a valid origin", origin))
    })?;

    // Match against the configured allowlist using canonical form so
    // `https://Example.com` and `https://example.com:443` collide.
    if config
        .allowed_origins
        .iter()
        .filter_map(|entry| canonicalize_origin(entry))
        .any(|c| c == canonical)
    {
        return Ok(());
    }

    // Allow localhost origins for development.
    if config.allow_localhost && is_loopback_origin_parsed(&canonical.0, &canonical.1) {
        return Ok(());
    }

    Err(SecurityError::InvalidOrigin(format!(
        "Origin '{}' not allowed",
        origin
    )))
}

/// Validate Origin header to prevent DNS rebinding attacks
///
/// Per the current MCP specification:
/// "Servers MUST validate the Origin header on all incoming connections
/// to prevent DNS rebinding attacks"
///
/// **Security Model**:
/// - DNS rebinding attacks require remote→localhost connections
/// - localhost→localhost connections are inherently safe (no DNS involved)
/// - If Origin header missing BUT client is localhost → allow (Claude Code case)
/// - If Origin header missing AND client is remote → reject, unless
///   [`OriginConfig::allow_missing`] admits non-browser clients
/// - If Origin header present → checked by [`validate_origin_value`] whatever
///   the client address, so a bad value is always refused
pub fn validate_origin(
    config: &OriginConfig,
    headers: &SecurityHeaders,
    client_ip: std::net::IpAddr,
) -> Result<(), SecurityError> {
    // Check if Origin header exists (case-insensitive per HTTP spec)
    match get_header_case_insensitive(headers, "Origin") {
        Some(origin) => validate_origin_value(config, origin),
        None => {
            // Origin missing → check if client is localhost
            // DNS rebinding attacks require remote clients, so localhost clients are safe
            if client_ip.is_loopback() {
                // localhost→localhost: No DNS rebinding risk, allow it
                // This enables Claude Code and other local clients
                return Ok(());
            }

            if config.allow_any || config.allow_missing {
                return Ok(());
            }

            // Remote client without Origin → potential security risk
            Err(SecurityError::InvalidOrigin(
                "Missing Origin header from remote client".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_origin_config_default() {
        let config = OriginConfig::default();
        assert!(config.allow_localhost);
        assert!(!config.allow_any);
        assert!(config.allowed_origins.is_empty());
    }

    #[test]
    fn test_validate_origin_allows_localhost() {
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://localhost:3000".to_string());
        let client_ip = "127.0.0.1".parse().unwrap();

        assert!(validate_origin(&config, &headers, client_ip).is_ok());
    }

    #[test]
    fn test_validate_origin_blocks_evil_origin() {
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://evil.com".to_string());
        let client_ip = "192.168.1.100".parse().unwrap();

        assert!(validate_origin(&config, &headers, client_ip).is_err());
    }

    #[test]
    fn test_validate_origin_allows_configured_origin() {
        let config = OriginConfig {
            allowed_origins: vec!["https://trusted.com".to_string()]
                .into_iter()
                .collect(),
            allow_localhost: false,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "https://trusted.com".to_string());
        let client_ip = "192.168.1.100".parse().unwrap();

        assert!(validate_origin(&config, &headers, client_ip).is_ok());
    }

    #[test]
    fn test_validate_origin_missing_header_localhost_client() {
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let headers = HashMap::new();
        let client_ip = "127.0.0.1".parse().unwrap();

        // localhost→localhost without Origin → allowed (Claude Code case)
        assert!(validate_origin(&config, &headers, client_ip).is_ok());
    }

    #[test]
    fn test_validate_origin_missing_header_remote_client() {
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let headers = HashMap::new();
        let client_ip = "192.168.1.100".parse().unwrap();

        // remote→localhost without Origin → blocked (security)
        assert!(validate_origin(&config, &headers, client_ip).is_err());
    }

    /// A remote non-browser client sends no `Origin`. `allow_missing` admits it
    /// without switching off validation for the requests that do carry one.
    #[test]
    fn allow_missing_admits_absence_but_still_rejects_a_bad_origin() {
        let config = OriginConfig {
            allowed_origins: ["https://app.example".to_string()].into_iter().collect(),
            allow_localhost: false,
            allow_any: false,
            allow_missing: true,
        };
        let remote = "203.0.113.5".parse().unwrap();

        assert!(validate_origin(&config, &HashMap::new(), remote).is_ok());

        let mut evil = HashMap::new();
        evil.insert("Origin".to_string(), "https://evil.example".to_string());
        assert!(
            validate_origin(&config, &evil, remote).is_err(),
            "a present, disallowed Origin is refused whatever allow_missing says"
        );

        let mut good = HashMap::new();
        good.insert("origin".to_string(), "https://app.example".to_string());
        assert!(validate_origin(&config, &good, remote).is_ok());
    }

    #[test]
    fn test_validate_origin_allow_any() {
        let config = OriginConfig {
            allow_localhost: true,
            allow_any: true,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert("Origin".to_string(), "http://anything.com".to_string());
        let client_ip = "192.168.1.100".parse().unwrap();

        assert!(validate_origin(&config, &headers, client_ip).is_ok());
    }

    #[test]
    fn test_validate_origin_rejects_localhost_lookalike_subdomain() {
        // `starts_with` would accept this; the URL parser sees the actual host.
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert(
            "Origin".to_string(),
            "http://localhost.evil.com".to_string(),
        );
        let client_ip = "192.168.1.100".parse().unwrap();
        assert!(validate_origin(&config, &headers, client_ip).is_err());
    }

    #[test]
    fn test_validate_origin_rejects_userinfo_smuggle() {
        // `http://localhost@evil.com` has host `evil.com`; userinfo is stripped.
        // Our canonicalizer rejects any origin with userinfo outright.
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let mut headers = HashMap::new();
        headers.insert(
            "Origin".to_string(),
            "http://localhost@evil.com".to_string(),
        );
        let client_ip = "192.168.1.100".parse().unwrap();
        assert!(validate_origin(&config, &headers, client_ip).is_err());
    }

    #[test]
    fn test_validate_origin_canonicalizes_case_and_default_port() {
        let mut allowed = HashSet::new();
        allowed.insert("https://example.com".to_string());
        let config = OriginConfig {
            allowed_origins: allowed,
            allow_localhost: false,
            allow_any: false,
            allow_missing: false,
        };
        let client_ip = "192.168.1.100".parse().unwrap();

        // Mixed case host + explicit default port should both match.
        for incoming in ["https://Example.com", "https://example.com:443"] {
            let mut headers = HashMap::new();
            headers.insert("Origin".to_string(), incoming.to_string());
            assert!(
                validate_origin(&config, &headers, client_ip).is_ok(),
                "expected {incoming} to be canonicalized to https://example.com"
            );
        }
    }

    #[test]
    fn test_validate_origin_rejects_path_or_query() {
        // RFC 6454 Origins are bare; anything with a path/query is suspect.
        let config = OriginConfig {
            allow_localhost: true,
            ..Default::default()
        };
        let client_ip = "127.0.0.1".parse().unwrap();
        for sneaky in [
            "http://localhost/admin",
            "http://localhost:3000/x",
            "http://localhost?x=1",
        ] {
            let mut headers = HashMap::new();
            headers.insert("Origin".to_string(), sneaky.to_string());
            assert!(
                validate_origin(&config, &headers, client_ip).is_err(),
                "expected origin '{sneaky}' to be rejected"
            );
        }
    }
}
