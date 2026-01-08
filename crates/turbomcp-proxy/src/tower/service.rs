//! Tower Service implementation for the proxy

use std::collections::HashMap;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use serde_json::Value;
use tower_service::Service;
use tracing::{debug, error, info};

use turbomcp_protocol::McpError;
use turbomcp_protocol::jsonrpc::JsonRpcRequest;
use turbomcp_transport::axum::McpService;

use crate::proxy::ProxyService;

use super::ProxyLayerConfig;

/// Request wrapper for Tower service
///
/// Wraps a JSON-RPC request with additional metadata.
#[derive(Debug, Clone)]
pub struct ProxyRequest {
    /// The JSON-RPC request
    pub request: JsonRpcRequest,
    /// Request metadata
    pub metadata: HashMap<String, Value>,
    /// Request timestamp
    pub timestamp: Instant,
}

impl ProxyRequest {
    /// Create a new proxy request
    #[must_use]
    pub fn new(request: JsonRpcRequest) -> Self {
        Self {
            request,
            metadata: HashMap::new(),
            timestamp: Instant::now(),
        }
    }

    /// Create a new proxy request with metadata
    #[must_use]
    pub fn with_metadata(request: JsonRpcRequest, metadata: HashMap<String, Value>) -> Self {
        Self {
            request,
            metadata,
            timestamp: Instant::now(),
        }
    }

    /// Get the request method
    #[must_use]
    pub fn method(&self) -> &str {
        &self.request.method
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }

    /// Get metadata value
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
}

/// Response wrapper for Tower service
///
/// Wraps a response with metadata and timing information.
#[derive(Debug, Clone)]
pub struct ProxyResponse {
    /// The response result
    pub result: Option<Value>,
    /// Error information (if failed)
    pub error: Option<turbomcp_protocol::Error>,
    /// Response metadata
    pub metadata: HashMap<String, Value>,
    /// Request duration
    pub duration: Duration,
}

impl ProxyResponse {
    /// Create a successful response
    #[must_use]
    pub fn success(result: Value, duration: Duration) -> Self {
        Self {
            result: Some(result),
            error: None,
            metadata: HashMap::new(),
            duration,
        }
    }

    /// Create an error response
    #[must_use]
    pub fn error(error: turbomcp_protocol::Error, duration: Duration) -> Self {
        Self {
            result: None,
            error: Some(error),
            metadata: HashMap::new(),
            duration,
        }
    }

    /// Check if the response is successful
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Check if the response is an error
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }

    /// Get metadata value
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
}

/// Tower Service that wraps the proxy service
///
/// This service implements `tower::Service` for proxy requests, allowing
/// the proxy to be composed with other Tower layers and services.
#[derive(Clone)]
pub struct ProxyTowerService {
    /// The underlying proxy service
    proxy: Arc<ProxyService>,
    /// Configuration
    config: ProxyLayerConfig,
}

impl std::fmt::Debug for ProxyTowerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyTowerService")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ProxyTowerService {
    /// Create a new proxy Tower service
    #[must_use]
    pub fn new(proxy: ProxyService, config: ProxyLayerConfig) -> Self {
        Self {
            proxy: Arc::new(proxy),
            config,
        }
    }

    /// Create a new proxy Tower service from an Arc
    #[must_use]
    pub fn from_arc(proxy: Arc<ProxyService>, config: ProxyLayerConfig) -> Self {
        Self { proxy, config }
    }

    /// Get a reference to the underlying proxy service
    #[must_use]
    pub fn proxy(&self) -> &ProxyService {
        &self.proxy
    }

    /// Get the configuration
    #[must_use]
    pub fn config(&self) -> &ProxyLayerConfig {
        &self.config
    }
}

