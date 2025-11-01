//! Context data structures for code generation
//!
//! This module defines the context structures that are passed to Handlebars templates
//! to generate proxy code. These structures bridge the gap between MCP `ServerSpec`
//! and template rendering.

use serde::{Deserialize, Serialize};

/// Main application context for main.rs template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainContext {
    /// Server name
    pub server_name: String,

    /// Server version
    pub server_version: String,

    /// Generation timestamp
    pub generation_date: String,

    /// Frontend transport type
    pub frontend_type: String,

    /// Backend transport type
    pub backend_type: String,

    /// Whether HTTP frontend is enabled
    pub has_http: bool,

    /// Whether STDIO backend is enabled
    pub has_stdio: bool,
}

/// Proxy implementation context for proxy.rs template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyContext {
    /// Server name
    pub server_name: String,

    /// Frontend type description
    pub frontend_type: String,

    /// Backend type description
    pub backend_type: String,

    /// List of tools
    pub tools: Vec<ToolDefinition>,

    /// List of resources
    pub resources: Vec<ResourceDefinition>,

    /// List of prompts
    pub prompts: Vec<PromptDefinition>,
}

/// Tool definition for code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name
    pub name: String,

    /// Tool description
    pub description: Option<String>,

    /// Input schema type name (if generated)
    pub input_type: Option<String>,

    /// Output schema type name (if generated)
    pub output_type: Option<String>,
}

/// Resource definition for code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDefinition {
    /// Resource name (derived from URI)
    pub name: String,

    /// Resource URI
    pub uri: String,

    /// Resource description
    pub description: Option<String>,

    /// MIME type
    pub mime_type: Option<String>,
}

/// Prompt definition for code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptDefinition {
    /// Prompt name
    pub name: String,

    /// Prompt description
    pub description: Option<String>,

    /// Arguments schema (if any)
    pub arguments: Option<String>,
}

/// Types context for types.rs template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypesContext {
    /// Server name
    pub server_name: String,

    /// Custom type definitions
    pub type_definitions: Vec<TypeDefinition>,

    /// Tool enum variants
    pub tool_enums: Vec<ToolEnumVariant>,

    /// Resource enum variants
    pub resource_enums: Vec<ResourceEnumVariant>,

    /// Prompt enum variants
    pub prompt_enums: Vec<PromptEnumVariant>,
}

/// A custom type definition (struct)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDefinition {
    /// Type name (`PascalCase`)
    pub name: String,

    /// Type description
    pub description: Option<String>,

    /// Serde rename attribute (if needed)
    pub rename: Option<String>,

    /// Struct fields
    pub fields: Vec<FieldDefinition>,
}

/// A field in a struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// Field name (`snake_case`)
    pub name: String,

    /// Rust type
    pub rust_type: String,

    /// Whether field is optional
    pub optional: bool,

    /// Field description
    pub description: Option<String>,
}

/// Tool enum variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEnumVariant {
    /// Tool name (original)
    pub name: String,

    /// Parameters
    pub params: Vec<ParamDefinition>,
}

/// Parameter in an enum variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDefinition {
    /// Parameter name
    pub name: String,

    /// Rust type
    pub rust_type: String,

    /// Whether parameter is optional
    pub optional: bool,
}

/// Resource enum variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceEnumVariant {
    /// Resource name
    pub name: String,

    /// Resource URI
    pub uri: String,
}

/// Prompt enum variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptEnumVariant {
    /// Prompt name
    pub name: String,
}

/// Cargo.toml context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoContext {
    /// Package name (kebab-case)
    pub package_name: String,

    /// Package version
    pub version: String,

    /// Server name for description
    pub server_name: String,

    /// `TurboMCP` version
    pub turbomcp_version: String,

    /// Frontend transport type (for conditional dependencies)
    pub frontend_type: String,

    /// Transport features to enable
    pub transport_features: Vec<String>,

    /// Additional dependencies
    pub additional_dependencies: Vec<Dependency>,
}

/// A Cargo dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    /// Crate name
    pub name: String,

    /// Version requirement
    pub version: Option<String>,

    /// Full dependency spec (if complex)
    pub spec: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_main_context_serialization() {
        let context = MainContext {
            server_name: "test-server".to_string(),
            server_version: "1.0.0".to_string(),
            generation_date: "2025-01-01".to_string(),
            frontend_type: "HTTP".to_string(),
            backend_type: "STDIO".to_string(),
            has_http: true,
            has_stdio: true,
        };

        let json = serde_json::to_string(&context);
        assert!(json.is_ok(), "MainContext should serialize to JSON");
    }

    #[test]
    fn test_tool_definition() {
        let tool = ToolDefinition {
            name: "search".to_string(),
            description: Some("Search for items".to_string()),
            input_type: Some("SearchInput".to_string()),
            output_type: Some("SearchOutput".to_string()),
        };

        assert_eq!(tool.name, "search");
        assert!(tool.description.is_some());
    }

    #[test]
    fn test_type_definition() {
        let type_def = TypeDefinition {
            name: "SearchInput".to_string(),
            description: Some("Search input parameters".to_string()),
            rename: None,
            fields: vec![
                FieldDefinition {
                    name: "query".to_string(),
                    rust_type: "String".to_string(),
                    optional: false,
                    description: Some("Search query".to_string()),
                },
                FieldDefinition {
                    name: "limit".to_string(),
                    rust_type: "i64".to_string(),
                    optional: true,
                    description: Some("Result limit".to_string()),
                },
            ],
        };

        assert_eq!(type_def.fields.len(), 2);
        assert!(!type_def.fields[0].optional);
        assert!(type_def.fields[1].optional);
    }

    #[test]
    fn test_cargo_context() {
        let context = CargoContext {
            package_name: "test-proxy".to_string(),
            version: "0.1.0".to_string(),
            server_name: "test-server".to_string(),
            turbomcp_version: "2.1.1".to_string(),
            frontend_type: "HTTP".to_string(),
            transport_features: vec!["http".to_string(), "stdio".to_string()],
            additional_dependencies: vec![],
        };

        assert_eq!(context.package_name, "test-proxy");
        assert_eq!(context.transport_features.len(), 2);
    }
}
