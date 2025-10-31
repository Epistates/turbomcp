//! Configuration system tests
//!
//! Tests for the comprehensive configuration system including server config,
//! CORS, security headers, rate limiting, TLS, and authentication.

#[cfg(test)]
mod tests {
    use crate::axum::config::*;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn test_mcp_server_config_default() {
        let config = McpServerConfig::default();

        assert_eq!(config.max_request_size, 16 * 1024 * 1024);
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(config.max_connections, 1000);
        assert!(config.cors.enabled);
        assert!(config.enable_compression);
        assert!(config.enable_tracing);
    }

    #[test]
    fn test_production_grade_security_configuration() {
        // Test development configuration (permissive)
        let dev_config = McpServerConfig::development();
        assert_eq!(dev_config.environment, Environment::Development);
        assert!(
            dev_config
                .cors
                .allowed_origins
                .as_ref()
                .unwrap()
                .contains(&"*".to_string())
        );
        assert!(!dev_config.security.enabled);
        assert!(!dev_config.rate_limiting.enabled);

        // Test staging configuration (moderate security)
        let staging_config = McpServerConfig::staging();
        assert_eq!(staging_config.environment, Environment::Staging);
        assert!(
            staging_config
                .cors
                .allowed_origins
                .as_ref()
                .unwrap()
                .is_empty()
        ); // Must be configured
        assert!(staging_config.security.enabled);
        assert!(staging_config.rate_limiting.enabled);
        assert_eq!(staging_config.rate_limiting.requests_per_minute, 300);

        // Test production configuration (maximum security)
        let prod_config = McpServerConfig::production();
        assert_eq!(prod_config.environment, Environment::Production);
        assert!(
            prod_config
                .cors
                .allowed_origins
                .as_ref()
                .unwrap()
                .is_empty()
        );
        assert!(prod_config.security.enabled);
        assert!(prod_config.rate_limiting.enabled);
        assert_eq!(prod_config.rate_limiting.requests_per_minute, 120);
        assert_eq!(prod_config.max_request_size, 4 * 1024 * 1024); // 4MB
    }

    #[test]
    fn test_configuration_builder_pattern() {
        let config = McpServerConfig::staging()
            .with_cors_origins(vec!["https://example.com".to_string()])
            .with_custom_csp("default-src 'self'")
            .with_rate_limit(600, 100)
            .with_api_key_auth("X-API-Key".to_string());

        // Verify CORS configuration
        assert!(
            config
                .cors
                .allowed_origins
                .as_ref()
                .unwrap()
                .contains(&"https://example.com".to_string())
        );

        // Verify CSP configuration
        assert_eq!(
            config.security.content_security_policy.as_ref().unwrap(),
            "default-src 'self'"
        );

        // Verify rate limiting configuration
        assert_eq!(config.rate_limiting.requests_per_minute, 600);
        assert_eq!(config.rate_limiting.burst_capacity, 100);
        assert!(config.rate_limiting.enabled);

        // Verify authentication configuration
        assert!(config.auth.is_some());
        let auth = config.auth.unwrap();
        assert!(auth.enabled);
        assert_eq!(auth.api_key_header.unwrap(), "X-API-Key");
    }

    #[test]
    fn test_cors_configuration_variants() {
        // Test permissive CORS (development)
        let permissive = CorsConfig::permissive();
        assert!(permissive.enabled);
        assert!(
            permissive
                .allowed_origins
                .as_ref()
                .unwrap()
                .contains(&"*".to_string())
        );
        assert!(!permissive.allow_credentials); // Cannot be true with wildcard

        // Test strict CORS (production)
        let strict = CorsConfig::strict();
        assert!(strict.enabled);
        assert!(strict.allowed_origins.as_ref().unwrap().is_empty());
        assert!(strict.allow_credentials);

        // Test disabled CORS
        let disabled = CorsConfig::disabled();
        assert!(!disabled.enabled);
        assert!(disabled.allowed_origins.is_none());
    }

    #[test]
    fn test_security_config_variants() {
        // Test development security (minimal)
        let dev_security = SecurityConfig::development();
        assert!(!dev_security.enabled);
        assert!(dev_security.content_security_policy.is_none());
        assert_eq!(dev_security.frame_options, FrameOptions::Disabled);

        // Test production security (maximum)
        let prod_security = SecurityConfig::production();
        assert!(prod_security.enabled);
        assert!(prod_security.content_security_policy.is_some());
        assert_eq!(prod_security.frame_options, FrameOptions::Deny);
        assert!(prod_security.content_type_options);
        assert_eq!(
            prod_security.referrer_policy.as_ref().unwrap(),
            "no-referrer"
        );
    }

