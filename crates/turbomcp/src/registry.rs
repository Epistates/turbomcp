//! Handler registration system using inventory for compile-time discovery

use crate::{CallToolResult, /*Context,*/ McpResult};
use std::future::Future;
use std::pin::Pin;

/// Type alias for tool handler function signature
type ToolHandler = fn(
    &dyn std::any::Any,
    ToolRequest,
) -> Pin<Box<dyn Future<Output = McpResult<CallToolResult>> + Send>>;

/// Type alias for resource handler function signature
type ResourceHandler = fn(
    &dyn std::any::Any,
    ResourceRequest,
) -> Pin<
    Box<dyn Future<Output = McpResult<turbomcp_protocol::types::ReadResourceResult>> + Send>,
>;

/// Type alias for prompt handler function signature
type PromptHandler = fn(
    &dyn std::any::Any,
    PromptRequest,
) -> Pin<
    Box<dyn Future<Output = McpResult<turbomcp_protocol::types::GetPromptResult>> + Send>,
>;

/// Tool registration entry collected by inventory
pub struct ToolRegistration {
    /// Tool name
    pub name: &'static str,
    /// Tool description
    pub description: &'static str,
    /// JSON schema for input (object schema)
    pub schema: Option<serde_json::Value>,
    /// Allowed roles (RBAC). If None or empty, allow all.
    pub allowed_roles: Option<&'static [&'static str]>,
    /// Handler function
    pub handler: ToolHandler,
}

inventory::collect!(ToolRegistration);

/// Simplified request type for tool handlers
pub struct ToolRequest {
    /// Request context
    pub context: turbomcp_protocol::RequestContext,
    /// Tool arguments
    pub arguments: std::collections::HashMap<String, serde_json::Value>,
}

/// Resource registration entry
pub struct ResourceRegistration {
    /// Resource name
    pub name: &'static str,
    /// Resource description
    pub description: &'static str,
    /// URI template pattern
    pub uri_template: Option<&'static str>,
    /// Handler function
    pub handler: ResourceHandler,
}

inventory::collect!(ResourceRegistration);

/// Simplified request type for resource handlers
pub struct ResourceRequest {
    /// Request context
    pub context: turbomcp_protocol::RequestContext,
    /// Resource URI
    pub uri: String,
    /// URI parameters
    pub parameters: std::collections::HashMap<String, String>,
}

/// Prompt registration entry
pub struct PromptRegistration {
    /// Prompt name
    pub name: &'static str,
    /// Prompt description
    pub description: &'static str,
    /// Handler function
    pub handler: PromptHandler,
}

inventory::collect!(PromptRegistration);

/// Simplified request type for prompt handlers
pub struct PromptRequest {
    /// Request context
    pub context: turbomcp_protocol::RequestContext,
    /// Prompt arguments
    pub arguments: std::collections::HashMap<String, serde_json::Value>,
}

/// Registry for collecting all registered handlers
pub struct HandlerRegistry {
    tools: Vec<&'static ToolRegistration>,
    resources: Vec<&'static ResourceRegistration>,
    prompts: Vec<&'static PromptRegistration>,
}

impl HandlerRegistry {
    /// Create a new registry by collecting all registered handlers
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: inventory::iter::<ToolRegistration>().collect(),
            resources: inventory::iter::<ResourceRegistration>().collect(),
            prompts: inventory::iter::<PromptRegistration>().collect(),
        }
    }

    /// Get all registered tools
    #[must_use]
    pub fn tools(&self) -> &[&'static ToolRegistration] {
        &self.tools
    }

    /// Get all registered resources
    #[must_use]
    pub fn resources(&self) -> &[&'static ResourceRegistration] {
        &self.resources
    }

    /// Get all registered prompts
    #[must_use]
    pub fn prompts(&self) -> &[&'static PromptRegistration] {
        &self.prompts
    }

    /// Find a tool by name
    #[must_use]
    pub fn find_tool(&self, name: &str) -> Option<&'static ToolRegistration> {
        self.tools.iter().find(|tool| tool.name == name).copied()
    }

    /// Find a resource by name or URI pattern
    #[must_use]
    pub fn find_resource(&self, name: &str) -> Option<&'static ResourceRegistration> {
        self.resources
            .iter()
            .find(|resource| resource.name == name)
            .copied()
    }

    /// Find a prompt by name
    #[must_use]
    pub fn find_prompt(&self, name: &str) -> Option<&'static PromptRegistration> {
        self.prompts
            .iter()
            .find(|prompt| prompt.name == name)
            .copied()
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
