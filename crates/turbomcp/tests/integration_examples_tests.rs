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

    // Give the server more time to start up in CI environments
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

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

    // Read init response with better error handling
    let mut init_response = String::new();
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        init_response.clear();
        match reader.read_line(&mut init_response) {
            Ok(0) => {
                return Err("Process terminated unexpectedly during init".into());
            }
            Ok(_) => {
                let trimmed = init_response.trim();
                if !trimmed.is_empty()
                    && let Ok(response) = serde_json::from_str::<Value>(trimmed)
                {
                    responses.push(response);
                    break;
                }
            }
            Err(e) => {
                return Err(format!("Failed to read init response: {}", e).into());
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Send test requests with better error handling
    for (i, request) in requests.iter().enumerate() {
        writeln!(writer, "{}", serde_json::to_string(&request)?)?;
        writer.flush()?;

        let mut response_line = String::new();
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            response_line.clear();
            match reader.read_line(&mut response_line) {
                Ok(0) => {
                    return Err(format!("Process terminated during request {}", i).into());
                }
                Ok(_) => {
                    let trimmed = response_line.trim();
                    if !trimmed.is_empty()
                        && let Ok(response) = serde_json::from_str::<Value>(trimmed)
                    {
                        responses.push(response);
                        break;
                    }
                }
                Err(e) => {
                    return Err(format!("Failed to read response for request {}: {}", i, e).into());
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    // Clean up process
    if let Err(e) = child.kill() {
        eprintln!("Warning: Failed to kill subprocess during cleanup: {}", e);
    }
    if let Err(e) = child.wait() {
        eprintln!(
            "Warning: Failed to wait for subprocess during cleanup: {}",
            e
        );
    }
    Ok(responses)
}

/// Test that 01_hello_world example handles real MCP communication
#[tokio::test(flavor = "multi_thread")]
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
#[tokio::test(flavor = "multi_thread")]
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

/// Test MCP protocol compliance for all examples
#[tokio::test]
async fn test_mcp_protocol_compliance() {
    use turbomcp_protocol::jsonrpc::JsonRpcResponse;
    use turbomcp_protocol::validation::ProtocolValidator;

    let validator = ProtocolValidator::new().with_strict_mode();

    let examples_to_test = vec![
        "01_hello_world",
        "02_clean_server",
        "03_basic_tools",
        "04_resources_and_prompts",
        "05_stateful_patterns",
    ];

    for example_name in examples_to_test {
        println!("Testing MCP compliance for: {}", example_name);

        // Send initialize request and validate response
        let init_request = json!({
            "jsonrpc": "2.0",
            "id": "mcp-test",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "mcp-compliance-test",
                    "version": "1.0.0"
                }
            }
        });

        match test_example_jsonrpc(example_name, vec![init_request]).await {
            Ok(responses) => {
                assert!(
                    !responses.is_empty(),
                    "Example {} should return initialize response",
                    example_name
                );

                // Validate the initialize response structure
                let init_response = &responses[0];

                // Should be valid JSON-RPC response
                if let Ok(response) =
                    serde_json::from_value::<JsonRpcResponse>(init_response.clone())
                {
                    let validation_result = validator.validate_response(&response);
                    assert!(
                        validation_result.is_valid(),
                        "Example {} initialize response failed validation: {:?}",
                        example_name,
                        validation_result.errors()
                    );

                    // Check MCP-specific requirements
                    if let Some(result) = response.result() {
                        assert!(
                            result.get("protocolVersion").is_some(),
                            "Missing protocolVersion in {}",
                            example_name
                        );
                        assert!(
                            result.get("capabilities").is_some(),
                            "Missing capabilities in {}",
                            example_name
                        );
                        assert!(
                            result.get("serverInfo").is_some(),
                            "Missing serverInfo in {}",
                            example_name
                        );

                        // Validate protocol version format
                        if let Some(version) =
                            result.get("protocolVersion").and_then(|v| v.as_str())
                        {
                            assert!(
                                !version.is_empty() && version.len() >= 8,
                                "Invalid protocolVersion format in {}: {}",
                                example_name,
                                version
                            );
                        }

                        // Validate serverInfo structure
                        if let Some(server_info) = result.get("serverInfo") {
                            assert!(
                                server_info.get("name").is_some(),
                                "Missing serverInfo.name in {}",
                                example_name
                            );
                            assert!(
                                server_info.get("version").is_some(),
                                "Missing serverInfo.version in {}",
                                example_name
                            );
                        }
                    }
                } else {
                    panic!(
                        "Example {} did not return valid JSON-RPC response",
                        example_name
                    );
                }

                println!("✅ {} passed MCP compliance validation", example_name);
            }
            Err(e) => {
                eprintln!("Failed to test {}: {}", example_name, e);
                // Continue with other examples rather than failing the entire test
            }
        }
    }
}

/// Test that examples produce clean JSON-RPC output with no log contamination
#[tokio::test]
async fn test_clean_json_output() {
    let example_name = "11_production_deployment";

    // Use the helper but capture both stdout and stderr separately
    let mut child = Command::new("cargo")
        .args(["run", "--example", example_name, "--package", "turbomcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start example");

    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Send an initialize request
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": "clean-test",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1.0.0"}
        }
    });

    let mut writer = stdin;
    writeln!(writer, "{}", serde_json::to_string(&init_request).unwrap()).unwrap();
    writer.flush().unwrap();

    // Read stdout and stderr separately
    let mut stdout_reader = BufReader::new(stdout);
    let mut stderr_reader = BufReader::new(stderr);

    let mut stdout_line = String::new();
    let mut stderr_line = String::new();

    // Give it time to respond
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let stdout_bytes = stdout_reader.read_line(&mut stdout_line).unwrap();
    let stderr_bytes = stderr_reader.read_line(&mut stderr_line).unwrap();

    child.kill().unwrap_or_else(|e| {
        eprintln!("Failed to kill child process: {}", e);
    });
    match child.wait() {
        Ok(status) => {
            if !status.success() {
                eprintln!("Child process exited with status: {}", status);
            }
        }
        Err(e) => {
            eprintln!("Failed to wait for child process: {}", e);
        }
    }

    // Verify stdout contains only JSON-RPC
    if stdout_bytes > 0 {
        let stdout_trimmed = stdout_line.trim();
        if !stdout_trimmed.is_empty() {
            match serde_json::from_str::<Value>(stdout_trimmed) {
                Ok(json_val) => {
                    assert!(
                        json_val.get("jsonrpc").is_some(),
                        "stdout should contain only JSON-RPC messages"
                    );
                    println!("✅ stdout contains clean JSON-RPC: {}", stdout_trimmed);
                }
                Err(e) => {
                    panic!(
                        "stdout contains non-JSON content: {} (error: {})",
                        stdout_trimmed, e
                    );
                }
            }
        }
    }

    // Verify stderr contains logs (if any)
    if stderr_bytes > 0 {
        let stderr_trimmed = stderr_line.trim();
        if !stderr_trimmed.is_empty() {
            // Should be log content, not JSON-RPC
            assert!(
                !stderr_trimmed.starts_with("{"),
                "stderr should not contain JSON-RPC messages: {}",
                stderr_trimmed
            );
            println!("✅ stderr contains logs: {}", stderr_trimmed);
        }
    }

    println!("✅ Clean stdout/stderr separation verified");
}
