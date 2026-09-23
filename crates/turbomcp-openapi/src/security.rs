//! Security utilities for SSRF protection.
//!
//! This module keeps HTTP requests made on behalf of an OpenAPI spec away from
//! internal/private resources, to prevent Server-Side Request Forgery (SSRF).
//!
//! Checking a URL once before sending it is not enough on its own:
//!
//! - A hostname is resolved again when the connection is made, and the second
//!   answer can differ from the one that was checked (DNS rebinding).
//! - A public host can answer with a redirect to `http://169.254.169.254/`.
//!
//! So the check runs at three points. [`SsrfGuard::check_request_target`] vets
//! the URL before the request is built. The client from
//! [`SsrfGuard::client_builder`] resolves hostnames through the guard itself,
//! so the addresses it connects to are the addresses that were validated, and
//! it re-checks every redirect hop before following it.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use ipnetwork::{Ipv4Network, Ipv6Network};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect;
use url::{Host, Url};

use crate::error::{OpenApiError, Result};

/// Redirect hops followed before giving up; reqwest's own default.
const MAX_REDIRECTS: usize = 10;

/// Blocked IPv4 ranges (private, loopback, link-local, etc.)
static BLOCKED_IPV4_RANGES: &[&str] = &[
    "0.0.0.0/8",          // "This" network
    "10.0.0.0/8",         // Private (Class A)
    "100.64.0.0/10",      // Carrier-grade NAT
    "127.0.0.0/8",        // Loopback
    "169.254.0.0/16",     // Link-local (including cloud metadata at 169.254.169.254)
    "172.16.0.0/12",      // Private (Class B)
    "192.0.0.0/24",       // IETF Protocol Assignments
    "192.0.2.0/24",       // TEST-NET-1
    "192.168.0.0/16",     // Private (Class C)
    "198.18.0.0/15",      // Network benchmark testing
    "198.51.100.0/24",    // TEST-NET-2
    "203.0.113.0/24",     // TEST-NET-3
    "224.0.0.0/4",        // Multicast
    "240.0.0.0/4",        // Reserved
    "255.255.255.255/32", // Broadcast
];

/// Blocked IPv6 ranges (loopback, link-local, private, etc.)
///
/// IPv4-mapped addresses (`::ffff:a.b.c.d`) are not listed: they are unwrapped
/// and judged by the IPv4 table instead.
static BLOCKED_IPV6_RANGES: &[&str] = &[
    "::/128",         // Unspecified: connecting to it reaches the local host
    "::1/128",        // Loopback
    "::/96",          // IPv4-compatible (deprecated), embeds an IPv4 address
    "64:ff9b::/96",   // IPv4/IPv6 translation (NAT64)
    "64:ff9b:1::/48", // Local-use IPv4/IPv6 translation
    "100::/64",       // Discard prefix
    "2001:db8::/32",  // Documentation
    "2002::/16",      // 6to4, embeds an IPv4 address
    "fe80::/10",      // Link-local
    "fc00::/7",       // Unique local addresses (private)
    "ff00::/8",       // Multicast
];

/// Where outbound requests may go.
///
/// Cheap to copy, so it can be handed to the resolver and the redirect policy
/// of every client it builds.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SsrfGuard {
    /// Let loopback addresses through, so tests can reach a local mock server.
    allow_loopback: bool,
}

impl SsrfGuard {
    /// A guard that lets loopback addresses through and nothing else new.
    #[cfg(test)]
    pub(crate) fn allowing_loopback() -> Self {
        Self {
            allow_loopback: true,
        }
    }

