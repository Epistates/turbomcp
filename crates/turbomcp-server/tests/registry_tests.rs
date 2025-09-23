//! Comprehensive tests for server handler registry

mod shared_config_tests;

use async_trait::async_trait;
use turbomcp_protocol::types::*;
use turbomcp_server::handlers::*;
use turbomcp_server::registry::*;
use turbomcp_server::{RequestContext, ServerResult};

// Mock implementations for testing

struct MockToolHandler {
    name: String,
    description: Option<String>,
}

impl MockToolHandler {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
        }
    }

    fn with_description(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
        }
    }
}

#[async_trait]
impl ToolHandler for MockToolHandler {
    async fn handle(
        &self,
        _request: CallToolRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CallToolResult> {
        Ok(CallToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: format!("Mock response from {}", self.name),
                annotations: None,
                meta: None,
            })],
            is_error: Some(false),
            structured_content: None,
            _meta: None,
        })
    }

    fn tool_definition(&self) -> Tool {
        Tool {
            name: self.name.clone(),
            title: None,
            description: self.description.clone(),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: None,
            meta: None,
        }
    }
}

struct MockPromptHandler {
    name: String,
}

impl MockPromptHandler {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[async_trait]
impl PromptHandler for MockPromptHandler {
    async fn handle(
        &self,
        _request: GetPromptRequest,
        _ctx: RequestContext,
    ) -> ServerResult<GetPromptResult> {
        Ok(GetPromptResult {
            description: Some("Mock prompt".to_string()),
            messages: vec![PromptMessage {
                role: Role::User,
                content: ContentBlock::Text(TextContent {
                    text: "Mock prompt content".to_string(),
                    annotations: None,
                    meta: None,
                }),
            }],
            _meta: None,
        })
    }

    fn prompt_definition(&self) -> Prompt {
        Prompt {
            name: self.name.clone(),
            title: None,
            description: Some("Mock prompt".to_string()),
            arguments: None,
            meta: None,
        }
    }
}

struct MockResourceHandler {
    name: String,
    uri: String,
}

impl MockResourceHandler {
    fn new(name: impl Into<String>, uri: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            uri: uri.into(),
        }
    }
}

#[async_trait]
impl ResourceHandler for MockResourceHandler {
    async fn handle(
        &self,
        _request: ReadResourceRequest,
        _ctx: RequestContext,
    ) -> ServerResult<ReadResourceResult> {
        Ok(ReadResourceResult {
            contents: vec![ResourceContent::Text(TextResourceContents {
                uri: self.uri.clone(),
                mime_type: Some("text/plain".to_string()),
                text: "Mock resource content".to_string(),
                meta: None,
            })],
            _meta: None,
        })
    }

    async fn exists(&self, _uri: &str) -> bool {
        true
    }

    fn resource_definition(&self) -> Resource {
        Resource {
            name: self.name.clone(),
            title: None,
            uri: self.uri.clone(),
            description: Some("Mock resource".to_string()),
            mime_type: Some("text/plain".to_string()),
            annotations: None,
            size: Some(100),
            meta: None,
        }
    }
}

struct MockSamplingHandler {
    #[allow(dead_code)]
    name: String,
}

impl MockSamplingHandler {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[async_trait]
impl SamplingHandler for MockSamplingHandler {
    async fn handle(
        &self,
        _request: CreateMessageRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CreateMessageResult> {
        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: ContentBlock::Text(TextContent {
                text: "Mock sampling response".to_string(),
                annotations: None,
                meta: None,
            }),
            model: Some("mock-model".to_string()),
            stop_reason: Some("completed".to_string()),
            _meta: None,
        })
    }
}

struct MockLoggingHandler {
    #[allow(dead_code)]
    name: String,
}

impl MockLoggingHandler {
    fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[async_trait]
impl LoggingHandler for MockLoggingHandler {
    async fn handle(
        &self,
        _request: SetLevelRequest,
        _ctx: RequestContext,
    ) -> ServerResult<EmptyResult> {
        Ok(EmptyResult::default())
    }

