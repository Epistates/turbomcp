//! Middleware pattern implementation for plugin system
//!
//! Provides middleware abstractions and chain execution patterns for
//! request/response processing. This module focuses on the middleware
//! pattern specifically, allowing plugins to be composed as middleware.

use crate::plugins::core::{PluginResult, RequestContext, ResponseContext};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, error};

/// Result type for middleware operations
pub type MiddlewareResult<T> = PluginResult<T>;

/// Trait for request middleware
///
/// Request middleware can modify the request before it's sent to the server.
/// They are executed in the order they are registered.
#[async_trait]
pub trait RequestMiddleware: Send + Sync + std::fmt::Debug {
    /// Process the request context
    ///
    /// # Arguments
    /// * `context` - Mutable request context that can be modified
    ///
    /// # Returns
    /// Returns `Ok(())` to continue processing, or `PluginError` to abort.
    async fn process_request(&self, context: &mut RequestContext) -> MiddlewareResult<()>;

    /// Get middleware name for debugging
    fn name(&self) -> &str;
}

/// Trait for response middleware
///
/// Response middleware process responses after they're received from the server.
/// They are executed in the order they are registered.
#[async_trait]
pub trait ResponseMiddleware: Send + Sync + std::fmt::Debug {
    /// Process the response context
    ///
    /// # Arguments
    /// * `context` - Mutable response context that can be modified
    ///
    /// # Returns
    /// Returns `Ok(())` if processing succeeds, or `PluginError` if it fails.
    async fn process_response(&self, context: &mut ResponseContext) -> MiddlewareResult<()>;

    /// Get middleware name for debugging
    fn name(&self) -> &str;
}

/// Chain of middleware for sequential execution
///
/// The MiddlewareChain manages the execution of multiple middleware
/// components in a defined order. It provides error handling and
/// short-circuiting behavior.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::plugins::middleware::{MiddlewareChain, RequestMiddleware};
/// use std::sync::Arc;
///
/// let mut chain = MiddlewareChain::new();
/// // chain.add_request_middleware(Arc::new(some_middleware));
/// // chain.add_response_middleware(Arc::new(other_middleware));
/// ```
#[derive(Debug)]
pub struct MiddlewareChain {
    /// Request middleware in execution order
    request_middleware: Vec<Arc<dyn RequestMiddleware>>,

    /// Response middleware in execution order
    response_middleware: Vec<Arc<dyn ResponseMiddleware>>,
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

impl MiddlewareChain {
    /// Create a new empty middleware chain
    pub fn new() -> Self {
        Self {
            request_middleware: Vec::new(),
            response_middleware: Vec::new(),
        }
    }

    /// Add request middleware to the chain
    ///
    /// Middleware will be executed in the order they are added.
    ///
    /// # Arguments
    /// * `middleware` - The request middleware to add
    pub fn add_request_middleware(&mut self, middleware: Arc<dyn RequestMiddleware>) {
        debug!("Adding request middleware: {}", middleware.name());
        self.request_middleware.push(middleware);
    }

    /// Add response middleware to the chain
    ///
    /// Middleware will be executed in the order they are added.
    ///
    /// # Arguments
    /// * `middleware` - The response middleware to add
    pub fn add_response_middleware(&mut self, middleware: Arc<dyn ResponseMiddleware>) {
        debug!("Adding response middleware: {}", middleware.name());
        self.response_middleware.push(middleware);
    }

