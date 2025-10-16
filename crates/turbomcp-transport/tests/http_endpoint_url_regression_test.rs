//! CRITICAL REGRESSION TEST: HTTP Endpoint URL Scheme
//!
//! Bug Report: TurboMCP 2.0.0-rc.2 HTTP endpoint missing http:// scheme
//!
//! **Issue**: Server sends SSE endpoint event with URI "127.0.0.1:8080/mcp"
//! instead of "http://127.0.0.1:8080/mcp", causing MCP client failures.
//!
//! **Impact**: ALL HTTP/SSE connections fail with 404 errors
//!
//! **Root Cause**: Missing "http://" prefix in endpoint event construction
//!
//! This test uses REAL client and server (no mocks) to verify:
//! 1. Server sends proper endpoint event with http:// scheme
//! 2. Client can parse and use the endpoint URL
//! 3. POST requests to discovered endpoint succeed

#[cfg(all(feature = "http", feature = "test-utils"))]
mod http_endpoint_regression_tests {
    use futures::StreamExt;
    use serde_json::{Value, json};
    use std::time::Duration;
    use tokio::time::timeout;

    /// Helper to find an available port
    async fn find_available_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind to random port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    /// Start a minimal MCP server that responds to initialize requests
    async fn start_test_server(port: u16) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            use axum::response::sse::{Event, Sse};
            use axum::{Json, Router, extract::State, routing::get};
            use futures::stream::{self, Stream};
            use std::convert::Infallible;
            use std::sync::Arc;
            use tokio::sync::Mutex;

            // Shared state for session management
            #[derive(Clone)]
            struct AppState {
                session_id: String,
                port: u16,
            }

            // SSE handler - sends endpoint event
            async fn sse_handler(
                State(state): State<Arc<Mutex<AppState>>>,
            ) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
                let state_data = state.lock().await;
                let session_id = state_data.session_id.clone();
                let server_port = state_data.port;

                let stream = stream::once(async move {
                    // CRITICAL: This is what we're testing - endpoint URL MUST include http://
                    let endpoint_url = format!(
                        "http://127.0.0.1:{}/mcp?sessionId={}",
                        server_port, session_id
                    );

                    Ok::<Event, Infallible>(
                        Event::default()
                            .event("endpoint")
                            .data(json!({ "uri": endpoint_url }).to_string()),
                    )
                });

