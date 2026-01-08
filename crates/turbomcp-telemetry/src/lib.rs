//! OpenTelemetry integration and observability for TurboMCP SDK
//!
//! This crate provides comprehensive telemetry capabilities for MCP servers and clients:
//!
//! - **Distributed Tracing**: OpenTelemetry traces with MCP-specific span attributes
//! - **Metrics Collection**: Request counts, latencies, error rates with Prometheus export
//! - **Structured Logging**: JSON-formatted logs correlated with traces
//! - **Tower Middleware**: Automatic instrumentation for MCP request handling
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use turbomcp_telemetry::{TelemetryConfig, TelemetryGuard};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize telemetry with OTLP export
//!     let config = TelemetryConfig::builder()
//!         .service_name("my-mcp-server")
//!         .otlp_endpoint("http://localhost:4317")
//!         .build();
//!
//!     let _guard = config.init()?;
//!
//!     // Your MCP server code here...
//!     Ok(())
//! }
//! ```
//!
//! # Feature Flags
//!
//! - `opentelemetry` - Full OpenTelemetry integration with OTLP export
//! - `prometheus` - Standalone Prometheus metrics (without OpenTelemetry)
//! - `tower` - Tower middleware for automatic request instrumentation
//! - `full` - All features enabled
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    TurboMCP Application                      │
//! ├─────────────────────────────────────────────────────────────┤
//! │  TelemetryLayer (Tower Middleware)                          │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
//! │  │   Tracing   │  │   Metrics   │  │  Context Propagation │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘ │
//! ├─────────────────────────────────────────────────────────────┤
//! │                    Export Layer                              │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
//! │  │    OTLP     │  │ Prometheus  │  │       Stdout        │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘ │
//! └─────────────────────────────────────────────────────────────┘
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]

mod config;
mod error;
mod init;

pub mod attributes;

#[cfg(feature = "tower")]
#[cfg_attr(docsrs, doc(cfg(feature = "tower")))]
pub mod tower;

#[cfg(feature = "prometheus")]
#[cfg_attr(docsrs, doc(cfg(feature = "prometheus")))]
pub mod metrics;

// Re-exports
pub use config::{TelemetryConfig, TelemetryConfigBuilder};
pub use error::{TelemetryError, TelemetryResult};
pub use init::TelemetryGuard;

// Re-export tracing macros for convenience
pub use tracing::{Instrument, instrument};
pub use tracing::{debug, error, info, trace, warn};
pub use tracing::{debug_span, error_span, info_span, trace_span, warn_span};

/// MCP span attribute keys following OpenTelemetry semantic conventions
pub mod span_attributes {
    /// MCP method name (e.g., "tools/call", "resources/read")
    pub const MCP_METHOD: &str = "mcp.method";
    /// Tool name for tools/call requests
    pub const MCP_TOOL_NAME: &str = "mcp.tool.name";
    /// Resource URI for resources/read requests
    pub const MCP_RESOURCE_URI: &str = "mcp.resource.uri";
    /// Prompt name for prompts/get requests
    pub const MCP_PROMPT_NAME: &str = "mcp.prompt.name";
    /// JSON-RPC request ID
    pub const MCP_REQUEST_ID: &str = "mcp.request.id";
    /// MCP session ID
    pub const MCP_SESSION_ID: &str = "mcp.session.id";
    /// Client implementation name
    pub const MCP_CLIENT_NAME: &str = "mcp.client.name";
    /// Client implementation version
    pub const MCP_CLIENT_VERSION: &str = "mcp.client.version";
    /// Server implementation name
    pub const MCP_SERVER_NAME: &str = "mcp.server.name";
    /// Server implementation version
    pub const MCP_SERVER_VERSION: &str = "mcp.server.version";
    /// Transport type (stdio, http, websocket, tcp, unix)
    pub const MCP_TRANSPORT: &str = "mcp.transport";
    /// Protocol version
    pub const MCP_PROTOCOL_VERSION: &str = "mcp.protocol.version";
    /// Tenant ID for multi-tenant deployments
    pub const MCP_TENANT_ID: &str = "mcp.tenant.id";
    /// User ID from authentication
    pub const MCP_USER_ID: &str = "mcp.user.id";
    /// Request duration in milliseconds
    pub const MCP_DURATION_MS: &str = "mcp.duration_ms";
    /// Response status (success, error)
    pub const MCP_STATUS: &str = "mcp.status";
    /// Error code if request failed
    pub const MCP_ERROR_CODE: &str = "mcp.error.code";
    /// Error message if request failed
    pub const MCP_ERROR_MESSAGE: &str = "mcp.error.message";
}

/// Prelude module for convenient imports
pub mod prelude {
    pub use super::config::{TelemetryConfig, TelemetryConfigBuilder};
    pub use super::error::{TelemetryError, TelemetryResult};
    pub use super::init::TelemetryGuard;
    pub use super::span_attributes;
    pub use tracing::{Instrument, debug, error, info, instrument, trace, warn};

    #[cfg(feature = "tower")]
    pub use super::tower::{TelemetryLayer, TelemetryLayerConfig};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_attributes_defined() {
        // Verify all span attributes are properly defined
        assert_eq!(span_attributes::MCP_METHOD, "mcp.method");
        assert_eq!(span_attributes::MCP_TOOL_NAME, "mcp.tool.name");
        assert_eq!(span_attributes::MCP_RESOURCE_URI, "mcp.resource.uri");
        assert_eq!(span_attributes::MCP_SESSION_ID, "mcp.session.id");
        assert_eq!(span_attributes::MCP_TRANSPORT, "mcp.transport");
    }
}