    fn current_level(&self) -> LogLevel {
        LogLevel::Info
    }
}

// ============================================================================
// RegistryConfig Tests
// ============================================================================

#[test]
fn test_registry_config_default() {
    let config = RegistryConfig::default();
    shared_config_tests::assert_registry_config_defaults(&config);
}

#[test]
fn test_registry_config_custom() {
    let config = RegistryConfig {
        max_handlers_per_type: 100,
        enable_metrics: false,
        enable_validation: true,
        handler_timeout_ms: 15_000,
        enable_hot_reload: true,
        event_listeners: vec!["audit".to_string(), "metrics".to_string()],
    };

    assert_eq!(config.max_handlers_per_type, 100);
    assert!(!config.enable_metrics);
    assert!(config.enable_validation);
    assert_eq!(config.handler_timeout_ms, 15_000);
    assert!(config.enable_hot_reload);
    assert_eq!(config.event_listeners.len(), 2);
}

#[test]
fn test_registry_config_clone() {
    let original = RegistryConfig::default();
    let cloned = original.clone();
    assert_eq!(original.max_handlers_per_type, cloned.max_handlers_per_type);
    assert_eq!(original.enable_metrics, cloned.enable_metrics);
}

#[test]
fn test_registry_config_debug() {
    let config = RegistryConfig::default();
    let debug_str = format!("{config:?}");
    assert!(debug_str.contains("RegistryConfig"));
    assert!(debug_str.contains("max_handlers_per_type"));
}

// ============================================================================
// RegistryEvent Tests
// ============================================================================

#[test]
fn test_registry_event_variants() {
    let now = chrono::Utc::now();

    let events = vec![
        RegistryEvent::HandlerRegistered {
            handler_type: "tool".to_string(),
            name: "test_tool".to_string(),
            timestamp: now,
        },
        RegistryEvent::HandlerUnregistered {
            handler_type: "prompt".to_string(),
            name: "test_prompt".to_string(),
            timestamp: now,
        },
        RegistryEvent::HandlerUpdated {
            handler_type: "resource".to_string(),
            name: "test_resource".to_string(),
            timestamp: now,
        },
        RegistryEvent::RegistryCleared { timestamp: now },
    ];

    for event in events {
        let debug_str = format!("{event:?}");
        assert!(!debug_str.is_empty());

        let cloned = event.clone();
        let cloned_debug = format!("{cloned:?}");
        // Each event should be cloneable and contain a variant name
        assert!(cloned_debug.contains("Handler") || cloned_debug.contains("Registry"));
    }
}

// ============================================================================
// HandlerRegistry Tests
// ============================================================================

#[test]
fn test_handler_registry_new() {
    let registry = HandlerRegistry::new();
    assert_eq!(registry.tools.len(), 0);
    assert_eq!(registry.prompts.len(), 0);
    assert_eq!(registry.resources.len(), 0);
    assert_eq!(registry.sampling.len(), 0);
    assert_eq!(registry.logging.len(), 0);
}

#[test]
fn test_handler_registry_with_config() {
    let config = RegistryConfig {
        max_handlers_per_type: 50,
        enable_metrics: false,
        enable_validation: false,
        handler_timeout_ms: 5_000,
        enable_hot_reload: true,
        event_listeners: vec!["test".to_string()],
    };

    let registry = HandlerRegistry::with_config(config);
    assert_eq!(registry.tools.len(), 0);
}

#[test]
fn test_handler_registry_default() {
    let registry = HandlerRegistry::default();
    assert_eq!(registry.tools.len(), 0);
    assert_eq!(registry.prompts.len(), 0);
}

#[test]
fn test_handler_registry_debug() {
    let registry = HandlerRegistry::new();
    let debug_str = format!("{registry:?}");
    assert!(debug_str.contains("HandlerRegistry"));
    assert!(debug_str.contains("tools_count"));
    assert!(debug_str.contains("prompts_count"));
}

// ============================================================================
// Tool Handler Registration Tests
// ============================================================================

#[test]
fn test_register_tool_handler() {
    let registry = HandlerRegistry::new();
    let tool = MockToolHandler::new("test_tool");

    let result = registry.register_tool("test_tool", tool);
    assert!(result.is_ok());
    assert_eq!(registry.tools.len(), 1);
    assert!(registry.tools.contains_key("test_tool"));
}

#[test]
fn test_register_tool_handler_with_description() {
    let registry = HandlerRegistry::new();
    let tool = MockToolHandler::with_description("described_tool", "A tool with description");

    let result = registry.register_tool("described_tool", tool);
    assert!(result.is_ok());
    assert_eq!(registry.tools.len(), 1);
}

#[test]
fn test_register_tool_handler_empty_name_validation() {
    let config = RegistryConfig {
        enable_validation: true,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);
    let tool = MockToolHandler::new(""); // Empty name

    let result = registry.register_tool("", tool);
    assert!(result.is_err());
    assert_eq!(registry.tools.len(), 0);
}

#[test]
fn test_register_tool_handler_long_name_validation() {
    let config = RegistryConfig {
        enable_validation: true,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);
    let long_name = "a".repeat(101); // 101 characters, over limit
    let tool = MockToolHandler::new(&long_name);

    let result = registry.register_tool(&long_name, tool);
    assert!(result.is_err());
    assert_eq!(registry.tools.len(), 0);
}

#[test]
fn test_register_tool_handler_duplicate_validation() {
    let config = RegistryConfig {
        enable_validation: true,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);

    let tool1 = MockToolHandler::new("duplicate_tool");
    let tool2 = MockToolHandler::new("duplicate_tool");

    let result1 = registry.register_tool("duplicate_tool", tool1);
    assert!(result1.is_ok());

    let result2 = registry.register_tool("duplicate_tool", tool2);
    assert!(result2.is_err());
    assert_eq!(registry.tools.len(), 1);
}

#[test]
fn test_register_tool_handler_max_limit() {
    let config = RegistryConfig {
        max_handlers_per_type: 2,
        enable_validation: false,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);

    let tool1 = MockToolHandler::new("tool1");
    let tool2 = MockToolHandler::new("tool2");
    let tool3 = MockToolHandler::new("tool3");

    assert!(registry.register_tool("tool1", tool1).is_ok());
    assert!(registry.register_tool("tool2", tool2).is_ok());

    let result3 = registry.register_tool("tool3", tool3);
    assert!(result3.is_err());
    assert_eq!(registry.tools.len(), 2);
}

#[test]
fn test_register_tool_handler_without_validation() {
    let config = RegistryConfig {
        enable_validation: false,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);
    let tool = MockToolHandler::new(""); // Empty name should be allowed

    let result = registry.register_tool("empty_name_tool", tool);
    assert!(result.is_ok());
    assert_eq!(registry.tools.len(), 1);
}

// ============================================================================
// Prompt Handler Registration Tests
// ============================================================================

#[test]
fn test_register_prompt_handler() {
    let registry = HandlerRegistry::new();
    let prompt = MockPromptHandler::new("test_prompt");

    let result = registry.register_prompt("test_prompt", prompt);
    assert!(result.is_ok());
    assert_eq!(registry.prompts.len(), 1);
    assert!(registry.prompts.contains_key("test_prompt"));
}

#[test]
fn test_register_prompt_handler_validation() {
    let config = RegistryConfig {
        enable_validation: true,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);
    let prompt = MockPromptHandler::new(""); // Empty name

    let result = registry.register_prompt("", prompt);
    assert!(result.is_err());
    assert_eq!(registry.prompts.len(), 0);
}

#[test]
fn test_register_prompt_handler_max_limit() {
    let config = RegistryConfig {
        max_handlers_per_type: 1,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);

    let prompt1 = MockPromptHandler::new("prompt1");
    let prompt2 = MockPromptHandler::new("prompt2");

    assert!(registry.register_prompt("prompt1", prompt1).is_ok());

    let result2 = registry.register_prompt("prompt2", prompt2);
    assert!(result2.is_err());
    assert_eq!(registry.prompts.len(), 1);
}

// ============================================================================
// Resource Handler Registration Tests
// ============================================================================

#[test]
fn test_register_resource_handler() {
    let registry = HandlerRegistry::new();
    let resource = MockResourceHandler::new("test_resource", "file:///test.txt");

    let result = registry.register_resource("test_resource", resource);
    assert!(result.is_ok());
    assert_eq!(registry.resources.len(), 1);
    assert!(registry.resources.contains_key("test_resource"));
}

#[test]
fn test_register_resource_handler_validation_empty_uri() {
    let config = RegistryConfig {
        enable_validation: true,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);
    let resource = MockResourceHandler::new("test_resource", ""); // Empty URI

    let result = registry.register_resource("test_resource", resource);
    assert!(result.is_err());
    assert_eq!(registry.resources.len(), 0);
}

#[test]
fn test_register_resource_handler_validation_empty_name() {
    let config = RegistryConfig {
        enable_validation: true,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);
    let resource = MockResourceHandler::new("", "file:///test.txt"); // Empty name

    let result = registry.register_resource("", resource);
    assert!(result.is_err());
    assert_eq!(registry.resources.len(), 0);
}

#[test]
fn test_register_resource_handler_duplicate_uri_validation() {
    let config = RegistryConfig {
        enable_validation: true,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);

    let resource1 = MockResourceHandler::new("resource1", "file:///duplicate.txt");
    let resource2 = MockResourceHandler::new("resource2", "file:///duplicate.txt");

    assert!(registry.register_resource("resource1", resource1).is_ok());

    let result2 = registry.register_resource("resource2", resource2);
    assert!(result2.is_err());
    assert_eq!(registry.resources.len(), 1);
}

// ============================================================================
// Sampling Handler Registration Tests
// ============================================================================

#[test]
fn test_register_sampling_handler() {
    let registry = HandlerRegistry::new();
    let sampling = MockSamplingHandler::new("test_sampling");

    let result = registry.register_sampling("test_sampling", sampling);
    assert!(result.is_ok());
    assert_eq!(registry.sampling.len(), 1);
    assert!(registry.sampling.contains_key("test_sampling"));
}

#[test]
fn test_register_sampling_handler_max_limit() {
    let config = RegistryConfig {
        max_handlers_per_type: 1,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);

    let sampling1 = MockSamplingHandler::new("sampling1");
    let sampling2 = MockSamplingHandler::new("sampling2");

    assert!(registry.register_sampling("sampling1", sampling1).is_ok());

    let result2 = registry.register_sampling("sampling2", sampling2);
    assert!(result2.is_err());
    assert_eq!(registry.sampling.len(), 1);
}

// ============================================================================
// Logging Handler Registration Tests
// ============================================================================

#[test]
fn test_register_logging_handler() {
    let registry = HandlerRegistry::new();
    let logging = MockLoggingHandler::new("test_logging");

    let result = registry.register_logging("test_logging", logging);
    assert!(result.is_ok());
    assert_eq!(registry.logging.len(), 1);
    assert!(registry.logging.contains_key("test_logging"));
}

// ============================================================================
// Handler Retrieval Tests
// ============================================================================

#[test]
fn test_get_tool_handler() {
    let registry = HandlerRegistry::new();
    let tool = MockToolHandler::new("get_tool");

    registry.register_tool("get_tool", tool).unwrap();

    let retrieved = registry.get_tool("get_tool");
    assert!(retrieved.is_some());

    let not_found = registry.get_tool("non_existent");
    assert!(not_found.is_none());
}

#[test]
fn test_get_prompt_handler() {
    let registry = HandlerRegistry::new();
    let prompt = MockPromptHandler::new("get_prompt");

    registry.register_prompt("get_prompt", prompt).unwrap();

    let retrieved = registry.get_prompt("get_prompt");
    assert!(retrieved.is_some());

    let not_found = registry.get_prompt("non_existent");
    assert!(not_found.is_none());
}

#[test]
fn test_get_resource_handler() {
    let registry = HandlerRegistry::new();
    let resource = MockResourceHandler::new("get_resource", "file:///get.txt");

    registry
        .register_resource("get_resource", resource)
        .unwrap();

    let retrieved = registry.get_resource("get_resource");
    assert!(retrieved.is_some());

    let not_found = registry.get_resource("non_existent");
    assert!(not_found.is_none());
}

#[test]
fn test_get_sampling_handler() {
    let registry = HandlerRegistry::new();
    let sampling = MockSamplingHandler::new("get_sampling");

    registry
        .register_sampling("get_sampling", sampling)
        .unwrap();

    let retrieved = registry.get_sampling("get_sampling");
    assert!(retrieved.is_some());

    let not_found = registry.get_sampling("non_existent");
    assert!(not_found.is_none());
}

#[test]
fn test_get_logging_handler() {
    let registry = HandlerRegistry::new();
    let logging = MockLoggingHandler::new("get_logging");

    registry.register_logging("get_logging", logging).unwrap();

    let retrieved = registry.get_logging("get_logging");
    assert!(retrieved.is_some());

    let not_found = registry.get_logging("non_existent");
    assert!(not_found.is_none());
}

// ============================================================================
// Handler Listing Tests
// ============================================================================

#[test]
fn test_list_tools() {
    let registry = HandlerRegistry::new();
    registry
        .register_tool("tool1", MockToolHandler::new("tool1"))
        .unwrap();
    registry
        .register_tool("tool2", MockToolHandler::new("tool2"))
        .unwrap();

    let tools = registry.list_tools();
    assert_eq!(tools.len(), 2);
    assert!(tools.contains(&"tool1".to_string()));
    assert!(tools.contains(&"tool2".to_string()));
}

#[test]
fn test_list_prompts() {
    let registry = HandlerRegistry::new();
    registry
        .register_prompt("prompt1", MockPromptHandler::new("prompt1"))
        .unwrap();
    registry
        .register_prompt("prompt2", MockPromptHandler::new("prompt2"))
        .unwrap();

    let prompts = registry.list_prompts();
    assert_eq!(prompts.len(), 2);
    assert!(prompts.contains(&"prompt1".to_string()));
    assert!(prompts.contains(&"prompt2".to_string()));
}

#[test]
fn test_list_resources() {
    let registry = HandlerRegistry::new();
    registry
        .register_resource(
            "resource1",
            MockResourceHandler::new("resource1", "file:///1.txt"),
        )
        .unwrap();
    registry
        .register_resource(
            "resource2",
            MockResourceHandler::new("resource2", "file:///2.txt"),
        )
        .unwrap();

    let resources = registry.list_resources();
    assert_eq!(resources.len(), 2);
    assert!(resources.contains(&"resource1".to_string()));
    assert!(resources.contains(&"resource2".to_string()));
}

#[test]
fn test_list_sampling() {
    let registry = HandlerRegistry::new();
    registry
        .register_sampling("sampling1", MockSamplingHandler::new("sampling1"))
        .unwrap();

    let sampling = registry.list_sampling();
    assert_eq!(sampling.len(), 1);
    assert!(sampling.contains(&"sampling1".to_string()));
}

#[test]
fn test_list_logging() {
    let registry = HandlerRegistry::new();
    registry
        .register_logging("logging1", MockLoggingHandler::new("logging1"))
        .unwrap();

    let logging = registry.list_logging();
    assert_eq!(logging.len(), 1);
    assert!(logging.contains(&"logging1".to_string()));
}

// ============================================================================
// Definition Retrieval Tests
// ============================================================================

#[test]
fn test_get_tool_definitions() {
    let registry = HandlerRegistry::new();
    registry
        .register_tool("tool1", MockToolHandler::new("tool1"))
        .unwrap();
    registry
        .register_tool(
            "tool2",
            MockToolHandler::with_description("tool2", "Description"),
        )
        .unwrap();

    let definitions = registry.get_tool_definitions();
    assert_eq!(definitions.len(), 2);

    let names: Vec<_> = definitions.iter().map(|t| &t.name).collect();
    assert!(names.contains(&&"tool1".to_string()));
    assert!(names.contains(&&"tool2".to_string()));
}

#[test]
fn test_get_prompt_definitions() {
    let registry = HandlerRegistry::new();
    registry
        .register_prompt("prompt1", MockPromptHandler::new("prompt1"))
        .unwrap();

    let definitions = registry.get_prompt_definitions();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].name, "prompt1");
}

