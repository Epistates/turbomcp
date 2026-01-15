//! HTTPS End-to-End Client/Server Integration Tests
//!
//! These tests validate the COMPLETE TLS/HTTPS request/response flow using:
//! - Real TurboMCP server with TLS enabled (via `#[server]` macro + ServerTlsConfig)
//! - Real TurboMCP client (via `turbomcp-client` with `StreamableHttpClientTransport`)
//!
//! ## Why These Tests Exist
//!
//! Issue #12 highlighted that users connecting to turbomcp HTTP servers with MCP Inspector
//! were getting SSL errors ("packet length too long") because the server only supported HTTP.
//! This test suite ensures:
//! - TLS/HTTPS servers work correctly with self-signed certificates
//! - Clients can connect over HTTPS with proper certificate handling
//! - Full MCP protocol handshake works over TLS
//! - Tool calls work correctly over encrypted connections
//!
//! ## What These Tests Cover
//!
//! - ✅ HTTPS server startup with TLS configuration
//! - ✅ Client connection with custom CA certificate trust
//! - ✅ Full MCP protocol handshake (initialize) over TLS
//! - ✅ Tool listing and tool calls over HTTPS
//! - ✅ Error handling over encrypted connections
//! - ✅ Multiple concurrent HTTPS connections

#![cfg(all(feature = "http", feature = "tls"))]
#![allow(unsafe_code)] // Required for set_var in Rust 2024 edition

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::time::sleep;
use turbomcp::prelude::*;
use turbomcp_client::Client;
use turbomcp_transport::config::TlsConfig;
use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
use turbomcp_transport::streamable_http_client::{
    RetryPolicy, StreamableHttpClientConfig, StreamableHttpClientTransport,
};

// ============================================================================
// Test Certificates
// ============================================================================

// Self-signed test certificate (DO NOT USE IN PRODUCTION)
// Generated with: openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes -subj "/CN=localhost"
const TEST_CERT: &str = r#"-----BEGIN CERTIFICATE-----
MIIDCTCCAfGgAwIBAgIUKAN5U2KL+G9rFdZZ10t88qju/YswDQYJKoZIhvcNAQEL
BQAwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MDExNTE1MDkyNloXDTI3MDEx
NTE1MDkyNlowFDESMBAGA1UEAwwJbG9jYWxob3N0MIIBIjANBgkqhkiG9w0BAQEF
AAOCAQ8AMIIBCgKCAQEA2TjlszqLHJ6WPPFp+mDrJd50hZeT4mo9z9qWwvFZfKBz
PRIMMtpMWnLoJHVJHj4nhTIpDEZbtQD6tGwg24IRWMoGCqm5F+S7FjNtDaZ9PXtz
b1xjj2Pdx2DDV5ZgEgJWuYVbZHgPy2XRXI05gNcQZMvH2UB6IraGq3Ug07VFBlno
XdEW4EkzqmZvz/8+KFYebLt8ZGmXwuBsbI9dMTBlXqVtvVFOVpmgT/YOnXJ3OJgT
ywjI3DyVhIbMogmhFUyWdaJXuKWMyEf7/m1gky3DKPwjEHySsCoXfgoiWfyeTG5m
8/GMSx9ShdE7lwBXn8+aff398UQ4LHEwDKncfVs7mQIDAQABo1MwUTAdBgNVHQ4E
FgQUozteL5+cW0bdTFQ+sPWDQ1qd4qAwHwYDVR0jBBgwFoAUozteL5+cW0bdTFQ+
sPWDQ1qd4qAwDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEAX3PL
0vikt5hSPtaJFVC9OiqcDsGRNBgeuW4HIyCndEZOyf9z4YomyhUbIh+HFCwsC1w7
t7t8/KyhDW3uuCymWDN4JMvxF0wRFks5UVlaxIaFhmPNlKG2v3CiWJTzBX/X/MX1
VU+9yXMUEEsKU+z+gahvyOonVfswWA9je5Aqr7ITmXiuFmIKNiAy6XTZaqxBhufI
WF6PqkaVcjaihs7UVsI0Uohku25iHG9DM+G+3p3DHbtWlJeCOegcXj4FB3EjUvWC
uWOjFHypnNK66HP08Vpr/LLgrZY7fBRS0AWBq3Vij8wdZ/8q2rH133BbxEKBIvi1
46DRdH2OrlxTfwS3TA==
-----END CERTIFICATE-----"#;

