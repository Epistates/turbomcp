//! MCP Server Specification Types
//!
//! The result of introspecting an MCP server: its handshake metadata plus the
//! full tool, resource, and prompt catalogue.
//!
//! The catalogue entries are the protocol's own types rather than proxy-local
//! mirrors of them. Mirrors only ever carried the fields someone remembered to
//! copy, so each spec revision silently dropped whatever it added — icons,
//! `execution`, `_meta`, the server's description — and a proxied server
//! looked poorer than the same server reached directly. Holding the wire types
//! makes the spec lossless by construction.

use serde::{Deserialize, Serialize};
use turbomcp_protocol::types::{
    Implementation, Prompt, Resource, ResourceTemplate, ServerCapabilities, Tool,
};

/// Complete specification of an MCP server discovered via introspection
///
/// This is the primary output of the introspection process, containing
/// everything needed to understand and interact with an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSpec {
    /// Server implementation info from the initialize response (name,
    /// version, title, description, icons, website URL)
    pub server_info: Implementation,

    /// Protocol version (for example, "2025-11-25")
    pub protocol_version: String,

    /// Server capabilities
    pub capabilities: ServerCapabilities,

    /// Discovered tools with JSON schemas
    pub tools: Vec<Tool>,

    /// Discovered resources
    pub resources: Vec<Resource>,

    /// Discovered resource templates (if any)
    #[serde(default)]
    pub resource_templates: Vec<ResourceTemplate>,

    /// Discovered prompts
    pub prompts: Vec<Prompt>,

    /// Optional server instructions (from initialize response)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

impl ServerSpec {
    /// Check if server supports a specific capability
    #[must_use]
    pub fn has_capability(&self, capability: &str) -> bool {
        match capability {
            "logging" => self.capabilities.logging.is_some(),
            "completions" => self.capabilities.completions.is_some(),
            "prompts" => self.capabilities.prompts.is_some(),
            "resources" => self.capabilities.resources.is_some(),
            "tools" => self.capabilities.tools.is_some(),
            _ => false,
        }
    }

    /// Check if server supports `list_changed` notifications for a capability
    #[must_use]
    pub fn supports_list_changed(&self, capability: &str) -> bool {
        match capability {
            "prompts" => self
                .capabilities
                .prompts
                .as_ref()
                .and_then(|c| c.list_changed)
                .unwrap_or(false),
            "resources" => self
                .capabilities
                .resources
                .as_ref()
                .and_then(|c| c.list_changed)
                .unwrap_or(false),
            "tools" => self
                .capabilities
                .tools
                .as_ref()
                .and_then(|c| c.list_changed)
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Check if server supports resource subscriptions
    #[must_use]
    pub fn supports_resource_subscriptions(&self) -> bool {
        self.capabilities
            .resources
            .as_ref()
            .and_then(|c| c.subscribe)
            .unwrap_or(false)
    }

    /// Get a summary of what the server offers
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} v{}: {} tools, {} resources, {} prompts",
            self.server_info.name,
            self.server_info.version,
            self.tools.len(),
            self.resources.len(),
            self.prompts.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_protocol::types::ToolsCapabilities;

    fn empty_spec(capabilities: ServerCapabilities) -> ServerSpec {
        ServerSpec {
            server_info: Implementation::new("test-server", "1.0.0"),
            protocol_version: "2025-11-25".to_string(),
            capabilities,
            tools: vec![],
            resources: vec![],
            resource_templates: vec![],
            prompts: vec![],
            instructions: None,
        }
    }

    #[test]
    fn test_server_spec_serialization() {
        let spec = empty_spec(ServerCapabilities::default());

        let json = serde_json::to_string_pretty(&spec).unwrap();
        assert!(json.contains("test-server"));
        assert!(json.contains("2025-11-25"));
    }

    #[test]
    fn test_capability_checks() {
        let spec = empty_spec(ServerCapabilities {
            tools: Some(ToolsCapabilities {
                list_changed: Some(true),
            }),
            ..Default::default()
        });

        assert!(spec.has_capability("tools"));
        assert!(!spec.has_capability("prompts"));
        assert!(spec.supports_list_changed("tools"));
        assert!(!spec.supports_list_changed("prompts"));
    }
}
