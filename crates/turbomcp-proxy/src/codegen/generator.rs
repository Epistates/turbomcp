//! Rust code generator implementation
//!
//! This module provides the main `RustCodeGenerator` that converts a `ServerSpec`
//! into a complete Rust project with Cargo.toml and source files.

use std::collections::HashSet;

use chrono::Utc;
use convert_case::{Case, Casing};

use crate::error::ProxyResult;
use crate::introspection::ServerSpec;

use super::context::{
    CargoContext, MainContext, PromptDefinition, PromptEnumVariant, ProxyContext,
    ResourceDefinition, ResourceEnumVariant, ToolDefinition, ToolEnumVariant, TypesContext,
};
use super::sanitize::{doc_line, sanitize_string_literal, sanitize_uri, unique_identifier};
use super::template_engine::TemplateEngine;
use super::type_generator::TypeGenerator;

/// Configuration for code generation
#[derive(Debug, Clone)]
pub struct GenConfig {
    /// Package name (defaults to server name in kebab-case)
    pub package_name: Option<String>,

    /// Package version (defaults to 0.1.0)
    pub version: Option<String>,

    /// Frontend transport type
    pub frontend_type: FrontendType,

    /// Backend transport type
    pub backend_type: BackendType,

    /// `TurboMCP` version to use
    pub turbomcp_version: String,
}

impl Default for GenConfig {
    fn default() -> Self {
        Self {
            package_name: None,
            version: None,
            frontend_type: FrontendType::Http,
            backend_type: BackendType::Stdio,
            // Pin to the proxy crate's own version so generated projects compile
            // against the same TurboMCP that produced them.
            turbomcp_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Frontend transport type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontendType {
    /// HTTP transport
    Http,
    /// STDIO transport
    Stdio,
    /// WebSocket transport
    WebSocket,
}

impl std::fmt::Display for FrontendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrontendType::Http => write!(f, "HTTP"),
            FrontendType::Stdio => write!(f, "STDIO"),
            FrontendType::WebSocket => write!(f, "WebSocket"),
        }
    }
}

/// Backend transport type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    /// STDIO transport
    Stdio,
    /// HTTP transport
    Http,
    /// WebSocket transport
    WebSocket,
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendType::Stdio => write!(f, "STDIO"),
            BackendType::Http => write!(f, "HTTP"),
            BackendType::WebSocket => write!(f, "WebSocket"),
        }
    }
}

/// Generated Rust project
#[derive(Debug, Clone)]
pub struct GeneratedProject {
    /// main.rs content
    pub main_rs: String,

    /// proxy.rs content
    pub proxy_rs: String,

    /// types.rs content
    pub types_rs: String,

    /// Cargo.toml content
    pub cargo_toml: String,

    /// Package name
    pub package_name: String,
}

/// Rust code generator
///
/// Converts a `ServerSpec` into a complete Rust project with type-safe code.
pub struct RustCodeGenerator {
    /// Template engine
    template_engine: TemplateEngine,

    /// Server specification
    spec: ServerSpec,

    /// Type generator for JSON Schema conversion
    type_generator: TypeGenerator,
}

impl RustCodeGenerator {
    /// Create a new Rust code generator
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the template engine fails to initialize.
    pub fn new(spec: ServerSpec) -> ProxyResult<Self> {
        let template_engine = TemplateEngine::new()?;
        let type_generator = TypeGenerator::new();

        Ok(Self {
            template_engine,
            spec,
            type_generator,
        })
    }

    /// Generate a complete Rust project
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if code generation or template rendering fails.
    pub fn generate(mut self, config: &GenConfig) -> ProxyResult<GeneratedProject> {
        tracing::info!("Generating Rust project for {}", self.spec.server_info.name);

        // Build contexts (types_context first to populate type_generator)
        let types_context = self.build_types_context();
        let main_context = self.build_main_context(config);
        let proxy_context = self.build_proxy_context(config);
        let cargo_context = self.build_cargo_context(config);

        // Render templates
        let main_rs = self.template_engine.render_main(&main_context)?;
        let proxy_rs = self.template_engine.render_proxy(&proxy_context)?;
        let types_rs = self.template_engine.render_types(&types_context)?;
        let cargo_toml = self.template_engine.render_cargo_toml(&cargo_context)?;

        Ok(GeneratedProject {
            main_rs,
            proxy_rs,
            types_rs,
            cargo_toml,
            package_name: cargo_context.package_name,
        })
    }

