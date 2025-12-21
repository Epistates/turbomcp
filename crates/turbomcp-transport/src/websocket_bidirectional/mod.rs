//! WebSocket bidirectional transport with elicitation support
//!
//! This module provides a WebSocket transport implementation
//! with full bidirectional communication support for the MCP 2025-06-18 protocol,
//! including server-initiated elicitation requests.
//!
//! ## Architecture
//!
//! The WebSocket bidirectional transport is organized into focused components:
//!
//! ```text
//! websocket_bidirectional/
//! ├── config.rs        # Configuration types and builders
//! ├── types.rs         # Core types and type aliases
//! ├── connection.rs    # Connection management and lifecycle
//! ├── tasks.rs         # Background task management
//! ├── elicitation.rs   # Elicitation handling and timeout management
//! ├── transport.rs     # Main Transport trait implementation
//! └── bidirectional.rs # BidirectionalTransport trait implementation
//! ```
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! // NOTE: This example uses the ElicitRequestParams::Form variant
//! use turbomcp_transport::websocket_bidirectional::{
//!     WebSocketBidirectionalTransport, WebSocketBidirectionalConfig
//! };
//! use turbomcp_protocol::types::{
//!     ElicitRequest, ElicitRequestParams, FormElicitRequestParams,
//!     ElicitationSchema, PrimitiveSchemaDefinition, StringSchema
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create client configuration
//! let config = WebSocketBidirectionalConfig::client("ws://localhost:8080".to_string())
//!     .with_max_concurrent_elicitations(5)
//!     .with_compression(true);
//!
//! // Create and connect transport
//! let mut transport = WebSocketBidirectionalTransport::new(config).await?;
//! transport.connect().await?;
//!
//! // Build elicitation schema
//! let string_schema = StringSchema {
//!     schema_type: "string".to_string(),
//!     title: Some("Name".to_string()),
//!     description: None,
//!     min_length: None,
//!     max_length: None,
//!     pattern: None,
//!     format: None,
//! };
//! let schema = ElicitationSchema::new()
//!     .add_property("name".to_string(), PrimitiveSchemaDefinition::String(string_schema));
//!
//! // Create elicitation request using Form variant
//! let request = ElicitRequest {
//!     params: ElicitRequestParams::Form(FormElicitRequestParams {
//!         message: "Please provide your name".to_string(),
//!         schema,
//!         timeout_ms: None,
//!         cancellable: Some(true),
//!     }),
//!     task: None,
//!     _meta: None,
//! };
//!
//! let result = transport.send_elicitation(request, None).await?;
//! println!("Elicitation result: {:?}", result);
//! # Ok(())
//! # }
//! ```
//!
//! ## Server Mode (Future Enhancement)
//!
//! ```rust,no_run
//! use turbomcp_transport::websocket_bidirectional::{
//!     WebSocketBidirectionalTransport, WebSocketBidirectionalConfig
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create server configuration
//! let config = WebSocketBidirectionalConfig::server("0.0.0.0:8080".to_string())
//!     .with_max_message_size(16 * 1024 * 1024)
//!     .with_keep_alive_interval(std::time::Duration::from_secs(30));
//!
//! let transport = WebSocketBidirectionalTransport::new(config).await?;
//! // Server mode implementation pending
//! # Ok(())
//! # }
//! ```
//!
//! ## Features
//!
//! - **Bidirectional Communication**: Full request-response patterns with correlation
//! - **Elicitation Support**: Server-initiated requests with timeout handling
//! - **Automatic Reconnection**: Configurable exponential backoff retry logic
//! - **Keep-Alive**: Periodic ping/pong to maintain connections
//! - **Compression**: Optional message compression support
//! - **TLS Support**: Secure WebSocket connections (WSS)
//! - **Metrics Collection**: Comprehensive transport metrics and monitoring
//! - **Background Tasks**: Efficient management of concurrent operations

pub mod bidirectional;
pub mod config;
pub mod connection;
pub mod elicitation;
pub mod mcp_methods;
pub mod tasks;
pub mod transport;
pub mod types;

// Re-export main types for convenience
pub use bidirectional::CorrelationInfo;
pub use config::{ReconnectConfig, TlsConfig, WebSocketBidirectionalConfig};
// Internal use only
pub use elicitation::ElicitationInfo;
// Internal use only
pub use transport::TransportStatus;
pub use types::{
    MessageProcessingResult, PendingElicitation, WebSocketBidirectionalTransport,
    WebSocketConnectionStats, WebSocketStreamHandler,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Transport;
    use std::time::Duration;

    #[tokio::test]
    async fn test_websocket_bidirectional_creation() {
        let config = WebSocketBidirectionalConfig::default();
        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        assert_eq!(
            transport.transport_type(),
            crate::core::TransportType::WebSocket
        );
        assert!(transport.capabilities().supports_bidirectional);
    }

    #[tokio::test]
    async fn test_elicitation_support() {
        let config = WebSocketBidirectionalConfig {
            max_concurrent_elicitations: 5,
            ..Default::default()
        };

        let transport = WebSocketBidirectionalTransport::new(config).await.unwrap();

        // Verify elicitation capability is advertised
        assert!(transport.capabilities.custom.contains_key("elicitation"));
        assert_eq!(
            transport.capabilities.custom.get("elicitation"),
            Some(&serde_json::json!(true))
        );
    }

    #[tokio::test]
    async fn test_reconnection_config() {
        let config = WebSocketBidirectionalConfig {
            reconnect: ReconnectConfig {
                enabled: true,
                max_retries: 5,
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_secs(10),
                backoff_factor: 2.0,
            },
            ..Default::default()
        };

        let _transport = WebSocketBidirectionalTransport::new(config.clone())
            .await
            .unwrap();

        assert!(config.reconnect.enabled);
        assert_eq!(config.reconnect.max_retries, 5);
    }

    #[tokio::test]
    async fn test_config_builders() {
        let config = WebSocketBidirectionalConfig::new()
            .with_max_message_size(1024)
            .with_compression(true)
            .with_max_concurrent_elicitations(10);

        assert_eq!(config.max_message_size, 1024);
        assert!(config.enable_compression);
        assert_eq!(config.max_concurrent_elicitations, 10);
    }

    #[tokio::test]
    async fn test_client_server_configs() {
        let client_config = WebSocketBidirectionalConfig::client("ws://example.com".to_string());
        assert_eq!(client_config.url, Some("ws://example.com".to_string()));
        assert_eq!(client_config.bind_addr, None);

        let server_config = WebSocketBidirectionalConfig::server("0.0.0.0:8080".to_string());
        assert_eq!(server_config.bind_addr, Some("0.0.0.0:8080".to_string()));
        assert_eq!(server_config.url, None);
    }
}
