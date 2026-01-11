//! HTTP transport for WASI MCP clients
//!
//! This transport uses WASI Preview 2's `wasi:http/outgoing-handler`
//! interface for HTTP-based MCP communication.
//!
//! # Features
//!
//! - Full JSON-RPC over HTTP POST
//! - Custom headers support
//! - Configurable timeouts
//! - TLS support via runtime

use super::transport::{
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, Transport, TransportError,
};
use serde::{Serialize, de::DeserializeOwned};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// HTTP transport for WASI environments
///
/// Uses `wasi:http/outgoing-handler` for HTTP requests.
///
/// # Example
///
/// ```ignore
/// use turbomcp_wasm::wasi::HttpTransport;
///
/// let transport = HttpTransport::new("https://api.example.com/mcp")
///     .with_header("Authorization", "Bearer token123")
///     .with_timeout_ms(30_000);
///
/// let result: serde_json::Value = transport.request("tools/list", None::<()>)?;
/// ```
pub struct HttpTransport {
    /// Base URL for the MCP server
    #[allow(dead_code)] // Used in WASI builds
    base_url: String,
    /// Custom headers to include in requests
    headers: RefCell<HashMap<String, String>>,
    /// Request timeout in milliseconds (0 = no timeout)
    timeout_ms: u64,
    /// Next request ID
    next_id: AtomicU64,
    /// Whether the transport is open
    is_open: AtomicBool,
}

impl HttpTransport {
    /// Create a new HTTP transport
    ///
    /// # Arguments
    ///
    /// * `base_url` - Base URL for the MCP server (e.g., `https://api.example.com/mcp`)
    #[must_use]
    pub fn new(base_url: impl Into<String>) -> Self {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        Self {
            base_url: base_url.into(),
            headers: RefCell::new(headers),
            timeout_ms: 30_000, // 30 second default
            next_id: AtomicU64::new(1),
            is_open: AtomicBool::new(true),
        }
    }

    /// Add a custom header to all requests
    #[must_use]
    pub fn with_header(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.borrow_mut().insert(key.into(), value.into());
        self
    }

    /// Set request timeout in milliseconds
    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Get the next request ID
    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Make an HTTP POST request using WASI
    fn http_post(&self, body: &str) -> Result<String, TransportError> {
        #[cfg(target_os = "wasi")]
        {
            use wasi::http::outgoing_handler;
            use wasi::http::types::{
                Fields, Method, OutgoingBody, OutgoingRequest, RequestOptions, Scheme,
            };

            // Parse the URL
            let url = url::Url::parse(&self.base_url)
                .map_err(|e| TransportError::Connection(format!("Invalid URL: {e}")))?;

            let scheme = match url.scheme() {
                "https" => Some(Scheme::Https),
                "http" => Some(Scheme::Http),
                _ => None,
            };

            let authority = url.host_str().map(|h| {
                if let Some(port) = url.port() {
                    format!("{h}:{port}")
                } else {
                    h.to_string()
                }
            });

            let path_with_query = if let Some(query) = url.query() {
                format!("{}?{query}", url.path())
            } else {
                url.path().to_string()
            };

            // Create headers
            let headers = Fields::new();
            for (key, value) in self.headers.borrow().iter() {
                headers
                    .append(&key.to_lowercase(), value.as_bytes())
                    .map_err(|e| {
                        TransportError::Connection(format!("Failed to set header: {e:?}"))
                    })?;
            }

            // Create the request
            let request = OutgoingRequest::new(headers);
            request
                .set_method(&Method::Post)
                .map_err(|_| TransportError::Connection("Failed to set HTTP method".to_string()))?;

            if let Some(scheme) = scheme {
                request
                    .set_scheme(Some(&scheme))
                    .map_err(|_| TransportError::Connection("Failed to set scheme".to_string()))?;
            }

            if let Some(ref auth) = authority {
                request.set_authority(Some(auth.as_str())).map_err(|_| {
                    TransportError::Connection("Failed to set authority".to_string())
                })?;
            }

            request
                .set_path_with_query(Some(&path_with_query))
                .map_err(|_| TransportError::Connection("Failed to set path".to_string()))?;

            // Write the body
            let outgoing_body = request.body().map_err(|_| {
                TransportError::Connection("Failed to get request body".to_string())
            })?;

            {
                let body_stream = outgoing_body.write().map_err(|_| {
                    TransportError::Connection("Failed to get body stream".to_string())
                })?;

                body_stream
                    .blocking_write_and_flush(body.as_bytes())
                    .map_err(|e| TransportError::Io(format!("Failed to write body: {e:?}")))?;

                // Drop the stream to signal we're done writing
                drop(body_stream);
            }

            // Finish the body
            OutgoingBody::finish(outgoing_body, None)
                .map_err(|e| TransportError::Connection(format!("Failed to finish body: {e:?}")))?;

            // Set request options
            let options = RequestOptions::new();
            if self.timeout_ms > 0 {
                options
                    .set_connect_timeout(Some(self.timeout_ms))
                    .map_err(|_| TransportError::Connection("Failed to set timeout".to_string()))?;
            }

            // Send the request
            let future_response =
                outgoing_handler::handle(request, Some(options)).map_err(|e| {
                    TransportError::Connection(format!("Failed to send request: {e:?}"))
                })?;

            // Wait for response
            let response = loop {
                if let Some(result) = future_response.get() {
                    break result
                        .map_err(|_| TransportError::Connection("Response error".to_string()))?
                        .map_err(|e| TransportError::Connection(format!("HTTP error: {e:?}")))?;
                }
                // Yield to allow other work (in single-threaded WASM)
            };

            // Check status
            let status = response.status();
            if status < 200 || status >= 300 {
                return Err(TransportError::Http {
                    status,
                    message: format!("HTTP request failed with status {status}"),
                });
            }

            // Read response body
            let incoming_body = response.consume().map_err(|_| {
                TransportError::Connection("Failed to get response body".to_string())
            })?;

            let body_stream = incoming_body
                .stream()
                .map_err(|_| TransportError::Connection("Failed to get body stream".to_string()))?;

            let mut response_bytes = Vec::new();
            loop {
                match body_stream.blocking_read(65536) {
                    Ok(chunk) => {
                        if chunk.is_empty() {
                            break;
                        }
                        response_bytes.extend_from_slice(&chunk);
                    }
                    Err(_) => break,
                }
            }

            String::from_utf8(response_bytes)
                .map_err(|e| TransportError::Io(format!("Invalid UTF-8 in response: {e}")))
        }

        #[cfg(not(target_os = "wasi"))]
        {
            // Fallback for non-WASI builds (testing) - just return an error
            // In real non-WASI environments, use turbomcp-http crate instead
            let _ = body;
            Err(TransportError::Connection(
                "HTTP transport requires WASI runtime. Use turbomcp-http for native builds."
                    .to_string(),
            ))
        }
    }
}

