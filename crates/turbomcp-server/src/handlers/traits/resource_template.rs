//! Resource template handler trait for parameterized resource access

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{
    ListResourceTemplatesRequest, ListResourceTemplatesResult, ResourceTemplate,
};

use crate::ServerResult;

/// Resource template handler trait for parameterized resource access
#[async_trait]
pub trait ResourceTemplateHandler: Send + Sync {
    /// Handle a list resource templates request
    async fn handle(
        &self,
        request: ListResourceTemplatesRequest,
        ctx: RequestContext,
    ) -> ServerResult<ListResourceTemplatesResult>;

    /// Get available resource templates
    async fn get_templates(&self, ctx: RequestContext) -> ServerResult<Vec<ResourceTemplate>>;

    /// Get a specific template by name
    async fn get_template(
        &self,
        name: &str,
        ctx: RequestContext,
    ) -> ServerResult<Option<ResourceTemplate>>;

    /// Validate template URI pattern (RFC 6570)
    fn validate_uri_template(&self, uri_template: &str) -> ServerResult<()> {
        // Basic validation - can be overridden for more sophisticated checking
        if uri_template.is_empty() {
            return Err(crate::McpError::handler("URI template cannot be empty"));
        }
        Ok(())
    }

    /// Expand URI template with parameters
    fn expand_template(
        &self,
        uri_template: &str,
        parameters: &HashMap<String, Value>,
    ) -> ServerResult<String> {
        // Basic template expansion - should be overridden for full RFC 6570 support
        let mut result = uri_template.to_string();

        for (key, value) in parameters {
            let placeholder = format!("{{{}}}", key);
            if let Some(str_value) = value.as_str() {
                result = result.replace(&placeholder, str_value);
            } else {
                result = result.replace(&placeholder, &value.to_string());
            }
        }

        Ok(result)
    }

    /// Validate template parameters
    async fn validate_parameters(
        &self,
        _template: &ResourceTemplate,
        _parameters: &HashMap<String, Value>,
        _ctx: RequestContext,
    ) -> ServerResult<()> {
        // Default implementation - no validation
        // Override in implementations to add specific parameter validation
        Ok(())
    }
}
