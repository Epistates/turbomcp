//! Manual integration test for proxy functionality
//!
//! This test verifies that the proxy can:
//! 1. Connect to the stdio_server backend
//! 2. Introspect tools
//! 3. Call tools through the proxy
//!
//! Run with: cargo test --package turbomcp-proxy --test manual_integration -- --ignored --nocapture

use std::collections::HashMap;
use turbomcp_proxy::proxy::{BackendConfig, BackendConnector, BackendTransport};

#[tokio::test]
#[ignore = "Requires building stdio_server example (60+ seconds), run manually with --ignored"]
async fn test_proxy_end_to_end() {
    println!("\n🧪 Testing Proxy End-to-End Functionality");
    println!("==========================================\n");

    // Test 1: Create backend connector
    println!("Test 1: Create backend connector to stdio_server...");

    let config = BackendConfig {
        transport: BackendTransport::Stdio {
            command: "cargo".to_string(),
            args: vec![
                "run".to_string(),
                "--example".to_string(),
                "stdio_server".to_string(),
            ],
            working_dir: Some("/Users/nickpaterno/work/turbomcp".to_string()),
        },
        client_name: "integration-test".to_string(),
        client_version: "1.0.0".to_string(),
    };

    let backend = BackendConnector::new(config)
        .await
        .expect("Failed to create backend connector");

    println!("✅ Backend connector created successfully\n");

    // Test 2: Introspect backend (list tools)
    println!("Test 2: Introspect backend (list tools)...");

    let spec = backend
        .introspect()
        .await
        .expect("Failed to introspect backend");

    println!("✅ Introspection successful");
    println!("   Found {} tools:", spec.tools.len());
    for tool in &spec.tools {
        println!(
            "   - {}: {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }
    println!();

    // Verify we have the expected tools
    let expected_tools = vec!["echo", "reverse"];
    for expected in &expected_tools {
        assert!(
            spec.tools.iter().any(|t| &t.name == expected),
            "Expected tool '{}' not found",
            expected
        );
    }
    println!("✅ All expected tools found\n");

    // Test 3: Call 'echo' tool
    println!("Test 3: Call 'echo' tool with message...");

    let mut args = HashMap::new();
    args.insert(
        "message".to_string(),
        serde_json::json!("Hello from proxy!"),
    );

    let result = backend
        .call_tool("echo", Some(args))
        .await
        .expect("Failed to call echo tool");

    println!("✅ Tool call successful");
    println!("   Result: {}", result);

    // Verify result contains our message
    let result_str = result.to_string();
    assert!(
        result_str.contains("Hello from proxy!"),
        "Result doesn't contain expected message: {}",
        result_str
    );
    println!("✅ Result contains expected message\n");

    // Test 4: Call 'reverse' tool
    println!("Test 4: Call 'reverse' tool...");

    let mut args = HashMap::new();
    args.insert("text".to_string(), serde_json::json!("turbomcp"));

    let result = backend
        .call_tool("reverse", Some(args))
        .await
        .expect("Failed to call reverse tool");

    println!("✅ Tool call successful");
    println!("   Result: {}", result);

    // Verify result is reversed
    let result_str = result.to_string();
    assert!(
        result_str.contains("pcmobr"),
        "Result is not correctly reversed: {}",
        result_str
    );
    println!("✅ Result is correctly reversed\n");

    println!("==========================================");
    println!("🎉 All Proxy Integration Tests Passed!");
    println!("==========================================\n");
}

#[tokio::test]
async fn test_proxy_quick_validation() {
    // Quick test that doesn't require building the stdio_server
    println!("\n🧪 Quick Proxy Validation (config only)");

    let config = BackendConfig {
        transport: BackendTransport::Stdio {
            command: "cargo".to_string(),
            args: vec!["run".to_string()],
            working_dir: None,
        },
        client_name: "test".to_string(),
        client_version: "1.0.0".to_string(),
    };

    // Just verify config creation works
    assert_eq!(config.client_name, "test");
    assert_eq!(config.client_version, "1.0.0");

    println!("✅ Proxy configuration validation passed\n");
}
