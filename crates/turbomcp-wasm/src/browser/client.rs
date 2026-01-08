//! Browser MCP client implementation

use super::transport::FetchTransport;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, to_value};
use turbomcp_core::types::{
    capabilities::{ClientCapabilities, ServerCapabilities},
    content::ResourceContent,
    core::Implementation,
    initialization::InitializeResult,
    prompts::{GetPromptResult, Prompt},
    resources::{Resource, ResourceTemplate},
    tools::{CallToolResult, Tool},
};
use wasm_bindgen::prelude::*;

/// MCP Client for browser environments
#[wasm_bindgen]
pub struct McpClient {
    transport: FetchTransport,
    initialized: bool,
    server_info: Option<Implementation>,
    server_capabilities: Option<ServerCapabilities>,
    protocol_version: String,
}

#[wasm_bindgen]
impl McpClient {
    /// Create a new MCP client
    #[wasm_bindgen(constructor)]
    pub fn new(base_url: &str) -> Self {
        Self {
            transport: FetchTransport::new(base_url),
            initialized: false,
            server_info: None,
            server_capabilities: None,
            protocol_version: "2025-11-25".to_string(),
        }
    }

    /// Add an authorization header
    #[wasm_bindgen(js_name = "withAuth")]
    pub fn with_auth(self, token: &str) -> Self {
        Self {
            transport: self
                .transport
                .with_header("Authorization", format!("Bearer {token}")),
            ..self
        }
    }

    /// Add a custom header
    #[wasm_bindgen(js_name = "withHeader")]
    pub fn with_header(self, key: &str, value: &str) -> Self {
        Self {
            transport: self.transport.with_header(key, value),
            ..self
        }
    }

    /// Set request timeout in milliseconds
    #[wasm_bindgen(js_name = "withTimeout")]
    pub fn with_timeout(self, timeout_ms: u32) -> Self {
        Self {
            transport: self.transport.with_timeout(timeout_ms),
            ..self
        }
    }

