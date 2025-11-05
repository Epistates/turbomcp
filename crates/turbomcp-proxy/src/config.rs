//! Configuration types for turbomcp-proxy

use ipnetwork::IpNetwork;
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum IdMappingStrategy {
    /// Prefix message IDs with session ID
    #[default]
    Prefix,

    /// Use UUID mapping table
    MappingTable,

    /// Pass through (no mapping)
    PassThrough,
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

/// SSRF protection level for backend URL validation
///
/// Controls which IP ranges and endpoints are blocked to prevent
/// Server-Side Request Forgery attacks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SsrfProtection {
    /// Strict: Block all private networks and cloud metadata endpoints
    ///
    /// This is the recommended default for public-facing proxies.
    /// Blocks:
    /// - Private IPv4 ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)
    /// - Loopback addresses (127.0.0.0/8, `::1`)
    /// - Link-local addresses (169.254.0.0/16, `fe80::/10`)
    /// - Cloud metadata endpoints (169.254.169.254, 168.63.129.16)
    /// - IPv6 unique local addresses (`fc00::/7`)
    #[default]
    Strict,

    /// Balanced: Block cloud metadata, allow specific private networks
    ///
    /// Use this for internal proxies that need to connect to private services.
    /// Configure `allowed_private_networks` to specify which private networks
    /// are permitted.
    Balanced {
        /// List of allowed private IP ranges
        ///
        /// Uses the industry-standard `ipnetwork` crate for CIDR notation.
        /// Create networks using `IpNetwork::from_str("10.0.0.0/8")` or
        /// `Ipv4Network::from_str("192.168.1.0/24")`.
        allowed_private_networks: Vec<IpNetwork>,
    },

    /// Disabled: No SSRF protection (USE ONLY BEHIND FIREWALL)
    ///
    /// Only use this when the proxy is behind a firewall that handles
    /// SSRF protection at the network level.
    Disabled,
}

/// Backend URL validation configuration
///
/// Controls SSRF protection, allowed schemes, and custom blocklists for
/// backend connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendValidationConfig {
    /// SSRF protection level
    pub ssrf_protection: SsrfProtection,

    /// Allowed URL schemes (default: http, https, ws, wss)
    pub allowed_schemes: Vec<String>,

    /// Additional blocked hostnames (custom blocklist)
    ///
    /// Use this to block specific hostnames beyond the default SSRF protection.
    pub blocked_hosts: Vec<String>,
}

impl Default for BackendValidationConfig {
    fn default() -> Self {
        Self {
            ssrf_protection: SsrfProtection::Strict,
            allowed_schemes: vec![
                "http".to_string(),
                "https".to_string(),
                "ws".to_string(),
                "wss".to_string(),
            ],
            blocked_hosts: vec![],
        }
    }
}
