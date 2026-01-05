//! Core transport types.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::{LimitsConfig, TimeoutConfig, TlsConfig};

/// Enumerates the types of transports supported by the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// Standard Input/Output, for command-line servers.
    Stdio,
    /// HTTP, including Server-Sent Events (SSE).
    Http,
    /// WebSocket for full-duplex communication.
    WebSocket,
    /// TCP sockets for network communication.
    Tcp,
    /// Unix domain sockets for local inter-process communication.
    Unix,
    /// A transport that manages a child process.
    ChildProcess,
    /// gRPC for high-performance RPC.
    #[cfg(feature = "grpc")]
    Grpc,
    /// QUIC for a modern, multiplexed transport.
    #[cfg(feature = "quic")]
    Quic,
}

impl fmt::Display for TransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdio => write!(f, "stdio"),
            Self::Http => write!(f, "http"),
            Self::WebSocket => write!(f, "websocket"),
            Self::Tcp => write!(f, "tcp"),
            Self::Unix => write!(f, "unix"),
            Self::ChildProcess => write!(f, "child_process"),
            #[cfg(feature = "grpc")]
            Self::Grpc => write!(f, "grpc"),
            #[cfg(feature = "quic")]
            Self::Quic => write!(f, "quic"),
        }
    }
}

/// Represents the current state of a transport connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportState {
    /// The transport is not connected.
    Disconnected,
    /// The transport is in the process of connecting.
    Connecting,
    /// The transport is connected and ready to send/receive messages.
    Connected,
    /// The transport is in the process of disconnecting.
    Disconnecting,
    /// The transport has encountered an unrecoverable error.
    Failed {
        /// A description of the failure reason.
        reason: String,
    },
}

impl fmt::Display for TransportState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected => write!(f, "disconnected"),
            Self::Connecting => write!(f, "connecting"),
            Self::Connected => write!(f, "connected"),
            Self::Disconnecting => write!(f, "disconnecting"),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
        }
    }
}

/// Describes the capabilities of a transport implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportCapabilities {
    /// The maximum message size in bytes that the transport can handle.
    pub max_message_size: Option<usize>,

    /// Whether the transport supports message compression.
    pub supports_compression: bool,

    /// Whether the transport supports streaming data.
    pub supports_streaming: bool,

    /// Whether the transport supports full-duplex bidirectional communication.
    pub supports_bidirectional: bool,

    /// Whether the transport can handle multiple concurrent requests over a single connection.
    pub supports_multiplexing: bool,

    /// A list of supported compression algorithms.
    pub compression_algorithms: Vec<String>,

    /// A map for any other custom capabilities.
    pub custom: HashMap<String, serde_json::Value>,
}

impl Default for TransportCapabilities {
    fn default() -> Self {
        Self {
            max_message_size: Some(turbomcp_protocol::MAX_MESSAGE_SIZE),
            supports_compression: false,
            supports_streaming: false,
            supports_bidirectional: true,
            supports_multiplexing: false,
            compression_algorithms: Vec::new(),
            custom: HashMap::new(),
        }
    }
}

/// Configuration for a transport instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// The type of the transport.
    pub transport_type: TransportType,

    /// The maximum time to wait for a connection to be established.
    pub connect_timeout: Duration,

    /// The maximum time to wait for a read operation to complete.
    pub read_timeout: Option<Duration>,

    /// The maximum time to wait for a write operation to complete.
    pub write_timeout: Option<Duration>,

    /// The interval for sending keep-alive messages to maintain the connection.
    pub keep_alive: Option<Duration>,

    /// The maximum number of concurrent connections allowed.
    pub max_connections: Option<usize>,

    /// Whether to enable message compression.
    pub compression: bool,

    /// The preferred compression algorithm to use.
    pub compression_algorithm: Option<String>,

    /// Size limits for requests and responses.
    #[serde(default)]
    pub limits: LimitsConfig,

    /// Timeout configuration for operations.
    #[serde(default)]
    pub timeouts: TimeoutConfig,

    /// TLS/HTTPS configuration.
    #[serde(default)]
    pub tls: TlsConfig,

    /// A map for any other custom configuration.
    pub custom: HashMap<String, serde_json::Value>,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            transport_type: TransportType::Stdio,
            connect_timeout: Duration::from_secs(30),
            read_timeout: None,
            write_timeout: None,
            keep_alive: None,
            max_connections: None,
            compression: false,
            compression_algorithm: None,
            limits: LimitsConfig::default(),
            timeouts: TimeoutConfig::default(),
            tls: TlsConfig::default(),
            custom: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_type_display() {
        assert_eq!(TransportType::Stdio.to_string(), "stdio");
        assert_eq!(TransportType::Http.to_string(), "http");
        assert_eq!(TransportType::WebSocket.to_string(), "websocket");
        assert_eq!(TransportType::Tcp.to_string(), "tcp");
        assert_eq!(TransportType::Unix.to_string(), "unix");
    }

    #[test]
    fn test_transport_state_display() {
        assert_eq!(TransportState::Connected.to_string(), "connected");
        assert_eq!(TransportState::Disconnected.to_string(), "disconnected");
        assert_eq!(
            TransportState::Failed {
                reason: "timeout".to_string()
            }
            .to_string(),
            "failed: timeout"
        );
    }

    #[test]
    fn test_transport_capabilities_default() {
        let caps = TransportCapabilities::default();
        assert!(caps.supports_bidirectional);
        assert!(!caps.supports_compression);
    }

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        assert_eq!(config.transport_type, TransportType::Stdio);
        assert_eq!(config.connect_timeout, Duration::from_secs(30));
    }
}
