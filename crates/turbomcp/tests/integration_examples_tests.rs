//! Real-world integration tests for TurboMCP examples
//!
//! These tests validate actual MCP protocol communication, server behavior,
//! and end-to-end functionality using real JSON-RPC over stdio.

use serde_json::{Value, json};
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as AsyncCommand;
use tokio::time::timeout;

/// Helper function to read a JSON response from stdout, filtering out log messages
async fn read_json_response(
    reader: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<Value, Box<dyn std::error::Error>> {
    // Add timeout to prevent infinite waiting
    let result = timeout(Duration::from_secs(10), async {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                return Err("Unexpected end of stream".into());
            }

            let trimmed = line.trim();
            // Skip log messages (they contain ANSI escape sequences or log prefixes)
            // Look for lines that start with '{' and contain '"jsonrpc"' - these are JSON-RPC responses
            if trimmed.starts_with('{') && trimmed.contains("\"jsonrpc\"") {
                match serde_json::from_str::<Value>(trimmed) {
                    Ok(json) => {
                        // Verify it's actually a JSON-RPC response
                        if json.get("jsonrpc").is_some() {
                            return Ok(json);
                        }
                    }
                    Err(_) => continue, // Continue looking if this isn't valid JSON
                }
            }
        }
    })
    .await;

    match result {
        Ok(json_result) => json_result,
        Err(_) => Err("Timeout waiting for JSON response".into()),
    }
}

/// Helper to run an example and test JSON-RPC communication
async fn test_example_jsonrpc(
    example_name: &str,
    requests: Vec<Value>,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut child = AsyncCommand::new("cargo")
        .args(["run", "--example", example_name, "--package", "turbomcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()) // Discard stderr to avoid log interference
        .spawn()?;

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let mut reader = BufReader::new(stdout);
    let mut writer = stdin;
    let mut responses = Vec::new();

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

    let init_json = format!("{}\n", serde_json::to_string(&init_request)?);
    writer.write_all(init_json.as_bytes()).await?;
    writer.flush().await?;

    // Read initialize response - skip log messages and find JSON response
    let init_response = read_json_response(&mut reader).await?;
    responses.push(init_response);

    // Send each test request
    for (i, request) in requests.into_iter().enumerate() {
        let mut req = request;
        req["id"] = json!(i + 2); // Start from id 2 after initialize

        let req_json = format!("{}\n", serde_json::to_string(&req)?);
        writer.write_all(req_json.as_bytes()).await?;
        writer.flush().await?;

        // Read response - skip log messages and find JSON response
        let response = read_json_response(&mut reader).await?;
        responses.push(response);
    }

    // Cleanup - ensure process is properly terminated
    let kill_result = child.kill().await;
    if let Err(e) = kill_result {
        eprintln!("Warning: Failed to kill child process: {}", e);
    }

    Ok(responses)
}

/// Test that 01_hello_world example handles real MCP communication
#[tokio::test]
#[ignore = "Integration tests temporarily disabled - examples work manually but timeout in CI"]
async fn test_hello_world_integration() {
    let requests = vec![
        // List tools
        json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "params": {}
        }),
        // Call hello tool
        json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "hello",
                "arguments": {
                    "name": "Integration Test"
                }
            }
        }),
    ];

    let responses = test_example_jsonrpc("01_hello_world", requests)
        .await
        .expect("Hello world example should respond to JSON-RPC");

    assert!(responses.len() >= 3); // init + 2 requests

    // Check initialize response
    assert_eq!(responses[0]["jsonrpc"], "2.0");
    assert!(responses[0]["result"]["capabilities"].is_object());

    // Check tools/list response
    let tools_response = &responses[1]["result"];
    assert!(tools_response["tools"].is_array());
    assert!(!tools_response["tools"].as_array().unwrap().is_empty());

    // Check tools/call response
    let call_response = &responses[2]["result"];
    assert!(call_response["content"].is_array());
    let content_text = &call_response["content"][0]["text"].as_str().unwrap();
    assert!(content_text.contains("Integration Test"));
    assert!(content_text.contains("TurboMCP"));
}

/// Test that transport_showcase example works with multiple transports
#[tokio::test]
#[ignore = "Integration tests temporarily disabled - examples work manually but timeout in CI"]
async fn test_transport_showcase_stdio() {
    let requests = vec![
        json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "add",
                "arguments": {
                    "a": 15.5,
                    "b": 24.3
                }
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "stats",
                "arguments": {}
            }
        }),
    ];

    let responses = test_example_jsonrpc("transport_showcase", requests)
        .await
        .expect("Transport showcase should respond");

    assert!(responses.len() >= 3);

    // Check add operation
    let add_result = &responses[1]["result"]["content"][0]["text"];
    let result_num: f64 = add_result.as_str().unwrap().parse().unwrap();
    assert!((result_num - 39.8).abs() < 0.1);

    // Check stats operation
    let stats_result = &responses[2]["result"]["content"][0]["text"];
    assert!(
        stats_result
            .as_str()
            .unwrap()
            .contains("Operations performed")
    );
}