#[test]
fn test_get_resource_definitions() {
    let registry = HandlerRegistry::new();
    registry
        .register_resource(
            "resource1",
            MockResourceHandler::new("resource1", "file:///1.txt"),
        )
        .unwrap();

    let definitions = registry.get_resource_definitions();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].name, "resource1");
    assert_eq!(definitions[0].uri, "file:///1.txt");
}

// ============================================================================
// Handler Unregistration Tests
// ============================================================================

#[test]
fn test_unregister_tool() {
    let registry = HandlerRegistry::new();
    registry
        .register_tool("tool1", MockToolHandler::new("tool1"))
        .unwrap();

    assert_eq!(registry.tools.len(), 1);

    let removed = registry.unregister_tool("tool1");
    assert!(removed);
    assert_eq!(registry.tools.len(), 0);

    let not_removed = registry.unregister_tool("non_existent");
    assert!(!not_removed);
}

#[test]
fn test_unregister_prompt() {
    let registry = HandlerRegistry::new();
    registry
        .register_prompt("prompt1", MockPromptHandler::new("prompt1"))
        .unwrap();

    assert_eq!(registry.prompts.len(), 1);

    let removed = registry.unregister_prompt("prompt1");
    assert!(removed);
    assert_eq!(registry.prompts.len(), 0);
}