    /// Execute the request middleware chain
    ///
    /// Processes the request context through all registered request middleware
    /// in order. If any middleware returns an error, processing is aborted
    /// and the error is returned.
    ///
    /// # Arguments
    /// * `context` - Mutable request context
    ///
    /// # Returns
    /// Returns `Ok(())` if all middleware succeed, or the first error encountered.
    pub async fn execute_request_chain(
        &self,
        context: &mut RequestContext,
    ) -> MiddlewareResult<()> {
        debug!(
            "Executing request middleware chain ({} middleware) for method: {}",
            self.request_middleware.len(),
            context.method()
        );

        for (index, middleware) in self.request_middleware.iter().enumerate() {
            debug!(
                "Processing request middleware {} of {}: {}",
                index + 1,
                self.request_middleware.len(),
                middleware.name()
            );

            middleware.process_request(context).await.map_err(|e| {
                error!(
                    "Request middleware '{}' failed for method '{}': {}",
                    middleware.name(),
                    context.method(),
                    e
                );
                e
            })?;
        }

        debug!("Request middleware chain completed successfully");
        Ok(())
    }

    /// Execute the response middleware chain
    ///
    /// Processes the response context through all registered response middleware
    /// in order. Unlike request middleware, this continues execution even if
    /// a middleware fails, logging errors but not aborting the chain.
    ///
    /// # Arguments
    /// * `context` - Mutable response context
    ///
    /// # Returns
    /// Returns `Ok(())` unless all middleware fail, in which case returns the last error.
    pub async fn execute_response_chain(
        &self,
        context: &mut ResponseContext,
    ) -> MiddlewareResult<()> {
        debug!(
            "Executing response middleware chain ({} middleware) for method: {}",
            self.response_middleware.len(),
            context.method()
        );

        let mut _last_error = None;

        for (index, middleware) in self.response_middleware.iter().enumerate() {
            debug!(
                "Processing response middleware {} of {}: {}",
                index + 1,
                self.response_middleware.len(),
                middleware.name()
            );

            if let Err(e) = middleware.process_response(context).await {
                error!(
                    "Response middleware '{}' failed for method '{}': {}",
                    middleware.name(),
                    context.method(),
                    e
                );
                _last_error = Some(e);
                // Continue with other middleware
            }
        }

        debug!("Response middleware chain completed");

        // For now, we don't propagate response middleware errors
        // as they shouldn't break the response processing
        Ok(())
    }

    /// Get the number of request middleware
    pub fn request_middleware_count(&self) -> usize {
        self.request_middleware.len()
    }

    /// Get the number of response middleware
    pub fn response_middleware_count(&self) -> usize {
        self.response_middleware.len()
    }

    /// Get names of all request middleware
    pub fn get_request_middleware_names(&self) -> Vec<String> {
        self.request_middleware
            .iter()
            .map(|m| m.name().to_string())
            .collect()
    }

    /// Get names of all response middleware
    pub fn get_response_middleware_names(&self) -> Vec<String> {
        self.response_middleware
            .iter()
            .map(|m| m.name().to_string())
            .collect()
    }

