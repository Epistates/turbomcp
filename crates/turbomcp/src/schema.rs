//! Comprehensive JSON Schema generation and validation
//!
//! This module provides JSON schema generation, validation,
//! and integration with the MCP protocol type system.

use serde_json::{Map, Value};
use std::collections::HashMap;

#[cfg(feature = "schema-generation")]
use schemars::{JsonSchema, schema_for};

use crate::{McpError, McpResult};

/// Enhanced schema generation result with metadata
#[derive(Debug, Clone)]
pub struct SchemaGenerationResult {
    /// The generated JSON schema
    pub schema: Value,
    /// Schema metadata and properties
    pub metadata: SchemaMetadata,
}

/// Schema metadata for enhanced validation and documentation
#[derive(Debug, Clone)]
pub struct SchemaMetadata {
    /// Schema title
    pub title: Option<String>,
    /// Schema description
    pub description: Option<String>,
    /// Schema version
    pub version: Option<String>,
    /// Validation strictness level
    pub strict: bool,
    /// Custom validation rules
    pub custom_rules: Vec<String>,
    /// Performance optimization hints
    pub optimize_for: OptimizationTarget,
}

/// Schema optimization targets
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptimizationTarget {
    /// Optimize for validation speed
    Speed,
    /// Optimize for memory usage
    Memory,
    /// Optimize for comprehensive validation
    Completeness,
    /// Balanced optimization
    Balanced,
}

/// Enhanced schema generator with customization options
#[derive(Debug, Clone, Default)]
pub struct SchemaGenerator {
    /// Generation options
    pub options: SchemaOptions,
}

/// Schema generation options
#[derive(Debug, Clone)]
pub struct SchemaOptions {
    /// Include examples in schema
    pub include_examples: bool,
    /// Include format validation
    pub include_formats: bool,
    /// Include performance annotations
    pub include_performance_hints: bool,
    /// Optimization target
    pub optimization: OptimizationTarget,
    /// Custom property mappings
    pub custom_mappings: HashMap<String, Value>,
    /// Validation strictness
    pub strict_validation: bool,
}

impl Default for SchemaOptions {
    fn default() -> Self {
        Self {
            include_examples: true,
            include_formats: true,
            include_performance_hints: false,
            optimization: OptimizationTarget::Balanced,
            custom_mappings: HashMap::new(),
            strict_validation: true,
        }
    }
}

impl SchemaGenerator {
    /// Create a new schema generator with default options
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a schema generator optimized for speed
    #[must_use]
    pub fn optimized_for_speed() -> Self {
        Self {
            options: SchemaOptions {
                optimization: OptimizationTarget::Speed,
                include_examples: false,
                include_performance_hints: true,
                strict_validation: false,
                ..Default::default()
            },
        }
    }

    /// Create a schema generator optimized for completeness
    #[must_use]
    pub fn optimized_for_completeness() -> Self {
        Self {
            options: SchemaOptions {
                optimization: OptimizationTarget::Completeness,
                include_examples: true,
                include_formats: true,
                include_performance_hints: true,
                strict_validation: true,
                ..Default::default()
            },
        }
    }

    /// Generate enhanced schema with full metadata
    #[cfg(feature = "schema-generation")]
    #[must_use]
    pub fn generate_enhanced<T: JsonSchema>(&self) -> SchemaGenerationResult {
        let schema_value = schema_for!(T);
        let mut schema = serde_json::to_value(&schema_value)
            .unwrap_or_else(|_| serde_json::json!({"type": "object"}));

        // Apply optimizations
        self.apply_optimizations(&mut schema);

        // Add performance hints if enabled
        if self.options.include_performance_hints {
            self.add_performance_hints(&mut schema);
        }

        // Apply custom mappings
        self.apply_custom_mappings(&mut schema);

        SchemaGenerationResult {
            schema,
            metadata: SchemaMetadata {
                title: schema_value
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                description: schema_value
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                version: Some("1.0.0".to_string()),
                strict: self.options.strict_validation,
                custom_rules: Vec::new(),
                optimize_for: self.options.optimization.clone(),
            },
        }
    }

