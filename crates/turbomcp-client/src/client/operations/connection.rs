//! Connection and utility operations for MCP client
//!
//! This module provides connection health checks and server configuration
//! operations.

use std::sync::atomic::Ordering;
use turbomcp_protocol::types::{LogLevel, PingResult, SetLevelRequest, SetLevelResult};
use turbomcp_protocol::{Error, Result};

impl<T: turbomcp_transport::Transport + 'static> super::super::core::Client<T> {
    /// Send a ping request to check server health and connectivity
    ///
    /// Sends a ping request to the server to verify the connection is active
    /// and the server is responding. This is useful for health checks and
    /// connection validation.
    ///
    /// # Returns
    ///
    /// Returns `PingResult` on successful ping.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server is not responding
    /// - The connection has failed
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let result = client.ping().await?;
    /// println!("Server is responding");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn ping(&self) -> Result<PingResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send ping request (no parameters needed)
        let response: PingResult = self.inner.protocol.request("ping", None).await?;
        Ok(response)
    }

    /// Set the logging level for the MCP server
    ///
    /// Controls the verbosity of logs sent from the server to the client.
    /// Higher log levels provide more detailed information about server operations.
    ///
    /// # Arguments
    ///
    /// * `level` - The logging level to set (Error, Warn, Info, Debug)
    ///
    /// # Returns
    ///
    /// Returns `SetLevelResult` confirming the logging level change.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server doesn't support logging configuration
    /// - The request fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_protocol::types::LogLevel;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Set server to debug logging
    /// client.set_log_level(LogLevel::Debug).await?;
    /// println!("Server logging level set to debug");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_log_level(&self, level: LogLevel) -> Result<SetLevelResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send logging/setLevel request
        let request = SetLevelRequest { level };

        let response: SetLevelResult = self
            .inner
            .protocol
            .request("logging/setLevel", Some(serde_json::to_value(request)?))
            .await?;
        Ok(response)
    }
}
