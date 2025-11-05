//! Rust code generator implementation
//!
//! This module provides the main `RustCodeGenerator` that converts a `ServerSpec`
//! into a complete Rust project with Cargo.toml and source files.

use chrono::Utc;
use convert_case::{Case, Casing};

use crate::error::ProxyResult;
use crate::introspection::ServerSpec;

use super::context::{
    CargoContext, MainContext, PromptDefinition, PromptEnumVariant, ProxyContext,
    ResourceDefinition, ResourceEnumVariant, ToolDefinition, ToolEnumVariant, TypesContext,
};
use super::sanitize::{sanitize_identifier, sanitize_string_literal, sanitize_uri};
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
            turbomcp_version: "2.1.1".to_string(),
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
            server_name: self.spec.server_info.name.clone(),
            server_version: self.spec.server_info.version.clone(),
            generation_date: Utc::now().to_rfc3339(),
            frontend_type: config.frontend_type.to_string(),
            backend_type: config.backend_type.to_string(),
            has_http: config.frontend_type == FrontendType::Http,
            has_stdio: config.backend_type == BackendType::Stdio,
        }
    }

    /// Build proxy.rs context
    #[allow(clippy::too_many_lines)]
    fn build_proxy_context(&mut self, config: &GenConfig) -> ProxyContext {
        let tools = self
            .spec
            .tools
            .iter()
            .filter_map(|tool| {
                // Convert to snake_case first (handles dashes, spaces, etc.)
                let snake_case_name = tool.name.to_case(Case::Snake);

                // Sanitize tool name - skip tools with invalid names
                let sanitized_name = match sanitize_identifier(&snake_case_name) {
                    Ok(name) => name,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping tool '{}': Invalid converted name '{}': {}",
                            tool.name,
                            snake_case_name,
                            e
                        );
                        return None;
                    }
                };

                // Generate type names for input/output
                let input_type_name = format!("{}Input", sanitized_name.to_case(Case::Pascal));
                let output_type_name = format!("{}Output", sanitized_name.to_case(Case::Pascal));

                // Sanitize description
                let description = tool
                    .description
                    .as_ref()
                    .map(|d| sanitize_string_literal(d));

                Some(ToolDefinition {
                    name: sanitized_name,
                    description,
                    input_type: Some(input_type_name),
                    output_type: tool.output_schema.as_ref().map(|_| output_type_name),
                })
            })
            .collect();

        let resources = self
            .spec
            .resources
            .iter()
            .filter_map(|resource| {
                // Sanitize URI first
                let sanitized_uri = match sanitize_uri(&resource.uri) {
                    Ok(uri) => uri,
                    Err(e) => {
                        tracing::warn!("Skipping resource '{}': {}", resource.uri, e);
                        return None;
                    }
                };

                // Derive name from URI (last segment)
                let derived_name = resource
                    .uri
                    .split('/')
                    .next_back()
                    .unwrap_or(&resource.uri)
                    .to_case(Case::Snake);

                // Sanitize derived name
                let sanitized_name = match sanitize_identifier(&derived_name) {
                    Ok(name) => name,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping resource '{}': Invalid derived name '{}': {}",
                            resource.uri,
                            derived_name,
                            e
                        );
                        return None;
                    }
                };

                // Sanitize description and MIME type
                let description = resource
                    .description
                    .as_ref()
                    .map(|d| sanitize_string_literal(d));
                let mime_type = resource
                    .mime_type
                    .as_ref()
                    .map(|m| sanitize_string_literal(m));

                Some(ResourceDefinition {
                    name: sanitized_name,
                    uri: sanitized_uri,
                    description,
                    mime_type,
                })
            })
            .collect();

        let prompts = self
            .spec
            .prompts
            .iter()
            .filter_map(|prompt| {
                // Convert to snake_case first (handles dashes, spaces, etc.)
                let snake_case_name = prompt.name.to_case(Case::Snake);

                // Sanitize prompt name
                let sanitized_name = match sanitize_identifier(&snake_case_name) {
                    Ok(name) => name,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping prompt '{}': Invalid converted name '{}': {}",
                            prompt.name,
                            snake_case_name,
                            e
                        );
                        return None;
                    }
                };

                // Sanitize description
                let description = prompt
                    .description
                    .as_ref()
                    .map(|d| sanitize_string_literal(d));

                Some(PromptDefinition {
                    name: sanitized_name,
                    description,
                    arguments: None, // TODO: Extract from arguments schema
                })
            })
            .collect();

        ProxyContext {
            server_name: self.spec.server_info.name.clone(),
            frontend_type: config.frontend_type.to_string(),
            backend_type: config.backend_type.to_string(),
            tools,
            resources,
            prompts,
        }
    }

    /// Build types.rs context
    #[allow(clippy::too_many_lines)]
    fn build_types_context(&mut self) -> TypesContext {
        // Generate type definitions from tool schemas
        let mut type_definitions = Vec::new();

        for tool in &self.spec.tools {
            // Convert to snake_case first (handles dashes, spaces, etc.)
            let snake_case_name = tool.name.to_case(Case::Snake);

            // Sanitize tool name - skip tools with invalid names
            let sanitized_name = match sanitize_identifier(&snake_case_name) {
                Ok(name) => name,
                Err(e) => {
                    tracing::warn!(
                        "Skipping type generation for tool '{}': Invalid converted name '{}': {}",
                        tool.name,
                        snake_case_name,
                        e
                    );
                    continue;
                }
            };

            // Generate input type
            let input_type_name = format!("{}Input", sanitized_name.to_case(Case::Pascal));

            // Convert input_schema to serde_json::Value for type generation
            let input_schema_value = serde_json::to_value(&tool.input_schema)
                .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));

            // Sanitize description
            let sanitized_description = tool
                .description
                .as_ref()
                .map(|d| sanitize_string_literal(d));

            if let Ok(type_def) = self.type_generator.generate_type_from_schema(
                &input_type_name,
                &input_schema_value,
                sanitized_description,
            ) {
                type_definitions.push(type_def);
            }

            // Generate output type if schema exists
            if let Some(ref output_schema) = tool.output_schema {
                let output_type_name = format!("{}Output", sanitized_name.to_case(Case::Pascal));
                let output_schema_value = serde_json::to_value(output_schema)
                    .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));

                if let Ok(type_def) = self.type_generator.generate_type_from_schema(
                    &output_type_name,
                    &output_schema_value,
                    None,
                ) {
                    type_definitions.push(type_def);
                }
            }
        }

        // Build tool enum variants with actual parameters from schemas
        let tool_enums = self
            .spec
            .tools
            .iter()
            .filter_map(|tool| {
                // Convert to snake_case first (handles dashes, spaces, etc.)
                let snake_case_name = tool.name.to_case(Case::Snake);

                // Sanitize tool name - skip tools with invalid names
                let sanitized_name = match sanitize_identifier(&snake_case_name) {
                    Ok(name) => name,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping enum variant for tool '{}': Invalid converted name '{}': {}",
                            tool.name,
                            snake_case_name,
                            e
                        );
                        return None;
                    }
                };

                let input_schema_value = serde_json::to_value(&tool.input_schema)
                    .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));

                Some(ToolEnumVariant {
                    name: sanitized_name,
                    params: self
                        .type_generator
                        .generate_params_from_schema(&input_schema_value),
                })
            })
            .collect();

        // Build resource enum variants
        let resource_enums = self
            .spec
            .resources
            .iter()
            .filter_map(|resource| {
                // Sanitize URI first
                let sanitized_uri = match sanitize_uri(&resource.uri) {
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

                // Derive name from URI (last segment)
                let derived_name = resource
                    .uri
                    .split('/')
                    .next_back()
                    .unwrap_or(&resource.uri)
                    .to_case(Case::Snake);

                // Sanitize derived name
                let sanitized_name = match sanitize_identifier(&derived_name) {
                    Ok(name) => name,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping enum variant for resource '{}': Invalid derived name '{}': {}",
                            resource.uri,
                            derived_name,
                            e
                        );
                        return None;
                    }
                };

                Some(ResourceEnumVariant {
                    name: sanitized_name,
                    uri: sanitized_uri,
                })
            })
            .collect();

        // Build prompt enum variants
        let prompt_enums = self
            .spec
            .prompts
            .iter()
            .filter_map(|prompt| {
                // Convert to snake_case first (handles dashes, spaces, etc.)
                let snake_case_name = prompt.name.to_case(Case::Snake);

                // Sanitize prompt name
                let sanitized_name = match sanitize_identifier(&snake_case_name) {
                    Ok(name) => name,
                    Err(e) => {
                        tracing::warn!(
                            "Skipping enum variant for prompt '{}': Invalid converted name '{}': {}",
                            prompt.name,
                            snake_case_name,
                            e
                        );
                        return None;
                    }
                };

                Some(PromptEnumVariant {
                    name: sanitized_name,
                })
            })
            .collect();

        TypesContext {
            server_name: self.spec.server_info.name.clone(),
            type_definitions,
            tool_enums,
            resource_enums,
            prompt_enums,
        }
    }

    /// Build Cargo.toml context
    fn build_cargo_context(&self, config: &GenConfig) -> CargoContext {
        let package_name = config
            .package_name
            .clone()
            .unwrap_or_else(|| self.spec.server_info.name.to_case(Case::Kebab));

        let version = config
            .version
            .clone()
            .unwrap_or_else(|| "0.1.0".to_string());

        // Determine transport features needed
        let mut transport_features = Vec::new();
        if config.frontend_type == FrontendType::Http || config.backend_type == BackendType::Http {
            transport_features.push("http".to_string());
        }
        if config.backend_type == BackendType::Stdio {
            transport_features.push("stdio".to_string());
        }

        CargoContext {
            package_name,
            version,
            server_name: self.spec.server_info.name.clone(),
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
    use crate::introspection::{
        PromptSpec, PromptsCapability, ResourceSpec, ResourcesCapability, ServerCapabilities,
        ServerInfo, ToolInputSchema, ToolSpec, ToolsCapability,
    };
    use std::collections::HashMap;

    fn create_test_spec() -> ServerSpec {
        ServerSpec {
            server_info: ServerInfo {
                name: "test-server".to_string(),
                version: "1.0.0".to_string(),
                title: Some("Test Server".to_string()),
            },
            protocol_version: "2025-06-18".to_string(),
            capabilities: ServerCapabilities {
                logging: None,
                completions: None,
                tools: Some(ToolsCapability { list_changed: None }),
                resources: Some(ResourcesCapability {
                    subscribe: None,
                    list_changed: None,
                }),
                prompts: Some(PromptsCapability { list_changed: None }),
                experimental: None,
            },
            tools: vec![ToolSpec {
                name: "search".to_string(),
                title: Some("Search".to_string()),
                description: Some("Search for items".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some(HashMap::from([(
                        "query".to_string(),
                        serde_json::json!({"type": "string"}),
                    )])),
                    required: None,
                    additional: HashMap::new(),
                },
                output_schema: None,
                annotations: None,
            }],
            resources: vec![ResourceSpec {
                uri: "file:///test/path".to_string(),
                name: "test-resource".to_string(),
                title: Some("Test Resource".to_string()),
                description: Some("Test resource".to_string()),
                mime_type: Some("text/plain".to_string()),
                size: None,
                annotations: None,
            }],
            resource_templates: vec![],
            prompts: vec![PromptSpec {
                name: "test-prompt".to_string(),
                title: Some("Test Prompt".to_string()),
                description: Some("Test prompt".to_string()),
                arguments: vec![],
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
        assert!(cargo_ctx.transport_features.contains(&"http".to_string()));
        assert!(cargo_ctx.transport_features.contains(&"stdio".to_string()));
    }
}
