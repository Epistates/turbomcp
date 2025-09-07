//! Server configuration tests - comprehensive coverage for normal and edge cases

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use turbomcp_server::config::*;

// ============================================================================
// Default Configuration Tests
// ============================================================================

#[test]
fn test_server_config_default() {
    let config = ServerConfig::default();

    assert_eq!(config.name, "turbomcp-server");
    assert_eq!(config.version, "1.0.4");
    assert_eq!(
        config.description,
        Some("Next generation MCP server".to_string())
    );
    assert_eq!(config.bind_address, "127.0.0.1");
    assert_eq!(config.port, 8080);
    assert!(!config.enable_tls);
    assert!(config.tls.is_none());

    // Test default timeout config
    let timeouts = &config.timeouts;
    assert_eq!(timeouts.request_timeout, Duration::from_secs(30));
    assert_eq!(timeouts.connection_timeout, Duration::from_secs(10));
    assert_eq!(timeouts.keep_alive_timeout, Duration::from_secs(60));

    // Test default rate limiting config
    let rate_limiting = &config.rate_limiting;
    assert!(rate_limiting.enabled);
    assert_eq!(rate_limiting.requests_per_second, 100);
    assert_eq!(rate_limiting.burst_capacity, 200);

    // Test default logging config
    let logging = &config.logging;
    assert_eq!(logging.level, "info");
    assert!(logging.structured);
    assert!(logging.file.is_none());

    // Test additional fields
    assert!(config.additional.is_empty());
}

#[test]
fn test_timeout_config_default() {
    let timeout_config = TimeoutConfig::default();

    assert_eq!(timeout_config.request_timeout, Duration::from_secs(30));
    assert_eq!(timeout_config.connection_timeout, Duration::from_secs(10));
    assert_eq!(timeout_config.keep_alive_timeout, Duration::from_secs(60));
}

#[test]
fn test_rate_limiting_config_default() {
    let rate_config = RateLimitingConfig::default();

    assert!(rate_config.enabled);
    assert_eq!(rate_config.requests_per_second, 100);
    assert_eq!(rate_config.burst_capacity, 200);
}

#[test]
fn test_logging_config_default() {
    let log_config = LoggingConfig::default();

    assert_eq!(log_config.level, "info");
    assert!(log_config.structured);
    assert!(log_config.file.is_none());
}

// ============================================================================
// Configuration Builder Tests - Normal Use Cases
// ============================================================================

#[test]
fn test_configuration_builder_new() {
    let builder = ConfigurationBuilder::new();
    let config = builder.build();

    // Should match default configuration
    let default_config = ServerConfig::default();
    assert_eq!(config.name, default_config.name);
    assert_eq!(config.version, default_config.version);
    assert_eq!(config.port, default_config.port);
}

#[test]
fn test_configuration_builder_default() {
    let builder1 = ConfigurationBuilder::default();
    let builder2 = ConfigurationBuilder::new();

    let config1 = builder1.build();
    let config2 = builder2.build();

    assert_eq!(config1.name, config2.name);
    assert_eq!(config1.port, config2.port);
}

#[test]
fn test_builder_name() {
    let config = ConfigurationBuilder::new().name("test-server").build();

    assert_eq!(config.name, "test-server");
}

#[test]
fn test_builder_version() {
    let config = ConfigurationBuilder::new().version("2.0.0").build();

    assert_eq!(config.version, "2.0.0");
}

#[test]
fn test_builder_description() {
    let config = ConfigurationBuilder::new()
        .description("Custom test server")
        .build();

    assert_eq!(config.description, Some("Custom test server".to_string()));
}

#[test]
fn test_builder_bind_address() {
    let config = ConfigurationBuilder::new().bind_address("0.0.0.0").build();

    assert_eq!(config.bind_address, "0.0.0.0");
}

#[test]
fn test_builder_port() {
    let config = ConfigurationBuilder::new().port(3000).build();

    assert_eq!(config.port, 3000);
}

#[test]
fn test_builder_tls() {
    let cert_path = PathBuf::from("/path/to/cert.pem");
    let key_path = PathBuf::from("/path/to/key.pem");

    let config = ConfigurationBuilder::new()
        .tls(cert_path.clone(), key_path.clone())
        .build();

    assert!(config.enable_tls);
    assert!(config.tls.is_some());

    let tls_config = config.tls.unwrap();
    assert_eq!(tls_config.cert_file, cert_path);
    assert_eq!(tls_config.key_file, key_path);
}

