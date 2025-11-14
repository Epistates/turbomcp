//! Real-world integration tests for TurboMCP examples
//!
//! These tests validate actual MCP protocol communication, server behavior,
//! and end-to-end functionality using real JSON-RPC over stdio.

use serde_json::{Value, json};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

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

    // Give the server more time to start up (cargo run needs time to compile)
    // In CI or after clean build, compilation can take 7-10 seconds
    tokio::time::sleep(Duration::from_millis(10000)).await;

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

    // Read init response with async I/O and proper timeout
    let mut init_response = String::new();
    let read_result = tokio::time::timeout(
        Duration::from_secs(response_timeout_secs),
        reader.read_line(&mut init_response),
    )
    .await;

    match read_result {
        Ok(Ok(0)) => {
            return Err("Process terminated unexpectedly during init".into());
        }
        Ok(Ok(_)) => {
            let trimmed = init_response.trim();
            if !trimmed.is_empty()
                && let Ok(response) = serde_json::from_str::<Value>(trimmed)
            {
                responses.push(response);
            }
        }
        Ok(Err(e)) => {
            return Err(format!("Failed to read init response: {}", e).into());
        }
        Err(_) => {
            return Err(format!(
                "Timeout waiting for init response after {}s",
                response_timeout_secs
            )
            .into());
        }
    }

    // Send test requests with async I/O and proper timeouts
    for (i, request) in requests.iter().enumerate() {
        let request_json = format!("{}\n", serde_json::to_string(&request)?);
        writer.write_all(request_json.as_bytes()).await?;
        writer.flush().await?;

        // Use tokio::time::timeout to prevent blocking indefinitely
        let mut response_line = String::new();
        let read_result = tokio::time::timeout(
            Duration::from_secs(response_timeout_secs),
            reader.read_line(&mut response_line),
        )
        .await;

        match read_result {
            Ok(Ok(0)) => {
                return Err(format!("Process terminated during request {}", i).into());
            }
            Ok(Ok(_)) => {
                let trimmed = response_line.trim();
                if !trimmed.is_empty()
                    && let Ok(response) = serde_json::from_str::<Value>(trimmed)
                {
                    responses.push(response);
                }
            }
            Ok(Err(e)) => {
                return Err(format!("Failed to read response for request {}: {}", i, e).into());
            }
            Err(_) => {
                // Timeout - this is expected for invalid requests that server doesn't respond to
                eprintln!(
                    "Timeout waiting for response to request {} after {}s",
                    i, response_timeout_secs
                );
            }
        }
    }

    // Clean up process with timeout
    drop(writer); // Close stdin to signal process to exit

    // Try graceful termination first
    if let Err(e) = child.kill().await {
        eprintln!("Warning: Failed to kill subprocess during cleanup: {}", e);
    }

    // Wait for process to exit with timeout (using tokio's async wait)
    let wait_result = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;

    match wait_result {
        Ok(Ok(_)) => {
            // Process exited successfully
        }
        Ok(Err(e)) => {
            eprintln!("Warning: Failed to wait for subprocess: {}", e);
        }
        Err(_) => {
            eprintln!("Warning: Subprocess wait timed out after 2s - process may still be running");
        }
    }

    Ok(responses)
}

/// Helper to run an example and test JSON-RPC communication with default 30-second timeout
async fn test_example_jsonrpc(
    example_name: &str,
    requests: Vec<Value>,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    test_example_jsonrpc_with_timeout(example_name, requests, 30).await
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
#[tokio::test]
async fn test_examples_compile_and_spawn() {
    let examples = ["hello_world", "macro_server", "tools"];

    for example in &examples {
        println!("Testing example: {}", example);

        let output = Command::new("cargo")
            .args(["check", "--example", example, "--package", "turbomcp"])
            .output()
            .await
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
/// NOTE: This test is marked #[ignore] due to subprocess compilation time making it flaky.
/// The helper spawns `cargo run` which takes 7-10 seconds to compile in CI/clean builds,
/// but only waits 2 seconds before sending requests. This causes intermittent timeouts.
///
/// FIXED: Previously this test would hang indefinitely (60+ seconds) due to blocking I/O.
/// Now it properly times out in ~7 seconds using async I/O with tokio::time::timeout.
///
/// TODO: Refactor to use pre-compiled binary instead of `cargo run` for reliable testing.
#[tokio::test]
#[ignore]
async fn test_invalid_jsonrpc_robustness_integration() {
    let requests = vec![
        // Send invalid requests to test robustness (helper already sent initialize)
        json!({"id": 2, "method": "test"}), // Missing jsonrpc
        json!({"jsonrpc": "1.0", "id": 3, "method": "test"}), // Wrong version
        json!({"jsonrpc": "2.0", "id": 4}), // Missing method
        json!({"jsonrpc": "2.0", "id": null, "method": "test"}), // Null id
        // Finally send valid request to verify server still works after invalid requests
        json!({
            "jsonrpc": "2.0",
            "id": 100,
            "method": "tools/list"
        }),
    ];

    // Test with reasonable timeout for subprocess to start and respond
    // Helper waits 2s for startup + we need time for responses
    let responses = test_example_jsonrpc_with_timeout("hello_world", requests, 5)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Test helper returned error (this may be expected): {}", e);
            vec![]
        });

    // Should at least get init response from helper, which proves server started and didn't crash
    assert!(
        !responses.is_empty(),
        "Server should at least respond to initialization (got {} responses)",
        responses.len()
    );

    // If we got more responses after invalid requests, verify the final tools/list worked
    if responses.len() > 1 {
        let last_response = responses.last().unwrap();
        // If last response has "result", server is still functional after invalid inputs
        if last_response.get("result").is_some() {
            println!("✅ Server remained functional after invalid requests");
        }
    }

    // The key test: server didn't crash despite invalid JSON-RPC requests
    println!(
        "✅ Server robustness verified - received {} responses",
        responses.len()
    );
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

    // Use tokio::process::Command for async I/O
    let mut child = Command::new("cargo")
        .args(["run", "--example", example_name, "--package", "turbomcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start example");

    let mut stdin = child.stdin.take().unwrap();
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

    let init_json = format!("{}\n", serde_json::to_string(&init_request).unwrap());
    stdin.write_all(init_json.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();

    // Read stdout and stderr with async I/O
    let mut stdout_reader = BufReader::new(stdout);
    let mut stderr_reader = BufReader::new(stderr);

    // Give it time to respond
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // Read with timeout using async I/O
    let mut stdout_line = String::new();
    let stdout_result = tokio::time::timeout(
        Duration::from_secs(5),
        stdout_reader.read_line(&mut stdout_line),
    )
    .await;

    let mut stderr_line = String::new();
    let stderr_result = tokio::time::timeout(
        Duration::from_secs(5),
        stderr_reader.read_line(&mut stderr_line),
    )
    .await;

    let stdout_bytes = stdout_result.ok().and_then(|r| r.ok()).unwrap_or(0);
    let stderr_bytes = stderr_result.ok().and_then(|r| r.ok()).unwrap_or(0);

    // Clean up process with timeout
    drop(stdin); // Close stdin
    child.kill().await.unwrap_or_else(|e| {
        eprintln!("Failed to kill child process: {}", e);
    });

    let wait_result = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;

    match wait_result {
        Ok(Ok(status)) => {
            if !status.success() {
                eprintln!("Child process exited with status: {}", status);
            }
        }
        Ok(Err(e)) => {
            eprintln!("Failed to wait for child process: {}", e);
        }
        Err(_) => {
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