    /// Build main.rs context
    fn build_main_context(&self, config: &GenConfig) -> MainContext {
        MainContext {
            server_name: doc_line(&self.spec.server_info.name),
            server_version: doc_line(&self.spec.server_info.version),
            generation_date: Utc::now().to_rfc3339(),
            frontend_type: config.frontend_type.to_string(),
            backend_type: config.backend_type.to_string(),
            has_http: config.frontend_type == FrontendType::Http,
            has_stdio: config.backend_type == BackendType::Stdio,
            has_websocket: config.frontend_type == FrontendType::WebSocket,
        }
    }

    /// Build proxy.rs context
    ///
    /// Each tool and prompt keeps two names. `name` is the upstream's, and is
    /// what the generated proxy lists and sends upstream; `ident` is only the
    /// Rust handler's. The generator used to route on a `snake_case` copy of
    /// the name, so every tool whose name wasn't already `snake_case`
    /// (`get-user`, `getUser`) reached the upstream under a name it didn't
    /// have, and tools whose names didn't survive the conversion were dropped.
    fn build_proxy_context(&self, config: &GenConfig) -> ProxyContext {
        let mut tool_idents = HashSet::new();
        let tools = self
            .spec
            .tools
            .iter()
            .map(|tool| {
                let ident = unique_identifier(&tool.name, Case::Snake, &mut tool_idents);
                ToolDefinition {
                    name: sanitize_string_literal(&tool.name),
                    input_type: Some(format!("{}Input", ident.to_case(Case::Pascal))),
                    output_type: tool
                        .output_schema
                        .as_ref()
                        .map(|_| format!("{}Output", ident.to_case(Case::Pascal))),
                    ident,
                    description: tool.description.as_deref().map(doc_line),
                }
            })
            .collect();

        let resources = self
            .spec
            .resources
            .iter()
            .filter_map(|resource| {
                let uri = match sanitize_uri(&resource.uri) {
                    Ok(uri) => uri,
                    Err(e) => {
                        tracing::warn!("Skipping resource '{}': {}", resource.uri, e);
                        return None;
                    }
                };
                Some(ResourceDefinition {
                    name: sanitize_string_literal(&resource.name),
                    uri,
                    description: resource.description.as_deref().map(doc_line),
                    mime_type: resource.mime_type.as_deref().map(sanitize_string_literal),
                })
            })
            .collect();

        let mut prompt_idents = HashSet::new();
        let prompts = self
            .spec
            .prompts
            .iter()
            .map(|prompt| PromptDefinition {
                name: sanitize_string_literal(&prompt.name),
                ident: unique_identifier(&prompt.name, Case::Snake, &mut prompt_idents),
                description: prompt.description.as_deref().map(doc_line),
                arguments: None,
            })
            .collect();

        ProxyContext {
            server_name: doc_line(&self.spec.server_info.name),
            frontend_type: config.frontend_type.to_string(),
            backend_type: config.backend_type.to_string(),
            tools,
            resources,
            prompts,
        }
    }