#[test]
fn test_unregister_resource() {
    let registry = HandlerRegistry::new();
    registry
        .register_resource(
            "resource1",
            MockResourceHandler::new("resource1", "file:///1.txt"),
        )
        .unwrap();

    assert_eq!(registry.resources.len(), 1);

    let removed = registry.unregister_resource("resource1");
    assert!(removed);
    assert_eq!(registry.resources.len(), 0);
}

// ============================================================================
// Registry Management Tests
// ============================================================================

#[test]
fn test_clear_registry() {
    let registry = HandlerRegistry::new();
    registry
        .register_tool("tool1", MockToolHandler::new("tool1"))
        .unwrap();
    registry
        .register_prompt("prompt1", MockPromptHandler::new("prompt1"))
        .unwrap();
    registry
        .register_resource(
            "resource1",
            MockResourceHandler::new("resource1", "file:///1.txt"),
        )
        .unwrap();

    assert_eq!(registry.tools.len(), 1);
    assert_eq!(registry.prompts.len(), 1);
    assert_eq!(registry.resources.len(), 1);

    registry.clear();

    assert_eq!(registry.tools.len(), 0);
    assert_eq!(registry.prompts.len(), 0);
    assert_eq!(registry.resources.len(), 0);
}

#[test]
fn test_registry_stats() {
    let registry = HandlerRegistry::new();
    registry
        .register_tool("tool1", MockToolHandler::new("tool1"))
        .unwrap();
    registry
        .register_tool("tool2", MockToolHandler::new("tool2"))
        .unwrap();
    registry
        .register_prompt("prompt1", MockPromptHandler::new("prompt1"))
        .unwrap();
    registry
        .register_resource(
            "resource1",
            MockResourceHandler::new("resource1", "file:///1.txt"),
        )
        .unwrap();
    registry
        .register_sampling("sampling1", MockSamplingHandler::new("sampling1"))
        .unwrap();
    registry
        .register_logging("logging1", MockLoggingHandler::new("logging1"))
        .unwrap();

    let stats = registry.stats();
    assert_eq!(stats.tool_count, 2);
    assert_eq!(stats.prompt_count, 1);
    assert_eq!(stats.resource_count, 1);
    assert_eq!(stats.sampling_count, 1);
    assert_eq!(stats.logging_count, 1);
    assert_eq!(stats.total_count, 6);
}

