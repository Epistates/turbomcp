//! Capability router for proxy
//!
//! Routes messages between frontend server and backend client, handling
//! request/response correlation, ID translation, and bidirectional notifications.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, trace};
use turbomcp_protocol::types::{
    CallToolResult, GetPromptResult, Prompt, Resource, ResourceContents, Tool,
};
use turbomcp_protocol::RequestContext;

use super::{BackendConnector, IdTranslator};
use crate::error::ProxyResult;
use crate::introspection::ServerSpec;

/// Capability router
///
/// The heart of the proxy - routes all messages between frontend and backend,
/// managing ID translation and request/response correlation.
pub struct CapabilityRouter {
    /// Backend connector
    backend: Arc<RwLock<BackendConnector>>,

    /// ID translator for message correlation
    id_translator: IdTranslator,

    /// Cached server spec (tools, resources, prompts)
    spec: Arc<RwLock<Option<ServerSpec>>>,
}

impl CapabilityRouter {
    /// Create a new capability router
    ///
    /// # Arguments
    ///
    /// * `backend` - The backend connector
    pub fn new(backend: BackendConnector) -> Self {
        Self {
            backend: Arc::new(RwLock::new(backend)),
            id_translator: IdTranslator::new(),
            spec: Arc::new(RwLock::new(None)),
        }
    }

    /// Initialize the router by introspecting the backend
    ///
    /// This discovers all backend capabilities and caches them for
    /// dynamic handler registration in the frontend.
    pub async fn initialize(&self) -> ProxyResult<ServerSpec> {
        debug!("Initializing capability router");

        // Introspect backend
        let spec = {
            let mut backend = self.backend.write().await;
            backend.introspect().await?
        };

        // Cache spec
        {
            let mut cached_spec = self.spec.write().await;
            *cached_spec = Some(spec.clone());
        }

        debug!(
            "Router initialized with {} tools, {} resources, {} prompts",
            spec.tools.len(),
            spec.resources.len(),
            spec.prompts.len()
        );

        Ok(spec)
    }

    /// Get the cached server spec
    pub async fn spec(&self) -> Option<ServerSpec> {
        let spec = self.spec.read().await;
        spec.clone()
    }

    /// Route a tool call from frontend to backend
    ///
    /// # Arguments
    ///
    /// * `frontend_ctx` - The frontend request context
    /// * `name` - Tool name
    /// * `arguments` - Tool arguments
    ///
    /// # Returns
    ///
    /// The tool call result from the backend
    pub async fn route_call_tool(
        &self,
        frontend_ctx: &RequestContext,
        name: String,
        arguments: Option<HashMap<String, Value>>,
    ) -> ProxyResult<CallToolResult> {
        trace!(
            "Routing tool call: {} (frontend ID: {:?})",
            name,
            frontend_ctx.id
        );

        // Allocate backend ID
        let _backend_id = self.id_translator.allocate(frontend_ctx.id.clone())?;

        // Call backend
        let result = {
            let backend = self.backend.read().await;
            backend.call_tool(&name, arguments).await?
        };

        // Release ID mapping (response sent)
        self.id_translator.release(&frontend_ctx.id);

        trace!("Tool call routed successfully: {}", name);

        Ok(result)
    }

    /// Route a list_tools request
    pub async fn route_list_tools(&self) -> ProxyResult<Vec<Tool>> {
        trace!("Routing list_tools");

        let backend = self.backend.read().await;
        backend.list_tools().await
    }

    /// Route a list_resources request
    pub async fn route_list_resources(&self) -> ProxyResult<Vec<Resource>> {
        trace!("Routing list_resources");

        let backend = self.backend.read().await;
        backend.list_resources().await
    }

