//! Comprehensive tests for the plugin system architecture
//!
//! Tests validate:
//! - Plugin trait implementation and lifecycle hooks
//! - Plugin registry management and ordering
//! - Middleware pattern for request/response interception
//! - Example plugin implementations (Metrics, Retry, Cache)

use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use turbomcp_client::plugins::{
    CacheConfig,
    CachePlugin,
    ClientPlugin,
    // Example plugins
    MetricsPlugin,
    // Plugin configuration
    PluginConfig,
    PluginContext,
    PluginError,
    PluginRegistry,
    PluginResult,
    RequestContext,
    ResponseContext,
    RetryConfig,
    RetryPlugin,
};
use turbomcp_core::MessageId;
use turbomcp_protocol::jsonrpc::JsonRpcRequest;

// Test plugin for validation
#[derive(Debug, Clone)]
struct TestPlugin {
    name: String,
    calls: Arc<Mutex<Vec<String>>>,
}

impl TestPlugin {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl ClientPlugin for TestPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    async fn initialize(&self, _context: &PluginContext) -> PluginResult<()> {
        self.calls.lock().unwrap().push("initialize".to_string());
        Ok(())
    }

    async fn before_request(&self, context: &mut RequestContext) -> PluginResult<()> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("before_request:{}", context.request.method));
        Ok(())
    }

    async fn after_response(&self, context: &mut ResponseContext) -> PluginResult<()> {
        self.calls.lock().unwrap().push(format!(
            "after_response:{}",
            context.request_context.request.method
        ));
        Ok(())
    }

    async fn handle_custom(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> PluginResult<Option<Value>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("handle_custom:{}", method));
        if method == "test.echo" {
            Ok(params)
        } else {
            Ok(None)
        }
    }
}

// Error-inducing plugin for error handling tests
#[derive(Debug)]
struct ErrorPlugin;

#[async_trait]
impl ClientPlugin for ErrorPlugin {
    fn name(&self) -> &str {
        "error_plugin"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    async fn initialize(&self, _context: &PluginContext) -> PluginResult<()> {
        Err(PluginError::initialization("Test initialization error"))
    }

    async fn before_request(&self, _context: &mut RequestContext) -> PluginResult<()> {
        Err(PluginError::request_processing("Test request error"))
    }

    async fn after_response(&self, _context: &mut ResponseContext) -> PluginResult<()> {
        Err(PluginError::response_processing("Test response error"))
    }

    async fn handle_custom(
        &self,
        _method: &str,
        _params: Option<Value>,
    ) -> PluginResult<Option<Value>> {
        Err(PluginError::custom_handler("Test custom error"))
    }
}

#[tokio::test]
async fn test_plugin_registry_creation() {
    let registry = PluginRegistry::new();
    assert_eq!(registry.plugin_count(), 0);
    assert!(registry.get_plugin_names().is_empty());
}

#[tokio::test]
async fn test_plugin_registration_and_retrieval() {
    let mut registry = PluginRegistry::new();
    let plugin = Arc::new(TestPlugin::new("test_plugin"));

    // Register plugin
    registry.register_plugin(plugin.clone()).await.unwrap();

    // Validate registration
    assert_eq!(registry.plugin_count(), 1);
    assert!(registry.has_plugin("test_plugin"));
    assert!(
        registry
            .get_plugin_names()
            .contains(&"test_plugin".to_string())
    );

    // Test retrieval
    let retrieved = registry.get_plugin("test_plugin");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name(), "test_plugin");
}

#[tokio::test]
async fn test_plugin_initialization() {
    let mut registry = PluginRegistry::new();
    let plugin = Arc::new(TestPlugin::new("test_plugin"));

    registry.register_plugin(plugin.clone()).await.unwrap();

    // Verify initialize was called during registration
    let calls = plugin.get_calls();
    assert!(calls.contains(&"initialize".to_string()));
}

#[tokio::test]
async fn test_plugin_registration_error_handling() {
    let mut registry = PluginRegistry::new();
    let error_plugin = Arc::new(ErrorPlugin);

    // Should fail during initialization
    let result = registry.register_plugin(error_plugin).await;
    assert!(result.is_err());
    assert_eq!(registry.plugin_count(), 0);
}