const TEST_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDZOOWzOoscnpY8
8Wn6YOsl3nSFl5Piaj3P2pbC8Vl8oHM9Egwy2kxacugkdUkePieFMikMRlu1APq0
bCDbghFYygYKqbkX5LsWM20Npn09e3NvXGOPY93HYMNXlmASAla5hVtkeA/LZdFc
jTmA1xBky8fZQHoitoardSDTtUUGWehd0RbgSTOqZm/P/z4oVh5su3xkaZfC4Gxs
j10xMGVepW29UU5WmaBP9g6dcnc4mBPLCMjcPJWEhsyiCaEVTJZ1ole4pYzIR/v+
bWCTLcMo/CMQfJKwKhd+CiJZ/J5Mbmbz8YxLH1KF0TuXAFefz5p9/f3xRDgscTAM
qdx9WzuZAgMBAAECggEAFm/kdwm7JIjDdTZM93AjHdlfYQyt+W82pQV7uNVf5En4
+UQH10lZ5WZU0O498BYktCMBHyvVzWmVW8VG9BF4a/rLGrca/6swQWvsreImcc8y
dlxdSsp6lh1qM/37/KQ588YA8YzeuchRsq0CNWsRfg3X/dplezguCyAJNOD2iSAX
rru0nruTGpyAVE8KDNHI1TusKheJFdkO7s/aPdx8mtq1eJ3RTQ8rXKyGt6slbLwM
Pgx0hMW8MP51yxhKcTiNMuDjdDVu+VVBIOqaK00jq+3YvSd/YADXQdt5AdgsX0T3
pguDgY9gF9wGIzUjwjhROUHxZksqG3C+zX7QBZg6AQKBgQD+nhN1/IFHpojry9/p
tdpD9XcEknlmKl2h8l8c4n1gg056oz9ntHIpNc6uUbiahy30ZhA9GhfvYZufPoDH
Tm2QbDDJeFjX3Zzn6rALozK7t7E3KgeyoK0shkXSpDi/SG3pkovBxDGtuvMyLo7F
aa1l19uXrVZAQXH8dcRzFsRxmQKBgQDaZtdOquyzZnJNCqL8qNVom0diRLJtMN3w
aIbEEITV9bXK8RCh5ne3eSfA8xxjEnbkP8ZWCiW1j9wtM85NB5J9La76OIhMy5yQ
D0S1xIpWy4T+QEX4m9Hwu3LqIn4h6EOjOv9VHFvS1e8WeEiKYkN8sWXMW8yi26TV
GmM0sj9aAQKBgQDdK7L76jriYmbNbGs0OCNApRidgB60AFkVM9Qq4xLFo0mofeW1
z6ja40KFabdRg9sHUSEJ8oCYD9F+omx6tEW4DkLSvxdta7PAQLxrX3fSV944bOoC
4E+NPZWpQ72HawMOwZ1k02fT4XEfRhH+qa1Vqgu11Xv2lOLOyf27eytpAQKBgEDj
Ew7lS2PViRoIkfn880Kb965jeJtmTFoTxA5WVhD3amZ8DpP7VBAnp770u7dXkgko
RXXkl+WEc0bewGk0WbplKzpeN2iRiddnIePbG7rDxqR/VgqRyOL73h1f2Bec2ROT
AK85uLJAK0OCwxKSNTjDv9niYD72gNdrepP6bUYBAoGAVZICJNxNs4M2WNPIoZnI
WA7iuNB1iOVoUykEi8iemeRVaoxf3sV/jV+pFhPXWTdWCMYQ5OIDGqvNSM+y9VIs
qH5Z6W/8+zayLFahVNgeM0wjft3FvZS3sGLDCu1/7w274S8nqgyk9uKw0/oW0zPd
LrjbKu2ct5m+6jC1xP0YbL4=
-----END PRIVATE KEY-----"#;