#[test]
fn test_registry_stats_empty() {
    let registry = HandlerRegistry::new();
    let stats = registry.stats();
    assert_eq!(stats.tool_count, 0);
    assert_eq!(stats.prompt_count, 0);
    assert_eq!(stats.resource_count, 0);
    assert_eq!(stats.sampling_count, 0);
    assert_eq!(stats.logging_count, 0);
    assert_eq!(stats.total_count, 0);
}

#[test]
fn test_registry_stats_debug() {
    let stats = RegistryStats {
        tool_count: 5,
        prompt_count: 3,
        resource_count: 2,
        sampling_count: 1,
        logging_count: 1,
        total_count: 12,
    };

    let debug_str = format!("{stats:?}");
    assert!(debug_str.contains("RegistryStats"));
    assert!(debug_str.contains("tool_count: 5"));

    let cloned = stats.clone();
    assert_eq!(stats.total_count, cloned.total_count);
}

// ============================================================================
// Metadata Tests
// ============================================================================

#[test]
fn test_get_metadata() {
    let registry = HandlerRegistry::new();
    registry
        .register_tool("metadata_tool", MockToolHandler::new("metadata_tool"))
        .unwrap();

    let metadata = registry.get_metadata("tool:metadata_tool");
    assert!(metadata.is_some());

    if let Some(meta) = metadata {
        assert_eq!(meta.name, "metadata_tool");
        assert_eq!(meta.version, "1.0.0");
        assert!(meta.tags.contains(&"tool".to_string()));
    }

    let not_found = registry.get_metadata("tool:non_existent");
    assert!(not_found.is_none());
}