    /// Build types.rs context
    fn build_types_context(&mut self) -> TypesContext {
        let mut type_definitions = Vec::new();
        let mut tool_idents = HashSet::new();
        let mut tool_enums = Vec::new();

        for tool in &self.spec.tools {
            let ident = unique_identifier(&tool.name, Case::Pascal, &mut tool_idents);

            let input_schema = serde_json::to_value(&tool.input_schema)
                .unwrap_or_else(|_| serde_json::json!({"type": "object", "properties": {}}));
            if let Ok(type_def) = self.type_generator.generate_type_from_schema(
                &format!("{ident}Input"),
                &input_schema,
                tool.description.as_deref().map(doc_line),
            ) {
                type_definitions.push(type_def);
            }

            if let Some(output_schema) = &tool.output_schema
                && let Ok(output_schema) = serde_json::to_value(output_schema)
                && let Ok(type_def) = self.type_generator.generate_type_from_schema(
                    &format!("{ident}Output"),
                    &output_schema,
                    None,
                )
            {
                type_definitions.push(type_def);
            }

            tool_enums.push(ToolEnumVariant {
                name: sanitize_string_literal(&tool.name),
                ident,
            });
        }

        let mut resource_idents = HashSet::new();
        let resource_enums = self
            .spec
            .resources
            .iter()
            .filter_map(|resource| {
                let uri = match sanitize_uri(&resource.uri) {
                    Ok(uri) => uri,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping enum variant for resource '{}': {}",
                            resource.uri,
                            e
                        );
                        return None;
                    }
                };
                Some(ResourceEnumVariant {
                    name: unique_identifier(&resource.name, Case::Pascal, &mut resource_idents),
                    uri,
                })
            })
            .collect();

        let mut prompt_idents = HashSet::new();
        let prompt_enums = self
            .spec
            .prompts
            .iter()
            .map(|prompt| PromptEnumVariant {
                name: sanitize_string_literal(&prompt.name),
                ident: unique_identifier(&prompt.name, Case::Pascal, &mut prompt_idents),
            })
            .collect();

        TypesContext {
            server_name: doc_line(&self.spec.server_info.name),
            type_definitions,
            tool_enums,
            resource_enums,
            prompt_enums,
        }
    }

    /// Build Cargo.toml context
    fn build_cargo_context(&self, config: &GenConfig) -> CargoContext {
        let package_name = config.package_name.clone().unwrap_or_else(|| {
            let mut taken = HashSet::new();
            unique_identifier(&self.spec.server_info.name, Case::Kebab, &mut taken)
                .replace('_', "-")
                .trim_matches('-')
                .to_string()
        });

        let version = config
            .version
            .clone()
            .unwrap_or_else(|| "0.1.0".to_string());

        // turbomcp-server features for the frontend; stdio is its default.
        let transport_features = match config.frontend_type {
            FrontendType::Http => vec!["http".to_string()],
            FrontendType::WebSocket => vec!["websocket".to_string()],
            FrontendType::Stdio => vec!["stdio".to_string()],
        };

        CargoContext {
            package_name,
            version,
            server_name: sanitize_string_literal(&self.spec.server_info.name),
            turbomcp_version: config.turbomcp_version.clone(),
            frontend_type: config.frontend_type.to_string(),
            transport_features,
            additional_dependencies: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_protocol::types::{
        Implementation, Prompt, PromptsCapabilities, Resource, ResourcesCapabilities,
        ServerCapabilities, Tool, ToolInputSchema, ToolsCapabilities,
    };

    fn create_test_spec() -> ServerSpec {
        ServerSpec {
            server_info: Implementation {
                title: Some("Test Server".to_string()),
                ..Implementation::new("test-server", "1.0.0")
            },
            protocol_version: "2025-11-25".to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapabilities::default()),
                resources: Some(ResourcesCapabilities::default()),
                prompts: Some(PromptsCapabilities::default()),
                ..Default::default()
            },
            tools: vec![Tool {
                title: Some("Search".to_string()),
                ..Tool::new("search", "Search for items").with_schema(ToolInputSchema::from_value(
                    serde_json::json!({
                        "type": "object",
                        "properties": { "query": { "type": "string" } }
                    }),
                ))
            }],
            resources: vec![Resource {
                title: Some("Test Resource".to_string()),
                ..Resource::new("file:///test/path", "test-resource")
                    .with_description("Test resource")
                    .with_mime_type("text/plain")
            }],
            resource_templates: vec![],
            prompts: vec![Prompt {
                name: "test-prompt".to_string(),
                title: Some("Test Prompt".to_string()),
                description: Some("Test prompt".to_string()),
                ..Default::default()
            }],
            instructions: None,
        }
    }

    #[test]
    fn test_rust_code_generator_creation() {
        let spec = create_test_spec();
        let generator = RustCodeGenerator::new(spec);
        assert!(
            generator.is_ok(),
            "Generator should be created successfully"
        );
    }

    #[test]
    fn test_generate_project() {
        let spec = create_test_spec();
        let generator = RustCodeGenerator::new(spec).unwrap();

        let config = GenConfig::default();
        let project = generator.generate(&config);

        assert!(project.is_ok(), "Should generate project successfully");

        let project = project.unwrap();
        assert!(!project.main_rs.is_empty(), "main.rs should not be empty");
        assert!(!project.proxy_rs.is_empty(), "proxy.rs should not be empty");
        assert!(!project.types_rs.is_empty(), "types.rs should not be empty");
        assert!(
            !project.cargo_toml.is_empty(),
            "Cargo.toml should not be empty"
        );

        // Verify content
        assert!(
            project.main_rs.contains("test-server"),
            "main.rs should contain server name"
        );
        assert!(
            project.cargo_toml.contains("test-server"),
            "Cargo.toml should contain server name"
        );
    }

    #[test]
    fn test_build_contexts() {
        let spec = create_test_spec();
        let mut generator = RustCodeGenerator::new(spec).unwrap();
        let config = GenConfig::default();

        let main_ctx = generator.build_main_context(&config);
        assert_eq!(main_ctx.server_name, "test-server");
        assert_eq!(main_ctx.frontend_type, "HTTP");
        assert_eq!(main_ctx.backend_type, "STDIO");

        let proxy_ctx = generator.build_proxy_context(&config);
        assert_eq!(proxy_ctx.tools.len(), 1);
        assert_eq!(proxy_ctx.resources.len(), 1);
        assert_eq!(proxy_ctx.prompts.len(), 1);

        let types_ctx = generator.build_types_context();
        assert_eq!(types_ctx.tool_enums.len(), 1);
        assert_eq!(types_ctx.resource_enums.len(), 1);
        assert_eq!(types_ctx.prompt_enums.len(), 1);

        // Check that types were generated
        assert!(
            !types_ctx.type_definitions.is_empty(),
            "Should generate at least input type"
        );

        let cargo_ctx = generator.build_cargo_context(&config);
        assert_eq!(cargo_ctx.package_name, "test-server");
        assert_eq!(cargo_ctx.transport_features, ["http"]);
    }

    /// PX-V11: the upstream's tool name is what the proxy lists and sends.
    /// Routing used a `snake_case` copy, so `get-user` went upstream as
    /// `get_user` and failed, and names that didn't convert to an identifier
    /// dropped the tool. Colliding identifiers are numbered instead.
    #[test]
    fn tools_keep_their_upstream_names() {
        let mut spec = create_test_spec();
        spec.tools = [
            "get-user",
            "get_user",
            "getUser",
            "type",
            "2fa",
            "say \"hi\"",
        ]
        .into_iter()
        .map(|name| Tool::new(name, "a tool"))
        .collect();
        let generator = RustCodeGenerator::new(spec).unwrap();
        let context = generator.build_proxy_context(&GenConfig::default());

        let names: Vec<&str> = context.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "get-user",
                "get_user",
                "getUser",
                "type",
                "2fa",
                "say \\\"hi\\\""
            ]
        );
        let idents: Vec<&str> = context.tools.iter().map(|t| t.ident.as_str()).collect();
        assert_eq!(
            idents,
            [
                "get_user",
                "get_user_2",
                "get_user_3",
                "type_",
                "_2_fa",
                "say_hi"
            ]
        );

        let project = generator.generate(&GenConfig::default()).unwrap();
        assert!(
            project
                .proxy_rs
                .contains("\"get-user\" => self.call_get_user(arguments)")
        );
        assert!(
            project
                .proxy_rs
                .contains("call_tool(\"get-user\", arguments, None)")
        );
    }
}
