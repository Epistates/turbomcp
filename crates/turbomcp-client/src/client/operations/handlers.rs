//! Handler registration operations for MCP client
//!
//! This module provides methods for registering and managing various event handlers
//! that process server-initiated operations and notifications.

use crate::handlers::{
    ElicitationHandler, LogHandler, ProgressHandler, ResourceUpdateHandler, RootsHandler,
};
use std::sync::Arc;

impl<T: turbomcp_transport::Transport> super::super::core::Client<T> {
    /// Register a roots handler for responding to server filesystem root requests
    ///
    /// Roots handlers respond to `roots/list` requests from servers (SERVER->CLIENT).
    /// Per MCP 2025-06-18 specification, servers ask clients what filesystem roots
    /// they have access to. This is commonly used when servers need to understand
    /// their operating boundaries, such as which repositories or project directories
    /// they can access.
    ///
    /// # Arguments
    ///
    /// * `handler` - The roots handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_client::handlers::{RootsHandler, HandlerResult};
    /// use turbomcp_protocol::types::Root;
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct MyRootsHandler {
    ///     project_dir: String,
    /// }
    ///
    /// #[async_trait]
    /// impl RootsHandler for MyRootsHandler {
    ///     async fn handle_roots_request(&self) -> HandlerResult<Vec<Root>> {
    ///         Ok(vec![Root {
    ///             uri: format!("file://{}", self.project_dir).into(),
    ///             name: Some("My Project".to_string()),
    ///         }])
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.on_roots(Arc::new(MyRootsHandler {
    ///     project_dir: "/home/user/projects/myproject".to_string(),
    /// }));
    /// ```
    pub fn on_roots(&self, handler: Arc<dyn RootsHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_roots_handler(handler);
    }

    /// Register an elicitation handler for processing user input requests
    ///
    /// Elicitation handlers are called when the server needs user input during
    /// operations. The handler should present the request to the user and
    /// collect their response according to the provided schema.
    ///
    /// # Arguments
    ///
    /// * `handler` - The elicitation handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_client::handlers::{ElicitationHandler, ElicitationRequest, ElicitationResponse, ElicitationAction, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    /// use serde_json::json;
    ///
    /// #[derive(Debug)]
    /// struct MyElicitationHandler;
    ///
    /// #[async_trait]
    /// impl ElicitationHandler for MyElicitationHandler {
    ///     async fn handle_elicitation(
    ///         &self,
    ///         request: ElicitationRequest,
    ///     ) -> HandlerResult<ElicitationResponse> {
    ///         Ok(ElicitationResponse {
    ///             action: ElicitationAction::Accept,
    ///             content: Some(json!({"user_input": "example"})),
    ///         })
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.on_elicitation(Arc::new(MyElicitationHandler));
    /// ```
    pub fn on_elicitation(&self, handler: Arc<dyn ElicitationHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_elicitation_handler(handler);
    }

    /// Register a progress handler for processing operation progress updates
    ///
    /// Progress handlers receive notifications about long-running server operations.
    /// Display progress bars, status updates, or other
    /// feedback to users.
    ///
    /// # Arguments
    ///
    /// * `handler` - The progress handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_client::handlers::{ProgressHandler, ProgressNotification, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct MyProgressHandler;
    ///
    /// #[async_trait]
    /// impl ProgressHandler for MyProgressHandler {
    ///     async fn handle_progress(&self, notification: ProgressNotification) -> HandlerResult<()> {
    ///         println!("Progress: {:?}", notification);
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.on_progress(Arc::new(MyProgressHandler));
    /// ```
    pub fn on_progress(&self, handler: Arc<dyn ProgressHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_progress_handler(handler);
    }

    /// Register a log handler for processing server log messages
    ///
    /// Log handlers receive log messages from the server and can route them
    /// to the client's logging system. This is useful for debugging and
    /// maintaining a unified log across client and server.
    ///
    /// # Arguments
    ///
    /// * `handler` - The log handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_client::handlers::{LogHandler, LogMessage, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct MyLogHandler;
    ///
    /// #[async_trait]
    /// impl LogHandler for MyLogHandler {
    ///     async fn handle_log(&self, log: LogMessage) -> HandlerResult<()> {
    ///         println!("Server log: {}", log.message);
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.on_log(Arc::new(MyLogHandler));
    /// ```
    pub fn on_log(&self, handler: Arc<dyn LogHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_log_handler(handler);
    }

    /// Register a resource update handler for processing resource change notifications
    ///
    /// Resource update handlers receive notifications when subscribed resources
    /// change on the server. Supports reactive updates to cached data or
    /// UI refreshes when server-side resources change.
    ///
    /// # Arguments
    ///
    /// * `handler` - The resource update handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_client::handlers::{ResourceUpdateHandler, ResourceUpdateNotification, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct MyResourceUpdateHandler;
    ///
    /// #[async_trait]
    /// impl ResourceUpdateHandler for MyResourceUpdateHandler {
    ///     async fn handle_resource_update(
    ///         &self,
    ///         notification: ResourceUpdateNotification,
    ///     ) -> HandlerResult<()> {
    ///         println!("Resource updated: {}", notification.uri);
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.on_resource_update(Arc::new(MyResourceUpdateHandler));
    /// ```
    pub fn on_resource_update(&self, handler: Arc<dyn ResourceUpdateHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_resource_update_handler(handler);
    }

    /// Check if a roots handler is registered
    pub fn has_roots_handler(&self) -> bool {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .has_roots_handler()
    }

    /// Check if an elicitation handler is registered
    pub fn has_elicitation_handler(&self) -> bool {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .has_elicitation_handler()
    }

    /// Check if a progress handler is registered
    pub fn has_progress_handler(&self) -> bool {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .has_progress_handler()
    }

    /// Check if a log handler is registered
    pub fn has_log_handler(&self) -> bool {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .has_log_handler()
    }

    /// Check if a resource update handler is registered
    pub fn has_resource_update_handler(&self) -> bool {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .has_resource_update_handler()
    }
}