#[test]
fn test_builder_request_timeout() {
    let config = ConfigurationBuilder::new()
        .request_timeout(Duration::from_secs(45))
        .build();

    assert_eq!(config.timeouts.request_timeout, Duration::from_secs(45));
}

#[test]
fn test_builder_rate_limiting() {
    let config = ConfigurationBuilder::new().rate_limiting(50, 100).build();

    assert!(config.rate_limiting.enabled);
    assert_eq!(config.rate_limiting.requests_per_second, 50);
    assert_eq!(config.rate_limiting.burst_capacity, 100);
}

#[test]
fn test_builder_log_level() {
    let config = ConfigurationBuilder::new().log_level("debug").build();

    assert_eq!(config.logging.level, "debug");
}

// ============================================================================
// Configuration Builder Chaining Tests
// ============================================================================

#[test]
fn test_builder_method_chaining() {
    let config = ConfigurationBuilder::new()
        .name("chained-server")
        .version("3.0.0")
        .description("Server built with method chaining")
        .bind_address("192.168.1.1")
        .port(8443)
        .tls(
            PathBuf::from("/etc/ssl/cert.pem"),
            PathBuf::from("/etc/ssl/private.key"),
        )
        .request_timeout(Duration::from_secs(120))
        .rate_limiting(200, 500)
        .log_level("trace")
        .build();

    assert_eq!(config.name, "chained-server");
    assert_eq!(config.version, "3.0.0");
    assert_eq!(
        config.description,
        Some("Server built with method chaining".to_string())
    );
    assert_eq!(config.bind_address, "192.168.1.1");
    assert_eq!(config.port, 8443);
    assert!(config.enable_tls);
    assert!(config.tls.is_some());
    assert_eq!(config.timeouts.request_timeout, Duration::from_secs(120));
    assert_eq!(config.rate_limiting.requests_per_second, 200);
    assert_eq!(config.rate_limiting.burst_capacity, 500);
    assert_eq!(config.logging.level, "trace");
}

#[test]
fn test_builder_partial_configuration() {
    let config = ConfigurationBuilder::new()
        .name("partial-server")
        .port(9000)
        .build();

    // Should have updated values
    assert_eq!(config.name, "partial-server");
    assert_eq!(config.port, 9000);

    // Should retain defaults for non-configured values
    assert_eq!(config.version, "1.0.4");
    assert_eq!(config.bind_address, "127.0.0.1");
    assert!(!config.enable_tls);
}

// ============================================================================
// Serialization and Deserialization Tests
// ============================================================================

#[test]
fn test_server_config_serialization() {
    let config = ServerConfig::default();

    let json = serde_json::to_string(&config).expect("Failed to serialize config");
    assert!(!json.is_empty());

    let deserialized: ServerConfig =
        serde_json::from_str(&json).expect("Failed to deserialize config");

    assert_eq!(config.name, deserialized.name);
    assert_eq!(config.version, deserialized.version);
    assert_eq!(config.port, deserialized.port);
}

