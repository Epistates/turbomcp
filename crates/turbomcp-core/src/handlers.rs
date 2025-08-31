//! Handler traits for extensible MCP protocol support
//!
//! This module provides trait definitions for handling various MCP protocol
//! features including elicitation, completion, resource templates, and ping.

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::context::{CompletionContext, ElicitationContext, ServerInitiatedContext};
use crate::error::Result;

/// Handler for server-initiated elicitation requests
#[async_trait]
pub trait ElicitationHandler: Send + Sync {
    /// Handle an elicitation request from the server
    async fn handle_elicitation(&self, context: &ElicitationContext)
    -> Result<ElicitationResponse>;

    /// Check if this handler can process the given elicitation
    fn can_handle(&self, context: &ElicitationContext) -> bool;

    /// Get handler priority (higher = higher priority)
    fn priority(&self) -> i32 {
        0
    }
}

/// Response to an elicitation request
#[derive(Debug, Clone)]
pub struct ElicitationResponse {
    /// Whether the elicitation was accepted
    pub accepted: bool,
    /// The response content if accepted
    pub content: Option<HashMap<String, Value>>,
    /// Optional reason for declining
    pub decline_reason: Option<String>,
}

/// Provider for argument completion
#[async_trait]
pub trait CompletionProvider: Send + Sync {
    /// Provide completions for the given context
    async fn provide_completions(&self, context: &CompletionContext)
    -> Result<Vec<CompletionItem>>;

    /// Check if this provider can handle the completion request
    fn can_provide(&self, context: &CompletionContext) -> bool;

    /// Get provider priority
    fn priority(&self) -> i32 {
        0
    }
}

/// A single completion item
#[derive(Debug, Clone)]
pub struct CompletionItem {
    /// The completion value
    pub value: String,
    /// Human-readable label
    pub label: Option<String>,
    /// Additional documentation
    pub documentation: Option<String>,
    /// Sort priority (lower = higher priority)
    pub sort_priority: Option<i32>,
    /// Text to insert
    pub insert_text: Option<String>,
    /// Item metadata
    pub metadata: HashMap<String, Value>,
}

/// Handler for resource templates
#[async_trait]
pub trait ResourceTemplateHandler: Send + Sync {
    /// List available resource templates
    async fn list_templates(&self) -> Result<Vec<ResourceTemplate>>;

    /// Get a specific resource template
    async fn get_template(&self, name: &str) -> Result<Option<ResourceTemplate>>;

    /// Resolve template parameters
    async fn resolve_template(
        &self,
        template: &ResourceTemplate,
        params: HashMap<String, Value>,
    ) -> Result<ResolvedResource>;
}

/// Resource template definition
#[derive(Debug, Clone)]
pub struct ResourceTemplate {
    /// Template name
    pub name: String,
    /// Template description
    pub description: Option<String>,
    /// URI template pattern
    pub uri_template: String,
    /// Template parameters
    pub parameters: Vec<TemplateParam>,
    /// Template metadata
    pub metadata: HashMap<String, Value>,
}

/// Template parameter definition
#[derive(Debug, Clone)]
pub struct TemplateParam {
    /// Parameter name
    pub name: String,
    /// Parameter description
    pub description: Option<String>,
    /// Whether the parameter is required
    pub required: bool,
    /// Parameter type
    pub param_type: String,
    /// Default value
    pub default_value: Option<Value>,
}

/// Resolved resource from template
#[derive(Debug, Clone)]
pub struct ResolvedResource {
    /// Resolved URI
    pub uri: String,
    /// Resource name
    pub name: String,
    /// Resource description
    pub description: Option<String>,
    /// Resource content
    pub content: Option<Value>,
    /// Resource metadata
    pub metadata: HashMap<String, Value>,
}

/// Handler for bidirectional ping requests
#[async_trait]
pub trait PingHandler: Send + Sync {
    /// Handle a ping request
    async fn handle_ping(&self, context: &ServerInitiatedContext) -> Result<PingResponse>;

    /// Send a ping to the remote party
    async fn send_ping(&self, target: &str) -> Result<PingResponse>;
}

/// Response to a ping request
#[derive(Debug, Clone)]
pub struct PingResponse {
    /// Whether the ping was successful
    pub success: bool,
    /// Round-trip time in milliseconds
    pub rtt_ms: Option<u64>,
    /// Additional metadata
    pub metadata: HashMap<String, Value>,
}

/// Capabilities for server-initiated features
#[derive(Debug, Clone, Default)]
pub struct ServerInitiatedCapabilities {
    /// Supports sampling/message creation
    pub sampling: bool,
    /// Supports roots listing
    pub roots: bool,
    /// Supports elicitation
    pub elicitation: bool,
    /// Maximum concurrent requests
    pub max_concurrent_requests: usize,
    /// Supported experimental features
    pub experimental: HashMap<String, bool>,
}

/// Handler capability tracking
#[derive(Debug, Clone, Default)]
pub struct HandlerCapabilities {
    /// Supports elicitation
    pub elicitation: bool,
    /// Supports completion
    pub completion: bool,
    /// Supports resource templates
    pub templates: bool,
    /// Supports bidirectional ping
    pub ping: bool,
    /// Server-initiated capabilities
    pub server_initiated: ServerInitiatedCapabilities,
}

impl HandlerCapabilities {
    /// Create new handler capabilities
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable elicitation support
    pub fn with_elicitation(mut self) -> Self {
        self.elicitation = true;
        self
    }

    /// Enable completion support
    pub fn with_completion(mut self) -> Self {
        self.completion = true;
        self
    }

    /// Enable template support
    pub fn with_templates(mut self) -> Self {
        self.templates = true;
        self
    }

    /// Enable ping support
    pub fn with_ping(mut self) -> Self {
        self.ping = true;
        self
    }

    /// Set server-initiated capabilities
    pub fn with_server_initiated(mut self, capabilities: ServerInitiatedCapabilities) -> Self {
        self.server_initiated = capabilities;
        self
    }
}