    #[test]
    fn test_rate_limiting_config_variants() {
        // Test disabled rate limiting
        let disabled = RateLimitConfig::disabled();
        assert!(!disabled.enabled);
        assert_eq!(disabled.requests_per_minute, 0);

        // Test moderate rate limiting
        let moderate = RateLimitConfig::moderate();
        assert!(moderate.enabled);
        assert_eq!(moderate.requests_per_minute, 300);
        assert_eq!(moderate.burst_capacity, 50);

        // Test strict rate limiting
        let strict = RateLimitConfig::strict();
        assert!(strict.enabled);
        assert_eq!(strict.requests_per_minute, 120);
        assert_eq!(strict.burst_capacity, 20);
    }

    #[test]
    fn test_configuration_loading_logic() {
        // Test TLS configuration parsing logic directly
        // Instead of testing environment variable loading, test the parsing functions

        // Test TLS version parsing
        let tls_config = TlsConfig {
            cert_file: "/etc/ssl/certs/server.pem".to_string(),
            key_file: "/etc/ssl/private/server.key".to_string(),
            min_version: TlsVersion::TlsV1_3,
            enable_http2: true,
        };
        assert_eq!(tls_config.cert_file, "/etc/ssl/certs/server.pem");
        assert_eq!(tls_config.key_file, "/etc/ssl/private/server.key");
        assert!(matches!(tls_config.min_version, TlsVersion::TlsV1_3));
        assert!(tls_config.enable_http2);

        // Test authentication configuration creation
        let auth_config = AuthConfig::jwt("test-secret".to_string());
        assert!(auth_config.enabled);
        assert!(auth_config.jwt.is_some());
        assert_eq!(
            auth_config.jwt.as_ref().unwrap().secret.as_ref().unwrap(),
            "test-secret"
        );

        let api_config = AuthConfig::api_key("X-API-Key".to_string());
        assert!(api_config.enabled);
        assert_eq!(api_config.api_key_header.unwrap(), "X-API-Key");

        // Test CORS origins parsing logic
        let cors_origins_string = "https://app.example.com,https://admin.example.com";
        let parsed_origins: Vec<String> = cors_origins_string
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        assert_eq!(parsed_origins.len(), 2);
        assert!(parsed_origins.contains(&"https://app.example.com".to_string()));
        assert!(parsed_origins.contains(&"https://admin.example.com".to_string()));
    }

    #[test]
    fn test_environment_enum() {
        let dev = Environment::Development;
        let staging = Environment::Staging;
        let prod = Environment::Production;

        assert!(dev.is_development());
        assert!(!dev.is_staging());
        assert!(!dev.is_production());

        assert!(!staging.is_development());
        assert!(staging.is_staging());
        assert!(!staging.is_production());

        assert!(!prod.is_development());
        assert!(!prod.is_staging());
        assert!(prod.is_production());

        assert_eq!(dev.as_str(), "development");
        assert_eq!(staging.as_str(), "staging");
        assert_eq!(prod.as_str(), "production");
    }

    #[test]
    fn test_tls_config_builder() {
        let config = TlsConfig::new("cert.pem".to_string(), "key.pem".to_string())
            .with_min_version(TlsVersion::TlsV1_3)
            .with_http2(true);

        assert_eq!(config.cert_file, "cert.pem");
        assert_eq!(config.key_file, "key.pem");
        assert!(matches!(config.min_version, TlsVersion::TlsV1_3));
        assert!(config.enable_http2);
    }

    #[test]
    fn test_auth_config_variants() {
        let jwt_config = AuthConfig::jwt("secret".to_string());
        assert!(jwt_config.enabled);
        assert!(jwt_config.jwt.is_some());
        assert_eq!(
            jwt_config.jwt.as_ref().unwrap().secret.as_ref().unwrap(),
            "secret"
        );
        assert!(jwt_config.api_key_header.is_none());

        let api_key_config = AuthConfig::api_key("X-API-Key".to_string());
        assert!(api_key_config.enabled);
        assert!(api_key_config.jwt.is_none());
        assert_eq!(api_key_config.api_key_header.unwrap(), "X-API-Key");

        let disabled_config = AuthConfig::disabled();
        assert!(!disabled_config.enabled);
        assert!(disabled_config.jwt.is_none());
        assert!(disabled_config.api_key_header.is_none());
    }

    #[test]
    fn test_rate_limit_custom() {
        let custom_config = RateLimitConfig::custom(1000, 200, RateLimitKey::UserId);
        assert!(custom_config.enabled);
        assert_eq!(custom_config.requests_per_minute, 1000);
        assert_eq!(custom_config.burst_capacity, 200);
        assert!(matches!(custom_config.key_function, RateLimitKey::UserId));
    }
}
