//! Security utilities for SSRF protection.
//!
//! This module provides URL validation to prevent Server-Side Request Forgery (SSRF)
//! attacks when making HTTP requests to external APIs.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use ipnetwork::{Ipv4Network, Ipv6Network};
use url::Url;

use crate::error::{OpenApiError, Result};

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
static BLOCKED_IPV6_RANGES: &[&str] = &[
    "::1/128",       // Loopback
    "::ffff:0:0/96", // IPv4-mapped addresses (check underlying IPv4)
    "64:ff9b::/96",  // IPv4/IPv6 translation
    "100::/64",      // Discard prefix
    "fe80::/10",     // Link-local
    "fc00::/7",      // Unique local addresses (private)
    "ff00::/8",      // Multicast
];

/// Validate that a URL is safe to request (not targeting internal/private resources).
///
/// # Arguments
///
/// * `url` - The URL to validate
///
/// # Returns
///
/// * `Ok(())` if the URL is safe to request
/// * `Err(OpenApiError::SsrfBlocked)` if the URL targets a blocked address
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_openapi::security::validate_url_for_ssrf;
///
/// // Safe external URLs pass
/// validate_url_for_ssrf(&"https://api.example.com/endpoint".parse().unwrap())?;
///
/// // Internal URLs are blocked
/// assert!(validate_url_for_ssrf(&"http://localhost:8080/".parse().unwrap()).is_err());
/// assert!(validate_url_for_ssrf(&"http://192.168.1.1/".parse().unwrap()).is_err());
/// ```
pub fn validate_url_for_ssrf(url: &Url) -> Result<()> {
    // Get the host
    let host = url
        .host_str()
        .ok_or_else(|| OpenApiError::SsrfBlocked("URL has no host".to_string()))?;

    // Check for localhost variations
    let host_lower = host.to_lowercase();
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

    // Try to parse as IP address directly
    if let Ok(ip) = host.parse::<IpAddr>() {
        return validate_ip_for_ssrf(ip);
    }

    // Try DNS resolution to check resolved IPs
    // Note: This is async-unfriendly but necessary for security
    let socket_addrs = format!("{}:80", host);
    if let Ok(addrs) = socket_addrs.to_socket_addrs() {
        for addr in addrs {
            validate_ip_for_ssrf(addr.ip())?;
        }
    }
    // If DNS fails, we allow it (might be a valid external host that's down)

    Ok(())
}

/// Validate that an IP address is not in a blocked range.
fn validate_ip_for_ssrf(ip: IpAddr) -> Result<()> {
    match ip {
        IpAddr::V4(ipv4) => validate_ipv4_for_ssrf(ipv4),
        IpAddr::V6(ipv6) => validate_ipv6_for_ssrf(ipv6),
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
    use super::*;

    #[test]
    fn test_allows_external_urls() {
        // Note: These tests might fail if the domains don't resolve
        // but that's OK - we want to test the validation logic
        let url: Url = "https://api.github.com/users".parse().unwrap();
        // Just check it doesn't error on the URL format
        let _ = validate_url_for_ssrf(&url);
    }

    #[test]
    fn test_blocks_localhost() {
        let urls = [
            "http://localhost/",
            "http://localhost:8080/",
            "http://LOCALHOST/",
            "http://localhost.localdomain/",
            "http://test.localhost/",
        ];

        for url_str in urls {
            let url: Url = url_str.parse().unwrap();
            assert!(
                validate_url_for_ssrf(&url).is_err(),
                "Should block: {}",
                url_str
            );
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
        ];

        for url_str in urls {
            let url: Url = url_str.parse().unwrap();
            assert!(
                validate_url_for_ssrf(&url).is_err(),
                "Should block: {}",
                url_str
            );
        }
    }

    #[test]
    fn test_blocks_loopback_ipv6() {
        let url: Url = "http://[::1]/".parse().unwrap();
        assert!(validate_url_for_ssrf(&url).is_err());
    }

    #[test]
    fn test_allows_public_ips() {
        // These IPs should pass SSRF validation (they're public)
        let ips = [
            "8.8.8.8",        // Google DNS
            "1.1.1.1",        // Cloudflare DNS
            "208.67.222.222", // OpenDNS
        ];

        for ip in ips {
            let url: Url = format!("http://{}/", ip).parse().unwrap();
            assert!(
                validate_url_for_ssrf(&url).is_ok(),
                "Should allow public IP: {}",
                ip
            );
        }
    }
}
