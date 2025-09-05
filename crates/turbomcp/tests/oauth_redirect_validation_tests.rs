//! Comprehensive tests for OAuth redirect URI validation security enhancements
//!
//! This test module validates the comprehensive security features added to prevent
//! open redirect attacks and enforce production security standards.

use std::collections::HashMap;
use turbomcp::auth::OAuth2Client;
use turbomcp::auth::{OAuth2Config, OAuth2FlowType, ProviderType, SecurityLevel};

/// Test basic redirect URI validation functionality
#[test]
fn test_valid_redirect_uris() {
    let valid_uris = vec![
        "http://localhost:8080/callback",
        "http://127.0.0.1:3000/auth",
        // Note: External domains require explicit whitelisting for security
    ];

    for uri in valid_uris {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            redirect_uri: uri.to_string(),
            scopes: vec!["read".to_string()],
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        // Should succeed for valid URIs
        let result = OAuth2Client::new(&config, ProviderType::Generic);
        assert!(
            result.is_ok(),
            "Failed to create client with valid URI: {}",
            uri
        );
    }
}

/// Test redirect URI validation rejects suspicious patterns
#[test]
fn test_suspicious_redirect_uris_rejected() {
    let suspicious_uris = vec![
        "http://evil.com/../admin/callback",            // Path traversal
        "https://example.com//malicious",               // Double slash
        "http://site.com/callback?javascript:alert(1)", // JavaScript injection
        "https://domain.com/auth?data:text/html,<script>alert(1)</script>", // Data URL injection
    ];

    for uri in suspicious_uris {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            redirect_uri: uri.to_string(),
            scopes: vec!["read".to_string()],
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        // Should fail for suspicious URIs
        let result = OAuth2Client::new(&config, ProviderType::Generic);
        assert!(result.is_err(), "Should reject suspicious URI: {}", uri);
    }
}