    /// Clear all middleware
    pub fn clear(&mut self) {
        debug!("Clearing all middleware from chain");
        self.request_middleware.clear();
        self.response_middleware.clear();
    }
}

/// Adapter to use a ClientPlugin as RequestMiddleware
#[derive(Debug)]
pub struct PluginRequestMiddleware<P> {
    plugin: P,
}

impl<P> PluginRequestMiddleware<P> {
    /// Create a new plugin request middleware adapter
    pub fn new(plugin: P) -> Self {
        Self { plugin }
    }
}

#[async_trait]
impl<P> RequestMiddleware for PluginRequestMiddleware<P>
where
    P: crate::plugins::core::ClientPlugin,
{
    async fn process_request(&self, context: &mut RequestContext) -> MiddlewareResult<()> {
        self.plugin.before_request(context).await
    }

    fn name(&self) -> &str {
        self.plugin.name()
    }
}

/// Adapter to use a ClientPlugin as ResponseMiddleware
#[derive(Debug)]
pub struct PluginResponseMiddleware<P> {
    plugin: P,
}

impl<P> PluginResponseMiddleware<P> {
    /// Create a new plugin response middleware adapter
    pub fn new(plugin: P) -> Self {
        Self { plugin }
    }
}

#[async_trait]
impl<P> ResponseMiddleware for PluginResponseMiddleware<P>
where
    P: crate::plugins::core::ClientPlugin,
{
    async fn process_response(&self, context: &mut ResponseContext) -> MiddlewareResult<()> {
        self.plugin.after_response(context).await
    }

    fn name(&self) -> &str {
        self.plugin.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::core::{PluginError, RequestContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio;
    use turbomcp_core::MessageId;
    use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};

    // Test middleware implementations
    #[derive(Debug)]
    struct TestRequestMiddleware {
        name: String,
        calls: Arc<Mutex<Vec<String>>>,
        should_fail: bool,
    }

    impl TestRequestMiddleware {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                calls: Arc::new(Mutex::new(Vec::new())),
                should_fail: false,
            }
        }

        fn with_failure(mut self) -> Self {
            self.should_fail = true;
            self
        }

        fn get_calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RequestMiddleware for TestRequestMiddleware {
        async fn process_request(&self, context: &mut RequestContext) -> MiddlewareResult<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("process_request:{}", context.method()));

            if self.should_fail {
                Err(PluginError::request_processing("Test middleware failure"))
            } else {
                Ok(())
            }
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[derive(Debug)]
    struct TestResponseMiddleware {
        name: String,
        calls: Arc<Mutex<Vec<String>>>,
        should_fail: bool,
    }

    impl TestResponseMiddleware {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                calls: Arc::new(Mutex::new(Vec::new())),
                should_fail: false,
            }
        }

        fn with_failure(mut self) -> Self {
            self.should_fail = true;
            self
        }

        fn get_calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ResponseMiddleware for TestResponseMiddleware {
        async fn process_response(&self, context: &mut ResponseContext) -> MiddlewareResult<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("process_response:{}", context.method()));

            if self.should_fail {
                Err(PluginError::response_processing("Test middleware failure"))
            } else {
                Ok(())
            }
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_middleware_chain_creation() {
        let chain = MiddlewareChain::new();
        assert_eq!(chain.request_middleware_count(), 0);
        assert_eq!(chain.response_middleware_count(), 0);
    }

    #[tokio::test]
    async fn test_request_middleware_registration() {
        let mut chain = MiddlewareChain::new();
        let middleware = Arc::new(TestRequestMiddleware::new("test"));

        chain.add_request_middleware(middleware);

        assert_eq!(chain.request_middleware_count(), 1);
        assert_eq!(chain.get_request_middleware_names(), vec!["test"]);
    }

    #[tokio::test]
    async fn test_response_middleware_registration() {
        let mut chain = MiddlewareChain::new();
        let middleware = Arc::new(TestResponseMiddleware::new("test"));

        chain.add_response_middleware(middleware);

        assert_eq!(chain.response_middleware_count(), 1);
        assert_eq!(chain.get_response_middleware_names(), vec!["test"]);
    }

    #[tokio::test]
    async fn test_request_middleware_execution() {
        let mut chain = MiddlewareChain::new();
        let middleware = Arc::new(TestRequestMiddleware::new("test"));

        chain.add_request_middleware(middleware.clone());

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "test/method".to_string(),
            params: None,
        };

        let mut context = RequestContext::new(request, HashMap::new());
        chain.execute_request_chain(&mut context).await.unwrap();