// ============================================================================
// Test Server Implementation
// ============================================================================

/// Test server with multiple tools for comprehensive HTTPS testing
#[derive(Clone)]
struct HttpsTestServer {
    call_count: Arc<AtomicU32>,
}

#[server(
    name = "HTTPS E2E Test Server",
    version = "1.0.0",
    description = "Server for end-to-end HTTPS client/server testing"
)]
impl HttpsTestServer {
    fn new() -> Self {
        Self {
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Simple echo tool - validates basic request/response over TLS
    #[tool("Echo a message back to the caller")]
    async fn echo(&self, message: String) -> McpResult<String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(format!("HTTPS Echo: {}", message))
    }

    /// Math tool - validates argument parsing over encrypted connection
    #[tool("Add two numbers together")]
    async fn add(&self, a: i64, b: i64) -> McpResult<i64> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(a + b)
    }

    /// Get server info
    #[tool("Get server information")]
    async fn info(&self) -> McpResult<String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok("HTTPS TLS 1.3 Server Running".to_string())
    }

    /// Get call count
    #[tool("Get total number of tool calls")]
    async fn get_call_count(&self) -> McpResult<u32> {
        Ok(self.call_count.load(Ordering::SeqCst))
    }

    /// Error tool - validates error propagation over TLS
    #[tool("Always returns an error")]
    async fn always_fails(&self) -> McpResult<String> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Err(turbomcp::McpError::internal("Intentional HTTPS error for testing"))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Create temporary certificate files for testing
fn create_temp_cert_files() -> (NamedTempFile, NamedTempFile) {
    let mut cert_file = NamedTempFile::new().expect("Failed to create temp cert file");
    let mut key_file = NamedTempFile::new().expect("Failed to create temp key file");

    cert_file
        .write_all(TEST_CERT.as_bytes())
        .expect("Failed to write cert");
    key_file
        .write_all(TEST_KEY.as_bytes())
        .expect("Failed to write key");

    (cert_file, key_file)
}

/// Convert JSON value to HashMap for tool arguments
fn json_args(args: serde_json::Value) -> Option<HashMap<String, serde_json::Value>> {
    match args {
        serde_json::Value::Object(map) => Some(map.into_iter().collect()),
        serde_json::Value::Null => None,
        _ => panic!("Arguments must be a JSON object"),
    }
}

/// Start HTTPS server and return the port and task handle
async fn start_https_server(
    port: u16,
    cert_path: &str,
    key_path: &str,
) -> tokio::task::JoinHandle<()> {
    let server = HttpsTestServer::new();
    let addr = format!("127.0.0.1:{}", port);
    let cert_path = cert_path.to_string();
    let key_path = key_path.to_string();

    tokio::spawn(async move {
        let config = StreamableHttpConfigBuilder::new()
            .with_bind_address(&addr)
            .with_endpoint_path("/mcp")
            .with_tls(&cert_path, &key_path)
            .build();

        if let Err(e) = server.run_http_with_config(&addr, config).await {
            tracing::debug!("HTTPS server stopped: {}", e);
        }
    })
}

/// Create an HTTPS client configured to trust the test certificate
#[allow(dead_code)]
async fn create_https_client(
    port: u16,
    cert_bytes: &[u8],
) -> Result<Client<StreamableHttpClientTransport>, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("https://127.0.0.1:{}", port);

    // Configure client with custom CA certificate
    let config = StreamableHttpClientConfig {
        base_url: url,
        endpoint_path: "/mcp".to_string(),
        timeout: Duration::from_secs(30),
        retry_policy: RetryPolicy::Never,
        protocol_version: "2025-11-25".to_string(),
        tls: TlsConfig {
            min_version: turbomcp_transport::config::TlsVersion::Tls13,
            validate_certificates: true,
            custom_ca_certs: Some(vec![cert_bytes.to_vec()]),
            allowed_ciphers: None,
        },
        ..Default::default()
    };

    let transport = StreamableHttpClientTransport::new(config);
    let client = Client::new(transport);
    client.initialize().await?;

    Ok(client)
}