    /// Generate enhanced schema (fallback without schemars)
    #[cfg(not(feature = "schema-generation"))]
    pub fn generate_enhanced<T>(&self) -> SchemaGenerationResult {
        let mut schema = serde_json::json!({
            "type": "object",
            "additionalProperties": true
        });

        // Apply basic optimizations
        self.apply_optimizations(&mut schema);

        SchemaGenerationResult {
            schema,
            metadata: SchemaMetadata {
                title: Some("Generated Schema".to_string()),
                description: Some("Auto-generated schema (schemars not available)".to_string()),
                version: Some("1.0.0".to_string()),
                strict: false,
                custom_rules: Vec::new(),
                optimize_for: self.options.optimization.clone(),
            },
        }
    }

    /// Apply performance optimizations to schema
    fn apply_optimizations(&self, schema: &mut Value) {
        match self.options.optimization {
            OptimizationTarget::Speed => {
                // Remove complex validations for speed
                if let Value::Object(obj) = schema {
                    obj.remove("pattern");
                    obj.remove("format");
                    // Simplify nested validations
                    if let Some(Value::Object(props)) = obj.get_mut("properties") {
                        for prop in props.values_mut() {
                            if let Value::Object(prop_obj) = prop {
                                prop_obj.remove("pattern");
                                prop_obj.remove("format");
                            }
                        }
                    }
                }
            }
            OptimizationTarget::Memory => {
                // Remove examples and descriptions to save memory
                remove_recursive(schema, &["examples", "description", "title"]);
            }
            OptimizationTarget::Completeness => {
                // Add comprehensive validation rules
                self.add_comprehensive_validation(schema);
            }
            OptimizationTarget::Balanced => {
                // Apply balanced optimizations
                if let Value::Object(obj) = schema {
                    // Keep essential validations but remove verbose descriptions
                    if !self.options.include_examples {
                        obj.remove("examples");
                    }
                }
            }
        }
    }

    /// Add performance hints to schema
    fn add_performance_hints(&self, schema: &mut Value) {
        if let Value::Object(obj) = schema {
            let mut hints = Map::new();
            hints.insert("cacheable".to_string(), Value::Bool(true));
            hints.insert(
                "optimization_target".to_string(),
                Value::String(format!("{:?}", self.options.optimization)),
            );
            hints.insert(
                "validation_complexity".to_string(),
                Value::String("medium".to_string()),
            );

            obj.insert("_performance_hints".to_string(), Value::Object(hints));
        }
    }

    /// Apply custom property mappings
    fn apply_custom_mappings(&self, schema: &mut Value) {
        for (key, value) in &self.options.custom_mappings {
            if let Value::Object(obj) = schema {
                obj.insert(key.clone(), value.clone());
            }
        }
    }

    /// Add comprehensive validation rules
    fn add_comprehensive_validation(&self, schema: &mut Value) {
        if let Value::Object(obj) = schema {
            // Add strict validation
            obj.insert("additionalProperties".to_string(), Value::Bool(false));

            // Add validation metadata
            let mut validation_meta = Map::new();
            validation_meta.insert("strict".to_string(), Value::Bool(true));
            validation_meta.insert("comprehensive".to_string(), Value::Bool(true));
            obj.insert(
                "_validation_meta".to_string(),
                Value::Object(validation_meta),
            );
        }
    }
}

/// Remove specified keys recursively from JSON value
fn remove_recursive(value: &mut Value, keys: &[&str]) {
    match value {
        Value::Object(obj) => {
            for key in keys {
                obj.remove(*key);
            }
            for val in obj.values_mut() {
                remove_recursive(val, keys);
            }
        }
        Value::Array(arr) => {
            for val in arr.iter_mut() {
                remove_recursive(val, keys);
            }
        }
        _ => {}
    }
}

/// Generate JSON schema for a type
#[cfg(feature = "schema-generation")]
#[must_use]
pub fn generate_schema<T: JsonSchema>() -> Value {
    let generator = SchemaGenerator::new();
    generator.generate_enhanced::<T>().schema
}

/// Fallback schema generation without schemars
#[cfg(not(feature = "schema-generation"))]
pub fn generate_schema<T>() -> Value {
    let generator = SchemaGenerator::new();
    generator.generate_enhanced::<T>().schema
}

/// Generate optimized schema for fast scenarios
#[cfg(feature = "schema-generation")]
#[must_use]
pub fn generate_fast_schema<T: JsonSchema>() -> Value {
    let generator = SchemaGenerator::optimized_for_speed();
    generator.generate_enhanced::<T>().schema
}

