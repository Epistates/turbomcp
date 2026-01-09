//! Tower Service implementation for client plugins

use std::collections::HashMap;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use serde_json::Value;
use tower_service::Service;
use tracing::{debug, error};

use turbomcp_protocol::McpError;
use turbomcp_protocol::jsonrpc::JsonRpcRequest;

use crate::plugins::core::{ClientPlugin, PluginContext, RequestContext, ResponseContext};

use super::PluginLayerConfig;

/// MCP request wrapper for Tower service
///
/// Wraps a JSON-RPC request with additional metadata that plugins can modify.
#[derive(Debug, Clone)]
pub struct McpRequest {
    /// The underlying JSON-RPC request
    pub request: JsonRpcRequest,
    /// Request metadata (can be modified by plugins)
    pub metadata: HashMap<String, Value>,
    /// Request timestamp
    pub timestamp: Instant,
}

impl McpRequest {
    /// Create a new MCP request
    pub fn new(request: JsonRpcRequest) -> Self {
        Self {
            request,
            metadata: HashMap::new(),
            timestamp: Instant::now(),
        }
    }

    /// Create a new MCP request with metadata
    pub fn with_metadata(request: JsonRpcRequest, metadata: HashMap<String, Value>) -> Self {
        Self {
            request,
            metadata,
            timestamp: Instant::now(),
        }
    }

    /// Get the request method
    pub fn method(&self) -> &str {
        &self.request.method
    }

    /// Get request parameters
    pub fn params(&self) -> Option<&Value> {
        self.request.params.as_ref()
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    /// Convert to RequestContext for plugin processing
    pub fn to_request_context(&self) -> RequestContext {
        RequestContext::new(self.request.clone(), self.metadata.clone())
    }
}

/// MCP response wrapper for Tower service
///
/// Wraps a response with metadata and timing information.
#[derive(Debug, Clone)]
pub struct McpResponse {
    /// The response data (if successful)
    pub result: Option<Value>,
    /// Error information (if failed)
    pub error: Option<turbomcp_protocol::Error>,
    /// Response metadata
    pub metadata: HashMap<String, Value>,
    /// Request duration
    pub duration: Duration,
}

impl McpResponse {
    /// Create a successful response
    pub fn success(result: Value, duration: Duration) -> Self {
        Self {
            result: Some(result),
            error: None,
            metadata: HashMap::new(),
            duration,
        }
    }

    /// Create an error response
    pub fn error(error: turbomcp_protocol::Error, duration: Duration) -> Self {
        Self {
            result: None,
            error: Some(error),
            metadata: HashMap::new(),
            duration,
        }
    }

    /// Check if the response is successful
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Check if the response is an error
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
}

/// Future type for plugin service responses
pub type PluginServiceFuture<T, E> = BoxFuture<'static, Result<T, E>>;

/// Tower Service that executes client plugins
///
/// This service wraps an inner service and executes plugin middleware
/// before and after each request.
///
/// # Type Parameters
///
/// * `S` - The inner service type
#[derive(Debug, Clone)]
pub struct PluginService<S> {
    inner: S,
    plugins: Vec<Arc<dyn ClientPlugin>>,
    /// Plugin context for initialization (stored for potential future use)
    #[allow(dead_code)]
    plugin_context: Option<PluginContext>,
    config: PluginLayerConfig,
}

impl<S> PluginService<S> {
    /// Create a new plugin service
    pub fn new(inner: S, plugins: Vec<Arc<dyn ClientPlugin>>, config: PluginLayerConfig) -> Self {
        Self {
            inner,
            plugins,
            plugin_context: None,
            config,
        }
    }

    /// Create a new plugin service with plugin context
    pub fn with_context(
        inner: S,
        plugins: Vec<Arc<dyn ClientPlugin>>,
        config: PluginLayerConfig,
        plugin_context: PluginContext,
    ) -> Self {
        Self {
            inner,
            plugins,
            plugin_context: Some(plugin_context),
            config,
        }
    }

    /// Get a reference to the inner service
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Get a mutable reference to the inner service
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Get the plugin count
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Get plugin names
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.name().to_string()).collect()
    }
}

