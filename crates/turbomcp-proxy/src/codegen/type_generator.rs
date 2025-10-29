//! JSON Schema to Rust type converter
//!
//! This module converts JSON Schema definitions from MCP tool specifications
//! into Rust type definitions with proper serde annotations.

use convert_case::{Case, Casing};
use serde_json::Value;
use std::collections::HashSet;

use super::context::{FieldDefinition, ParamDefinition, TypeDefinition};
use crate::error::{ProxyError, ProxyResult};

/// Type generator for converting JSON Schemas to Rust types
pub struct TypeGenerator {
    /// Track generated type names to avoid duplicates
    generated_types: HashSet<String>,
}

impl TypeGenerator {
    /// Create a new type generator
    pub fn new() -> Self {
        Self {
            generated_types: HashSet::new(),
        }
    }

    /// Convert a JSON Schema to a Rust type name
    ///
    /// Returns the Rust type string (e.g., "String", "Vec<i64>", "CustomType")
    pub fn schema_to_rust_type(&self, schema: &Value, type_name_hint: Option<&str>) -> String {
        // Handle references
        if let Some(ref_str) = schema.get("$ref").and_then(|v| v.as_str()) {
            // Extract type name from $ref (e.g., "#/definitions/MyType" -> "MyType")
            return ref_str
                .split('/')
                .last()
                .unwrap_or("Value")
                .to_case(Case::Pascal);
        }

        // Handle type field
        let type_str = schema
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("object");

        match type_str {
            "string" => self.handle_string_type(schema),
            "number" => "f64".to_string(),
            "integer" => self.handle_integer_type(schema),
            "boolean" => "bool".to_string(),
            "array" => self.handle_array_type(schema),
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
        }
    }

    /// Generate a TypeDefinition from a JSON Schema object
    pub fn generate_type_from_schema(
        &mut self,
        name: &str,
        schema: &Value,
        description: Option<String>,
    ) -> ProxyResult<TypeDefinition> {
        let type_name = name.to_case(Case::Pascal);

        // Check for duplicate
        if self.generated_types.contains(&type_name) {
            return Err(ProxyError::codegen(format!(
                "Type {} already generated",
                type_name
            )));
        }

        self.generated_types.insert(type_name.clone());

        // Extract properties
        let properties = schema
            .get("properties")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                ProxyError::codegen(format!("Schema for {} missing properties", name))
            })?;

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
            let rust_type = self.schema_to_rust_type(
                field_schema,
                Some(&format!("{}{}", name, field_name.to_case(Case::Pascal))),
            );
            let field_description = field_schema
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from);

            fields.push(FieldDefinition {
                name: field_name.to_case(Case::Snake),
                rust_type,
                optional: !required.contains(field_name),
                description: field_description,
            });
        }

        Ok(TypeDefinition {
            name: type_name,
            description,
            rename: None,
            fields,
        })
    }

    /// Generate parameters from a JSON Schema for enum variants
    pub fn generate_params_from_schema(&self, schema: &Value) -> Vec<ParamDefinition> {
        let properties = match schema.get("properties").and_then(|v| v.as_object()) {
            Some(props) => props,
            None => return vec![],
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
            .map(|(name, prop_schema)| {
                let rust_type = self.schema_to_rust_type(prop_schema, None);
                ParamDefinition {
                    name: name.to_case(Case::Snake),
                    rust_type,
                    optional: !required.contains(name),
                }
            })
            .collect()
    }

    // Private helper methods

    fn handle_string_type(&self, schema: &Value) -> String {
        // Check for enum (string union type)
        if schema.get("enum").is_some() {
            // Could generate a proper enum, but for simplicity use String
            "String".to_string()
        } else {
            "String".to_string()
        }
    }

    fn handle_integer_type(&self, schema: &Value) -> String {
        // Check format hint
        if let Some(format) = schema.get("format").and_then(|v| v.as_str()) {
            match format {
                "int32" => "i32".to_string(),
                "int64" => "i64".to_string(),
                "uint32" => "u32".to_string(),
                "uint64" => "u64".to_string(),
                _ => "i64".to_string(),
            }
        } else {
            "i64".to_string()
        }
    }

    fn handle_array_type(&self, schema: &Value) -> String {
        let items = schema.get("items");

        if let Some(items_schema) = items {
            let item_type = self.schema_to_rust_type(items_schema, None);
            format!("Vec<{}>", item_type)
        } else {
            "Vec<serde_json::Value>".to_string()
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
        let gen = TypeGenerator::new();

        assert_eq!(
            gen.schema_to_rust_type(&json!({"type": "string"}), None),
            "String"
        );
        assert_eq!(
            gen.schema_to_rust_type(&json!({"type": "number"}), None),
            "f64"
        );
        assert_eq!(
            gen.schema_to_rust_type(&json!({"type": "integer"}), None),
            "i64"
        );
        assert_eq!(
            gen.schema_to_rust_type(&json!({"type": "boolean"}), None),
            "bool"
        );
    }

    #[test]
    fn test_array_type() {
        let gen = TypeGenerator::new();

        let schema = json!({
            "type": "array",
            "items": {"type": "string"}
        });

        assert_eq!(gen.schema_to_rust_type(&schema, None), "Vec<String>");
    }

    #[test]
    fn test_nested_array() {
        let gen = TypeGenerator::new();

        let schema = json!({
            "type": "array",
            "items": {
                "type": "array",
                "items": {"type": "integer"}
            }
        });

        assert_eq!(gen.schema_to_rust_type(&schema, None), "Vec<Vec<i64>>");
    }

    #[test]
    fn test_integer_formats() {
        let gen = TypeGenerator::new();

        assert_eq!(
            gen.schema_to_rust_type(&json!({"type": "integer", "format": "int32"}), None),
            "i32"
        );
        assert_eq!(
            gen.schema_to_rust_type(&json!({"type": "integer", "format": "int64"}), None),
            "i64"
        );
    }

    #[test]
    fn test_generate_type_from_schema() {
        let mut gen = TypeGenerator::new();

        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "User name"},
                "age": {"type": "integer"},
                "email": {"type": "string"}
            },
            "required": ["name", "age"]
        });

        let type_def = gen
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
        let gen = TypeGenerator::new();

        let schema = json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer"},
                "offset": {"type": "integer"}
            },
            "required": ["query"]
        });

        let params = gen.generate_params_from_schema(&schema);

        assert_eq!(params.len(), 3);

        let query_param = params.iter().find(|p| p.name == "query").unwrap();
        assert_eq!(query_param.rust_type, "String");
        assert!(!query_param.optional);

        let limit_param = params.iter().find(|p| p.name == "limit").unwrap();
        assert!(limit_param.optional);
    }

    #[test]
    fn test_duplicate_type_prevention() {
        let mut gen = TypeGenerator::new();

        let schema = json!({
            "type": "object",
            "properties": {
                "field": {"type": "string"}
            }
        });

        // First generation should succeed
        assert!(gen.generate_type_from_schema("User", &schema, None).is_ok());

        // Second generation of same type should fail
        assert!(gen
            .generate_type_from_schema("User", &schema, None)
            .is_err());
    }

    #[test]
    fn test_complex_nested_type() {
        let gen = TypeGenerator::new();

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

        let params = gen.generate_params_from_schema(&schema);

        let tags = params.iter().find(|p| p.name == "tags").unwrap();
        assert_eq!(tags.rust_type, "Vec<String>");

        let scores = params.iter().find(|p| p.name == "scores").unwrap();
        assert_eq!(scores.rust_type, "Vec<Vec<f64>>");
    }
}
