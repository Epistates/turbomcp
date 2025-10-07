//! Common test utilities and helpers to reduce duplication across test suite

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::RequestId;
use turbomcp_protocol::jsonrpc::*;
use turbomcp_server::middleware::*;
use turbomcp_server::{ServerError, ServerResult};

/// Test helper macro for default config tests to reduce duplication
#[macro_export]
macro_rules! test_default_config {
    ($config_type:ty, $expected_field:ident, $expected_value:expr) => {
        #[test]
        fn test_default_config() {
            let config = <$config_type>::default();
            assert_eq!(config.$expected_field, $expected_value);
        }
    };
}

/// Test helper macro for debug config tests
#[macro_export]
macro_rules! test_debug_config {
    ($config_type:ty) => {
        #[test]
        fn test_debug_config() {
            let config = <$config_type>::default();
            let debug_str = format!("{:?}", config);
            assert!(!debug_str.is_empty());
        }
    };
}

/// Test helper macro for clone config tests
#[macro_export]
macro_rules! test_clone_config {
    ($config_type:ty) => {
        #[test]
        fn test_clone_config() {
            let config1 = <$config_type>::default();
            let config2 = config1.clone();
            // Just verify cloning doesn't panic and produces equivalent object
            assert_eq!(format!("{:?}", config1), format!("{:?}", config2));
        }
    };
}

/// Create a standardized test JSON-RPC request
pub fn create_test_request() -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: JsonRpcVersion,
        id: RequestId::Number(1),
        method: "test/method".to_string(),
        params: Some(json!({"test": "value"})),
    }
}

/// Create a standardized test request context
pub fn create_test_context() -> RequestContext {
    RequestContext::new()
        .with_session_id("test_session")
        .with_metadata("test_key", "test_value")
}

/// Create a standardized test response
pub fn create_test_response() -> JsonRpcResponse {
    JsonRpcResponse::success(json!({"success": true}), RequestId::Number(1))
}

/// Simple test middleware for consistent testing
#[derive(Debug)]
pub struct TestMiddleware {
    pub name: String,
    pub should_fail: bool,
    pub delay_ms: Option<u64>,
}

impl TestMiddleware {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            should_fail: false,
            delay_ms: None,
        }
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }

    pub fn with_delay(mut self, delay_ms: u64) -> Self {
        self.delay_ms = Some(delay_ms);
        self
    }
}

#[async_trait]
impl Middleware for TestMiddleware {
    async fn process_request(
        &self,
        _request: &mut JsonRpcRequest,
        ctx: &mut RequestContext,
    ) -> ServerResult<()> {
        if let Some(delay) = self.delay_ms {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        }

        if self.should_fail {
            return Err(ServerError::handler(format!(
                "Test middleware {} failed",
                self.name
            )));
        }

        // Add tracking metadata
        let meta = Arc::make_mut(&mut ctx.metadata);
        meta.insert(format!("processed_by_{}", self.name), json!(true));

        Ok(())
    }

    async fn process_response(
        &self,
        _response: &mut JsonRpcResponse,
        _ctx: &RequestContext,
    ) -> ServerResult<()> {
        if self.should_fail {
            return Err(ServerError::handler(format!(
                "Test middleware {} failed on response",
                self.name
            )));
        }
        Ok(())
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Test utility to verify middleware processing metadata
pub fn assert_middleware_processed(ctx: &RequestContext, middleware_name: &str) {
    let key = format!("processed_by_{middleware_name}");
    assert_eq!(
        ctx.metadata.get(&key),
        Some(&json!(true)),
        "Middleware {middleware_name} did not process request"
    );
}

/// Property testing helper for middleware configurations
pub fn test_config_properties<T>()
where
    T: Default + std::fmt::Debug + Clone,
{
    let config = T::default();

    // Test Debug implementation
    let debug_str = format!("{config:?}");
    assert!(!debug_str.is_empty());

    // Test Clone implementation
    let _cloned = config.clone();
    // Just verify cloning doesn't panic
}
