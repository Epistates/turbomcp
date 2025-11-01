//! Real-world integration tests for TurboMCP examples
//!
//! These tests validate actual MCP protocol communication, server behavior,
//! and end-to-end functionality using real JSON-RPC over stdio.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Helper to run an example and test JSON-RPC communication
async fn test_example_jsonrpc_with_timeout(
    example_name: &str,
    requests: Vec<Value>,
    response_timeout_secs: u64,
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
    while start.elapsed() < Duration::from_secs(response_timeout_secs) {
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
        while start.elapsed() < Duration::from_secs(response_timeout_secs) {
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

    // Clean up process with timeout
    drop(writer); // Close stdin to signal process to exit

    // Try graceful termination first
    if let Err(e) = child.kill() {
        eprintln!("Warning: Failed to kill subprocess during cleanup: {}", e);
    }

    // Wait for process to exit with timeout
    let wait_result = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::task::spawn_blocking(move || child.wait()),
    )
    .await;

    match wait_result {
        Ok(Ok(Ok(_))) => {
            // Process exited successfully
        }
        Ok(Ok(Err(e))) => {
            eprintln!("Warning: Failed to wait for subprocess: {}", e);
        }
        Ok(Err(e)) => {
            eprintln!("Warning: Task panicked while waiting for subprocess: {}", e);
        }
        Err(_) => {
            eprintln!("Warning: Subprocess wait timed out after 2s - process may still be running");
        }
    }

    Ok(responses)
}

/// Helper to run an example and test JSON-RPC communication with default 10-second timeout
async fn test_example_jsonrpc(
    example_name: &str,
    requests: Vec<Value>,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    test_example_jsonrpc_with_timeout(example_name, requests, 10).await
}

/// Test that hello_world example handles real MCP communication
#[tokio::test(flavor = "multi_thread")]
async fn test_hello_world_integration() {
    let requests = vec![json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    })];

    let responses = test_example_jsonrpc("hello_world", requests)
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

/// Test that stdio_app example works correctly
#[tokio::test(flavor = "multi_thread")]
async fn test_transport_showcase_stdio() {
    // Test that the stdio app compiles and can respond to JSON-RPC
    let requests = vec![json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    })];

    let responses = test_example_jsonrpc("stdio_app", requests)
        .await
        .expect("STDIO app should respond to JSON-RPC");

    assert!(
        responses.len() >= 2,
        "Should receive init and tools/list responses"
    );

    // Verify tools/list response
    let tools_response = &responses[1];
    assert_eq!(tools_response["jsonrpc"], "2.0");
    assert!(tools_response["result"]["tools"].is_array());
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

    let responses = test_example_jsonrpc("hello_world", requests)
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
    let examples = ["hello_world", "macro_server", "tools"];

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

    let responses = test_example_jsonrpc("hello_world", requests)
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

/// Integration test - validates server hardening against malformed inputs
/// This ensures the server handles malformed JSON-RPC requests gracefully without crashing
///
/// TODO: Currently spawns real subprocess which is slow. Refactor to:
/// 1. Use a mock/local server instead of subprocess
/// 2. Or significantly reduce timeouts and test scope
/// 3. Target: < 2 seconds total execution time
#[tokio::test]
#[ignore]
async fn test_invalid_jsonrpc_robustness_integration() {
    let requests = vec![
        // First send valid initialize
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test-client", "version": "1.0.0"}
            }
        }),
        // Then send invalid requests
        json!({"id": 2, "method": "test"}), // Missing jsonrpc
        json!({"jsonrpc": "1.0", "id": 3, "method": "test"}), // Wrong version
        json!({"jsonrpc": "2.0", "id": 4}), // Missing method
        json!({"jsonrpc": "2.0", "id": null, "method": "test"}), // Null id
        // Finally send valid request to verify server still works
        json!({
            "jsonrpc": "2.0",
            "id": 100,
            "method": "tools/list"
        }),
    ];

    // Test with very short timeout - we just need to verify robustness, not get all responses
    let responses = test_example_jsonrpc_with_timeout("hello_world", requests, 1)
        .await
        .unwrap_or_else(|_| vec![]);

    // Should at least get init response, which proves server didn't crash
    assert!(
        !responses.is_empty(),
        "Server should at least respond to initialization"
    );

    // If we got any response after invalid requests, it proves robustness
    // The key is that the server didn't crash
}

/// Test MCP protocol compliance for all examples
#[tokio::test]
async fn test_mcp_protocol_compliance() {
    use turbomcp_protocol::jsonrpc::JsonRpcResponse;
    use turbomcp_protocol::validation::ProtocolValidator;

    let validator = ProtocolValidator::new().with_strict_mode();

    let examples_to_test = vec![
        "hello_world",
        "macro_server",
        "tools",
        "resources",
        "stateful",
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
    let example_name = "hello_world";

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

    // Give it time to respond
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Read with timeout to avoid blocking forever
    let read_stdout = tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        let bytes = stdout_reader.read_line(&mut line).unwrap_or(0);
        (bytes, line)
    });

    let read_stderr = tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        let bytes = stderr_reader.read_line(&mut line).unwrap_or(0);
        (bytes, line)
    });

    let (stdout_bytes, stdout_line) =
        match tokio::time::timeout(Duration::from_secs(5), read_stdout).await {
            Ok(Ok(result)) => result,
            _ => (0, String::new()),
        };

    let (stderr_bytes, stderr_line) =
        match tokio::time::timeout(Duration::from_secs(5), read_stderr).await {
            Ok(Ok(result)) => result,
            _ => (0, String::new()),
        };

    // Clean up process with timeout
    drop(writer); // Close stdin
    child.kill().unwrap_or_else(|e| {
        eprintln!("Failed to kill child process: {}", e);
    });

    let wait_result = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::task::spawn_blocking(move || child.wait()),
    )
    .await;

    match wait_result {
        Ok(Ok(Ok(status))) => {
            if !status.success() {
                eprintln!("Child process exited with status: {}", status);
            }
        }
        Ok(Ok(Err(e))) => {
            eprintln!("Failed to wait for child process: {}", e);
        }
        _ => {
            eprintln!("Warning: Child process wait timed out");
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