#[test]
fn test_server_config_json_roundtrip() {
    let mut additional = HashMap::new();
    additional.insert("custom_key".to_string(), serde_json::json!("custom_value"));
    additional.insert("number_key".to_string(), serde_json::json!(42));

    let original_config = ServerConfig {
        name: "test-server".to_string(),
        version: "1.2.3".to_string(),
        description: Some("Test server for JSON roundtrip".to_string()),
        bind_address: "0.0.0.0".to_string(),
        port: 8080,
        enable_tls: true,
        tls: Some(TlsConfig {
            cert_file: PathBuf::from("/tmp/cert.pem"),
            key_file: PathBuf::from("/tmp/key.pem"),
        }),
        timeouts: TimeoutConfig {
            request_timeout: Duration::from_secs(60),
            connection_timeout: Duration::from_secs(15),
            keep_alive_timeout: Duration::from_secs(90),
        },
        rate_limiting: RateLimitingConfig {
            enabled: false,
            requests_per_second: 50,
            burst_capacity: 100,
        },
        logging: LoggingConfig {
            level: "warn".to_string(),
            structured: false,
            file: Some(PathBuf::from("/var/log/server.log")),
        },
        additional,
    };

    let json = serde_json::to_string(&original_config).expect("Serialization failed");
    let deserialized_config: ServerConfig =
        serde_json::from_str(&json).expect("Deserialization failed");

    assert_eq!(original_config.name, deserialized_config.name);
    assert_eq!(original_config.version, deserialized_config.version);
    assert_eq!(original_config.description, deserialized_config.description);
    assert_eq!(
        original_config.bind_address,
        deserialized_config.bind_address
    );
    assert_eq!(original_config.port, deserialized_config.port);
    assert_eq!(original_config.enable_tls, deserialized_config.enable_tls);

    // Test TLS config
    assert!(deserialized_config.tls.is_some());
    let tls = deserialized_config.tls.unwrap();
    let original_tls = original_config.tls.unwrap();
    assert_eq!(tls.cert_file, original_tls.cert_file);
    assert_eq!(tls.key_file, original_tls.key_file);

    // Test additional fields
    assert_eq!(
        original_config.additional.len(),
        deserialized_config.additional.len()
    );
    assert_eq!(
        original_config.additional.get("custom_key"),
        deserialized_config.additional.get("custom_key")
    );
}

#[test]
fn test_tls_config_serialization() {
    let tls_config = TlsConfig {
        cert_file: PathBuf::from("/path/to/cert.pem"),
        key_file: PathBuf::from("/path/to/key.pem"),
    };

    let json = serde_json::to_string(&tls_config).expect("TLS serialization failed");
    let deserialized: TlsConfig = serde_json::from_str(&json).expect("TLS deserialization failed");

    assert_eq!(tls_config.cert_file, deserialized.cert_file);
    assert_eq!(tls_config.key_file, deserialized.key_file);
}

// ============================================================================
// Edge Cases and Validation Tests
// ============================================================================

#[test]
fn test_extreme_timeout_values() {
    let config = ConfigurationBuilder::new()
        .request_timeout(Duration::from_millis(1)) // Very short
        .build();

    assert_eq!(config.timeouts.request_timeout, Duration::from_millis(1));

    let config = ConfigurationBuilder::new()
        .request_timeout(Duration::from_secs(3600)) // Very long
        .build();

    assert_eq!(config.timeouts.request_timeout, Duration::from_secs(3600));
}

#[test]
fn test_extreme_port_values() {
    let config1 = ConfigurationBuilder::new()
        .port(1) // Minimum port
        .build();

    assert_eq!(config1.port, 1);

    let config2 = ConfigurationBuilder::new()
        .port(65535) // Maximum port
        .build();

    assert_eq!(config2.port, 65535);
}

#[test]
fn test_extreme_rate_limiting_values() {
    // Test with zero values
    let config1 = ConfigurationBuilder::new().rate_limiting(0, 0).build();

    assert!(config1.rate_limiting.enabled);
    assert_eq!(config1.rate_limiting.requests_per_second, 0);
    assert_eq!(config1.rate_limiting.burst_capacity, 0);

    // Test with large values
    let config2 = ConfigurationBuilder::new()
        .rate_limiting(u32::MAX, u32::MAX)
        .build();

    assert_eq!(config2.rate_limiting.requests_per_second, u32::MAX);
    assert_eq!(config2.rate_limiting.burst_capacity, u32::MAX);
}

#[test]
fn test_empty_string_configurations() {
    let config = ConfigurationBuilder::new()
        .name("")
        .version("")
        .description("")
        .bind_address("")
        .log_level("")
        .build();

    assert_eq!(config.name, "");
    assert_eq!(config.version, "");
    assert_eq!(config.description, Some("".to_string()));
    assert_eq!(config.bind_address, "");
    assert_eq!(config.logging.level, "");
}

#[test]
fn test_unicode_string_configurations() {
    let config = ConfigurationBuilder::new()
        .name("сервер-тест") // Cyrillic
        .description("测试服务器") // Chinese
        .bind_address("::1") // IPv6 loopback
        .log_level("отладка") // Cyrillic debug
        .build();

    assert_eq!(config.name, "сервер-тест");
    assert_eq!(config.description, Some("测试服务器".to_string()));
    assert_eq!(config.bind_address, "::1");
    assert_eq!(config.logging.level, "отладка");
}

// ============================================================================
// Configuration Manipulation Tests
// ============================================================================