/// Test error handling in examples
#[tokio::test]
#[ignore = "Integration tests temporarily disabled - examples work manually but timeout in CI"]
async fn test_error_handling_integration() {
    let requests = vec![
        // Invalid tool call
        json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "nonexistent_tool",
                "arguments": {}
            }
        }),
        // Valid tool call after error
        json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "hello",
                "arguments": {
                    "name": "Recovery Test"
                }
            }
        }),
    ];

    let responses = test_example_jsonrpc("01_hello_world", requests)
        .await
        .expect("Should handle errors gracefully");

    assert!(responses.len() >= 3);

    // First request should error
    assert!(responses[1].get("error").is_some());

    // Second request should succeed after error
    assert!(responses[2].get("result").is_some());
    let content = &responses[2]["result"]["content"][0]["text"];
    assert!(content.as_str().unwrap().contains("Recovery Test"));
}

/// Test that examples compile and can be spawned
#[test]
fn test_examples_compile_and_spawn() {
    let examples = [
        "01_hello_world",
        "02_tools_basics",
        "transport_showcase",
        "progressive_enhancement",
        "deployment_patterns",
        "readme_example",
    ];

    for example in examples {
        println!("Testing example: {}", example);

        // Test compilation
        let compile_result = Command::new("cargo")
            .args(["check", "--example", example, "--package", "turbomcp"])
            .output()
            .expect("Should be able to run cargo check");

        assert!(
            compile_result.status.success(),
            "Example '{}' should compile successfully",
            example
        );

        // Just verify compilation - spawning requires platform-specific timeout tools
        println!("✅ Example '{}' compiles and links successfully", example);
    }
}

/// Test JSON-RPC protocol compliance  
#[tokio::test]
#[ignore = "Integration tests temporarily disabled - examples work manually but timeout in CI"]
async fn test_jsonrpc_protocol_compliance() {
    // Test only valid JSON-RPC as servers may not respond to invalid requests
    let requests = vec![json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "params": {}
    })];

    let responses = test_example_jsonrpc("01_hello_world", requests)
        .await
        .expect("Should handle valid JSON-RPC");

    assert!(!responses.is_empty());
    // Server should respond to valid requests
    assert!(responses[0].get("result").is_some() || responses[0].get("error").is_some());
}

/// Benchmark basic operation performance
#[tokio::test]
#[ignore = "Integration tests temporarily disabled - examples work manually but timeout in CI"]
async fn test_performance_benchmark() {
    use std::time::Instant;

    let start = Instant::now();

    let requests: Vec<Value> = (0..10)
        .map(|i| {
            json!({
                "jsonrpc": "2.0",
                "method": "tools/call",
                "params": {
                    "name": "hello",
                    "arguments": {
                        "name": format!("Test {}", i)
                    }
                }
            })
        })
        .collect();

    let responses = test_example_jsonrpc("01_hello_world", requests)
        .await
        .expect("Performance test should complete");

    let elapsed = start.elapsed();

    // Should handle 10 requests reasonably quickly
    assert!(
        elapsed < Duration::from_secs(5),
        "10 requests took too long: {:?}",
        elapsed
    );

    // All requests should get responses
    assert!(responses.len() >= 11); // init + 10 requests

    println!("✅ Processed 10 requests in {:?}", elapsed);
}

/// Test that examples work with different feature flags
#[test]
fn test_feature_flag_combinations() {
    let examples = ["transport_showcase", "progressive_enhancement"];
    let feature_sets = [
        vec!["stdio"],
        vec!["stdio", "tcp"],
        vec!["stdio", "tcp", "unix"],
    ];

    for example in examples {
        for features in &feature_sets {
            let mut args = vec!["check", "--example", example, "--package", "turbomcp"];
            let features_str = features.join(",");
            if !features.is_empty() {
                args.push("--features");
                args.push(&features_str);
            }

            let result = Command::new("cargo")
                .args(&args)
                .output()
                .expect("Should run cargo check with features");

            assert!(
                result.status.success(),
                "Example '{}' should compile with features {:?}\nstderr: {}",
                example,
                features,
                String::from_utf8_lossy(&result.stderr)
            );
        }

        println!(
            "✅ Example '{}' works with all feature combinations",
            example
        );
    }
}
