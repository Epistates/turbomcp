//! JSON Schema to Rust type converter
//!
//! This module converts JSON Schema definitions from MCP tool specifications
//! into Rust type definitions with proper serde annotations.

use convert_case::{Case, Casing};
use serde_json::Value;
use std::collections::HashSet;

use super::context::{FieldDefinition, ParamDefinition, TypeDefinition};
use super::sanitize::{sanitize_identifier, sanitize_string_literal, sanitize_type};
use crate::error::{ProxyError, ProxyResult};

/// Type generator for converting JSON Schemas to Rust types
pub struct TypeGenerator {
    /// Track generated type names to avoid duplicates
    generated_types: HashSet<String>,
}

impl TypeGenerator {
    /// Create a new type generator
    #[must_use]
    pub fn new() -> Self {
        Self {
            generated_types: HashSet::new(),
        }
    }

    /// Convert a JSON Schema to a Rust type name
    ///
    /// Returns the Rust type string (e.g., "String", "Vec<i64>", "`CustomType`")
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the generated type is invalid.
    pub fn schema_to_rust_type(
        &self,
        schema: &Value,
        type_name_hint: Option<&str>,
    ) -> ProxyResult<String> {
        // Handle references
        if let Some(ref_str) = schema.get("$ref").and_then(|v| v.as_str()) {
            // Extract type name from $ref (e.g., "#/definitions/MyType" -> "MyType")
            let type_name = ref_str
                .split('/')
                .next_back()
                .unwrap_or("Value")
                .to_case(Case::Pascal);
            return sanitize_type(&type_name);
        }

        // Handle type field
        let type_str = schema
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("object");

        let rust_type = match type_str {
            "string" => Self::handle_string_type(schema),
            "number" => "f64".to_string(),
            "integer" => Self::handle_integer_type(schema),
            "boolean" => "bool".to_string(),
            "array" => self.handle_array_type(schema)?,
            "object" => {
                // For object types, we either reference a named type or use Value
                if let Some(name) = type_name_hint {
                    name.to_case(Case::Pascal)
                } else {
                    "serde_json::Value".to_string()
                }
            }
            "null" => "()".to_string(),
            _ => "serde_json::Value".to_string(),
        };

        sanitize_type(&rust_type)
    }

    /// Generate a `TypeDefinition` from a JSON Schema object
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the schema is missing required properties or a type with the same name already exists.
    pub fn generate_type_from_schema(
        &mut self,
        name: &str,
        schema: &Value,
        description: Option<String>,
    ) -> ProxyResult<TypeDefinition> {
        let type_name = name.to_case(Case::Pascal);

        // Sanitize type name
        let sanitized_type_name = sanitize_identifier(&type_name)?;

        // Check for duplicate
        if self.generated_types.contains(&sanitized_type_name) {
            return Err(ProxyError::codegen(format!(
                "Type {sanitized_type_name} already generated"
            )));
        }

        self.generated_types.insert(sanitized_type_name.clone());

        // Extract properties
        let properties = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .ok_or_else(|| ProxyError::codegen(format!("Schema for {name} missing properties")))?;

        // Extract required fields
        let required: HashSet<String> = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Generate fields
        let mut fields = Vec::new();
        for (field_name, field_schema) in properties {
            // Sanitize field name
            let snake_case_name = field_name.to_case(Case::Snake);
            let sanitized_field_name = match sanitize_identifier(&snake_case_name) {
                Ok(name) => name,
                Err(e) => {
                    tracing::warn!(
                        "Skipping field '{}' in type '{}': {}",
                        field_name,
                        sanitized_type_name,
                        e
                    );
                    continue;
                }
            };

            let rust_type = self.schema_to_rust_type(
                field_schema,
                Some(&format!("{}{}", name, field_name.to_case(Case::Pascal))),
            )?;

            // Sanitize field description
            let field_description = field_schema
                .get("description")
                .and_then(|v| v.as_str())
                .map(sanitize_string_literal);

            fields.push(FieldDefinition {
                name: sanitized_field_name,
                rust_type,
                optional: !required.contains(field_name),
                description: field_description,
            });
        }

        Ok(TypeDefinition {
            name: sanitized_type_name,
            description,
            rename: None,
            fields,
        })
    }

    /// Generate parameters from a JSON Schema for enum variants
    #[must_use]
    pub fn generate_params_from_schema(&self, schema: &Value) -> Vec<ParamDefinition> {
        let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) else {
            return vec![];
        };

        let required: HashSet<String> = schema
            .get("required")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        properties
            .iter()
            .filter_map(|(name, prop_schema)| {
                // Sanitize parameter name
                let snake_case_name = name.to_case(Case::Snake);
                let sanitized_name = match sanitize_identifier(&snake_case_name) {
                    Ok(n) => n,
                    Err(e) => {
                        tracing::warn!("Skipping parameter '{}': {}", name, e);
                        return None;
                    }
                };

                // Get rust type (if this fails, skip the parameter)
                let rust_type = match self.schema_to_rust_type(prop_schema, None) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("Skipping parameter '{}': {}", name, e);
                        return None;
                    }
                };

