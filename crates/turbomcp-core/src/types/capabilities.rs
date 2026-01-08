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
    /// Tasks capability (MCP 2025-11-25)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<TasksCapability>,
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
        self.roots = Some(RootsCapability {
            list_changed: Some(list_changed),
        });
        self
    }

    /// Enable elicitation capability
    #[must_use]
    pub fn with_elicitation(mut self) -> Self {
        self.elicitation = Some(ElicitationCapability::default());
        self
    }

    /// Enable elicitation capability with full support (form + URL modes)
    #[must_use]
    pub fn with_full_elicitation(mut self) -> Self {
        self.elicitation = Some(ElicitationCapability::full());
        self
    }

    /// Enable tasks capability (MCP 2025-11-25)
    #[must_use]
    pub fn with_tasks(mut self, tasks: TasksCapability) -> Self {
        self.tasks = Some(tasks);
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
    /// Tasks capability (MCP 2025-11-25)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks: Option<TasksCapability>,
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
        self.tools = Some(ToolsCapability {
            list_changed: Some(list_changed),
        });
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
        self.prompts = Some(PromptsCapability {
            list_changed: Some(list_changed),
        });
        self
    }

    /// Enable logging capability
    #[must_use]
    pub fn with_logging(mut self) -> Self {
        self.logging = Some(LoggingCapability::default());
        self
    }

    /// Enable tasks capability (MCP 2025-11-25)
    #[must_use]
    pub fn with_tasks(mut self, tasks: TasksCapability) -> Self {
        self.tasks = Some(tasks);
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

/// Elicitation capability (client) - MCP 2025-11-25
///
/// Supports two modes:
/// - `form`: In-band structured data collection (default if empty)
/// - `url`: Out-of-band interactions (OAuth, credentials, payments)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ElicitationCapability {
    /// Form mode elicitation support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<HashMap<String, Value>>,
    /// URL mode elicitation support (MCP 2025-11-25)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<HashMap<String, Value>>,
}

impl ElicitationCapability {
    /// Create with both form and URL mode support
    #[must_use]
    pub fn full() -> Self {
        Self {
            form: Some(HashMap::new()),
            url: Some(HashMap::new()),
        }
    }

    /// Create with only form mode support
    #[must_use]
    pub fn form_only() -> Self {
        Self {
            form: Some(HashMap::new()),
            url: None,
        }
    }
}

/// Tasks capability (MCP 2025-11-25)
///
/// Enables durable state machines for long-running operations.
/// Can be declared by both clients and servers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TasksCapability {
    /// List tasks support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<HashMap<String, Value>>,
    /// Cancel tasks support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel: Option<HashMap<String, Value>>,
    /// Supported request types for task augmentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<TaskRequestsCapability>,
}

/// Task requests capability - which request types support task augmentation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskRequestsCapability {
    /// Tools task support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<TaskToolsCapability>,
    /// Sampling task support (client capability)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<TaskSamplingCapability>,
    /// Elicitation task support (client capability)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<TaskElicitationCapability>,
}

/// Task support for tools/call
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskToolsCapability {
    /// Support for tools/call task augmentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<HashMap<String, Value>>,
}

/// Task support for sampling/createMessage
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskSamplingCapability {
    /// Support for sampling/createMessage task augmentation
    #[serde(rename = "createMessage", skip_serializing_if = "Option::is_none")]
    pub create_message: Option<HashMap<String, Value>>,
}

/// Task support for elicitation/create
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskElicitationCapability {
    /// Support for elicitation/create task augmentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create: Option<HashMap<String, Value>>,
}

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
    fn test_server_capabilities_with_tasks() {
        let caps = ServerCapabilities::new()
            .with_tools(true)
            .with_tasks(TasksCapability::default());

        assert!(caps.tools.is_some());
        assert!(caps.tasks.is_some());
    }

    #[test]
    fn test_client_capabilities() {
        let caps = ClientCapabilities::new().with_sampling().with_roots(true);

        assert!(caps.sampling.is_some());
        assert!(caps.roots.is_some());
    }

    #[test]
    fn test_client_capabilities_with_full_elicitation() {
        let caps = ClientCapabilities::new().with_full_elicitation();

        assert!(caps.elicitation.is_some());
        let elicit = caps.elicitation.unwrap();
        assert!(elicit.form.is_some());
        assert!(elicit.url.is_some());
    }

    #[test]
    fn test_client_capabilities_with_tasks() {
        let caps = ClientCapabilities::new().with_tasks(TasksCapability::default());

        assert!(caps.tasks.is_some());
    }

    #[test]
    fn test_elicitation_capability_modes() {
        // Full support
        let full = ElicitationCapability::full();
        assert!(full.form.is_some());
        assert!(full.url.is_some());

        // Form only
        let form = ElicitationCapability::form_only();
        assert!(form.form.is_some());
        assert!(form.url.is_none());
    }

    #[test]
    fn test_tasks_capability_serde() {
        let tasks = TasksCapability {
            list: Some(HashMap::new()),
            cancel: Some(HashMap::new()),
            requests: Some(TaskRequestsCapability {
                tools: Some(TaskToolsCapability {
                    call: Some(HashMap::new()),
                }),
                sampling: None,
                elicitation: None,
            }),
        };

        let json = serde_json::to_string(&tasks).unwrap();
        assert!(json.contains("\"list\""));
        assert!(json.contains("\"cancel\""));
        assert!(json.contains("\"requests\""));
        assert!(json.contains("\"tools\""));
        assert!(json.contains("\"call\""));
    }
}
