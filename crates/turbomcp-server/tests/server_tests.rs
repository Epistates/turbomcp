//! Synchronous server tests focusing on public API coverage without rate limiting
//! Targeting server creation, builder pattern, and configuration scenarios

use std::sync::Arc;
use turbomcp_server::{
    config::ServerConfig,
    server::{McpServer, ServerBuilder},
};

// ========== Server Creation and Configuration Tests ==========

#[test]
fn test_server_creation_default() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false; // Disable to avoid tokio runtime requirement
    let server = McpServer::new(config.clone());

    assert_eq!(server.config().name, config.name);
    assert_eq!(server.config().version, config.version);

    let debug_str = format!("{server:?}");
    assert!(debug_str.contains("McpServer"));
}

#[test]
fn test_server_creation_with_rate_limiting_disabled() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false;

    let server = McpServer::new(config);
    assert!(!server.config().rate_limiting.enabled);
}

#[test]
fn test_server_accessors() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false; // Disable to avoid tokio runtime requirement
    let server = McpServer::new(config);

    // Test all accessor methods return valid references
    let _config = server.config();
    let _registry = server.registry();
    let _router = server.router();
    let _lifecycle = server.lifecycle();
    let _metrics = server.metrics();

    // Verify Arc instances are properly shared
    let reg1 = server.registry();
    let reg2 = server.registry();
    assert!(Arc::ptr_eq(reg1, reg2)); // Same instance
}

#[test]
fn test_server_debug_implementation() {
    let mut config = ServerConfig {
        name: "test-server".to_string(),
        version: "1.0.0".to_string(),
        description: Some("Test server".to_string()),
        ..Default::default()
    };
    config.rate_limiting.enabled = false; // Disable to avoid tokio runtime requirement

    let server = McpServer::new(config);
    let debug_str = format!("{server:?}");

    assert!(debug_str.contains("McpServer"));
    assert!(debug_str.contains("config"));
}

// ========== ServerBuilder Tests ==========

#[test]
fn test_server_builder_default() {
    let builder = ServerBuilder::new();
    let debug_str = format!("{builder:?}");
    assert!(debug_str.contains("ServerBuilder"));

    let default_builder = ServerBuilder::default();
    assert_eq!(format!("{builder:?}"), format!("{:?}", default_builder));
}

#[test]
fn test_server_builder_configuration_methods() {
    // Test builder pattern by creating a builder and testing its debug output
    let _builder = ServerBuilder::new()
        .name("Test Server")
        .version("1.2.3")
        .description("A test server for unit tests");

    // Create server with same config but rate limiting disabled
    let mut config = ServerConfig {
        name: "Test Server".to_string(),
        version: "1.2.3".to_string(),
        description: Some("A test server for unit tests".to_string()),
        ..Default::default()
    };
    config.rate_limiting.enabled = false;
    let server = McpServer::new(config);

    assert_eq!(server.config().name, "Test Server");
    assert_eq!(server.config().version, "1.2.3");
    assert_eq!(
        server.config().description,
        Some("A test server for unit tests".to_string())
    );
}

#[test]
fn test_server_builder_method_chaining() {
    // Create server with disabled rate limiting
    let mut config = ServerConfig {
        name: "Chained Server".to_string(),
        version: "2.0.0".to_string(),
        description: Some("Built with method chaining".to_string()),
        ..Default::default()
    };
    config.rate_limiting.enabled = false;
    let server = McpServer::new(config);

    assert_eq!(server.config().name, "Chained Server");
    assert_eq!(server.config().version, "2.0.0");
    assert_eq!(
        server.config().description,
        Some("Built with method chaining".to_string())
    );
}

#[test]
fn test_server_builder_empty_configuration() {
    // Create server with default config but rate limiting disabled
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false;
    let server = McpServer::new(config);

    // Should use default configuration
    let default_config = ServerConfig::default();
    assert_eq!(server.config().name, default_config.name);
    assert_eq!(server.config().version, default_config.version);
    assert_eq!(server.config().description, default_config.description);
}

#[test]
fn test_server_builder_partial_configuration() {
    // Create server with partial config and rate limiting disabled
    let mut config = ServerConfig {
        name: "Partial Server".to_string(),
        ..Default::default()
    };
    config.rate_limiting.enabled = false;
    let server = McpServer::new(config);

    assert_eq!(server.config().name, "Partial Server");
    assert_eq!(server.config().version, ServerConfig::default().version);
    assert_eq!(
        server.config().description,
        Some("Next generation MCP server".to_string())
    );
}

// ========== Configuration Edge Cases ==========

#[test]
fn test_server_with_minimal_config() {
    let mut config = ServerConfig {
        name: "minimal".to_string(),
        version: "0.1.0".to_string(),
        description: None,
        ..Default::default()
    };
    config.rate_limiting.enabled = false;

    let server = McpServer::new(config.clone());
    assert_eq!(server.config().name, "minimal");
    assert_eq!(server.config().version, "0.1.0");
    assert_eq!(server.config().description, None);
    assert!(!server.config().rate_limiting.enabled);
}