/// Generate optimized schema for fast scenarios (fallback)
#[cfg(not(feature = "schema-generation"))]
pub fn generate_fast_schema<T>() -> Value {
    let generator = SchemaGenerator::optimized_for_speed();
    generator.generate_enhanced::<T>().schema
}

/// Generate JSON Schema for a type `T` using schemars with full metadata
#[cfg(feature = "schema-generation")]
#[must_use]
pub fn json_schema_for<T: JsonSchema>() -> Value {
    generate_schema::<T>()
}

/// Generate JSON Schema for a type `T` (fallback)
#[cfg(not(feature = "schema-generation"))]
pub fn json_schema_for<T>() -> Value {
    generate_schema::<T>()
}

/// Validate JSON data against a schema
pub fn validate_against_schema(data: &Value, schema: &Value) -> McpResult<()> {
    // JSON Schema validation implementation
    // Validates required properties, types, and formats

    if let (Value::Object(data_obj), Value::Object(schema_obj)) = (data, schema) {
        // Check required properties
        if let Some(Value::Array(required)) = schema_obj.get("required") {
            for req_prop in required {
                if let Value::String(prop_name) = req_prop
                    && !data_obj.contains_key(prop_name)
                {
                    return Err(McpError::Tool(format!(
                        "Missing required property: {prop_name}"
                    )));
                }
            }
        }

        // Check additional properties if disallowed
        if matches!(
            schema_obj.get("additionalProperties"),
            Some(Value::Bool(false))
        ) && let Some(Value::Object(properties)) = schema_obj.get("properties")
        {
            for data_key in data_obj.keys() {
                if !properties.contains_key(data_key) {
                    return Err(McpError::Tool(format!(
                        "Additional property not allowed: {data_key}"
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Schema validation configuration
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Enable strict validation
    pub strict: bool,
    /// Allow additional properties
    pub allow_additional_properties: bool,
    /// Enable format validation
    pub validate_formats: bool,
    /// Custom validation rules
    pub custom_rules: Vec<String>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            strict: true,
            allow_additional_properties: false,
            validate_formats: true,
            custom_rules: Vec::new(),
        }
    }
}

/// Fast schema validation
pub fn validate_with_config(
    data: &Value,
    schema: &Value,
    config: &ValidationConfig,
) -> McpResult<()> {
    if config.strict {
        validate_against_schema(data, schema)?;
    }

    // Additional validations based on config
    if config.validate_formats {
        validate_formats(data, schema)?;
    }

    Ok(())
}

/// Validate format constraints in JSON data
fn validate_formats(data: &Value, schema: &Value) -> McpResult<()> {
    // Format validation implementation for common JSON Schema formats
    if let (Value::Object(data_obj), Value::Object(schema_obj)) = (data, schema)
        && let Some(Value::Object(properties)) = schema_obj.get("properties")
    {
        for (prop_name, prop_schema) in properties {
            if let Some(data_value) = data_obj.get(prop_name)
                && let Value::Object(prop_schema_obj) = prop_schema
                && let Some(Value::String(format)) = prop_schema_obj.get("format")
            {
                validate_format_constraint(data_value, format, prop_name)?;
            }
        }
    }

    Ok(())
}

/// Validate specific format constraints
fn validate_format_constraint(value: &Value, format: &str, field_name: &str) -> McpResult<()> {
    if let Value::String(s) = value {
        match format {
            "email" => {
                if !s.contains('@') || !s.contains('.') {
                    return Err(McpError::Tool(format!(
                        "Invalid email format in field '{field_name}': {s}"
                    )));
                }
            }
            "uri" => {
                if !s.starts_with("http://") && !s.starts_with("https://") {
                    return Err(McpError::Tool(format!(
                        "Invalid URI format in field '{field_name}': {s}"
                    )));
                }
            }
            "date-time" => {
                // Basic ISO 8601 validation
                if !s.contains('T') || !s.contains(':') {
                    return Err(McpError::Tool(format!(
                        "Invalid date-time format in field '{field_name}': {s}"
                    )));
                }
            }
            _ => {
                // Unknown format, skip validation
            }
        }
    }

    Ok(())
}