/// Test environment-based host whitelist validation
#[test]
#[allow(unsafe_code)] // Environment variable operations are now unsafe in Rust 2024
fn test_environment_host_whitelist() {
    // Set environment variable for testing
    unsafe {
        std::env::set_var(
            "OAUTH_ALLOWED_REDIRECT_HOSTS",
            "trusted.com,api.trusted.com",
        );
    }

    let config_trusted = OAuth2Config {
        client_id: "test_client".to_string(),
        client_secret: "test_secret".to_string(),
        auth_url: "https://auth.example.com/oauth/authorize".to_string(),
        token_url: "https://auth.example.com/oauth/token".to_string(),
        redirect_uri: "https://trusted.com/callback".to_string(),
        scopes: vec!["read".to_string()],
        flow_type: OAuth2FlowType::AuthorizationCode,
        additional_params: HashMap::new(),
        security_level: SecurityLevel::Standard,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let config_untrusted = OAuth2Config {
        client_id: "test_client".to_string(),
        client_secret: "test_secret".to_string(),
        auth_url: "https://auth.example.com/oauth/authorize".to_string(),
        token_url: "https://auth.example.com/oauth/token".to_string(),
        redirect_uri: "https://evil.com/callback".to_string(),
        scopes: vec!["read".to_string()],
        flow_type: OAuth2FlowType::AuthorizationCode,
        additional_params: HashMap::new(),
        security_level: SecurityLevel::Standard,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    // Should succeed for whitelisted host
    let result_trusted = OAuth2Client::new(&config_trusted, ProviderType::Generic);
    assert!(result_trusted.is_ok(), "Should allow whitelisted host");

    // Should fail for non-whitelisted host
    let result_untrusted = OAuth2Client::new(&config_untrusted, ProviderType::Generic);
    assert!(
        result_untrusted.is_err(),
        "Should reject non-whitelisted host"
    );

    // Clean up environment
    unsafe {
        std::env::remove_var("OAUTH_ALLOWED_REDIRECT_HOSTS");
    }
}

/// Test domain-based whitelist validation
#[test]
#[allow(unsafe_code)] // Environment variable operations are now unsafe in Rust 2024
fn test_main_domain_validation() {
    // Set main domain for testing
    unsafe {
        std::env::set_var("OAUTH_MAIN_DOMAIN", "mycompany.com");
    }

    let valid_domains = vec![
        "https://mycompany.com/callback",
        "https://api.mycompany.com/oauth",
        "https://auth.mycompany.com/callback",
    ];

    let invalid_domains = vec![
        "https://evilmycompany.com/callback",      // Not a subdomain
        "https://mycompany.com.evil.com/callback", // Domain suffix attack
        "https://fake-mycompany.com/callback",     // Similar but different domain
    ];

    for uri in valid_domains {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            redirect_uri: uri.to_string(),
            scopes: vec!["read".to_string()],
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        let result = OAuth2Client::new(&config, ProviderType::Generic);
        assert!(result.is_ok(), "Should allow valid subdomain: {}", uri);
    }

    for uri in invalid_domains {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            redirect_uri: uri.to_string(),
            scopes: vec!["read".to_string()],
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        let result = OAuth2Client::new(&config, ProviderType::Generic);
        assert!(result.is_err(), "Should reject invalid domain: {}", uri);
    }

    // Clean up environment
    unsafe {
        std::env::remove_var("OAUTH_MAIN_DOMAIN");
    }
}

/// Test that localhost is always allowed for development
#[test]
fn test_localhost_always_allowed() {
    let localhost_uris = vec![
        "http://localhost:8080/callback",
        "http://127.0.0.1:3000/auth",
        "http://0.0.0.0:8080/callback",
        // Note: IPv6 localhost may need explicit parsing
    ];

    for uri in localhost_uris {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            redirect_uri: uri.to_string(),
            scopes: vec!["read".to_string()],
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        let result = OAuth2Client::new(&config, ProviderType::Generic);
        assert!(
            result.is_ok(),
            "Localhost should always be allowed: {}",
            uri
        );
    }
}

/// Test comprehensive security validation prevents common attack vectors
#[test]
fn test_security_attack_vectors_blocked() {
    let attack_vectors = vec![
        // Open redirect attacks
        ("https://trusted.com@evil.com/callback", "Username in host"),
        (
            "https://trusted.com.evil.com/callback",
            "Domain suffix attack",
        ),
        // Injection attacks
        (
            "https://example.com/callback?redirect=javascript:alert(1)",
            "JavaScript injection",
        ),
        (
            "https://example.com/callback?next=data:text/html,<script>",
            "Data URL injection",
        ),
        (
            "https://example.com/callback?return=vbscript:msgbox(1)",
            "VBScript injection",
        ),
        // Path manipulation
        ("https://example.com/../admin/callback", "Path traversal"),
        (
            "https://example.com/callback/%2e%2e/admin",
            "URL encoded traversal",
        ),
        ("https://example.com//callback", "Protocol relative"),
        // Protocol attacks
        ("file:///etc/passwd", "File protocol"),
        ("data:text/html,<h1>Evil</h1>", "Data protocol"),
    ];

    for (uri, description) in attack_vectors {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            redirect_uri: uri.to_string(),
            scopes: vec!["read".to_string()],
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        let result = OAuth2Client::new(&config, ProviderType::Generic);
        assert!(
            result.is_err(),
            "Should block attack vector ({}): {}",
            description,
            uri
        );
    }
}

/// Test that external domains are properly rejected without whitelist configuration
#[test]
fn test_external_domains_require_whitelist() {
    let external_uris = vec![
        "https://yourdomain.com/oauth/callback",
        "https://api.example.com/callback",
        "https://auth.myservice.io/oauth",
    ];

    for uri in external_uris {
        let config = OAuth2Config {
            client_id: "test_client".to_string(),
            client_secret: "test_secret".to_string(),
            auth_url: "https://auth.example.com/oauth/authorize".to_string(),
            token_url: "https://auth.example.com/oauth/token".to_string(),
            redirect_uri: uri.to_string(),
            scopes: vec!["read".to_string()],
            flow_type: OAuth2FlowType::AuthorizationCode,
            additional_params: HashMap::new(),
            security_level: SecurityLevel::Standard,
            #[cfg(feature = "dpop")]
            dpop_config: None,
        };

        let result = OAuth2Client::new(&config, ProviderType::Generic);
        assert!(
            result.is_err(),
            "External domain should be rejected without whitelist: {}",
            uri
        );
    }
}