impl Service<ProxyRequest> for ProxyTowerService {
    type Response = ProxyResponse;
    type Error = McpError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut req: ProxyRequest) -> Self::Future {
        let method = req.method().to_string();
        let start = Instant::now();

        // Check if this method should bypass proxy processing
        if self.config.should_bypass(&method) {
            return Box::pin(async move {
                Err(McpError::protocol(format!(
                    "Method '{method}' is bypassed by proxy configuration"
                )))
            });
        }

        // Add default metadata from config
        for (key, value) in &self.config.default_metadata {
            if !req.metadata.contains_key(key) {
                req.metadata.insert(key.clone(), value.clone());
            }
        }

        let proxy = Arc::clone(&self.proxy);
        let config = self.config.clone();

        Box::pin(async move {
            if config.enable_logging {
                debug!("Proxy processing request: method={method}");
            }

            // Convert to JSON value for the proxy service
            let request_value = serde_json::to_value(&req.request)
                .map_err(|e| McpError::serialization(e.to_string()))?;

            // Create a dummy session for the proxy service
            let session = turbomcp_transport::tower::SessionInfo {
                id: uuid::Uuid::new_v4().to_string(),
                created_at: std::time::Instant::now(),
                last_activity: std::time::Instant::now(),
                remote_addr: req
                    .metadata
                    .get("remote_addr")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                user_agent: req
                    .metadata
                    .get("user_agent")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                metadata: HashMap::new(),
            };

            // Forward to proxy service
            let result = proxy.process_request(request_value, &session).await;

            let duration = start.elapsed();

            match result {
                Ok(response_value) => {
                    if config.enable_logging {
                        info!("Proxy request completed: method={method}, duration={duration:?}");
                    }

                    let mut response = ProxyResponse::success(response_value, duration);

                    if config.include_timing {
                        response
                            .add_metadata("duration_ms", serde_json::json!(duration.as_millis()));
                    }

                    Ok(response)
                }
                Err(e) => {
                    if config.enable_logging {
                        error!(
                            "Proxy request failed: method={method}, error={e}, duration={duration:?}"
                        );
                    }

                    let mut response = ProxyResponse::error(e.clone(), duration);

                    if config.include_timing {
                        response
                            .add_metadata("duration_ms", serde_json::json!(duration.as_millis()));
                    }

                    // Return the error response but don't error the service
                    Ok(response)
                }
            }
        })
    }
}

/// Implement Service for JSON-RPC requests directly
impl Service<JsonRpcRequest> for ProxyTowerService {
    type Response = ProxyResponse;
    type Error = McpError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: JsonRpcRequest) -> Self::Future {
        // Convert to ProxyRequest and delegate
        let proxy_req = ProxyRequest::new(req);
        Service::<ProxyRequest>::call(self, proxy_req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_protocol::MessageId;
    use turbomcp_protocol::jsonrpc::JsonRpcVersion;

    #[test]
    fn test_proxy_request_creation() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "tools/list".to_string(),
            params: None,
        };

        let proxy_req = ProxyRequest::new(request);
        assert_eq!(proxy_req.method(), "tools/list");
        assert!(proxy_req.metadata.is_empty());
    }

    #[test]
    fn test_proxy_request_metadata() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "tools/list".to_string(),
            params: None,
        };

        let mut proxy_req = ProxyRequest::new(request);
        proxy_req.add_metadata("user_id", json!("user123"));

        assert_eq!(proxy_req.get_metadata("user_id"), Some(&json!("user123")));
    }

    #[test]
    fn test_proxy_response_success() {
        let response = ProxyResponse::success(json!({"tools": []}), Duration::from_millis(100));
        assert!(response.is_success());
        assert!(!response.is_error());
        assert_eq!(response.result, Some(json!({"tools": []})));
    }

    #[test]
    fn test_proxy_response_error() {
        let error = turbomcp_protocol::Error::internal("Test error");
        let response = ProxyResponse::error(error, Duration::from_millis(100));
        assert!(!response.is_success());
        assert!(response.is_error());
    }

    #[test]
    fn test_proxy_response_metadata() {
        let mut response = ProxyResponse::success(json!({}), Duration::from_millis(100));
        response.add_metadata("cache_hit", json!(true));

        assert_eq!(response.get_metadata("cache_hit"), Some(&json!(true)));
    }
}