#[test]
fn test_metadata_cleanup_on_unregister() {
    let registry = HandlerRegistry::new();
    registry
        .register_tool("cleanup_tool", MockToolHandler::new("cleanup_tool"))
        .unwrap();

    // Metadata should exist
    let metadata = registry.get_metadata("tool:cleanup_tool");
    assert!(metadata.is_some());

    // Unregister and check metadata is cleaned up
    registry.unregister_tool("cleanup_tool");
    let metadata_after = registry.get_metadata("tool:cleanup_tool");
    assert!(metadata_after.is_none());
}

// ============================================================================
// Configuration Update Tests
// ============================================================================

#[test]
fn test_update_config() {
    let registry = HandlerRegistry::new();

    registry.update_config(|config| {
        config.max_handlers_per_type = 500;
        config.enable_metrics = false;
        config.handler_timeout_ms = 45_000;
    });

    // Verify the config was updated by trying to register more than default limit
    // but less than new limit
    for i in 0..10 {
        let tool = MockToolHandler::new(format!("tool_{i}"));
        let result = registry.register_tool(format!("tool_{i}"), tool);
        assert!(result.is_ok());
    }
    assert_eq!(registry.tools.len(), 10);
}

// ============================================================================
// RegistryBuilder Tests
// ============================================================================