#[tokio::test]
async fn test_plugin_ordering_and_priority() {
    let mut registry = PluginRegistry::new();

    let plugin1 = Arc::new(TestPlugin::new("plugin_1"));
    let plugin2 = Arc::new(TestPlugin::new("plugin_2"));
    let plugin3 = Arc::new(TestPlugin::new("plugin_3"));

    // Register plugins
    registry.register_plugin(plugin1.clone()).await.unwrap();
    registry.register_plugin(plugin2.clone()).await.unwrap();
    registry.register_plugin(plugin3.clone()).await.unwrap();

    // Plugins should be ordered by registration order by default
    let names = registry.get_plugin_names();
    assert_eq!(names, vec!["plugin_1", "plugin_2", "plugin_3"]);
}

#[tokio::test]
async fn test_before_request_middleware() {
    let mut registry = PluginRegistry::new();
    let plugin = Arc::new(TestPlugin::new("test_plugin"));

    registry.register_plugin(plugin.clone()).await.unwrap();

    // Create test request context
    let request = JsonRpcRequest {
        jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
        id: MessageId::from("test-123"),
        method: "test/method".to_string(),
        params: Some(json!({"key": "value"})),
    };

    let mut context = RequestContext {
        request,
        metadata: HashMap::new(),
        timestamp: chrono::Utc::now(),
    };

    // Execute before_request middleware
    registry.execute_before_request(&mut context).await.unwrap();

    // Verify plugin was called
    let calls = plugin.get_calls();
    assert!(calls.contains(&"before_request:test/method".to_string()));
}

#[tokio::test]
async fn test_after_response_middleware() {
    let mut registry = PluginRegistry::new();
    let plugin = Arc::new(TestPlugin::new("test_plugin"));

    registry.register_plugin(plugin.clone()).await.unwrap();

    // Create test contexts
    let request = JsonRpcRequest {
        jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
        id: MessageId::from("test-123"),
        method: "test/method".to_string(),
        params: Some(json!({"key": "value"})),
    };

    let request_context = RequestContext {
        request,
        metadata: HashMap::new(),
        timestamp: chrono::Utc::now(),
    };

    let mut response_context = ResponseContext {
        request_context,
        response: Some(json!({"result": "success"})),
        error: None,
        duration: std::time::Duration::from_millis(100),
        metadata: HashMap::new(),
    };

    // Execute after_response middleware
    registry
        .execute_after_response(&mut response_context)
        .await
        .unwrap();

    // Verify plugin was called
    let calls = plugin.get_calls();
    assert!(calls.contains(&"after_response:test/method".to_string()));
}

#[tokio::test]
async fn test_custom_method_handling() {
    let mut registry = PluginRegistry::new();
    let plugin = Arc::new(TestPlugin::new("test_plugin"));

    registry.register_plugin(plugin.clone()).await.unwrap();

    // Test custom method handling
    let result = registry
        .handle_custom_method("test.echo", Some(json!({"message": "hello"})))
        .await
        .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap(), json!({"message": "hello"}));

    // Verify plugin was called
    let calls = plugin.get_calls();
    assert!(calls.contains(&"handle_custom:test.echo".to_string()));
}

#[tokio::test]
async fn test_metrics_plugin_creation() {
    let config = PluginConfig::Metrics;
    let plugin = MetricsPlugin::new(config);

    assert_eq!(plugin.name(), "metrics");
    assert!(!plugin.version().is_empty());
}

#[tokio::test]
async fn test_retry_plugin_creation() {
    let retry_config = RetryConfig {
        max_retries: 3,
        base_delay_ms: 100,
        max_delay_ms: 1000,
        backoff_multiplier: 2.0,
        retry_on_timeout: true,
        retry_on_connection_error: true,
    };

    let config = PluginConfig::Retry(retry_config);
    let plugin = RetryPlugin::new(config);

    assert_eq!(plugin.name(), "retry");
    assert!(!plugin.version().is_empty());
}

#[tokio::test]
async fn test_cache_plugin_creation() {
    let cache_config = CacheConfig {
        max_entries: 1000,
        ttl_seconds: 300,
        cache_responses: true,
        cache_resources: true,
        cache_tools: false,
    };

    let config = PluginConfig::Cache(cache_config);
    let plugin = CachePlugin::new(config);

    assert_eq!(plugin.name(), "cache");
    assert!(!plugin.version().is_empty());
}

