//! Resource handler trait for processing resource requests

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use turbomcp_core::RequestContext;
use turbomcp_protocol::types::{ReadResourceRequest, ReadResourceResult, Resource};

use crate::ServerResult;

/// Resource handler trait for processing resource requests
#[async_trait]
pub trait ResourceHandler: Send + Sync {
    /// Handle a resource read request
    async fn handle(
        &self,
        request: ReadResourceRequest,
        ctx: RequestContext,
    ) -> ServerResult<ReadResourceResult>;

    /// Get the resource definition
    fn resource_definition(&self) -> Resource;

    /// Check if resource exists
    async fn exists(&self, uri: &str) -> bool;

    /// Get resource metadata
    async fn metadata(&self, _uri: &str) -> Option<HashMap<String, Value>> {
        None
    }
}