    /// Route a read_resource request
    pub async fn route_read_resource(
        &self,
        frontend_ctx: &RequestContext,
        uri: String,
    ) -> ProxyResult<Vec<ResourceContents>> {
        trace!(
            "Routing read_resource: {} (frontend ID: {:?})",
            uri,
            frontend_ctx.id
        );

        // Allocate backend ID
        let _backend_id = self.id_translator.allocate(frontend_ctx.id.clone())?;

        // Call backend
        let result = {
            let backend = self.backend.read().await;
            backend.read_resource(&uri).await?
        };

        // Release ID mapping
        self.id_translator.release(&frontend_ctx.id);

        trace!("Resource read routed successfully: {}", uri);

        Ok(result)
    }

    /// Route a list_prompts request
    pub async fn route_list_prompts(&self) -> ProxyResult<Vec<Prompt>> {
        trace!("Routing list_prompts");

        let backend = self.backend.read().await;
        backend.list_prompts().await
    }

    /// Route a get_prompt request
    pub async fn route_get_prompt(
        &self,
        frontend_ctx: &RequestContext,
        name: String,
        arguments: Option<HashMap<String, String>>,
    ) -> ProxyResult<GetPromptResult> {
        trace!(
            "Routing get_prompt: {} (frontend ID: {:?})",
            name,
            frontend_ctx.id
        );

        // Allocate backend ID
        let _backend_id = self.id_translator.allocate(frontend_ctx.id.clone())?;

        // Call backend
        let result = {
            let backend = self.backend.read().await;
            backend.get_prompt(&name, arguments).await?
        };

        // Release ID mapping
        self.id_translator.release(&frontend_ctx.id);

        trace!("Prompt get routed successfully: {}", name);

        Ok(result)
    }

    /// Refresh capabilities from backend
    ///
    /// Re-introspects the backend server to discover any new tools, resources,
    /// or prompts that may have been registered at runtime.
    pub async fn refresh_capabilities(&self) -> ProxyResult<ServerSpec> {
        debug!("Refreshing capabilities from backend");

        let spec = {
            let mut backend = self.backend.write().await;
            backend.introspect().await?
        };

        // Update cached spec
        {
            let mut cached_spec = self.spec.write().await;
            *cached_spec = Some(spec.clone());
        }

        debug!("Capabilities refreshed");

        Ok(spec)
    }

    /// Get current ID mapping count (for monitoring/debugging)
    pub async fn active_requests(&self) -> usize {
        self.id_translator.mapping_count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::BackendConfig;
    use crate::proxy::BackendTransport;

    async fn create_test_router() -> Option<CapabilityRouter> {
        let config = BackendConfig {
            transport: BackendTransport::Stdio {
                command: "cargo".to_string(),
                args: vec![
                    "run".to_string(),
                    "--package".to_string(),
                    "turbomcp".to_string(),
                    "--example".to_string(),
                    "stdio_server".to_string(),
                ],
                working_dir: Some("/Users/nickpaterno/work/turbomcp".to_string()),
            },
            client_name: "test-router".to_string(),
            client_version: "1.0.0".to_string(),
        };

        match BackendConnector::new(config).await {
            Ok(backend) => Some(CapabilityRouter::new(backend)),
            Err(_) => None,
        }
    }

    #[tokio::test]
    async fn test_router_initialization() {
        if let Some(router) = create_test_router().await {
            let result = router.initialize().await;
            if let Ok(spec) = result {
                assert!(!spec.tools.is_empty(), "Should have tools");

                // Verify cached spec
                let cached = router.spec().await;
                assert!(cached.is_some(), "Spec should be cached");
            }
        }
    }

    #[tokio::test]
    async fn test_active_requests_tracking() {
        if let Some(router) = create_test_router().await {
            // Initial count should be 0
            let initial = router.active_requests().await;
            assert_eq!(initial, 0);

            // Initialize router
            let _ = router.initialize().await;

            // After initialization, should still be 0 (no pending requests)
            let after_init = router.active_requests().await;
            assert_eq!(after_init, 0);
        }
    }
}
