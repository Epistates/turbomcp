//! Template context types for resource template operations.
//!
//! This module contains types for handling URI template operations,
//! parameter validation, and template metadata management.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Context for resource template operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplateContext {
    /// Template name
    pub template_name: String,
    /// URI template pattern (RFC 6570)
    pub uri_template: String,
    /// Available template parameters
    pub parameters: HashMap<String, TemplateParameter>,
    /// Template description
    pub description: Option<String>,
    /// Template category/preset type
    pub preset_type: Option<String>,
    /// Template metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Template parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateParameter {
    /// Parameter name
    pub name: String,
    /// Parameter type
    pub param_type: String,
    /// Whether parameter is required
    pub required: bool,
    /// Default value
    pub default: Option<serde_json::Value>,
    /// Parameter description
    pub description: Option<String>,
    /// Validation pattern (regex)
    pub pattern: Option<String>,
    /// Enum values (if applicable)
    pub enum_values: Option<Vec<serde_json::Value>>,
}