    /// Initialize the MCP session
    #[wasm_bindgen]
    pub async fn initialize(&mut self) -> Result<JsValue, JsValue> {
        let params = InitializeParams {
            protocol_version: self.protocol_version.clone(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "turbomcp-wasm".to_string(),
                title: Some("TurboMCP WASM Client".to_string()),
                description: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                icon: None,
            },
        };

        let result: InitializeResult = self
            .transport
            .request("initialize", Some(params))
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        self.initialized = true;
        self.server_info = Some(result.server_info.clone());
        self.server_capabilities = Some(result.capabilities.clone());
        self.protocol_version = result.protocol_version.clone();

        // Send initialized notification
        let _: serde_json::Value = self
            .transport
            .request("notifications/initialized", None::<()>)
            .await
            .unwrap_or_default();

        to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Check if initialized
    #[wasm_bindgen(js_name = "isInitialized")]
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get server info (after initialization)
    #[wasm_bindgen(js_name = "getServerInfo")]
    pub fn get_server_info(&self) -> Result<JsValue, JsValue> {
        match &self.server_info {
            Some(info) => to_value(info).map_err(|e| JsValue::from_str(&e.to_string())),
            None => Ok(JsValue::NULL),
        }
    }

    /// Get server capabilities (after initialization)
    #[wasm_bindgen(js_name = "getServerCapabilities")]
    pub fn get_server_capabilities(&self) -> Result<JsValue, JsValue> {
        match &self.server_capabilities {
            Some(caps) => to_value(caps).map_err(|e| JsValue::from_str(&e.to_string())),
            None => Ok(JsValue::NULL),
        }
    }

    /// List available tools
    #[wasm_bindgen(js_name = "listTools")]
    pub async fn list_tools(&self) -> Result<JsValue, JsValue> {
        self.ensure_initialized()?;

        let result: ListToolsResult = self
            .transport
            .request("tools/list", None::<()>)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        to_value(&result.tools).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Call a tool
    #[wasm_bindgen(js_name = "callTool")]
    pub async fn call_tool(&self, name: &str, arguments: JsValue) -> Result<JsValue, JsValue> {
        self.ensure_initialized()?;

        let args: Option<serde_json::Value> = if arguments.is_undefined() || arguments.is_null() {
            None
        } else {
            Some(from_value(arguments).map_err(|e| JsValue::from_str(&e.to_string()))?)
        };

        let params = CallToolParams {
            name: name.to_string(),
            arguments: args,
        };

        let result: CallToolResult = self
            .transport
            .request("tools/call", Some(params))
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// List available resources
    #[wasm_bindgen(js_name = "listResources")]
    pub async fn list_resources(&self) -> Result<JsValue, JsValue> {
        self.ensure_initialized()?;

        let result: ListResourcesResult = self
            .transport
            .request("resources/list", None::<()>)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        to_value(&result.resources).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Read a resource
    #[wasm_bindgen(js_name = "readResource")]
    pub async fn read_resource(&self, uri: &str) -> Result<JsValue, JsValue> {
        self.ensure_initialized()?;

        let params = ReadResourceParams {
            uri: uri.to_string(),
        };

        let result: ReadResourceResult = self
            .transport
            .request("resources/read", Some(params))
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        to_value(&result.contents).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// List resource templates
    #[wasm_bindgen(js_name = "listResourceTemplates")]
    pub async fn list_resource_templates(&self) -> Result<JsValue, JsValue> {
        self.ensure_initialized()?;

        let result: ListResourceTemplatesResult = self
            .transport
            .request("resources/templates/list", None::<()>)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        to_value(&result.resource_templates).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// List available prompts
    #[wasm_bindgen(js_name = "listPrompts")]
    pub async fn list_prompts(&self) -> Result<JsValue, JsValue> {
        self.ensure_initialized()?;

        let result: ListPromptsResult = self
            .transport
            .request("prompts/list", None::<()>)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        to_value(&result.prompts).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get a prompt
    #[wasm_bindgen(js_name = "getPrompt")]
    pub async fn get_prompt(&self, name: &str, arguments: JsValue) -> Result<JsValue, JsValue> {
        self.ensure_initialized()?;

        let args: Option<serde_json::Value> = if arguments.is_undefined() || arguments.is_null() {
            None
        } else {
            Some(from_value(arguments).map_err(|e| JsValue::from_str(&e.to_string()))?)
        };

        let params = GetPromptParams {
            name: name.to_string(),
            arguments: args,
        };

        let result: GetPromptResult = self
            .transport
            .request("prompts/get", Some(params))
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Ping the server
    #[wasm_bindgen]
    pub async fn ping(&self) -> Result<(), JsValue> {
        let _: serde_json::Value = self
            .transport
            .request("ping", None::<()>)
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        Ok(())
    }

    // Private helpers

    fn ensure_initialized(&self) -> Result<(), JsValue> {
        if !self.initialized {
            Err(JsValue::from_str(
                "Client not initialized. Call initialize() first.",
            ))
        } else {
            Ok(())
        }
    }
}

// Request/Response types

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    protocol_version: String,
    capabilities: ClientCapabilities,
    client_info: Implementation,
}

#[derive(Serialize)]
struct CallToolParams {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct ReadResourceParams {
    uri: String,
}

#[derive(Serialize)]
struct GetPromptParams {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ListToolsResult {
    tools: Vec<Tool>,
}

#[derive(Deserialize)]
struct ListResourcesResult {
    resources: Vec<Resource>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResourceTemplatesResult {
    resource_templates: Vec<ResourceTemplate>,
}

#[derive(Deserialize)]
struct ReadResourceResult {
    contents: Vec<ResourceContent>,
}

#[derive(Deserialize)]
struct ListPromptsResult {
    prompts: Vec<Prompt>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_builder() {
        let client = McpClient::new("https://api.example.com")
            .with_auth("token123")
            .with_header("X-Custom", "value")
            .with_timeout(60_000);

        assert!(!client.is_initialized());
    }
}