impl Transport for HttpTransport {
    fn request<P, R>(&self, method: &str, params: Option<P>) -> Result<R, TransportError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        if !self.is_open.load(Ordering::SeqCst) {
            return Err(TransportError::Connection(
                "Transport is closed".to_string(),
            ));
        }

        // Create request with unique ID
        let id = self.next_request_id();
        let request = JsonRpcRequest::new(id, method, params);

        // Serialize and send
        let request_json = serde_json::to_string(&request)?;
        let response_json = self.http_post(&request_json)?;

        // Parse response
        let response: JsonRpcResponse<R> = serde_json::from_str(&response_json)?;

        // Check response ID matches
        if response.id != Some(id) {
            return Err(TransportError::Protocol(format!(
                "Response ID mismatch: expected {id}, got {:?}",
                response.id
            )));
        }

        response.into_result()
    }

    fn notify<P>(&self, method: &str, params: Option<P>) -> Result<(), TransportError>
    where
        P: Serialize,
    {
        if !self.is_open.load(Ordering::SeqCst) {
            return Err(TransportError::Connection(
                "Transport is closed".to_string(),
            ));
        }

        let notification = JsonRpcNotification::new(method, params);
        let json = serde_json::to_string(&notification)?;

        // For notifications, we still send but ignore the response
        let _ = self.http_post(&json)?;

        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.is_open.load(Ordering::SeqCst)
    }

    fn close(&self) -> Result<(), TransportError> {
        self.is_open.store(false, Ordering::SeqCst);
        Ok(())
    }
}

// Note: HttpTransport is designed for single-threaded WASM environments
// Send/Sync are not implemented due to RefCell usage, but this is fine
// since WASM is single-threaded

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_transport_creation() {
        let transport = HttpTransport::new("https://api.example.com/mcp");
        assert!(transport.is_ready());
        assert_eq!(transport.base_url, "https://api.example.com/mcp");
    }

    #[test]
    fn test_http_transport_with_headers() {
        let transport = HttpTransport::new("https://api.example.com/mcp")
            .with_header("Authorization", "Bearer token123")
            .with_header("X-Custom", "value");

        let headers = transport.headers.borrow();
        assert_eq!(
            headers.get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(headers.get("X-Custom"), Some(&"value".to_string()));
    }

    #[test]
    fn test_http_transport_with_timeout() {
        let transport = HttpTransport::new("https://api.example.com/mcp").with_timeout_ms(60_000);
        assert_eq!(transport.timeout_ms, 60_000);
    }

    #[test]
    fn test_http_transport_close() {
        let transport = HttpTransport::new("https://api.example.com/mcp");
        assert!(transport.is_ready());
        transport.close().unwrap();
        assert!(!transport.is_ready());
    }
}
