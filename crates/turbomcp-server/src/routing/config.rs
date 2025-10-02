//! Router configuration types and defaults

/// Router configuration
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Enable request validation
    pub validate_requests: bool,
    /// Enable response validation
    pub validate_responses: bool,
    /// Default request timeout in milliseconds
    pub default_timeout_ms: u64,
    /// Enable request tracing
    pub enable_tracing: bool,
    /// Maximum concurrent requests
    pub max_concurrent_requests: usize,
    /// Enable bidirectional routing (server-initiated requests)
    pub enable_bidirectional: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            validate_requests: true,
            validate_responses: true,
            default_timeout_ms: 30_000,
            enable_tracing: true,
            max_concurrent_requests: 1000,
            enable_bidirectional: true,
        }
    }
}