#[test]
fn test_config_clone() {
    let original = ConfigurationBuilder::new()
        .name("original")
        .port(8080)
        .build();

    let cloned = original.clone();

    assert_eq!(original.name, cloned.name);
    assert_eq!(original.port, cloned.port);

    // Ensure they are independent
    assert_eq!(original.name, "original");
    assert_eq!(cloned.name, "original");
}

#[test]
fn test_config_debug_formatting() {
    let config = ServerConfig::default();
    let debug_str = format!("{config:?}");

    assert!(debug_str.contains("ServerConfig"));
    assert!(debug_str.contains("turbomcp-server"));
    assert!(!debug_str.is_empty());
}

#[test]
fn test_tls_config_creation() {
    let tls = TlsConfig {
        cert_file: PathBuf::from("/etc/ssl/cert.pem"),
        key_file: PathBuf::from("/etc/ssl/key.pem"),
    };

    assert_eq!(tls.cert_file, PathBuf::from("/etc/ssl/cert.pem"));
    assert_eq!(tls.key_file, PathBuf::from("/etc/ssl/key.pem"));

    let debug_str = format!("{tls:?}");
    assert!(debug_str.contains("TlsConfig"));
}

// ============================================================================
// Complex Scenario Tests
// ============================================================================

#[test]
fn test_production_like_config() {
    let config = ConfigurationBuilder::new()
        .name("production-mcp-server")
        .version("1.0.0")
        .description("Production MCP server with high performance settings")
        .bind_address("0.0.0.0")
        .port(443)
        .tls(
            PathBuf::from("/etc/ssl/certs/server.crt"),
            PathBuf::from("/etc/ssl/private/server.key"),
        )
        .request_timeout(Duration::from_secs(300)) // 5 minutes
        .rate_limiting(1000, 2000) // High throughput
        .log_level("info")
        .build();

    assert_eq!(config.name, "production-mcp-server");
    assert_eq!(config.port, 443);
    assert!(config.enable_tls);
    assert_eq!(config.timeouts.request_timeout, Duration::from_secs(300));
    assert_eq!(config.rate_limiting.requests_per_second, 1000);
    assert_eq!(config.rate_limiting.burst_capacity, 2000);
    assert_eq!(config.logging.level, "info");
}

#[test]
fn test_development_config() {
    let config = ConfigurationBuilder::new()
        .name("dev-server")
        .description("Development server with debug settings")
        .bind_address("127.0.0.1")
        .port(8080)
        .request_timeout(Duration::from_secs(5)) // Short for development
        .rate_limiting(10, 20) // Low limits for testing
        .log_level("trace")
        .build();

    assert_eq!(config.name, "dev-server");
    assert_eq!(config.port, 8080);
    assert!(!config.enable_tls); // No TLS in dev
    assert_eq!(config.timeouts.request_timeout, Duration::from_secs(5));
    assert_eq!(config.rate_limiting.requests_per_second, 10);
    assert_eq!(config.logging.level, "trace");
}

#[test]
fn test_configuration_compatibility() {
    // Test that configurations can be used as the type alias
    let _config: Configuration = ServerConfig::default();
    let _config: Configuration = ConfigurationBuilder::new().build();

    // Type alias compatibility validated by successful compilation
}

// ============================================================================
// Performance and Stress Tests
// ============================================================================

#[test]
fn test_large_additional_fields() {
    let mut additional = HashMap::new();

    // Add many fields
    for i in 0..100 {
        additional.insert(
            format!("key_{i}"),
            serde_json::json!({
                "nested_value": i,
                "description": format!("Value number {}", i)
            }),
        );
    }

    let config = ServerConfig {
        additional,
        ..Default::default()
    };

    assert_eq!(config.additional.len(), 100);

    // Test serialization with large additional fields
    let json = serde_json::to_string(&config).expect("Large config serialization failed");
    let deserialized: ServerConfig =
        serde_json::from_str(&json).expect("Large config deserialization failed");

    assert_eq!(config.additional.len(), deserialized.additional.len());
}

#[test]
fn test_builder_reuse() {
    // Test that the builder consumes self and cannot be reused
    let builder = ConfigurationBuilder::new().name("test");

    let _config = builder.build(); // builder is consumed here

    // Move semantics validated by successful compilation
}
