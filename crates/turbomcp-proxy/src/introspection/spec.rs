//! MCP Server Specification Types
//!
//! Complete type definitions for representing an MCP server's capabilities
//! as discovered through introspection.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete specification of an MCP server discovered via introspection
///
/// This is the primary output of the introspection process, containing
/// everything needed to understand and interact with an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSpec {
    /// Server information from initialize response
    pub server_info: ServerInfo,

    /// Protocol version (e.g., "2025-06-18")
    pub protocol_version: String,

    /// Server capabilities
    pub capabilities: ServerCapabilities,

    /// Discovered tools with JSON schemas
    pub tools: Vec<ToolSpec>,

    /// Discovered resources
    pub resources: Vec<ResourceSpec>,

    /// Discovered resource templates (if any)
    #[serde(default)]
    pub resource_templates: Vec<ResourceTemplateSpec>,

    /// Discovered prompts
    pub prompts: Vec<PromptSpec>,

    /// Optional server instructions (from initialize response)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Server information (name and version)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Server name (identifier)
    pub name: String,

    /// Server version
    pub version: String,

    /// Optional display title (for UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Server capabilities from MCP protocol
///
/// Indicates which optional features the server supports.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    /// Server supports logging
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingCapability>,

    /// Server supports argument autocompletion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<EmptyCapability>,

    /// Server offers prompts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,

    /// Server offers resources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,

    /// Server offers tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,

    /// Experimental capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, serde_json::Value>>,
}

/// Logging capability (empty object if present)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingCapability {}

/// Empty capability marker (for completions, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyCapability {}

/// Prompts capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptsCapability {
    /// Whether server supports notifications for prompt list changes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Resources capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourcesCapability {
    /// Whether server supports subscribing to resource updates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,

    /// Whether server supports notifications for resource list changes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Tools capability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsCapability {
    /// Whether server supports notifications for tool list changes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Tool specification with complete schema information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Tool name (identifier)
    pub name: String,

    /// Optional display title (for UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// JSON Schema for input parameters (always an object schema)
    pub input_schema: ToolInputSchema,

    /// Optional JSON Schema for output (if server provides it)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<ToolOutputSchema>,

    /// Optional annotations (hints about tool behavior)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

/// Tool input schema (JSON Schema object)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    /// Schema type (always "object" for MCP tools)
    #[serde(rename = "type")]
    pub schema_type: String,

    /// Properties definition
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,

    /// Required property names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,

    /// Additional schema fields
    #[serde(flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// Tool output schema (JSON Schema object)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutputSchema {
    /// Schema type (always "object" for MCP tools)
    #[serde(rename = "type")]
    pub schema_type: String,

    /// Properties definition
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, serde_json::Value>>,

    /// Required property names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,

    /// Additional schema fields
    #[serde(flatten)]
    pub additional: HashMap<String, serde_json::Value>,
}

/// Tool annotations (hints about tool behavior)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAnnotations {
    /// Display title (for UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// If true, the tool does not modify its environment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,

    /// If true, the tool may perform destructive updates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,

    /// If true, calling repeatedly with same args has no additional effect
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,

    /// If true, tool interacts with "open world" of external entities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// Resource specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    /// Resource URI
    pub uri: String,

    /// Resource name (identifier)
    pub name: String,

    /// Optional display title (for UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// MIME type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Size in bytes (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// Optional annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

/// Resource template specification (for URI templates)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplateSpec {
    /// URI template (RFC 6570)
    pub uri_template: String,

    /// Template name (identifier)
    pub name: String,

    /// Optional display title (for UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// MIME type (if all resources matching template have same type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Optional annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
}

/// Prompt specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSpec {
    /// Prompt name (identifier)
    pub name: String,

    /// Optional display title (for UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Template arguments
    #[serde(default)]
    pub arguments: Vec<PromptArgument>,
}

/// Prompt argument specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    /// Argument name
    pub name: String,

    /// Optional display title (for UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether argument is required
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// Generic annotations for resources/tools/prompts
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Annotations {
    /// Arbitrary annotation fields
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

impl ServerSpec {
    /// Check if server supports a specific capability
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

    /// Check if server supports list_changed notifications for a capability
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
    pub fn supports_resource_subscriptions(&self) -> bool {
        self.capabilities
            .resources
            .as_ref()
            .and_then(|c| c.subscribe)
            .unwrap_or(false)
    }

    /// Get a summary of what the server offers
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

    #[test]
    fn test_server_spec_serialization() {
        let spec = ServerSpec {
            server_info: ServerInfo {
                name: "test-server".to_string(),
                version: "1.0.0".to_string(),
                title: None,
            },
            protocol_version: "2025-06-18".to_string(),
            capabilities: ServerCapabilities::default(),
            tools: vec![],
            resources: vec![],
            resource_templates: vec![],
            prompts: vec![],
            instructions: None,
        };

        let json = serde_json::to_string_pretty(&spec).unwrap();
        assert!(json.contains("test-server"));
        assert!(json.contains("2025-06-18"));
    }

    #[test]
    fn test_capability_checks() {
        let spec = ServerSpec {
            server_info: ServerInfo {
                name: "test-server".to_string(),
                version: "1.0.0".to_string(),
                title: None,
            },
            protocol_version: "2025-06-18".to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(true),
                }),
                ..Default::default()
            },
            tools: vec![],
            resources: vec![],
            resource_templates: vec![],
            prompts: vec![],
            instructions: None,
        };

        assert!(spec.has_capability("tools"));
        assert!(!spec.has_capability("prompts"));
        assert!(spec.supports_list_changed("tools"));
        assert!(!spec.supports_list_changed("prompts"));
    }
}