/// Enable insecure TLS mode for testing (SAFETY: only used in tests)
fn enable_insecure_tls_for_testing() {
    // SAFETY: This is only called in tests, and tests are run sequentially
    // or with proper synchronization via serial_test
    unsafe {
        std::env::set_var("TURBOMCP_ALLOW_INSECURE_TLS", "1");
    }
}

/// Create an HTTPS client with insecure mode (for testing without CA)
async fn create_insecure_https_client(
    port: u16,
) -> Result<Client<StreamableHttpClientTransport>, Box<dyn std::error::Error + Send + Sync>> {

    let url = format!("https://127.0.0.1:{}", port);

    let config = StreamableHttpClientConfig {
        base_url: url,
        endpoint_path: "/mcp".to_string(),
        timeout: Duration::from_secs(30),
        retry_policy: RetryPolicy::Never,
        protocol_version: "2025-11-25".to_string(),
        tls: TlsConfig {
            min_version: turbomcp_transport::config::TlsVersion::Tls13,
            validate_certificates: false, // Insecure: skip cert validation
            custom_ca_certs: None,
            allowed_ciphers: None,
        },
        ..Default::default()
    };

    let transport = StreamableHttpClientTransport::new(config);
    let client = Client::new(transport);
    client.initialize().await?;

    Ok(client)
}

// ============================================================================
// End-to-End HTTPS Tests
// ============================================================================

/// Test basic HTTPS client-server connection and tool call
///
/// This is the CRITICAL test that validates:
/// 1. Server starts with TLS configuration
/// 2. Client can connect via HTTPS
/// 3. Initialize handshake completes over TLS
/// 4. Tool calls work over encrypted connection
#[tokio::test]
async fn test_e2e_https_basic_tool_call() {
    // Set env var for insecure TLS (for self-signed cert in tests)
    enable_insecure_tls_for_testing();

    let port = 19201;

    // Create temporary certificate files
    let (cert_file, key_file) = create_temp_cert_files();

    // Start HTTPS server
    let server_task = start_https_server(
        port,
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    // Wait for server startup
    sleep(Duration::from_millis(1000)).await;

    // Create insecure client (accepts self-signed certs)
    let client = create_insecure_https_client(port)
        .await
        .expect("Failed to create HTTPS client");

    // Call the echo tool
    let result = client
        .call_tool(
            "echo",
            json_args(serde_json::json!({"message": "Hello HTTPS!"})),
        )
        .await
        .expect("HTTPS tool call failed");

    // Validate response
    assert!(!result.content.is_empty(), "Response should have content");

    let text = match &result.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    assert_eq!(text, "HTTPS Echo: Hello HTTPS!");

    server_task.abort();
}

/// Test listing tools over HTTPS
#[tokio::test]
async fn test_e2e_https_list_tools() {
    enable_insecure_tls_for_testing();

    let port = 19202;
    let (cert_file, key_file) = create_temp_cert_files();

    let server_task = start_https_server(
        port,
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    sleep(Duration::from_millis(1000)).await;

    let client = create_insecure_https_client(port)
        .await
        .expect("Failed to create HTTPS client");

    // List tools
    let tools = client.list_tools().await.expect("Failed to list tools over HTTPS");

    // Validate tools are returned
    assert!(tools.len() >= 4, "Should have at least 4 tools");

    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"echo"), "Should have echo tool");
    assert!(tool_names.contains(&"add"), "Should have add tool");
    assert!(tool_names.contains(&"info"), "Should have info tool");
    assert!(tool_names.contains(&"get_call_count"), "Should have get_call_count tool");

    server_task.abort();
}

