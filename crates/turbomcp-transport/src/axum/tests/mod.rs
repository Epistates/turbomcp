//! Comprehensive test suite for the Axum MCP integration
//!
//! This test suite is organized into focused modules to test different
//! aspects of the MCP integration:
//!
//! - `config` - Configuration system tests (server, CORS, security, etc.)
//! - `router` - Router extension trait tests (AxumMcpExt functionality)
//! - `integration` - End-to-end integration tests
//!
//! All tests maintain the original test coverage while being organized
//! by functionality for better maintainability.

#[cfg(test)]
#[cfg(feature = "http")]
pub mod config;
#[cfg(test)]
#[cfg(feature = "http")]
pub mod integration;
#[cfg(test)]
#[cfg(feature = "http")]
pub mod router;

#[cfg(test)]
#[cfg(feature = "http")]
pub mod common {
    //! Common test utilities and mock services

    use super::super::*;
    use crate::tower::SessionInfo;
    use turbomcp_protocol::Result as McpResult;

    /// Test MCP service implementation for use in tests
    #[derive(Clone, Debug)]
    pub struct TestMcpService;

    #[async_trait::async_trait]
    impl McpService for TestMcpService {
        async fn process_request(
            &self,
            request: serde_json::Value,
            _session: &SessionInfo,
        ) -> McpResult<serde_json::Value> {
            // Echo the request back as result
            Ok(serde_json::json!({
                "echo": request,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }))
        }
    }
}
