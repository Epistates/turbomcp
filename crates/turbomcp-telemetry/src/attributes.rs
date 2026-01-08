//! MCP-specific span attributes and context helpers
//!
//! Provides utilities for creating properly attributed spans for MCP operations.

use crate::span_attributes::*;
use std::time::Duration;
use tracing::{Span, info_span};

/// MCP request context for span attribution
#[derive(Debug, Clone, Default)]
pub struct McpSpanContext {
    /// MCP method (e.g., "tools/call", "resources/read")
    pub method: Option<String>,
    /// JSON-RPC request ID
    pub request_id: Option<String>,
    /// Session ID
    pub session_id: Option<String>,
    /// Tool name (for tools/call)
    pub tool_name: Option<String>,
    /// Resource URI (for resources/read)
    pub resource_uri: Option<String>,
    /// Prompt name (for prompts/get)
    pub prompt_name: Option<String>,
    /// Transport type
    pub transport: Option<String>,
    /// Protocol version
    pub protocol_version: Option<String>,
    /// Tenant ID
    pub tenant_id: Option<String>,
    /// User ID
    pub user_id: Option<String>,
    /// Client name
    pub client_name: Option<String>,
    /// Client version
    pub client_version: Option<String>,
    /// Server name
    pub server_name: Option<String>,
    /// Server version
    pub server_version: Option<String>,
}

impl McpSpanContext {
    /// Create a new empty context
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the MCP method
    #[must_use]
    pub fn method(mut self, method: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self
    }

    /// Set the request ID
    #[must_use]
    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Set the session ID
    #[must_use]
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Set the tool name
    #[must_use]
    pub fn tool_name(mut self, name: impl Into<String>) -> Self {
        self.tool_name = Some(name.into());
        self
    }

    /// Set the resource URI
    #[must_use]
    pub fn resource_uri(mut self, uri: impl Into<String>) -> Self {
        self.resource_uri = Some(uri.into());
        self
    }

    /// Set the prompt name
    #[must_use]
    pub fn prompt_name(mut self, name: impl Into<String>) -> Self {
        self.prompt_name = Some(name.into());
        self
    }

    /// Set the transport type
    #[must_use]
    pub fn transport(mut self, transport: impl Into<String>) -> Self {
        self.transport = Some(transport.into());
        self
    }

    /// Set the protocol version
    #[must_use]
    pub fn protocol_version(mut self, version: impl Into<String>) -> Self {
        self.protocol_version = Some(version.into());
        self
    }

    /// Set the tenant ID
    #[must_use]
    pub fn tenant_id(mut self, id: impl Into<String>) -> Self {
        self.tenant_id = Some(id.into());
        self
    }

    /// Set the user ID
    #[must_use]
    pub fn user_id(mut self, id: impl Into<String>) -> Self {
        self.user_id = Some(id.into());
        self
    }

    /// Set client information
    #[must_use]
    pub fn client(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.client_name = Some(name.into());
        self.client_version = Some(version.into());
        self
    }

    /// Set server information
    #[must_use]
    pub fn server(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.server_name = Some(name.into());
        self.server_version = Some(version.into());
        self
    }

    /// Create a tracing span from this context
    #[must_use]
    pub fn into_span(self) -> Span {
        let method = self.method.as_deref().unwrap_or("unknown");

        let span = info_span!(
            "mcp.request",
            { MCP_METHOD } = method,
            { MCP_REQUEST_ID } = tracing::field::Empty,
            { MCP_SESSION_ID } = tracing::field::Empty,
            { MCP_TOOL_NAME } = tracing::field::Empty,
            { MCP_RESOURCE_URI } = tracing::field::Empty,
            { MCP_PROMPT_NAME } = tracing::field::Empty,
            { MCP_TRANSPORT } = tracing::field::Empty,
            { MCP_PROTOCOL_VERSION } = tracing::field::Empty,
            { MCP_TENANT_ID } = tracing::field::Empty,
            { MCP_USER_ID } = tracing::field::Empty,
            { MCP_CLIENT_NAME } = tracing::field::Empty,
            { MCP_CLIENT_VERSION } = tracing::field::Empty,
            { MCP_SERVER_NAME } = tracing::field::Empty,
            { MCP_SERVER_VERSION } = tracing::field::Empty,
        );

        // Record optional fields
        if let Some(ref id) = self.request_id {
            span.record(MCP_REQUEST_ID, id.as_str());
        }
        if let Some(ref id) = self.session_id {
            span.record(MCP_SESSION_ID, id.as_str());
        }
        if let Some(ref name) = self.tool_name {
            span.record(MCP_TOOL_NAME, name.as_str());
        }
        if let Some(ref uri) = self.resource_uri {
            span.record(MCP_RESOURCE_URI, uri.as_str());
        }
        if let Some(ref name) = self.prompt_name {
            span.record(MCP_PROMPT_NAME, name.as_str());
        }
        if let Some(ref transport) = self.transport {
            span.record(MCP_TRANSPORT, transport.as_str());
        }
        if let Some(ref version) = self.protocol_version {
            span.record(MCP_PROTOCOL_VERSION, version.as_str());
        }
        if let Some(ref id) = self.tenant_id {
            span.record(MCP_TENANT_ID, id.as_str());
        }
        if let Some(ref id) = self.user_id {
            span.record(MCP_USER_ID, id.as_str());
        }
        if let Some(ref name) = self.client_name {
            span.record(MCP_CLIENT_NAME, name.as_str());
        }
        if let Some(ref version) = self.client_version {
            span.record(MCP_CLIENT_VERSION, version.as_str());
        }
        if let Some(ref name) = self.server_name {
            span.record(MCP_SERVER_NAME, name.as_str());
        }
        if let Some(ref version) = self.server_version {
            span.record(MCP_SERVER_VERSION, version.as_str());
        }

        span
    }
}

