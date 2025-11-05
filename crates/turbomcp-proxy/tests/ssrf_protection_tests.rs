//! SSRF Protection Tests for turbomcp-proxy
//!
//! Comprehensive security tests for Server-Side Request Forgery (SSRF) protection
//! covering HTTP and WebSocket backends with IPv4 and IPv6 validation.

use ipnetwork::IpNetwork;
use std::str::FromStr;
use turbomcp_proxy::config::{BackendValidationConfig, SsrfProtection};
use turbomcp_proxy::runtime::RuntimeProxyBuilder;

// Helper to create test builder with validation config
fn test_builder_with_validation(validation: BackendValidationConfig) -> RuntimeProxyBuilder {
    RuntimeProxyBuilder::new()
        .with_backend_validation(validation)
        .with_http_frontend("127.0.0.1:3000")
}

/// Test IpNetwork CIDR parsing and contains logic
mod ip_network_tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn test_ipv4_network_contains() {
        let network = IpNetwork::from_str("10.0.0.0/8").unwrap();

        // Should contain IPs in 10.0.0.0/8
        assert!(network.contains("10.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(network.contains("10.255.255.255".parse::<IpAddr>().unwrap()));
        assert!(network.contains("10.128.0.0".parse::<IpAddr>().unwrap()));

        // Should not contain IPs outside range
        assert!(!network.contains("11.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(!network.contains("192.168.1.1".parse::<IpAddr>().unwrap()));
        assert!(!network.contains("9.255.255.255".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn test_ipv4_network_24_prefix() {
        let network = IpNetwork::from_str("192.168.1.0/24").unwrap();

        // Should contain IPs in 192.168.1.0/24
        assert!(network.contains("192.168.1.0".parse::<IpAddr>().unwrap()));
        assert!(network.contains("192.168.1.255".parse::<IpAddr>().unwrap()));
        assert!(network.contains("192.168.1.128".parse::<IpAddr>().unwrap()));

        // Should not contain IPs outside range
        assert!(!network.contains("192.168.2.0".parse::<IpAddr>().unwrap()));
        assert!(!network.contains("192.168.0.255".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn test_ipv6_network_contains() {
        let network = IpNetwork::from_str("fc00::/7").unwrap();

        // Should contain IPs in fc00::/7 (unique local)
        assert!(network.contains("fc00::1".parse::<IpAddr>().unwrap()));
        assert!(network.contains("fd00::1".parse::<IpAddr>().unwrap()));
        assert!(
            network.contains(
                "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"
                    .parse::<IpAddr>()
                    .unwrap()
            )
        );

        // Should not contain IPs outside range
        assert!(!network.contains("fe00::1".parse::<IpAddr>().unwrap()));
        assert!(!network.contains("2001:db8::1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn test_ipv6_link_local_network() {
        let network = IpNetwork::from_str("fe80::/10").unwrap();

        // Should contain link-local IPs
        assert!(network.contains("fe80::1".parse::<IpAddr>().unwrap()));
        assert!(network.contains("feb0::1".parse::<IpAddr>().unwrap()));

        // Should not contain non-link-local
        assert!(!network.contains("fec0::1".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn test_from_cidr() {
        let network = IpNetwork::from_str("10.0.0.0/8").unwrap();
        assert_eq!(network.prefix(), 8);
        assert!(network.contains("10.1.2.3".parse::<IpAddr>().unwrap()));

        let network = IpNetwork::from_str("192.168.1.0/24").unwrap();
        assert_eq!(network.prefix(), 24);
        assert!(network.contains("192.168.1.100".parse::<IpAddr>().unwrap()));
    }

    #[test]
    fn test_invalid_prefix_length() {
        // IPv4 prefix too large
        assert!(IpNetwork::from_str("10.0.0.0/33").is_err());

        // IPv6 prefix too large
        assert!(IpNetwork::from_str("fc00::/129").is_err());
    }

    #[test]
    fn test_different_ip_versions_dont_match() {
        let ipv4_network = IpNetwork::from_str("10.0.0.0/8").unwrap();
        let ipv6_addr = "fe80::1".parse::<IpAddr>().unwrap();

        // IPv6 address should not match IPv4 network
        assert!(!ipv4_network.contains(ipv6_addr));
    }
}

/// Test Strict SSRF protection (default)
mod strict_ssrf_tests {
    use super::*;

    #[tokio::test]
    async fn test_strict_blocks_private_ipv4() {
        let validation = BackendValidationConfig::default(); // Strict by default

        // 10.0.0.0/8 - use https so we get past scheme check to SSRF check
        let result = test_builder_with_validation(validation.clone())
            .with_http_backend("https://10.0.0.1:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Private") || err.to_string().contains("blocked"));

        // 192.168.0.0/16
        let result = test_builder_with_validation(validation.clone())
            .with_http_backend("https://192.168.1.1:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Private") || err.to_string().contains("blocked"));

        // 172.16.0.0/12
        let result = test_builder_with_validation(validation.clone())
            .with_http_backend("https://172.16.0.1:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Private") || err.to_string().contains("blocked"));
    }

    #[tokio::test]
    async fn test_strict_blocks_link_local() {
        let validation = BackendValidationConfig::default();

        // 169.254.0.0/16 (link-local)
        let result = test_builder_with_validation(validation)
            .with_http_backend("https://169.254.1.1:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("Private IPv4"));
    }

    #[tokio::test]
    async fn test_strict_blocks_aws_metadata() {
        let validation = BackendValidationConfig::default();

        let result = test_builder_with_validation(validation)
            .with_http_backend("https://169.254.169.254", None)
            .build()
            .await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("metadata endpoint")
        );
    }

    #[tokio::test]
    async fn test_strict_blocks_azure_metadata() {
        let validation = BackendValidationConfig::default();

        let result = test_builder_with_validation(validation)
            .with_http_backend("https://168.63.129.16", None)
            .build()
            .await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("metadata endpoint")
        );
    }

    #[tokio::test]
    async fn test_strict_blocks_private_ipv6() {
        let validation = BackendValidationConfig::default();

        // Unique local address (ULA) - fc00::/7
        let result = test_builder_with_validation(validation.clone())
            .with_http_backend("https://[fc00::1]:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        println!("Error for fc00::1: {}", err);
        // Check for Private or blocked
        assert!(err.to_string().contains("Private") || err.to_string().contains("blocked"));

        // Link-local address - fe80::/10
        let result = test_builder_with_validation(validation)
            .with_http_backend("https://[fe80::1]:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        println!("Error for fe80::1: {}", err);
        assert!(err.to_string().contains("Private") || err.to_string().contains("blocked"));
    }

    #[tokio::test]
    async fn test_strict_allows_localhost() {
        let validation = BackendValidationConfig::default();

        // IPv4 loopback
        let result = test_builder_with_validation(validation.clone())
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_backend("http://127.0.0.1:8080", None)
            .build()
            .await;
        // Should fail on connection, not on validation
        assert!(result.is_err());
        // But error should NOT be about private IP
        let err = result.err().unwrap();
        assert!(!err.to_string().contains("Private"));

        // IPv6 loopback
        let result = test_builder_with_validation(validation)
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_backend("http://[::1]:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(!err.to_string().contains("Private"));
    }

    #[tokio::test]
    async fn test_strict_allows_public_ips() {
        let validation = BackendValidationConfig::default();

        // Public IPv4 (Google DNS)
        let result = test_builder_with_validation(validation)
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_backend("http://8.8.8.8:8080", None)
            .build()
            .await;
        // Should fail on connection, not validation
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(!err.to_string().contains("Private"));
        assert!(!err.to_string().contains("blocked"));
    }
}

/// Test WebSocket SSRF protection
mod websocket_ssrf_tests {
    use super::*;

    #[tokio::test]
    async fn test_websocket_blocks_private_ipv4() {
        let validation = BackendValidationConfig::default();

        let result = test_builder_with_validation(validation)
            .with_websocket_backend("wss://10.0.0.1:8080")
            .build()
            .await;
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("Private IPv4"));
    }

    #[tokio::test]
    async fn test_websocket_blocks_aws_metadata() {
        let validation = BackendValidationConfig::default();

        let result = test_builder_with_validation(validation)
            .with_websocket_backend("wss://169.254.169.254")
            .build()
            .await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("metadata endpoint")
        );
    }

    #[tokio::test]
    async fn test_websocket_blocks_private_ipv6() {
        let validation = BackendValidationConfig::default();

        let result = test_builder_with_validation(validation)
            .with_websocket_backend("wss://[fc00::1]:8080")
            .build()
            .await;
        assert!(result.is_err());
        assert!(result.err().unwrap().to_string().contains("Private IPv6"));
    }

    #[tokio::test]
    async fn test_websocket_requires_wss_for_non_localhost() {
        let validation = BackendValidationConfig::default();

        // ws:// with public IP should fail (requires wss://)
        let result = test_builder_with_validation(validation)
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_websocket_backend("ws://8.8.8.8:8080")
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Secure protocol required"));
    }

    #[tokio::test]
    async fn test_websocket_allows_ws_for_localhost() {
        let validation = BackendValidationConfig::default();

        let result = test_builder_with_validation(validation)
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_websocket_backend("ws://127.0.0.1:8080")
            .build()
            .await;
        // Should fail on connection, not validation
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(!err.to_string().contains("Secure protocol"));
    }
}

/// Test Balanced SSRF protection
mod balanced_ssrf_tests {
    use super::*;

    #[tokio::test]
    async fn test_balanced_allows_configured_private_networks() {
        let validation = BackendValidationConfig {
            ssrf_protection: SsrfProtection::Balanced {
                allowed_private_networks: vec![
                    IpNetwork::from_str("10.0.0.0/8").unwrap(),
                    IpNetwork::from_str("192.168.1.0/24").unwrap(),
                ],
            },
            ..Default::default()
        };

        // Should allow 10.0.0.0/8
        let result = test_builder_with_validation(validation.clone())
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_backend("http://10.1.2.3:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        // Error should NOT be about private IP blocking
        let err = result.err().unwrap();
        assert!(!err.to_string().contains("Private IP"));

        // Should allow 192.168.1.0/24
        let result = test_builder_with_validation(validation)
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_backend("https://192.168.1.100:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(!err.to_string().contains("Private IP"));
    }

    #[tokio::test]
    async fn test_balanced_blocks_non_configured_private_networks() {
        let validation = BackendValidationConfig {
            ssrf_protection: SsrfProtection::Balanced {
                allowed_private_networks: vec![IpNetwork::from_str("10.0.0.0/8").unwrap()],
            },
            ..Default::default()
        };

        // Should block 192.168.0.0/16 (not in allowlist)
        let result = test_builder_with_validation(validation)
            .with_http_backend("https://192.168.1.1:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("not in allowed networks")
        );
    }

    #[tokio::test]
    async fn test_balanced_always_blocks_cloud_metadata() {
        let validation = BackendValidationConfig {
            ssrf_protection: SsrfProtection::Balanced {
                // Even if we allow link-local, metadata should be blocked
                allowed_private_networks: vec![IpNetwork::from_str("169.254.0.0/16").unwrap()],
            },
            ..Default::default()
        };

        // AWS metadata should still be blocked
        let result = test_builder_with_validation(validation)
            .with_http_backend("https://169.254.169.254", None)
            .build()
            .await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("metadata endpoint")
        );
    }

    #[tokio::test]
    async fn test_balanced_with_ipv6_networks() {
        let validation = BackendValidationConfig {
            ssrf_protection: SsrfProtection::Balanced {
                allowed_private_networks: vec![IpNetwork::from_str("fc00::/7").unwrap()],
            },
            ..Default::default()
        };

        // Should allow fc00::/7
        let result = test_builder_with_validation(validation.clone())
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_backend("https://[fc00::1]:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(!err.to_string().contains("Private"));

        // Should block fe80::/10 (link-local, not in allowlist)
        let result = test_builder_with_validation(validation)
            .with_http_backend("https://[fe80::1]:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("not in allowed networks")
        );
    }
}

/// Test Disabled SSRF protection
mod disabled_ssrf_tests {
    use super::*;

    #[tokio::test]
    async fn test_disabled_allows_all_private_ips() {
        let validation = BackendValidationConfig {
            ssrf_protection: SsrfProtection::Disabled,
            ..Default::default()
        };

        // Should allow private IPv4
        let result = test_builder_with_validation(validation.clone())
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_backend("http://10.0.0.1:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(!err.to_string().contains("Private"));

        // Should allow metadata endpoints (not recommended!)
        let result = test_builder_with_validation(validation)
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_backend("https://169.254.169.254", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(!err.to_string().contains("metadata"));
    }
}

/// Test custom blocklist
mod custom_blocklist_tests {
    use super::*;

    #[tokio::test]
    async fn test_custom_blocklist_blocks_specified_hosts() {
        let validation = BackendValidationConfig {
            ssrf_protection: SsrfProtection::Disabled, // Disable to test blocklist only
            blocked_hosts: vec!["evil.com".to_string(), "malicious.io".to_string()],
            ..Default::default()
        };

        let result = test_builder_with_validation(validation)
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .with_http_backend("https://evil.com", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("blocked by custom blocklist"));
    }
}

/// Test scheme validation
mod scheme_validation_tests {
    use super::*;

    #[tokio::test]
    async fn test_only_allowed_schemes_permitted() {
        let validation = BackendValidationConfig {
            allowed_schemes: vec!["https".to_string(), "wss".to_string()],
            ..Default::default()
        };

        // http should be rejected
        let result = test_builder_with_validation(validation.clone())
            .with_http_backend("http://127.0.0.1:8080", None)
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Scheme 'http' not allowed"));

        // ws should be rejected
        let result = test_builder_with_validation(validation)
            .with_websocket_backend("ws://127.0.0.1:8080")
            .build()
            .await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Scheme 'ws' not allowed"));
    }
}