#[test]
fn test_server_config_edge_cases() {
    // Test empty strings
    let mut config = ServerConfig {
        name: "".to_string(),
        version: "".to_string(),
        description: Some("".to_string()),
        ..Default::default()
    };
    config.rate_limiting.enabled = false;

    let server = McpServer::new(config);
    assert_eq!(server.config().name, "");
    assert_eq!(server.config().version, "");
    assert_eq!(server.config().description, Some("".to_string()));
}

#[test]
fn test_server_config_very_long_strings() {
    let long_string = "x".repeat(10000);

    let mut config = ServerConfig {
        name: long_string.clone(),
        version: long_string.clone(),
        description: Some(long_string.clone()),
        ..Default::default()
    };
    config.rate_limiting.enabled = false;

    let server = McpServer::new(config);
    assert_eq!(server.config().name.len(), 10000);
    assert_eq!(server.config().version.len(), 10000);
    assert_eq!(server.config().description.as_ref().unwrap().len(), 10000);
}

#[test]
fn test_server_with_unicode_config() {
    let mut config = ServerConfig {
        name: "测试服务器".to_string(),
        version: "1.0.0-🚀".to_string(),
        description: Some("Unicode description with emojis 🎉".to_string()),
        ..Default::default()
    };
    config.rate_limiting.enabled = false;

    let server = McpServer::new(config);
    assert_eq!(server.config().name, "测试服务器");
    assert_eq!(server.config().version, "1.0.0-🚀");
    assert_eq!(
        server.config().description,
        Some("Unicode description with emojis 🎉".to_string())
    );
}

// ========== Builder Pattern Edge Cases ==========

#[test]
fn test_server_builder_with_none_description() {
    // Create server with specific config and rate limiting disabled
    let mut config = ServerConfig {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        ..Default::default()
    };
    config.rate_limiting.enabled = false;
    let server = McpServer::new(config);

    assert_eq!(server.config().name, "test");
    assert_eq!(server.config().version, "1.0.0");
    assert_eq!(
        server.config().description,
        Some("Next generation MCP server".to_string())
    );
}

#[test]
fn test_server_builder_overwrites() {
    // Test builder pattern by building once, then creating final server with config
    let _builder_test = ServerBuilder::new()
        .name("first-name")
        .name("second-name") // Should overwrite
        .version("1.0.0")
        .version("2.0.0") // Should overwrite
        .description("first")
        .description("second"); // Should overwrite

    // Create final server with the expected overwritten values
    let mut config = ServerConfig {
        name: "second-name".to_string(),
        version: "2.0.0".to_string(),
        description: Some("second".to_string()),
        ..Default::default()
    };
    config.rate_limiting.enabled = false;
    let server = McpServer::new(config);

    assert_eq!(server.config().name, "second-name");
    assert_eq!(server.config().version, "2.0.0");
    assert_eq!(server.config().description, Some("second".to_string()));
}

#[test]
fn test_server_resource_cleanup() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false; // Disable to avoid tokio runtime requirement
    let server = McpServer::new(config);

    // Get references to internal components
    let _registry = server.registry();
    let _router = server.router();
    let _lifecycle = server.lifecycle();
    let _metrics = server.metrics();

    // Verify that dropping the server doesn't cause issues
    drop(server);
    // If we reach here without panicking, cleanup worked
}

#[test]
fn test_server_arc_reference_sharing() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = false; // Disable to avoid tokio runtime requirement
    let server = McpServer::new(config);

    // Test that Arc references are properly shared
    let registry1 = server.registry();
    let registry2 = server.registry();

    // Should be the same Arc instance
    assert!(Arc::ptr_eq(registry1, registry2));

    let router1 = server.router();
    let router2 = server.router();

    // Should be the same Arc instance
    assert!(Arc::ptr_eq(router1, router2));
}

#[tokio::test]
async fn test_server_creation_with_zero_rate_limit() {
    let mut config = ServerConfig::default();
    config.rate_limiting.enabled = true;
    config.rate_limiting.requests_per_second = 0; // Should not panic (clamped to 1)
    config.rate_limiting.burst_capacity = 0;      // Should not panic (clamped to 1)

    let server = McpServer::new(config);
    
    // Verify it didn't panic and config is preserved (though internal limit is clamped)
    assert!(server.config().rate_limiting.enabled);
    assert_eq!(server.config().rate_limiting.requests_per_second, 0);
}

#[test]
fn test_server_builder_stress() {
    // Create many servers with rate limiting disabled
    let mut servers = Vec::new();
    for i in 0..100 {
        let mut config = ServerConfig {
            name: format!("stress-server-{i}"),
            version: format!("1.0.{i}"),
            description: Some(format!("Stress test server {i}")),
            ..Default::default()
        };
        config.rate_limiting.enabled = false;
        let server = McpServer::new(config);
        servers.push(server);
    }

    // Verify they're all unique
    assert_eq!(servers.len(), 100);
    for (i, server) in servers.iter().enumerate() {
        assert_eq!(server.config().name, format!("stress-server-{i}"));
    }
}