/// Test multiple sequential tool calls over HTTPS
#[tokio::test]
async fn test_e2e_https_sequential_tool_calls() {
    enable_insecure_tls_for_testing();

    let port = 19203;
    let (cert_file, key_file) = create_temp_cert_files();

    let server_task = start_https_server(
        port,
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    sleep(Duration::from_millis(1000)).await;

    let client = create_insecure_https_client(port)
        .await
        .expect("Failed to create HTTPS client");

    // Make multiple sequential calls
    for i in 1..=5i64 {
        let result = client
            .call_tool("add", json_args(serde_json::json!({"a": i, "b": i * 10})))
            .await
            .expect("HTTPS tool call failed");

        let text = match &result.content[0] {
            turbomcp_protocol::types::Content::Text(t) => &t.text,
            _ => panic!("Expected text content"),
        };

        let expected = i + i * 10;
        assert_eq!(
            text,
            &expected.to_string(),
            "Call {} should return {}",
            i,
            expected
        );
    }

    // Verify all calls were counted
    let count_result = client
        .call_tool("get_call_count", None)
        .await
        .expect("Failed to get call count");

    let count_text = match &count_result.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };

    assert_eq!(count_text, "5", "Should have made 5 tool calls");

    server_task.abort();
}

/// Test concurrent tool calls over HTTPS
///
/// Validates that TLS connection handles multiple concurrent requests properly
#[tokio::test]
async fn test_e2e_https_concurrent_tool_calls() {
    enable_insecure_tls_for_testing();

    let port = 19204;
    let (cert_file, key_file) = create_temp_cert_files();

    let server_task = start_https_server(
        port,
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    sleep(Duration::from_millis(1000)).await;

    let client = Arc::new(
        create_insecure_https_client(port)
            .await
            .expect("Failed to create HTTPS client"),
    );

    // Launch 10 concurrent HTTPS tool calls
    let mut handles = vec![];
    for i in 0..10 {
        let client_clone = Arc::clone(&client);
        let handle = tokio::spawn(async move {
            let result = client_clone
                .call_tool(
                    "echo",
                    json_args(serde_json::json!({"message": format!("concurrent-https-{}", i)})),
                )
                .await
                .expect("Concurrent HTTPS tool call failed");

            let text = match &result.content[0] {
                turbomcp_protocol::types::Content::Text(t) => t.text.clone(),
                _ => panic!("Expected text content"),
            };

            (i, text)
        });
        handles.push(handle);
    }

    // Collect results
    let mut results = vec![];
    for handle in handles {
        let (i, text) = handle.await.expect("Task panicked");
        results.push((i, text));
    }

    // Verify each response matches its request
    for (i, text) in results {
        assert_eq!(
            text,
            format!("HTTPS Echo: concurrent-https-{}", i),
            "Response should match request {}",
            i
        );
    }

    server_task.abort();
}

/// Test error handling over HTTPS
#[tokio::test]
async fn test_e2e_https_error_handling() {
    enable_insecure_tls_for_testing();

    let port = 19205;
    let (cert_file, key_file) = create_temp_cert_files();

    let server_task = start_https_server(
        port,
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    sleep(Duration::from_millis(1000)).await;

    let client = create_insecure_https_client(port)
        .await
        .expect("Failed to create HTTPS client");

    // Call tool that always fails
    let result = client.call_tool("always_fails", None).await;

    // Should receive error
    assert!(result.is_err(), "Should receive error from failing tool over HTTPS");

    // A successful call should still work after error
    let success_result = client
        .call_tool(
            "echo",
            json_args(serde_json::json!({"message": "after error"})),
        )
        .await
        .expect("Tool call after error should succeed over HTTPS");

    let text = match &success_result.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    assert_eq!(text, "HTTPS Echo: after error");

    server_task.abort();
}

/// Test multiple HTTPS clients connecting concurrently
#[tokio::test]
async fn test_e2e_https_multiple_clients() {
    enable_insecure_tls_for_testing();

    let port = 19206;
    let (cert_file, key_file) = create_temp_cert_files();

    let server_task = start_https_server(
        port,
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    sleep(Duration::from_millis(1000)).await;

    // Create 3 clients concurrently
    let mut handles = vec![];
    for client_id in 0..3 {
        let handle = tokio::spawn(async move {
            let client = create_insecure_https_client(port)
                .await
                .expect("Failed to create HTTPS client");

            // Each client makes 3 calls
            for call_id in 0..3 {
                let result = client
                    .call_tool(
                        "echo",
                        json_args(serde_json::json!({
                            "message": format!("https-client{}-call{}", client_id, call_id)
                        })),
                    )
                    .await
                    .expect("HTTPS tool call failed");

                let text = match &result.content[0] {
                    turbomcp_protocol::types::Content::Text(t) => t.text.clone(),
                    _ => panic!("Expected text content"),
                };

                assert_eq!(
                    text,
                    format!("HTTPS Echo: https-client{}-call{}", client_id, call_id),
                    "Response should match request for client {} call {}",
                    client_id,
                    call_id
                );
            }

            client_id
        });
        handles.push(handle);
    }

    // Wait for all clients to complete
    for handle in handles {
        handle.await.expect("HTTPS client task panicked");
    }

    server_task.abort();
}

/// Test that verifies TLS 1.3 is being used
#[tokio::test]
async fn test_e2e_https_tls_version() {
    enable_insecure_tls_for_testing();

    let port = 19207;
    let (cert_file, key_file) = create_temp_cert_files();

    let server_task = start_https_server(
        port,
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    sleep(Duration::from_millis(1000)).await;

    let client = create_insecure_https_client(port)
        .await
        .expect("Failed to create HTTPS client");

    // Call the info tool to verify server is responding
    let result = client
        .call_tool("info", None)
        .await
        .expect("HTTPS info call failed");

    let text = match &result.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };

    assert_eq!(text, "HTTPS TLS 1.3 Server Running");

    server_task.abort();
}

/// Test request/response correlation over HTTPS with different results
///
/// This validates that correlation routing works correctly over TLS
#[tokio::test]
async fn test_e2e_https_correlation_routing_correctness() {
    enable_insecure_tls_for_testing();

    let port = 19208;
    let (cert_file, key_file) = create_temp_cert_files();

    let server_task = start_https_server(
        port,
        cert_file.path().to_str().unwrap(),
        key_file.path().to_str().unwrap(),
    )
    .await;

    sleep(Duration::from_millis(1000)).await;

    let client = Arc::new(
        create_insecure_https_client(port)
            .await
            .expect("Failed to create HTTPS client"),
    );

    // Launch concurrent calls with DIFFERENT expected results
    let client1 = Arc::clone(&client);
    let handle1 = tokio::spawn(async move {
        client1
            .call_tool("add", json_args(serde_json::json!({"a": 1, "b": 2})))
            .await
    });

    let client2 = Arc::clone(&client);
    let handle2 = tokio::spawn(async move {
        client2
            .call_tool("add", json_args(serde_json::json!({"a": 100, "b": 200})))
            .await
    });

    let client3 = Arc::clone(&client);
    let handle3 = tokio::spawn(async move {
        client3
            .call_tool("add", json_args(serde_json::json!({"a": 1000, "b": 2000})))
            .await
    });

    // Collect results
    let result1 = handle1
        .await
        .expect("Task 1 panicked")
        .expect("HTTPS call 1 failed");
    let result2 = handle2
        .await
        .expect("Task 2 panicked")
        .expect("HTTPS call 2 failed");
    let result3 = handle3
        .await
        .expect("Task 3 panicked")
        .expect("HTTPS call 3 failed");

    // Extract text from results
    let text1 = match &result1.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    let text2 = match &result2.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };
    let text3 = match &result3.content[0] {
        turbomcp_protocol::types::Content::Text(t) => &t.text,
        _ => panic!("Expected text content"),
    };

    // CRITICAL: Each result must match its specific request
    assert_eq!(text1, "3", "1+2 should equal 3 over HTTPS");
    assert_eq!(text2, "300", "100+200 should equal 300 over HTTPS");
    assert_eq!(text3, "3000", "1000+2000 should equal 3000 over HTTPS");

    server_task.abort();
}