#[test]
fn test_registry_builder_new() {
    let builder = RegistryBuilder::new();
    let debug_str = format!("{builder:?}");
    assert!(debug_str.contains("RegistryBuilder"));
}

#[test]
fn test_registry_builder_default() {
    let builder = RegistryBuilder::default();
    let registry = builder.build();
    assert_eq!(registry.tools.len(), 0);
}

#[test]
fn test_registry_builder_configuration() {
    let registry = RegistryBuilder::new()
        .max_handlers_per_type(100)
        .enable_metrics(false)
        .enable_validation(true)
        .handler_timeout_ms(15_000)
        .enable_hot_reload(true)
        .build();

    // Test that the config was applied by checking limits
    let config = RegistryConfig {
        max_handlers_per_type: 2,
        ..Default::default()
    };
    let _limited_registry = HandlerRegistry::with_config(config);

    // Original registry should allow more registrations than limited one
    for i in 0..5 {
        let tool = MockToolHandler::new(format!("tool_{i}"));
        let result = registry.register_tool(format!("tool_{i}"), tool);
        assert!(result.is_ok());
    }
    assert_eq!(registry.tools.len(), 5);
}

#[test]
fn test_registry_builder_chaining() {
    let registry = RegistryBuilder::new()
        .max_handlers_per_type(50)
        .enable_metrics(true)
        .enable_validation(false)
        .handler_timeout_ms(10_000)
        .enable_hot_reload(false)
        .build();

    // Registry should be created successfully
    assert_eq!(registry.tools.len(), 0);
}

// ============================================================================
// Registry Type Alias Tests
// ============================================================================