/// Record request completion on a span
pub fn record_completion(span: &Span, duration: Duration, success: bool, error: Option<&str>) {
    span.record(MCP_DURATION_MS, duration.as_millis() as i64);
    span.record(MCP_STATUS, if success { "success" } else { "error" });

    if let Some(err) = error {
        span.record(MCP_ERROR_MESSAGE, err);
    }
}

/// Create a span for a tool call
#[must_use]
pub fn tool_call_span(tool_name: &str, request_id: Option<&str>) -> Span {
    let mut ctx = McpSpanContext::new()
        .method("tools/call")
        .tool_name(tool_name);

    if let Some(id) = request_id {
        ctx = ctx.request_id(id);
    }

    ctx.into_span()
}

/// Create a span for a resource read
#[must_use]
pub fn resource_read_span(uri: &str, request_id: Option<&str>) -> Span {
    let mut ctx = McpSpanContext::new()
        .method("resources/read")
        .resource_uri(uri);

    if let Some(id) = request_id {
        ctx = ctx.request_id(id);
    }

    ctx.into_span()
}

/// Create a span for a prompt get
#[must_use]
pub fn prompt_get_span(prompt_name: &str, request_id: Option<&str>) -> Span {
    let mut ctx = McpSpanContext::new()
        .method("prompts/get")
        .prompt_name(prompt_name);

    if let Some(id) = request_id {
        ctx = ctx.request_id(id);
    }

    ctx.into_span()
}

/// Create a span for initialization
#[must_use]
pub fn initialize_span(client_name: &str, client_version: &str) -> Span {
    McpSpanContext::new()
        .method("initialize")
        .client(client_name, client_version)
        .into_span()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_context_builder() {
        let ctx = McpSpanContext::new()
            .method("tools/call")
            .tool_name("calculator")
            .request_id("req-123")
            .session_id("sess-456")
            .transport("http")
            .tenant_id("tenant-789");

        assert_eq!(ctx.method, Some("tools/call".to_string()));
        assert_eq!(ctx.tool_name, Some("calculator".to_string()));
        assert_eq!(ctx.request_id, Some("req-123".to_string()));
        assert_eq!(ctx.session_id, Some("sess-456".to_string()));
        assert_eq!(ctx.transport, Some("http".to_string()));
        assert_eq!(ctx.tenant_id, Some("tenant-789".to_string()));
    }

    #[test]
    fn test_convenience_spans() {
        // These should not panic
        let _span = tool_call_span("test_tool", Some("req-1"));
        let _span = resource_read_span("file:///test.txt", None);
        let _span = prompt_get_span("greeting", Some("req-2"));
        let _span = initialize_span("test-client", "1.0.0");
    }

    #[test]
    fn test_span_context_to_span() {
        // Use a test subscriber to ensure spans are not disabled
        let subscriber = tracing_subscriber::registry();
        tracing::subscriber::with_default(subscriber, || {
            let ctx = McpSpanContext::new()
                .method("tools/list")
                .request_id("req-abc");

            // Verify span can be created - it will still be "disabled" without
            // an active layer, but the construction should succeed
            let _span = ctx.into_span();
        });
    }
}