    /// Start a client whose connections and redirects are held to this guard.
    ///
    /// Hostnames resolve through [`Self::resolve`], so an address that failed
    /// validation is never connected to, however the name answers the second
    /// time. Each redirect target goes through [`Self::check_url`] first; its
    /// hostname then resolves through the same resolver.
    pub(crate) fn client_builder(self) -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .dns_resolver(self)
            .redirect(redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    return attempt.error("too many redirects");
                }
                match self.check_url(attempt.url()) {
                    Ok(()) => attempt.follow(),
                    Err(e) => attempt.error(e),
                }
            }))
    }

    /// Vet a URL before a request is sent to it, resolving its hostname.
    ///
    /// The client from [`Self::client_builder`] repeats the address check at
    /// connect time. This earlier pass is what still protects a request whose
    /// connection does not go through that resolver: one sent with a
    /// caller-supplied client, or through an HTTP proxy, which resolves the
    /// name itself.
    pub(crate) async fn check_request_target(self, url: &Url) -> Result<()> {
        self.check_url(url)?;
        if let Some(Host::Domain(domain)) = url.host() {
            self.resolve(domain).await?;
        }
        Ok(())
    }

    /// Check a URL's host without touching the network.
    ///
    /// Literal addresses are judged directly and names that always mean this
    /// machine are refused. Any other hostname passes here; its addresses are
    /// checked when it is resolved.
    pub(crate) fn check_url(self, url: &Url) -> Result<()> {
        match url.host() {
            None => Err(OpenApiError::SsrfBlocked("URL has no host".to_string())),
            Some(Host::Ipv4(ip)) => self.check_ip(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => self.check_ip(IpAddr::V6(ip)),
            Some(Host::Domain(host)) => {
                // A trailing dot is the same name, fully qualified.
                let host_lower = host.trim_end_matches('.').to_lowercase();
                if host_lower == "localhost"
                    || host_lower == "localhost.localdomain"
                    || host_lower.ends_with(".localhost")
                    || host_lower.ends_with(".local")
                {
                    return Err(OpenApiError::SsrfBlocked(format!(
                        "localhost hostname blocked: {}",
                        host
                    )));
                }
                Ok(())
            }
        }
    }

    /// Resolve `host` and return its addresses only if every one is allowed.
    ///
    /// Fails closed: a name that does not resolve, or resolves to nothing, is
    /// refused rather than waved through. A name with any blocked address is
    /// refused outright instead of filtered, since public and private answers
    /// for one name are exactly what a rebinding attack looks like.
    async fn resolve(self, host: &str) -> Result<Vec<SocketAddr>> {
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, 0))
            .await
            .map_err(|e| OpenApiError::SsrfBlocked(format!("could not resolve {host}: {e}")))?
            .collect();
        if addrs.is_empty() {
            return Err(OpenApiError::SsrfBlocked(format!(
                "{host} resolved to no addresses"
            )));
        }
        for addr in &addrs {
            self.check_ip(addr.ip())?;
        }
        Ok(addrs)
    }

    /// Validate that an IP address is not in a blocked range.
    fn check_ip(self, ip: IpAddr) -> Result<()> {
        if self.allow_loopback && ip.is_loopback() {
            return Ok(());
        }
        match ip {
            IpAddr::V4(ipv4) => validate_ipv4_for_ssrf(ipv4),
            IpAddr::V6(ipv6) => validate_ipv6_for_ssrf(ipv6),
        }
    }
}

