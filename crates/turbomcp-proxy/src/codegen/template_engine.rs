//! Template engine for code generation
//!
//! This module provides a Handlebars-based template engine that loads and renders
//! templates for generating Rust proxy code.

use convert_case::{Case, Casing};
use handlebars::Handlebars;

use crate::error::ProxyResult;

/// Template engine for rendering Rust code
///
/// Uses Handlebars templates embedded in the binary via include_str!.
/// Provides helpers for case conversion (snake_case, PascalCase, etc.)
pub struct TemplateEngine {
    handlebars: Handlebars<'static>,
}

impl TemplateEngine {
    /// Create a new template engine with all templates loaded
    pub fn new() -> ProxyResult<Self> {
        let mut hb = Handlebars::new();

        // Register templates (embedded in binary)
        hb.register_template_string("main", include_str!("templates/main.rs.hbs"))
            .map_err(|e| {
                crate::error::ProxyError::codegen(format!(
                    "Failed to register main.rs template: {}",
                    e
                ))
            })?;

        hb.register_template_string("proxy", include_str!("templates/proxy.rs.hbs"))
            .map_err(|e| {
                crate::error::ProxyError::codegen(format!(
                    "Failed to register proxy.rs template: {}",
                    e
                ))
            })?;

        hb.register_template_string("types", include_str!("templates/types.rs.hbs"))
            .map_err(|e| {
                crate::error::ProxyError::codegen(format!(
                    "Failed to register types.rs template: {}",
                    e
                ))
            })?;

        hb.register_template_string("cargo_toml", include_str!("templates/Cargo.toml.hbs"))
            .map_err(|e| {
                crate::error::ProxyError::codegen(format!(
                    "Failed to register Cargo.toml template: {}",
                    e
                ))
            })?;

        // Register helpers for case conversion
        hb.register_helper("snake_case", Box::new(snake_case_helper));
        hb.register_helper("pascal_case", Box::new(pascal_case_helper));
        hb.register_helper("camel_case", Box::new(camel_case_helper));
        hb.register_helper("kebab_case", Box::new(kebab_case_helper));

        // Register conditional helper
        hb.register_helper("eq", Box::new(eq_helper));

        Ok(Self { handlebars: hb })
    }

    /// Render the main.rs template
    pub fn render_main(&self, context: &impl serde::Serialize) -> ProxyResult<String> {
        self.handlebars.render("main", context).map_err(|e| {
            crate::error::ProxyError::codegen(format!("Failed to render main.rs: {}", e))
        })
    }

    /// Render the proxy.rs template
    pub fn render_proxy(&self, context: &impl serde::Serialize) -> ProxyResult<String> {
        self.handlebars.render("proxy", context).map_err(|e| {
            crate::error::ProxyError::codegen(format!("Failed to render proxy.rs: {}", e))
        })
    }

    /// Render the types.rs template
    pub fn render_types(&self, context: &impl serde::Serialize) -> ProxyResult<String> {
        self.handlebars.render("types", context).map_err(|e| {
            crate::error::ProxyError::codegen(format!("Failed to render types.rs: {}", e))
        })
    }

    /// Render the Cargo.toml template
    pub fn render_cargo_toml(&self, context: &impl serde::Serialize) -> ProxyResult<String> {
        self.handlebars.render("cargo_toml", context).map_err(|e| {
            crate::error::ProxyError::codegen(format!("Failed to render Cargo.toml: {}", e))
        })
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new().expect("Failed to create template engine")
    }
}

// Handlebars helpers for case conversion

fn snake_case_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> Result<(), handlebars::RenderError> {
    let param = h.param(0).ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "snake_case requires one parameter".to_string(),
        ))
    })?;

    let value = param.value().as_str().ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "snake_case parameter must be a string".to_string(),
        ))
    })?;

    out.write(&value.to_case(Case::Snake))?;
    Ok(())
}

fn pascal_case_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> Result<(), handlebars::RenderError> {
    let param = h.param(0).ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "pascal_case requires one parameter".to_string(),
        ))
    })?;

    let value = param.value().as_str().ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "pascal_case parameter must be a string".to_string(),
        ))
    })?;

    out.write(&value.to_case(Case::Pascal))?;
    Ok(())
}

fn camel_case_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> Result<(), handlebars::RenderError> {
    let param = h.param(0).ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "camel_case requires one parameter".to_string(),
        ))
    })?;

    let value = param.value().as_str().ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "camel_case parameter must be a string".to_string(),
        ))
    })?;

    out.write(&value.to_case(Case::Camel))?;
    Ok(())
}

fn kebab_case_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> Result<(), handlebars::RenderError> {
    let param = h.param(0).ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "kebab_case requires one parameter".to_string(),
        ))
    })?;

    let value = param.value().as_str().ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "kebab_case parameter must be a string".to_string(),
        ))
    })?;

    out.write(&value.to_case(Case::Kebab))?;
    Ok(())
}

fn eq_helper(
    h: &handlebars::Helper,
    _: &Handlebars,
    _: &handlebars::Context,
    _: &mut handlebars::RenderContext,
    out: &mut dyn handlebars::Output,
) -> Result<(), handlebars::RenderError> {
    let param1 = h.param(0).ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "eq requires two parameters".to_string(),
        ))
    })?;

    let param2 = h.param(1).ok_or_else(|| {
        handlebars::RenderError::from(handlebars::RenderErrorReason::Other(
            "eq requires two parameters".to_string(),
        ))
    })?;

    // Compare as strings
    let val1 = param1.value().as_str().unwrap_or("");
    let val2 = param2.value().as_str().unwrap_or("");

    // Return boolean result
    out.write(if val1 == val2 { "true" } else { "false" })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct TestContext {
        server_name: String,
        server_version: String,
        generation_date: String,
    }

    #[test]
    fn test_template_engine_creation() {
        let engine = TemplateEngine::new();
        assert!(
            engine.is_ok(),
            "Template engine should be created successfully"
        );
    }

    #[test]
    fn test_render_main_template() {
        let engine = TemplateEngine::new().unwrap();
        let context = TestContext {
            server_name: "test-server".to_string(),
            server_version: "1.0.0".to_string(),
            generation_date: "2025-01-01".to_string(),
        };

        let result = engine.render_main(&context);
        assert!(result.is_ok(), "Should render main.rs template");

        let output = result.unwrap();
        assert!(
            output.contains("test-server"),
            "Output should contain server name"
        );
        assert!(output.contains("1.0.0"), "Output should contain version");
    }

    #[test]
    fn test_case_conversion_helpers() {
        let engine = TemplateEngine::new();

        // Just verify the engine can be created successfully with helpers registered
        assert!(
            engine.is_ok(),
            "Template engine with helpers should be created successfully"
        );
    }
}