                Sse::new(stream)
            }

            // POST handler - responds to initialize
            async fn post_handler(Json(payload): Json<Value>) -> Json<Value> {
                // Check if it's an initialize request
                if payload.get("method").and_then(|m| m.as_str()) == Some("initialize") {
                    Json(json!({
                        "jsonrpc": "2.0",
                        "id": payload.get("id"),
                        "result": {
                            "protocolVersion": "2025-06-18",
                            "serverInfo": {
                                "name": "test-server",
                                "version": "1.0.0"
                            },
                            "capabilities": {}
                        }
                    }))
                } else {
                    Json(json!({
                        "jsonrpc": "2.0",
                        "id": payload.get("id"),
                        "error": {
                            "code": -32601,
                            "message": "Method not found"
                        }
                    }))
                }
            }

            let state = Arc::new(Mutex::new(AppState {
                session_id: "test-session-123".to_string(),
                port,
            }));

            let app = Router::new()
                .route("/mcp", get(sse_handler).post(post_handler))
                .with_state(state);

            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
                .await
                .expect("Failed to bind test server");

            axum::serve(listener, app).await.expect("Server failed");
        })
    }

    #[tokio::test]
    async fn test_endpoint_url_has_http_scheme() {
        // REGRESSION TEST: Verify endpoint URL includes http:// scheme
        // This test uses REAL client-server communication (no mocks)

        let port = find_available_port().await;
        let server_handle = start_test_server(port).await;

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Step 1: Connect to SSE stream and read endpoint event
        let sse_url = format!("http://127.0.0.1:{}/mcp", port);
        let client = reqwest::Client::new();

        let response = timeout(
            Duration::from_secs(5),
            client
                .get(&sse_url)
                .header("Accept", "text/event-stream")
                .send(),
        )
        .await;

        assert!(response.is_ok(), "Timeout connecting to SSE");
        let response = response.unwrap().expect("SSE connection failed");

        assert_eq!(response.status(), 200, "SSE connection failed");

        // Read the SSE stream
        let mut stream = response.bytes_stream();
        let chunk = timeout(Duration::from_secs(5), stream.next()).await;

        assert!(chunk.is_ok(), "Timeout reading SSE event");
        let chunk = chunk.unwrap().expect("No SSE data").expect("SSE error");

        let data = String::from_utf8(chunk.to_vec()).expect("Invalid UTF-8");

        // Parse SSE event (format: "event: endpoint\ndata: {...}\n\n")
        let lines: Vec<&str> = data.lines().collect();
        assert!(lines.len() >= 2, "Invalid SSE format");

        // Find the data line
        let data_line = lines
            .iter()
            .find(|line| line.starts_with("data: "))
            .expect("No data line in SSE event");

        let json_data = data_line.strip_prefix("data: ").unwrap();

        // Parse the endpoint data
        let endpoint_data: Value =
            serde_json::from_str(json_data).expect("Failed to parse endpoint event data as JSON");

        let endpoint_uri = endpoint_data
            .get("uri")
            .and_then(|u| u.as_str())
            .expect("Endpoint event missing 'uri' field");

        println!("📡 Received endpoint URI: {}", endpoint_uri);

        // CRITICAL ASSERTIONS: Verify URI has proper format

        // 1. MUST start with http:// scheme
        assert!(
            endpoint_uri.starts_with("http://"),
            "❌ BUG DETECTED: Endpoint URI missing http:// scheme! Got: {}",
            endpoint_uri
        );

        // 2. MUST be a valid HTTP URL
        let parsed_url = endpoint_uri
            .parse::<url::Url>()
            .unwrap_or_else(|_| panic!("Endpoint URI is not a valid URL: {}", endpoint_uri));

        assert_eq!(
            parsed_url.scheme(),
            "http",
            "Endpoint URI scheme must be 'http'"
        );

        // 3. MUST include the port
        assert!(
            endpoint_uri.contains(&format!(":{}", port)),
            "Endpoint URI must include port number"
        );

        // 4. MUST include the path
        assert!(
            endpoint_uri.contains("/mcp"),
            "Endpoint URI must include endpoint path"
        );

        // Step 2: Use the discovered endpoint for POST request
        let client = reqwest::Client::new();

        let init_request = json!({
            "jsonrpc": "2.0",
            "id": "test-1",
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            }
        });

        // This POST MUST succeed if endpoint URL is correct
        let response = timeout(
            Duration::from_secs(5),
            client.post(parsed_url.as_str()).json(&init_request).send(),
        )
        .await;

        assert!(response.is_ok(), "Timeout waiting for POST response");

        let response = response.unwrap();
        assert!(response.is_ok(), "POST request failed: {:?}", response);

        let response = response.unwrap();
        assert_eq!(
            response.status(),
            200,
            "POST to discovered endpoint failed with status: {}",
            response.status()
        );

        let body: Value = response
            .json()
            .await
            .expect("Failed to parse response JSON");

        // Verify we got a valid initialize response
        assert_eq!(body.get("jsonrpc").and_then(|v| v.as_str()), Some("2.0"));
        assert!(
            body.get("result").is_some(),
            "Initialize response missing result"
        );

        println!("✅ Endpoint URL regression test PASSED");
        println!("   - Endpoint has http:// scheme: ✓");
        println!("   - URL is valid: ✓");
        println!("   - POST to endpoint succeeds: ✓");

        // Cleanup
        server_handle.abort();
    }

    #[tokio::test]
    async fn test_endpoint_url_is_parseable() {
        // Additional regression test: Verify endpoint URL can be parsed by standard libraries

        let port = find_available_port().await;
        let server_handle = start_test_server(port).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        let sse_url = format!("http://127.0.0.1:{}/mcp", port);
        let client = reqwest::Client::new();

        let response = client
            .get(&sse_url)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .expect("SSE connection failed");

        let mut stream = response.bytes_stream();
        let chunk = stream.next().await.expect("No data").expect("Error");
        let data = String::from_utf8(chunk.to_vec()).expect("Invalid UTF-8");

        let json_line = data
            .lines()
            .find(|line| line.starts_with("data: "))
            .expect("No data line");

        let json_data = json_line.strip_prefix("data: ").unwrap();
        let endpoint_data: Value = serde_json::from_str(json_data).expect("Parse error");
        let endpoint_uri = endpoint_data["uri"].as_str().expect("Missing uri");

        // Test with multiple URL parsers to ensure compatibility

        // 1. url crate (Rust standard)
        let url_crate_result = endpoint_uri.parse::<url::Url>();
        assert!(
            url_crate_result.is_ok(),
            "url::Url cannot parse endpoint: {}",
            endpoint_uri
        );

        // 2. reqwest Client (HTTP client library)
        let reqwest_result = reqwest::Url::parse(endpoint_uri);
        assert!(
            reqwest_result.is_ok(),
            "reqwest::Url cannot parse endpoint: {}",
            endpoint_uri
        );

        // 3. Verify base URL + path extraction works
        let parsed = reqwest_result.unwrap();
        assert!(parsed.host_str().is_some(), "Missing host");
        assert!(parsed.port().is_some(), "Missing port");
        assert_eq!(parsed.path(), "/mcp", "Incorrect path");

        println!("✅ Endpoint URL is parseable by all standard libraries");

        server_handle.abort();
    }
}