impl Resolve for SsrfGuard {
    fn resolve(&self, name: Name) -> Resolving {
        let guard = *self;
        Box::pin(async move {
            let addrs = guard.resolve(name.as_str()).await?;
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

fn validate_ipv4_for_ssrf(ip: Ipv4Addr) -> Result<()> {
    for range_str in BLOCKED_IPV4_RANGES {
        if let Ok(network) = range_str.parse::<Ipv4Network>()
            && network.contains(ip)
        {
            return Err(OpenApiError::SsrfBlocked(format!(
                "IP address {} is in blocked range {}",
                ip, range_str
            )));
        }
    }
    Ok(())
}

fn validate_ipv6_for_ssrf(ip: Ipv6Addr) -> Result<()> {
    // Check for IPv4-mapped IPv6 addresses (::ffff:x.x.x.x)
    if let Some(ipv4) = ip.to_ipv4_mapped() {
        return validate_ipv4_for_ssrf(ipv4);
    }

    for range_str in BLOCKED_IPV6_RANGES {
        if let Ok(network) = range_str.parse::<Ipv6Network>()
            && network.contains(ip)
        {
            return Err(OpenApiError::SsrfBlocked(format!(
                "IP address {} is in blocked range {}",
                ip, range_str
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn check(url: &str) -> Result<()> {
        SsrfGuard::default().check_url(&url.parse().unwrap())
    }

    #[test]
    fn test_blocks_localhost() {
        let urls = [
            "http://localhost/",
            "http://localhost:8080/",
            "http://LOCALHOST/",
            "http://localhost./",
            "http://localhost.localdomain/",
            "http://test.localhost/",
        ];

        for url_str in urls {
            assert!(check(url_str).is_err(), "Should block: {}", url_str);
        }
    }

    #[test]
    fn test_blocks_private_ipv4() {
        let urls = [
            "http://127.0.0.1/",
            "http://10.0.0.1/",
            "http://172.16.0.1/",
            "http://192.168.1.1/",
            "http://169.254.169.254/", // Cloud metadata endpoint
            "http://100.64.0.1/",      // Carrier-grade NAT
            "http://0.0.0.0/",
            "http://0x7f.1/", // 127.0.0.1, spelled the way a URL parser accepts it
        ];

        for url_str in urls {
            assert!(check(url_str).is_err(), "Should block: {}", url_str);
        }
    }

    #[test]
    fn test_blocks_special_ipv6() {
        let urls = [
            "http://[::1]/",
            // The unspecified address connects to the local host. It used to
            // pass: `host_str` keeps the brackets, so the literal never parsed
            // as an address and fell through to a DNS lookup that allowed it.
            "http://[::]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[::ffff:169.254.169.254]/",
            "http://[::127.0.0.1]/",
            "http://[64:ff9b::a00:1]/",
            "http://[fe80::1]/",
            "http://[fd00::1]/",
        ];

        for url_str in urls {
            assert!(check(url_str).is_err(), "Should block: {}", url_str);
        }
    }

    #[test]
    fn test_allows_public_ips() {
        // These IPs should pass SSRF validation (they're public)
        let urls = [
            "http://8.8.8.8/",                // Google DNS
            "http://1.1.1.1/",                // Cloudflare DNS
            "http://208.67.222.222/",         // OpenDNS
            "http://[2606:4700:4700::1111]/", // Cloudflare DNS over IPv6
            "http://[::ffff:8.8.8.8]/",       // A public address, IPv4-mapped
        ];

        for url_str in urls {
            assert!(check(url_str).is_ok(), "Should allow: {}", url_str);
        }
    }

    #[tokio::test]
    async fn test_resolver_refuses_names_that_resolve_to_blocked_addresses() {
        // The connect-time resolver is the check that DNS rebinding cannot get
        // past: it hands reqwest only addresses it has just validated.
        let result =
            Resolve::resolve(&SsrfGuard::default(), Name::from_str("localhost").unwrap()).await;
        let err = result
            .err()
            .expect("localhost must not resolve")
            .to_string();
        assert!(err.contains("blocked range"), "{err}");
    }

    #[tokio::test]
    async fn test_unresolvable_host_fails_closed() {
        // `.invalid` is reserved never to resolve (RFC 2606). A lookup failure
        // used to be treated as "probably an external host that is down".
        let url: Url = "http://ssrf-check.invalid/".parse().unwrap();
        let result = SsrfGuard::default().check_request_target(&url).await;
        assert!(
            matches!(result, Err(OpenApiError::SsrfBlocked(_))),
            "{result:?}"
        );
    }

    #[test]
    fn test_loopback_allowance_is_only_loopback() {
        let guard = SsrfGuard::allowing_loopback();
        assert!(
            guard
                .check_url(&"http://127.0.0.1:8080/".parse().unwrap())
                .is_ok()
        );
        assert!(guard.check_url(&"http://[::1]/".parse().unwrap()).is_ok());
        assert!(
            guard
                .check_url(&"http://10.0.0.1/".parse().unwrap())
                .is_err()
        );
        assert!(
            guard
                .check_url(&"http://169.254.169.254/".parse().unwrap())
                .is_err()
        );
    }
}
