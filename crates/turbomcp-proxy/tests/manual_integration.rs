//! Manual integration test for proxy functionality
//!
//! This test verifies that the proxy can:
//! 1. Connect to the manual_server backend
//! 2. Introspect tools
//! 3. Call tools through the proxy
//!
//! Run with: cargo test --package turbomcp-proxy --test manual_integration -- --ignored --nocapture

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use turbomcp_proxy::proxy::{BackendConfig, BackendConnector, BackendTransport};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn manual_server_binary() -> PathBuf {
    workspace_root()
        .join("target")
        .join("debug")
        .join("examples")
        .join(format!("manual_server{}", std::env::consts::EXE_SUFFIX))
}

fn ensure_manual_server_built() {
    let binary = manual_server_binary();
    if binary.exists() {
        return;
    }

    let status = Command::new("cargo")
        .arg("build")
        .arg("--package")
        .arg("turbomcp-server")
        .arg("--example")
        .arg("manual_server")
        .current_dir(workspace_root())
        .status()
        .expect("Failed to invoke cargo build for manual_server example");

    assert!(status.success(), "Building manual_server example failed");
}

#[tokio::test]
#[ignore = "Requires building manual_server example, run manually with --ignored"]
async fn test_proxy_end_to_end() {
    println!("\n🧪 Testing Proxy End-to-End Functionality");
    println!("==========================================\n");

    // Test 1: Create backend connector
    println!("Test 1: Create backend connector to manual_server...");
    ensure_manual_server_built();

    let binary = manual_server_binary();

    let config = BackendConfig {
        transport: BackendTransport::Stdio {
            command: binary.display().to_string(),
            args: vec![],
            working_dir: Some(workspace_root().display().to_string()),
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
    let expected_tools = vec!["echo"];
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