                Some(ParamDefinition {
                    name: sanitized_name,
                    rust_type,
                    optional: !required.contains(name),
                })
            })
            .collect()
    }

    // Private helper methods

    fn handle_string_type(schema: &Value) -> String {
        // Check for enum (string union type)
        if schema.get("enum").is_some() {
            // Could generate a proper enum, but for simplicity use String
            "String".to_string()
        } else {
            "String".to_string()
        }
    }

    fn handle_integer_type(schema: &Value) -> String {
        // Check format hint
        if let Some(format) = schema.get("format").and_then(|v| v.as_str()) {
            match format {
                "int32" => "i32".to_string(),
                "uint32" => "u32".to_string(),
                "uint64" => "u64".to_string(),
                _ => "i64".to_string(), // Includes "int64" and unknown formats
            }
        } else {
            "i64".to_string()
        }
    }

    fn handle_array_type(&self, schema: &Value) -> ProxyResult<String> {
        let items = schema.get("items");

        if let Some(items_schema) = items {
            let item_type = self.schema_to_rust_type(items_schema, None)?;
            Ok(format!("Vec<{item_type}>"))
        } else {
            Ok("Vec<serde_json::Value>".to_string())
        }
    }
}

impl Default for TypeGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_types() {
        let type_gen = TypeGenerator::new();

        assert_eq!(
            type_gen
                .schema_to_rust_type(&json!({"type": "string"}), None)
                .unwrap(),
            "String"
        );
        assert_eq!(
            type_gen
                .schema_to_rust_type(&json!({"type": "number"}), None)
                .unwrap(),
            "f64"
        );
        assert_eq!(
            type_gen
                .schema_to_rust_type(&json!({"type": "integer"}), None)
                .unwrap(),
            "i64"
        );
        assert_eq!(
            type_gen
                .schema_to_rust_type(&json!({"type": "boolean"}), None)
                .unwrap(),
            "bool"
        );
    }

    #[test]
    fn test_array_type() {
        let type_gen = TypeGenerator::new();

        let schema = json!({
            "type": "array",
            "items": {"type": "string"}
        });

        assert_eq!(
            type_gen.schema_to_rust_type(&schema, None).unwrap(),
            "Vec<String>"
        );
    }

    #[test]
    fn test_nested_array() {
        let type_gen = TypeGenerator::new();

        let schema = json!({
            "type": "array",
            "items": {
                "type": "array",
                "items": {"type": "integer"}
            }
        });

        assert_eq!(
            type_gen.schema_to_rust_type(&schema, None).unwrap(),
            "Vec<Vec<i64>>"
        );
    }

    #[test]
    fn test_integer_formats() {
        let type_gen = TypeGenerator::new();

        assert_eq!(
            type_gen
                .schema_to_rust_type(&json!({"type": "integer", "format": "int32"}), None)
                .unwrap(),
            "i32"
        );
        assert_eq!(
            type_gen
                .schema_to_rust_type(&json!({"type": "integer", "format": "int64"}), None)
                .unwrap(),
            "i64"
        );
    }

    #[test]
    fn test_generate_type_from_schema() {
        let mut type_gen = TypeGenerator::new();

        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "User name"},
                "age": {"type": "integer"},
                "email": {"type": "string"}
            },
            "required": ["name", "age"]
        });

        let type_def = type_gen
            .generate_type_from_schema("User", &schema, Some("User information".to_string()))
            .unwrap();

        assert_eq!(type_def.name, "User");
        assert_eq!(type_def.description, Some("User information".to_string()));
        assert_eq!(type_def.fields.len(), 3);

        // Check name field (required)
        let name_field = &type_def.fields[0];
        assert_eq!(name_field.name, "name");
        assert_eq!(name_field.rust_type, "String");
        assert!(!name_field.optional);

        // Check email field (optional)
        let email_field = type_def.fields.iter().find(|f| f.name == "email").unwrap();
        assert!(email_field.optional);
    }

    #[test]
    fn test_generate_params_from_schema() {
        let type_gen = TypeGenerator::new();

        let schema = json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer"},
                "offset": {"type": "integer"}
            },
            "required": ["query"]
        });

        let params = type_gen.generate_params_from_schema(&schema);

        assert_eq!(params.len(), 3);

        let query_param = params.iter().find(|p| p.name == "query").unwrap();
        assert_eq!(query_param.rust_type, "String");
        assert!(!query_param.optional);

        let limit_param = params.iter().find(|p| p.name == "limit").unwrap();
        assert!(limit_param.optional);
    }

    #[test]
    fn test_duplicate_type_prevention() {
        let mut type_gen = TypeGenerator::new();

        let schema = json!({
            "type": "object",
            "properties": {
                "field": {"type": "string"}
            }
        });

        // First generation should succeed
        assert!(
            type_gen
                .generate_type_from_schema("User", &schema, None)
                .is_ok()
        );

        // Second generation of same type should fail
        assert!(
            type_gen
                .generate_type_from_schema("User", &schema, None)
                .is_err()
        );
    }

    #[test]
    fn test_complex_nested_type() {
        let type_gen = TypeGenerator::new();

        let schema = json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": {"type": "string"}
                },
                "metadata": {
                    "type": "object"
                },
                "scores": {
                    "type": "array",
                    "items": {
                        "type": "array",
                        "items": {"type": "number"}
                    }
                }
            }
        });

        let params = type_gen.generate_params_from_schema(&schema);

        let tags = params.iter().find(|p| p.name == "tags").unwrap();
        assert_eq!(tags.rust_type, "Vec<String>");

        let scores = params.iter().find(|p| p.name == "scores").unwrap();
        assert_eq!(scores.rust_type, "Vec<Vec<f64>>");
    }
}