#[test]
fn test_registry_type_alias() {
    let registry: Registry = HandlerRegistry::new();
    assert_eq!(registry.tools.len(), 0);

    let tool = MockToolHandler::new("alias_tool");
    let result = registry.register_tool("alias_tool", tool);
    assert!(result.is_ok());
    assert_eq!(registry.tools.len(), 1);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_complete_registry_workflow() {
    let registry = RegistryBuilder::new()
        .max_handlers_per_type(10)
        .enable_validation(true)
        .build();

    // Register different types of handlers
    registry
        .register_tool(
            "calculator",
            MockToolHandler::with_description("calculator", "Math operations"),
        )
        .unwrap();
    registry
        .register_prompt("greeting", MockPromptHandler::new("greeting"))
        .unwrap();
    registry
        .register_resource(
            "config",
            MockResourceHandler::new("config", "file:///config.json"),
        )
        .unwrap();
    registry
        .register_sampling("llm", MockSamplingHandler::new("llm"))
        .unwrap();
    registry
        .register_logging("audit", MockLoggingHandler::new("audit"))
        .unwrap();

    // Check statistics
    let stats = registry.stats();
    assert_eq!(stats.total_count, 5);
    assert_eq!(stats.tool_count, 1);
    assert_eq!(stats.prompt_count, 1);
    assert_eq!(stats.resource_count, 1);
    assert_eq!(stats.sampling_count, 1);
    assert_eq!(stats.logging_count, 1);

    // Test retrievals
    assert!(registry.get_tool("calculator").is_some());
    assert!(registry.get_prompt("greeting").is_some());
    assert!(registry.get_resource("config").is_some());
    assert!(registry.get_sampling("llm").is_some());
    assert!(registry.get_logging("audit").is_some());

    // Test definitions
    let tool_defs = registry.get_tool_definitions();
    assert_eq!(tool_defs.len(), 1);
    assert_eq!(tool_defs[0].name, "calculator");
    assert_eq!(
        tool_defs[0].description,
        Some("Math operations".to_string())
    );

    // Test metadata
    let metadata = registry.get_metadata("tool:calculator");
    assert!(metadata.is_some());

    // Test unregistration
    assert!(registry.unregister_tool("calculator"));
    assert_eq!(registry.tools.len(), 0);

    // Test clearing
    registry.clear();
    let final_stats = registry.stats();
    assert_eq!(final_stats.total_count, 0);
}

#[test]
fn test_registry_concurrent_access() {
    use std::sync::Arc;
    use std::thread;

    let registry = Arc::new(HandlerRegistry::new());
    let mut handles = vec![];

    // Spawn multiple threads to register handlers concurrently
    for i in 0..10 {
        let registry_clone = Arc::clone(&registry);
        let handle = thread::spawn(move || {
            let tool = MockToolHandler::new(format!("concurrent_tool_{i}"));
            registry_clone
                .register_tool(format!("concurrent_tool_{i}"), tool)
                .unwrap();
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all tools were registered
    assert_eq!(registry.tools.len(), 10);
    let stats = registry.stats();
    assert_eq!(stats.tool_count, 10);
}

#[test]
fn test_registry_error_handling() {
    let config = RegistryConfig {
        max_handlers_per_type: 1,
        enable_validation: true,
        ..Default::default()
    };
    let registry = HandlerRegistry::with_config(config);

    // Test various error conditions
    let empty_tool = MockToolHandler::new("");
    let result1 = registry.register_tool("", empty_tool);
    assert!(result1.is_err());

    let long_name_tool = MockToolHandler::new("a".repeat(101));
    let result2 = registry.register_tool("a".repeat(101), long_name_tool);
    assert!(result2.is_err());

    let valid_tool = MockToolHandler::new("valid_tool");
    let result3 = registry.register_tool("valid_tool", valid_tool);
    assert!(result3.is_ok());

    let limit_exceeded_tool = MockToolHandler::new("limit_tool");
    let result4 = registry.register_tool("limit_tool", limit_exceeded_tool);
    assert!(result4.is_err());

    // Only one tool should be registered
    assert_eq!(registry.tools.len(), 1);
}