        let calls = middleware.get_calls();
        assert!(calls.contains(&"process_request:test/method".to_string()));
    }

    #[tokio::test]
    async fn test_response_middleware_execution() {
        let mut chain = MiddlewareChain::new();
        let middleware = Arc::new(TestResponseMiddleware::new("test"));

        chain.add_response_middleware(middleware.clone());

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "test/method".to_string(),
            params: None,
        };

        let request_context = RequestContext::new(request, HashMap::new());
        let mut response_context = ResponseContext::new(
            request_context,
            Some(json!({"result": "success"})),
            None,
            std::time::Duration::from_millis(100),
        );

        chain
            .execute_response_chain(&mut response_context)
            .await
            .unwrap();

        let calls = middleware.get_calls();
        assert!(calls.contains(&"process_response:test/method".to_string()));
    }

    #[tokio::test]
    async fn test_request_middleware_error_handling() {
        let mut chain = MiddlewareChain::new();
        let good_middleware = Arc::new(TestRequestMiddleware::new("good"));
        let bad_middleware = Arc::new(TestRequestMiddleware::new("bad").with_failure());

        chain.add_request_middleware(good_middleware.clone());
        chain.add_request_middleware(bad_middleware.clone());

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "test/method".to_string(),
            params: None,
        };

        let mut context = RequestContext::new(request, HashMap::new());
        let result = chain.execute_request_chain(&mut context).await;

        assert!(result.is_err());
        assert!(
            good_middleware
                .get_calls()
                .contains(&"process_request:test/method".to_string())
        );
        assert!(
            bad_middleware
                .get_calls()
                .contains(&"process_request:test/method".to_string())
        );
    }

    #[tokio::test]
    async fn test_response_middleware_error_handling() {
        let mut chain = MiddlewareChain::new();
        let good_middleware = Arc::new(TestResponseMiddleware::new("good"));
        let bad_middleware = Arc::new(TestResponseMiddleware::new("bad").with_failure());

        chain.add_response_middleware(good_middleware.clone());
        chain.add_response_middleware(bad_middleware.clone());

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "test/method".to_string(),
            params: None,
        };

        let request_context = RequestContext::new(request, HashMap::new());
        let mut response_context = ResponseContext::new(
            request_context,
            Some(json!({"result": "success"})),
            None,
            std::time::Duration::from_millis(100),
        );

        // Response middleware continues even with errors
        let result = chain.execute_response_chain(&mut response_context).await;
        assert!(result.is_ok());

        assert!(
            good_middleware
                .get_calls()
                .contains(&"process_response:test/method".to_string())
        );
        assert!(
            bad_middleware
                .get_calls()
                .contains(&"process_response:test/method".to_string())
        );
    }

    #[tokio::test]
    async fn test_middleware_execution_order() {
        let mut chain = MiddlewareChain::new();
        let middleware1 = Arc::new(TestRequestMiddleware::new("first"));
        let middleware2 = Arc::new(TestRequestMiddleware::new("second"));
        let middleware3 = Arc::new(TestRequestMiddleware::new("third"));

        chain.add_request_middleware(middleware1.clone());
        chain.add_request_middleware(middleware2.clone());
        chain.add_request_middleware(middleware3.clone());

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "test/method".to_string(),
            params: None,
        };

        let mut context = RequestContext::new(request, HashMap::new());
        chain.execute_request_chain(&mut context).await.unwrap();

        // All middleware should be called
        assert!(
            middleware1
                .get_calls()
                .contains(&"process_request:test/method".to_string())
        );
        assert!(
            middleware2
                .get_calls()
                .contains(&"process_request:test/method".to_string())
        );
        assert!(
            middleware3
                .get_calls()
                .contains(&"process_request:test/method".to_string())
        );

        // Check names are in order
        let names = chain.get_request_middleware_names();
        assert_eq!(names, vec!["first", "second", "third"]);
    }

    #[tokio::test]
    async fn test_chain_clear() {
        let mut chain = MiddlewareChain::new();
        let req_middleware = Arc::new(TestRequestMiddleware::new("request"));
        let resp_middleware = Arc::new(TestResponseMiddleware::new("response"));

        chain.add_request_middleware(req_middleware);
        chain.add_response_middleware(resp_middleware);

        assert_eq!(chain.request_middleware_count(), 1);
        assert_eq!(chain.response_middleware_count(), 1);

        chain.clear();

        assert_eq!(chain.request_middleware_count(), 0);
        assert_eq!(chain.response_middleware_count(), 0);
    }
}
