//! `TurboMCP` Transport Ergonomics
//!
//! This module provides ergonomic wrappers and extensions over the comprehensive
//! mcp-transport infrastructure. It focuses on developer experience while leveraging
//! the well-established foundation.

//use async_trait::async_trait;
//use std::collections::HashMap;

// Re-export core transport functionality
pub use turbomcp_transport::{StdioTransport, Transport, TransportConfig, TransportResult};

#[cfg(feature = "websocket")]
use crate::{McpError, McpResult};

#[cfg(feature = "http")]
pub use turbomcp_transport::{AxumMcpExt, McpAppState, McpServerConfig, McpService};

// Note: Use WebSocketBidirectionalTransport for MCP 2025-06-18 compliant WebSocket support
#[cfg(feature = "websocket")]
pub use turbomcp_transport::WebSocketBidirectionalTransport;

/// Ergonomic transport factory for quick setup
pub struct TransportFactory;

impl TransportFactory {
    /// Create stdio transport (most common for development)
    #[must_use]
    pub fn stdio() -> StdioTransport {
        StdioTransport::new()
    }

    /// Create HTTP server with ergonomic defaults (Note: Use AxumMcpExt for HTTP server functionality)
    #[cfg(feature = "http")]
    pub fn http_server_note() -> &'static str {
        "HTTP server functionality available via AxumMcpExt trait - see axum_integration module"
    }

    /// Create WebSocket bidirectional transport with ergonomic defaults
    #[cfg(feature = "websocket")]
    pub fn websocket(endpoint: impl Into<String>) -> McpResult<WebSocketBidirectionalTransport> {
        let ep: String = endpoint.into();
        // Synchronous wrapper over async constructor for DX in non-async contexts
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let config = turbomcp_transport::WebSocketBidirectionalConfig {
            url: Some(ep),
            ..Default::default()
        };
        let transport = rt
            .block_on(async {
                turbomcp_transport::WebSocketBidirectionalTransport::new(config).await
            })
            .map_err(|e| McpError::transport(e.to_string()))?;
        Ok(transport)
    }
}

/// Transport configuration builder for advanced use cases
pub struct TransportConfigBuilder {
    inner: TransportConfig,
}

impl TransportConfigBuilder {
    /// Create new transport config builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: TransportConfig::default(),
        }
    }

    /// Set connection timeout (maps to `connect_timeout`)
    #[must_use]
    pub const fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.inner.connect_timeout = timeout;
        self
    }

    /// Set read timeout
    #[must_use]
    pub const fn read_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.inner.read_timeout = Some(timeout);
        self
    }

    /// Set write timeout
    #[must_use]
    pub const fn write_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.inner.write_timeout = Some(timeout);
        self
    }

    /// Build the transport config
    #[must_use]
    pub fn build(self) -> TransportConfig {
        self.inner
    }
}

impl Default for TransportConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience macro for creating transports
#[macro_export]
macro_rules! transport {
    (stdio) => {
        $crate::transport::TransportFactory::stdio()
    };

    // Note: Use WebSocketBidirectionalTransport for full MCP 2025-06-18 support
    (websocket, $endpoint:expr) => {
        $crate::transport::TransportFactory::websocket($endpoint)?
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_factory() {
        let _stdio = TransportFactory::stdio();

        #[cfg(feature = "http")]
        {
            let _note = TransportFactory::http_server_note();
            assert!(_note.contains("AxumMcpExt"));
        }
    }

    #[test]
    fn test_config_builder() {
        let config = TransportConfigBuilder::new()
            .timeout(std::time::Duration::from_secs(30))
            .build();

        // Test that we can build a config - specific field checks removed
        // as the underlying TransportConfig fields may vary
        assert_eq!(config.connect_timeout, std::time::Duration::from_secs(30));
    }
}
