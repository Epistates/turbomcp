//! Configuration types for turbomcp-proxy

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Session timeout
    pub session_timeout: Duration,

    /// Maximum concurrent sessions
    pub max_sessions: usize,

    /// Request timeout
    pub request_timeout: Duration,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            session_timeout: Duration::from_secs(300),
            max_sessions: 1000,
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// ID mapping strategy
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum IdMappingStrategy {
    /// Prefix message IDs with session ID
    Prefix,

    /// Use UUID mapping table
    MappingTable,

    /// Pass through (no mapping)
    PassThrough,
}

impl Default for IdMappingStrategy {
    fn default() -> Self {
        Self::Prefix
    }
}

/// Backend configuration for runtime proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BackendConfig {
    /// Standard I/O backend (subprocess)
    Stdio {
        /// Command to execute (e.g., "python", "node")
        command: String,
        /// Command arguments
        args: Vec<String>,
        /// Optional working directory
        #[serde(skip_serializing_if = "Option::is_none")]
        working_dir: Option<String>,
    },
    /// HTTP backend with Server-Sent Events
    Http {
        /// Base URL of the HTTP server
        url: String,
        /// Optional authentication token
        #[serde(skip_serializing_if = "Option::is_none")]
        auth_token: Option<String>,
    },
    /// TCP backend with bidirectional communication
    Tcp {
        /// Host or IP address
        host: String,
        /// Port number
        port: u16,
    },
    /// Unix domain socket backend
    Unix {
        /// Socket file path
        path: String,
    },
    /// WebSocket backend with bidirectional communication
    WebSocket {
        /// WebSocket URL (ws:// or wss://)
        url: String,
    },
}

/// Frontend type for runtime proxy
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FrontendType {
    /// Standard I/O frontend
    Stdio,
    /// HTTP with Server-Sent Events frontend
    Http,
    /// WebSocket bidirectional frontend
    WebSocket,
}