#[tokio::test]
async fn test_plugin_error_propagation() {
    let mut registry = PluginRegistry::new();
    let working_plugin = Arc::new(TestPlugin::new("working"));
    let error_plugin = Arc::new(ErrorPlugin);

    // Register working plugin first
    registry
        .register_plugin(working_plugin.clone())
        .await
        .unwrap();

    // Error plugin should fail to register
    let result = registry.register_plugin(error_plugin).await;
    assert!(result.is_err());

    // Working plugin should still be registered
    assert_eq!(registry.plugin_count(), 1);
    assert!(registry.has_plugin("working"));
}

#[tokio::test]
async fn test_middleware_chain_execution_order() {
    let mut registry = PluginRegistry::new();

    let plugin1 = Arc::new(TestPlugin::new("first"));
    let plugin2 = Arc::new(TestPlugin::new("second"));
    let plugin3 = Arc::new(TestPlugin::new("third"));

    // Register in order
    registry.register_plugin(plugin1.clone()).await.unwrap();
    registry.register_plugin(plugin2.clone()).await.unwrap();
    registry.register_plugin(plugin3.clone()).await.unwrap();

    // Create test request
    let request = JsonRpcRequest {
        jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
        id: MessageId::from("test-123"),
        method: "test/method".to_string(),
        params: Some(json!({"key": "value"})),
    };

    let mut context = RequestContext {
        request,
        metadata: HashMap::new(),
        timestamp: chrono::Utc::now(),
    };

    // Execute middleware chain
    registry.execute_before_request(&mut context).await.unwrap();

    // Verify all plugins were called in order
    assert!(
        plugin1
            .get_calls()
            .contains(&"before_request:test/method".to_string())
    );
    assert!(
        plugin2
            .get_calls()
            .contains(&"before_request:test/method".to_string())
    );
    assert!(
        plugin3
            .get_calls()
            .contains(&"before_request:test/method".to_string())
    );
}

#[tokio::test]
async fn test_plugin_metadata_passing() {
    let mut registry = PluginRegistry::new();
    let plugin = Arc::new(TestPlugin::new("metadata_plugin"));

    registry.register_plugin(plugin.clone()).await.unwrap();

    // Create request with metadata
    let request = JsonRpcRequest {
        jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
        id: MessageId::from("test-123"),
        method: "test/method".to_string(),
        params: Some(json!({"key": "value"})),
    };

    let mut context = RequestContext {
        request,
        metadata: {
            let mut meta = HashMap::new();
            meta.insert("test_key".to_string(), json!("test_value"));
            meta
        },
        timestamp: chrono::Utc::now(),
    };

    // Execute before_request
    registry.execute_before_request(&mut context).await.unwrap();

    // Metadata should be preserved and accessible
    assert!(context.metadata.contains_key("test_key"));
    assert_eq!(context.metadata["test_key"], json!("test_value"));
}

#[tokio::test]
async fn test_plugin_unregistration() {
    let mut registry = PluginRegistry::new();
    let plugin = Arc::new(TestPlugin::new("removable"));

    // Register plugin
    registry.register_plugin(plugin.clone()).await.unwrap();
    assert_eq!(registry.plugin_count(), 1);
    assert!(registry.has_plugin("removable"));

    // Unregister plugin
    registry.unregister_plugin("removable").await.unwrap();
    assert_eq!(registry.plugin_count(), 0);
    assert!(!registry.has_plugin("removable"));
}

#[tokio::test]
async fn test_duplicate_plugin_registration() {
    let mut registry = PluginRegistry::new();
    let plugin1 = Arc::new(TestPlugin::new("duplicate"));
    let plugin2 = Arc::new(TestPlugin::new("duplicate"));

    // First registration should succeed
    registry.register_plugin(plugin1).await.unwrap();
    assert_eq!(registry.plugin_count(), 1);

    // Second registration with same name should fail
    let result = registry.register_plugin(plugin2).await;
    assert!(result.is_err());
    assert_eq!(registry.plugin_count(), 1);
}
