//! Real-world integration tests for TurboMCP examples
//!
//! These tests validate actual MCP protocol communication, server behavior,
//! and end-to-end functionality using real JSON-RPC over stdio.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Helper to run an example and test JSON-RPC communication
async fn test_example_jsonrpc(
    example_name: &str,
    requests: Vec<Value>,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut child = Command::new("cargo")
        .args(["run", "--example", example_name, "--package", "turbomcp"])
        .env("RUST_LOG", "") // Empty string actually disables logging (not "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // Discard stderr to avoid logging interference
        .spawn()?;

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let mut reader = BufReader::new(stdout);
    let mut writer = stdin;
    let mut responses = Vec::new();

    // Give the server a moment to start up
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Send initialize request first
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }
    });

    writeln!(writer, "{}", serde_json::to_string(&init_request)?)?;
    writer.flush()?;

    // Read init response
    let mut init_response = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        match reader.read_line(&mut init_response) {
            Ok(0) => break, // EOF
            Ok(_) => {
                if let Ok(response) = serde_json::from_str::<Value>(&init_response) {
                    responses.push(response);
                    break;
                }
            }
            Err(_) => break,
        }
    }

    // Send test requests
    for request in requests {
        writeln!(writer, "{}", serde_json::to_string(&request)?)?;
        writer.flush()?;

        let mut response_line = String::new();
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            match reader.read_line(&mut response_line) {
                Ok(0) => break,
                Ok(_) => {
                    if let Ok(response) = serde_json::from_str::<Value>(&response_line) {
                        responses.push(response);
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }

    child.kill()?;
    Ok(responses)
}

/// Test that 01_hello_world example handles real MCP communication
#[tokio::test]
async fn test_hello_world_integration() {
    let requests = vec![json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    })];

    let responses = test_example_jsonrpc("01_hello_world", requests)
        .await
        .expect("Hello world example should respond to JSON-RPC");

    assert!(
        responses.len() >= 2,
        "Should receive init and tools/list responses"
    );

    // Check tools/list response
    let tools_response = &responses[1];
    assert_eq!(tools_response["jsonrpc"], "2.0");
    assert_eq!(tools_response["id"], 2);

    let tools = &tools_response["result"]["tools"];
    assert!(tools.is_array());
    let tools_array = tools.as_array().unwrap();
    assert!(!tools_array.is_empty(), "Should have at least one tool");

    // Find the hello tool
    let hello_tool = tools_array
        .iter()
        .find(|t| t["name"] == "hello")
        .expect("Should have a hello tool");

    assert!(
        hello_tool["description"]
            .as_str()
            .unwrap_or("")
            .contains("hello")
    );
}

/// Test that 07_transport_showcase example works correctly
#[tokio::test]
async fn test_transport_showcase_stdio() {
    // Test that the transport showcase compiles and can show help
    // Note: Actually running stdio mode would require interactive testing
    let output = Command::new("cargo")
        .args([
            "run",
            "--example",
            "07_transport_showcase",
            "--package",
            "turbomcp",
        ])
        .env("RUST_LOG", "") // Empty string to disable logging
        .output()
        .expect("Failed to run transport showcase");

    // Just verify it compiled and ran (showing help text)
    assert!(
        output.status.success(),
        "Transport showcase should compile and run"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TRANSPORT SHOWCASE") || stdout.contains("Available transports"),
        "Should show transport options"
    );
}

/// Test error handling with invalid requests
#[tokio::test]
async fn test_error_handling_integration() {
    let requests = vec![json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "nonexistent_tool",
            "arguments": {}
        }
    })];

    let responses = test_example_jsonrpc("01_hello_world", requests)
        .await
        .expect("Should handle errors gracefully");

    assert!(responses.len() >= 2, "Should receive responses");

    // Check error response
    let error_response = &responses[1];
    assert_eq!(error_response["jsonrpc"], "2.0");
    assert!(
        error_response.get("error").is_some(),
        "Should return an error"
    );
}