impl<S> Service<McpRequest> for PluginService<S>
where
    S: Service<McpRequest, Response = McpResponse> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Into<McpError>,
{
    type Response = McpResponse;
    type Error = McpError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, mut req: McpRequest) -> Self::Future {
        let method = req.method().to_string();

        // Check if this method should bypass plugin processing
        if self.config.should_bypass(&method) {
            let inner = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, inner);
            return Box::pin(async move { inner.call(req).await.map_err(Into::into) });
        }

        // Add default metadata from config
        for (key, value) in &self.config.default_metadata {
            if !req.metadata.contains_key(key) {
                req.metadata.insert(key.clone(), value.clone());
            }
        }

        let plugins = self.plugins.clone();
        let config = self.config.clone();
        let inner = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move {
            let start = Instant::now();

            // Execute before_request plugins
            let mut request_context = req.to_request_context();

            for plugin in &plugins {
                debug!(
                    "Executing before_request for plugin '{}' on method '{}'",
                    plugin.name(),
                    method
                );

                if let Err(e) = plugin.before_request(&mut request_context).await {
                    error!(
                        "Plugin '{}' before_request failed for method '{}': {}",
                        plugin.name(),
                        method,
                        e
                    );

                    if config.abort_on_request_error {
                        return Err(McpError::internal(format!(
                            "Plugin '{}' error: {}",
                            plugin.name(),
                            e
                        )));
                    }
                }
            }

            // Update request with any modifications from plugins
            req.request = request_context.request.clone();
            req.metadata = request_context.metadata.clone();

            // Call inner service
            let response = inner.call(req.clone()).await.map_err(Into::into)?;

            // Execute after_response plugins
            let mut response_context = ResponseContext::new(
                request_context,
                response.result.clone(),
                response.error.clone(),
                start.elapsed(),
            );

            for plugin in &plugins {
                debug!(
                    "Executing after_response for plugin '{}' on method '{}'",
                    plugin.name(),
                    method
                );

                if let Err(e) = plugin.after_response(&mut response_context).await {
                    error!(
                        "Plugin '{}' after_response failed for method '{}': {}",
                        plugin.name(),
                        method,
                        e
                    );

                    if !config.continue_on_response_error {
                        return Err(McpError::internal(format!(
                            "Plugin '{}' error: {}",
                            plugin.name(),
                            e
                        )));
                    }
                }
            }

            // Build final response with updated metadata
            let mut final_response = response;
            final_response.metadata.extend(response_context.metadata);
            final_response.duration = start.elapsed();

            Ok(final_response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::core::PluginError;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;
    use turbomcp_protocol::MessageId;
    use turbomcp_protocol::jsonrpc::JsonRpcVersion;

    #[derive(Debug)]
    struct MockPlugin {
        name: String,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl MockPlugin {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        #[allow(dead_code)]  // Test utility for future use
        fn get_calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ClientPlugin for MockPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            "1.0.0"
        }

        async fn initialize(&self, _context: &PluginContext) -> Result<(), PluginError> {
            self.calls.lock().unwrap().push("initialize".to_string());
            Ok(())
        }

        async fn before_request(&self, context: &mut RequestContext) -> Result<(), PluginError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("before_request:{}", context.method()));
            Ok(())
        }

        async fn after_response(&self, context: &mut ResponseContext) -> Result<(), PluginError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("after_response:{}", context.method()));
            Ok(())
        }

        async fn handle_custom(
            &self,
            _method: &str,
            _params: Option<Value>,
        ) -> Result<Option<Value>, PluginError> {
            Ok(None)
        }
    }

    #[test]
    fn test_mcp_request_creation() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "test/method".to_string(),
            params: Some(json!({"key": "value"})),
        };

        let mcp_request = McpRequest::new(request);
        assert_eq!(mcp_request.method(), "test/method");
        assert_eq!(mcp_request.params(), Some(&json!({"key": "value"})));
        assert!(mcp_request.metadata.is_empty());
    }

    #[test]
    fn test_mcp_request_metadata() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "test/method".to_string(),
            params: None,
        };

        let mut mcp_request = McpRequest::new(request);
        mcp_request.add_metadata("user_id", json!("user123"));

        assert_eq!(mcp_request.get_metadata("user_id"), Some(&json!("user123")));
    }

    #[test]
    fn test_mcp_response_success() {
        let response = McpResponse::success(json!({"result": "ok"}), Duration::from_millis(100));
        assert!(response.is_success());
        assert!(!response.is_error());
        assert_eq!(response.result, Some(json!({"result": "ok"})));
    }

    #[test]
    fn test_mcp_response_error() {
        let error = turbomcp_protocol::Error::internal("Test error");
        let response = McpResponse::error(error, Duration::from_millis(100));
        assert!(!response.is_success());
        assert!(response.is_error());
    }

    #[test]
    fn test_plugin_service_creation() {
        let mock_service = tower::service_fn(|_req: McpRequest| async move {
            Ok::<_, McpError>(McpResponse::success(
                json!({"result": "ok"}),
                Duration::from_millis(10),
            ))
        });

        let plugins: Vec<Arc<dyn ClientPlugin>> = vec![Arc::new(MockPlugin::new("test"))];

        let service = PluginService::new(mock_service, plugins, PluginLayerConfig::default());

        assert_eq!(service.plugin_count(), 1);
        assert_eq!(service.plugin_names(), vec!["test"]);
    }
}
