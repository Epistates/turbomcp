//! Structured output support with automatic JSON schema generation

use serde::{Deserialize, Serialize};
use std::fmt;

/// Wrapper type for structured JSON output with automatic schema generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Json<T>(pub T);

impl<T> Json<T> {
    /// Create a new Json wrapper
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Extract the inner value
    pub fn into_inner(self) -> T {
        self.0
    }

    /// Get a reference to the inner value
    pub const fn inner(&self) -> &T {
        &self.0
    }

    /// Get a mutable reference to the inner value
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> std::ops::Deref for Json<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Json<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for Json<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> fmt::Display for Json<T>
where
    T: Serialize,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_string_pretty(&self.0) {
            Ok(json) => write!(f, "{json}"),
            Err(_) => write!(f, "[Serialization Error]"),
        }
    }
}

/// Generate JSON schema for a type (always available - schema generation is required by MCP)
#[must_use]
pub fn generate_json_schema<T: schemars::JsonSchema>() -> serde_json::Value {
    crate::schema::generate_schema::<T>()
}

/// Trait for types that can be converted to structured output
pub trait ToStructuredOutput {
    /// Convert to a structured output with optional schema
    fn to_structured_output(&self) -> crate::McpResult<StructuredOutput>;
}

/// Structured output with content and optional schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredOutput {
    /// The JSON content
    pub content: serde_json::Value,
    /// Optional JSON schema describing the content
    pub schema: Option<serde_json::Value>,
    /// MIME type for the content
    pub mime_type: String,
}

impl<T> ToStructuredOutput for Json<T>
where
    T: Serialize,
{
    fn to_structured_output(&self) -> crate::McpResult<StructuredOutput> {
        let content = serde_json::to_value(&self.0).map_err(crate::McpError::Serialization)?;

        Ok(StructuredOutput {
            content,
            schema: None, // Will be populated by the macro system
            mime_type: "application/json".to_string(),
        })
    }
}

/// Parameters wrapper for type-safe parameter extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameters<T>(pub T);

impl<T> Parameters<T> {
    /// Create new parameters wrapper
    pub const fn new(params: T) -> Self {
        Self(params)
    }

    /// Extract the inner parameters
    pub fn into_inner(self) -> T {
        self.0
    }

    /// Get a reference to the inner parameters
    pub const fn inner(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::Deref for Parameters<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Parameters<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for Parameters<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

/// Try to parse parameters from JSON value
impl<T> Parameters<T>
where
    T: for<'de> Deserialize<'de>,
{
    /// Parse parameters from a JSON value
    pub fn from_json(value: serde_json::Value) -> crate::McpResult<Self> {
        let params = serde_json::from_value(value)
            .map_err(|e| crate::McpError::Tool(format!("Parameter parsing error: {e}")))?;
        Ok(Self(params))
    }

    /// Parse parameters from a map of string values
    pub fn from_map(
        map: std::collections::HashMap<String, serde_json::Value>,
    ) -> crate::McpResult<Self> {
        let value = serde_json::to_value(map).map_err(crate::McpError::Serialization)?;
        Self::from_json(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[test]
    fn test_json_wrapper() {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let json_data = Json::new(data.clone());
        assert_eq!(json_data.inner(), &data);
        assert_eq!(json_data.into_inner(), data);
    }

    #[test]
    fn test_json_serialization() {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let json_data = Json::new(data);
        let serialized = serde_json::to_string(&json_data).unwrap();
        let deserialized: Json<TestData> = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.value, 42);
    }

    #[test]
    fn test_parameters_wrapper() {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let params = Parameters::new(data.clone());
        assert_eq!(params.inner(), &data);
        assert_eq!(params.into_inner(), data);
    }

    #[test]
    fn test_parameters_from_json() {
        let json_value = serde_json::json!({
            "name": "test",
            "value": 42
        });

        let params: Parameters<TestData> = Parameters::from_json(json_value).unwrap();
        assert_eq!(params.name, "test");
        assert_eq!(params.value, 42);
    }
}