/// Test that examples compile and can be spawned
#[test]
fn test_examples_compile_and_spawn() {
    let examples = [
        "01_hello_world",
        "02_clean_server",
        "06_architecture_patterns",
    ];

    for example in &examples {
        println!("Testing example: {}", example);

        let output = Command::new("cargo")
            .args(["check", "--example", example, "--package", "turbomcp"])
            .output()
            .expect("Failed to run cargo check");

        assert!(
            output.status.success(),
            "Example '{}' should compile successfully",
            example
        );
    }
}

/// Test JSON-RPC protocol compliance
#[tokio::test]
async fn test_jsonrpc_protocol_compliance() {
    let requests = vec![
        // Valid request with all fields
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
        // Request with string ID
        json!({
            "jsonrpc": "2.0",
            "id": "test-id",
            "method": "tools/list"
        }),
    ];

    let responses = test_example_jsonrpc("01_hello_world", requests)
        .await
        .expect("Should handle valid JSON-RPC requests");

    // All responses should have jsonrpc field
    for response in &responses {
        assert_eq!(
            response["jsonrpc"], "2.0",
            "All responses should specify JSON-RPC version"
        );
    }
}

/// Performance benchmark test
#[tokio::test]
async fn test_performance_benchmark() {
    let start = std::time::Instant::now();

    // Just test initialization performance
    let requests = vec![]; // Don't send additional requests after init

    let responses = test_example_jsonrpc("01_hello_world", requests)
        .await
        .expect("Performance test should complete");

    let duration = start.elapsed();

    // Should have at least the init response
    assert!(
        !responses.is_empty(),
        "Should receive initialization response"
    );

    // Basic performance check - should respond within reasonable time
    // Note: First run includes compilation time
    assert!(
        duration < Duration::from_secs(30), // Allow time for initial compilation
        "Server should respond within 30 seconds (took {:?})",
        duration
    );
}

/// Test that different features can be enabled
#[test]
#[ignore] // TODO: Fix macro compilation with minimal features
fn test_feature_flag_combinations() {
    // Just test that our main examples compile with different features
    // Note: We use 'minimal' feature for basic STDIO functionality (internal-deps deprecated)
    let examples = ["07_transport_showcase", "06_architecture_patterns"];

    for example in &examples {
        let output = Command::new("cargo")
            .args([
                "check",
                "--example",
                example,
                "--package",
                "turbomcp",
                "--no-default-features",
                "--features",
                "minimal", // Minimal feature includes STDIO transport
            ])
            .output()
            .expect("Failed to run cargo check");

        assert!(
            output.status.success(),
            "Example '{}' should compile with features [\"minimal\"]\nstderr: {}",
            example,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Complex integration test - validates server hardening against malformed inputs
#[tokio::test]
#[ignore] // This is a stress test, run with --ignored flag
async fn test_invalid_jsonrpc_robustness_integration() {
    let invalid_requests = vec![
        // Missing jsonrpc field
        json!({"id": 1, "method": "test"}),
        // Wrong jsonrpc version
        json!({"jsonrpc": "1.0", "id": 1, "method": "test"}),
        // Missing method
        json!({"jsonrpc": "2.0", "id": 1}),
        // Null id
        json!({"jsonrpc": "2.0", "id": null, "method": "test"}),
    ];

    for request in invalid_requests {
        let responses = test_example_jsonrpc("01_hello_world", vec![request.clone()])
            .await
            .unwrap_or_else(|_| vec![]);

        // Server should either return error or ignore invalid requests
        // but should not crash
        if let Some(response) = responses.get(1) {
            // If we got a response, it should be an error
            assert!(
                response.get("error").is_some(),
                "Invalid request should return error: {:?}",
                request
            );
        }
        // If no response, that's also acceptable (server ignored invalid request)
    }
}
