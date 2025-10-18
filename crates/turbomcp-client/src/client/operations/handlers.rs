//! Handler registration operations for MCP client
//!
//! This module provides methods for registering and managing various event handlers
//! that process server-initiated operations and notifications.

use crate::handlers::{
    CancellationHandler, ElicitationHandler, LogHandler, PromptListChangedHandler,
    ResourceListChangedHandler, ResourceUpdateHandler, RootsHandler, ToolListChangedHandler,
};
use std::sync::Arc;

impl<T: turbomcp_transport::Transport + 'static> super::super::core::Client<T> {
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
    /// client.set_roots_handler(Arc::new(MyRootsHandler {
    ///     project_dir: "/home/user/projects/myproject".to_string(),
    /// }));
    /// ```
    pub fn set_roots_handler(&self, handler: Arc<dyn RootsHandler>) {
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
    ///         let mut content = std::collections::HashMap::new();
    ///         content.insert("user_input".to_string(), json!("example"));
    ///         Ok(ElicitationResponse::accept(content))
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.set_elicitation_handler(Arc::new(MyElicitationHandler));
    /// ```
    pub fn set_elicitation_handler(&self, handler: Arc<dyn ElicitationHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_elicitation_handler(handler);
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
    /// use turbomcp_client::handlers::{LogHandler, LoggingNotification, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct MyLogHandler;
    ///
    /// #[async_trait]
    /// impl LogHandler for MyLogHandler {
    ///     async fn handle_log(&self, log: LoggingNotification) -> HandlerResult<()> {
    ///         println!("Server log: {}", log.data);
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.set_log_handler(Arc::new(MyLogHandler));
    /// ```
    pub fn set_log_handler(&self, handler: Arc<dyn LogHandler>) {
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
    /// use turbomcp_client::handlers::{ResourceUpdateHandler, ResourceUpdatedNotification, HandlerResult};
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
    ///         notification: ResourceUpdatedNotification,
    ///     ) -> HandlerResult<()> {
    ///         println!("Resource updated: {}", notification.uri);
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.set_resource_update_handler(Arc::new(MyResourceUpdateHandler));
    /// ```
    pub fn set_resource_update_handler(&self, handler: Arc<dyn ResourceUpdateHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_resource_update_handler(handler);
    }

    /// Register a cancellation handler for processing cancellation notifications
    ///
    /// Per MCP 2025-06-18 specification, cancellation notifications can be sent
    /// by the server to indicate that a previously-issued request is being cancelled.
    ///
    /// # Arguments
    ///
    /// * `handler` - The cancellation handler implementation
    pub fn set_cancellation_handler(&self, handler: Arc<dyn CancellationHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_cancellation_handler(handler);
    }

    /// Register a resource list changed handler
    ///
    /// This handler is called when the server's available resource list changes.
    ///
    /// # Arguments
    ///
    /// * `handler` - The resource list changed handler implementation
    pub fn set_resource_list_changed_handler(&self, handler: Arc<dyn ResourceListChangedHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_resource_list_changed_handler(handler);
    }

    /// Register a prompt list changed handler
    ///
    /// This handler is called when the server's available prompt list changes.
    ///
    /// # Arguments
    ///
    /// * `handler` - The prompt list changed handler implementation
    pub fn set_prompt_list_changed_handler(&self, handler: Arc<dyn PromptListChangedHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_prompt_list_changed_handler(handler);
    }

    /// Register a tool list changed handler
    ///
    /// This handler is called when the server's available tool list changes.
    ///
    /// # Arguments
    ///
    /// * `handler` - The tool list changed handler implementation
    pub fn set_tool_list_changed_handler(&self, handler: Arc<dyn ToolListChangedHandler>) {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .set_tool_list_changed_handler(handler);
    }

    /// Check if a roots handler is registered
    #[must_use]
    pub fn has_roots_handler(&self) -> bool {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .has_roots_handler()
    }

    /// Check if an elicitation handler is registered
    #[must_use]
    pub fn has_elicitation_handler(&self) -> bool {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .has_elicitation_handler()
    }

    /// Check if a log handler is registered
    #[must_use]
    pub fn has_log_handler(&self) -> bool {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .has_log_handler()
    }

    /// Check if a resource update handler is registered
    #[must_use]
    pub fn has_resource_update_handler(&self) -> bool {
        self.inner
            .handlers
            .lock()
            .expect("handlers mutex poisoned")
            .has_resource_update_handler()
    }
}
