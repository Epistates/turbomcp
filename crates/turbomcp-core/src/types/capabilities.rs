//! Capability types for MCP negotiation.

use alloc::string::String;
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Client capabilities for MCP
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Sampling capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingCapability>,
    /// Roots capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,
    /// Elicitation capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapability>,
    /// Experimental capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, Value>>,
}

impl ClientCapabilities {
    /// Create a new client capabilities with defaults
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable sampling capability
    #[must_use]
    pub fn with_sampling(mut self) -> Self {
        self.sampling = Some(SamplingCapability::default());
        self
    }

    /// Enable roots capability
    #[must_use]
    pub fn with_roots(mut self, list_changed: bool) -> Self {
        self.roots = Some(RootsCapability { list_changed: Some(list_changed) });
        self
    }

    /// Enable elicitation capability
    #[must_use]
    pub fn with_elicitation(mut self) -> Self {
        self.elicitation = Some(ElicitationCapability::default());
        self
    }
}

/// Server capabilities for MCP
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Prompts capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
    /// Resources capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    /// Tools capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    /// Logging capability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingCapability>,
    /// Experimental capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, Value>>,
}

impl ServerCapabilities {
    /// Create new server capabilities
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable tools capability
    #[must_use]
    pub fn with_tools(mut self, list_changed: bool) -> Self {
        self.tools = Some(ToolsCapability { list_changed: Some(list_changed) });
        self
    }

    /// Enable resources capability
    #[must_use]
    pub fn with_resources(mut self, subscribe: bool, list_changed: bool) -> Self {
        self.resources = Some(ResourcesCapability {
            subscribe: Some(subscribe),
            list_changed: Some(list_changed),
        });
        self
    }

    /// Enable prompts capability
    #[must_use]
    pub fn with_prompts(mut self, list_changed: bool) -> Self {
        self.prompts = Some(PromptsCapability { list_changed: Some(list_changed) });
        self
    }

    /// Enable logging capability
    #[must_use]
    pub fn with_logging(mut self) -> Self {
        self.logging = Some(LoggingCapability::default());
        self
    }
}

/// Tools capability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsCapability {
    /// Whether tool list changed notifications are supported
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Resources capability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourcesCapability {
    /// Whether resource subscriptions are supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    /// Whether resource list changed notifications are supported
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Prompts capability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptsCapability {
    /// Whether prompt list changed notifications are supported
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Logging capability
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoggingCapability {}

/// Sampling capability (client)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SamplingCapability {}

/// Roots capability (client)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RootsCapability {
    /// Whether roots list changed notifications are supported
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Elicitation capability (client)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ElicitationCapability {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_capabilities() {
        let caps = ServerCapabilities::new()
            .with_tools(true)
            .with_resources(true, true)
            .with_prompts(false);

        assert!(caps.tools.is_some());
        assert!(caps.resources.is_some());
        assert!(caps.prompts.is_some());
    }

    #[test]
    fn test_client_capabilities() {
        let caps = ClientCapabilities::new()
            .with_sampling()
            .with_roots(true);

        assert!(caps.sampling.is_some());
        assert!(caps.roots.is_some());
    }
}
