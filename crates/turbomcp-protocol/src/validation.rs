//! # Protocol Validation
//!
//! This module provides comprehensive validation for MCP protocol messages,
//! ensuring data integrity and specification compliance.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::jsonrpc::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use crate::types::*;

/// Cached regex for URI validation (compiled once)
static URI_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*:").expect("Invalid URI regex pattern"));

/// Cached regex for method name validation (compiled once)
static METHOD_NAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-zA-Z][a-zA-Z0-9_/]*$").expect("Invalid method name regex pattern")
});

/// Protocol message validator
#[derive(Debug, Clone)]
pub struct ProtocolValidator {
    /// Validation rules
    rules: ValidationRules,
    /// Strict validation mode
    strict_mode: bool,
}

/// Validation rules configuration
#[derive(Debug, Clone)]
pub struct ValidationRules {
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Maximum string length
    pub max_string_length: usize,
    /// Maximum array length
    pub max_array_length: usize,
    /// Maximum object depth
    pub max_object_depth: usize,
    /// Required fields per message type
    pub required_fields: HashMap<String, HashSet<String>>,
}

impl ValidationRules {
    /// Get the URI validation regex (cached globally)
    #[inline]
    pub fn uri_regex(&self) -> &Regex {
        &URI_REGEX
    }

    /// Get the method name validation regex (cached globally)
    #[inline]
    pub fn method_name_regex(&self) -> &Regex {
        &METHOD_NAME_REGEX
    }
}

/// Validation result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationResult {
    /// Validation passed
    Valid,
    /// Validation passed with warnings
    ValidWithWarnings(Vec<ValidationWarning>),
    /// Validation failed
    Invalid(Vec<ValidationError>),
}

/// Validation warning
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationWarning {
    /// Warning code
    pub code: String,
    /// Warning message
    pub message: String,
    /// Field path (if applicable)
    pub field_path: Option<String>,
}

/// Validation error
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Error code
    pub code: String,
    /// Error message
    pub message: String,
    /// Field path (if applicable)
    pub field_path: Option<String>,
}

/// Validation context for tracking state during validation
#[derive(Debug, Clone)]
struct ValidationContext {
    /// Current field path
    path: Vec<String>,
    /// Current object depth
    depth: usize,
    /// Accumulated warnings
    warnings: Vec<ValidationWarning>,
    /// Accumulated errors
    errors: Vec<ValidationError>,
}

impl Default for ValidationRules {
    fn default() -> Self {
        let mut required_fields = HashMap::new();

        // JSON-RPC required fields
        required_fields.insert(
            "request".to_string(),
            ["jsonrpc", "method", "id"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        required_fields.insert(
            "response".to_string(),
            ["jsonrpc", "id"].iter().map(|s| s.to_string()).collect(),
        );
        required_fields.insert(
            "notification".to_string(),
            ["jsonrpc", "method"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );

        // MCP message required fields
        required_fields.insert(
            "initialize".to_string(),
            ["protocolVersion", "capabilities", "clientInfo"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        required_fields.insert(
            "tool".to_string(),
            ["name", "inputSchema"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        required_fields.insert(
            "prompt".to_string(),
            ["name"].iter().map(|s| s.to_string()).collect(),
        );
        required_fields.insert(
            "resource".to_string(),
            ["uri", "name"].iter().map(|s| s.to_string()).collect(),
        );

        Self {
            max_message_size: 10 * 1024 * 1024, // 10MB
            max_batch_size: 100,
            max_string_length: 1024 * 1024, // 1MB
            max_array_length: 10000,
            max_object_depth: 32,
            required_fields,
        }
    }
}

impl ProtocolValidator {
    /// Create a new validator with default rules
    pub fn new() -> Self {
        Self {
            rules: ValidationRules::default(),
            strict_mode: false,
        }
    }

    /// Enable strict validation mode
    pub fn with_strict_mode(mut self) -> Self {
        self.strict_mode = true;
        self
    }

    /// Set custom validation rules
    pub fn with_rules(mut self, rules: ValidationRules) -> Self {
        self.rules = rules;
        self
    }

    /// Validate a JSON-RPC request
    pub fn validate_request(&self, request: &JsonRpcRequest) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        // Validate JSON-RPC structure (includes method name validation)
        self.validate_jsonrpc_request(request, &mut ctx);

        // Validate parameters based on method
        if let Some(params) = &request.params {
            self.validate_method_params(&request.method, params, &mut ctx);
        }

        ctx.into_result()
    }

    /// Validate a JSON-RPC response
    pub fn validate_response(&self, response: &JsonRpcResponse) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        // Validate JSON-RPC structure
        self.validate_jsonrpc_response(response, &mut ctx);

        // Ensure either result or error is present (but not both)
        // Note: This validation is now enforced at the type level with JsonRpcResponsePayload enum
        // But we still validate for completeness
        match (response.result().is_some(), response.error().is_some()) {
            (true, true) => {
                ctx.add_error(
                    "RESPONSE_BOTH_RESULT_AND_ERROR",
                    "Response cannot have both result and error".to_string(),
                    None,
                );
            }
            (false, false) => {
                ctx.add_error(
                    "RESPONSE_MISSING_RESULT_OR_ERROR",
                    "Response must have either result or error".to_string(),
                    None,
                );
            }
            _ => {} // Valid
        }

        ctx.into_result()
    }

    /// Validate a JSON-RPC notification
    pub fn validate_notification(&self, notification: &JsonRpcNotification) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        // Validate JSON-RPC structure
        self.validate_jsonrpc_notification(notification, &mut ctx);

        // Validate method name
        self.validate_method_name(&notification.method, &mut ctx);

        // Validate parameters based on method
        if let Some(params) = &notification.params {
            self.validate_method_params(&notification.method, params, &mut ctx);
        }

        ctx.into_result()
    }

    /// Validate MCP protocol types
    pub fn validate_tool(&self, tool: &Tool) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        // Validate tool name
        if tool.name.is_empty() {
            ctx.add_error(
                "TOOL_EMPTY_NAME",
                "Tool name cannot be empty".to_string(),
                Some("name".to_string()),
            );
        }

        if tool.name.len() > self.rules.max_string_length {
            ctx.add_error(
                "TOOL_NAME_TOO_LONG",
                format!(
                    "Tool name exceeds maximum length of {}",
                    self.rules.max_string_length
                ),
                Some("name".to_string()),
            );
        }

        // Validate input schema
        self.validate_tool_input(&tool.input_schema, &mut ctx);

        ctx.into_result()
    }

    /// Validate a prompt
    pub fn validate_prompt(&self, prompt: &Prompt) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        // Validate prompt name
        if prompt.name.is_empty() {
            ctx.add_error(
                "PROMPT_EMPTY_NAME",
                "Prompt name cannot be empty".to_string(),
                Some("name".to_string()),
            );
        }

        // Validate arguments if present
        if let Some(arguments) = &prompt.arguments
            && arguments.len() > self.rules.max_array_length
        {
            ctx.add_error(
                "PROMPT_TOO_MANY_ARGS",
                format!(
                    "Prompt has too many arguments (max: {})",
                    self.rules.max_array_length
                ),
                Some("arguments".to_string()),
            );
        }

        ctx.into_result()
    }

    /// Validate a resource
    pub fn validate_resource(&self, resource: &Resource) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        // Validate URI
        if !self.rules.uri_regex().is_match(&resource.uri) {
            ctx.add_error(
                "RESOURCE_INVALID_URI",
                format!("Invalid URI format: {}", resource.uri),
                Some("uri".to_string()),
            );
        }

        // Validate name
        if resource.name.is_empty() {
            ctx.add_error(
                "RESOURCE_EMPTY_NAME",
                "Resource name cannot be empty".to_string(),
                Some("name".to_string()),
            );
        }

        ctx.into_result()
    }

    /// Validate initialization request
    pub fn validate_initialize_request(&self, request: &InitializeRequest) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        // Validate protocol version
        if !crate::SUPPORTED_VERSIONS.contains(&request.protocol_version.as_str()) {
            ctx.add_warning(
                "UNSUPPORTED_PROTOCOL_VERSION",
                format!(
                    "Protocol version {} is not officially supported",
                    request.protocol_version
                ),
                Some("protocolVersion".to_string()),
            );
        }

        // Validate client info
        if request.client_info.name.is_empty() {
            ctx.add_error(
                "EMPTY_CLIENT_NAME",
                "Client name cannot be empty".to_string(),
                Some("clientInfo.name".to_string()),
            );
        }

        if request.client_info.version.is_empty() {
            ctx.add_error(
                "EMPTY_CLIENT_VERSION",
                "Client version cannot be empty".to_string(),
                Some("clientInfo.version".to_string()),
            );
        }

        ctx.into_result()
    }

    /// Validate model preferences (priority ranges must be 0.0-1.0)
    ///
    /// Per MCP 2025-06-18 schema (lines 1346-1370), priority values must be in range [0.0, 1.0].
    pub fn validate_model_preferences(
        &self,
        prefs: &crate::types::ModelPreferences,
    ) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        // Validate each priority field
        let priorities = [
            ("costPriority", prefs.cost_priority),
            ("speedPriority", prefs.speed_priority),
            ("intelligencePriority", prefs.intelligence_priority),
        ];

        for (name, value) in priorities {
            if let Some(v) = value
                && !(0.0..=1.0).contains(&v)
            {
                ctx.add_error(
                    "PRIORITY_OUT_OF_RANGE",
                    format!(
                        "{} must be between 0.0 and 1.0 (inclusive), got {}",
                        name, v
                    ),
                    Some(name.to_string()),
                );
            }
        }

        ctx.into_result()
    }

    /// Validate elicitation result (content required for 'accept' action)
    ///
    /// Per MCP 2025-06-18 schema (line 634), content is "only present when action is 'accept'".
    pub fn validate_elicit_result(
        &self,
        result: &crate::types::ElicitResult,
    ) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        use crate::types::ElicitationAction;

        match result.action {
            ElicitationAction::Accept => {
                if result.content.is_none() {
                    ctx.add_error(
                        "MISSING_CONTENT_ON_ACCEPT",
                        "ElicitResult must have content when action is 'accept'".to_string(),
                        Some("content".to_string()),
                    );
                }
            }
            ElicitationAction::Decline | ElicitationAction::Cancel => {
                if result.content.is_some() {
                    ctx.add_warning(
                        "UNEXPECTED_CONTENT",
                        format!(
                            "Content should not be present when action is '{:?}'",
                            result.action
                        ),
                        Some("content".to_string()),
                    );
                }
            }
        }

        ctx.into_result()
    }

    /// Validate elicitation schema structure
    ///
    /// Per MCP 2025-06-18 spec, schemas must be flat objects with primitive properties only.
    pub fn validate_elicitation_schema(
        &self,
        schema: &crate::types::ElicitationSchema,
    ) -> ValidationResult {
        let mut ctx = ValidationContext::new();

        // Schema type must be "object" (schema.json:585)
        if schema.schema_type != "object" {
            ctx.add_error(
                "SCHEMA_NOT_OBJECT",
                format!(
                    "Elicitation schema type must be 'object', got '{}'",
                    schema.schema_type
                ),
                Some("type".to_string()),
            );
        }

        // Validate additionalProperties = false (flat constraint)
        if let Some(additional) = schema.additional_properties
            && additional
        {
            ctx.add_warning(
                "ADDITIONAL_PROPERTIES_NOT_RECOMMENDED",
                "Elicitation schemas should have additionalProperties=false for flat structure"
                    .to_string(),
                Some("additionalProperties".to_string()),
            );
        }

        // Validate properties
        for (key, prop) in &schema.properties {
            self.validate_primitive_schema(prop, &format!("properties.{}", key), &mut ctx);
        }

        ctx.into_result()
    }

    /// Validate primitive schema definition
    fn validate_primitive_schema(
        &self,
        schema: &crate::types::PrimitiveSchemaDefinition,
        field_path: &str,
        ctx: &mut ValidationContext,
    ) {
        use crate::types::PrimitiveSchemaDefinition;

        match schema {
            PrimitiveSchemaDefinition::String {
                enum_values,
                enum_names,
                format,
                ..
            } => {
                // Validate enum/enumNames length match (schema.json:679-708)
                if let (Some(values), Some(names)) = (enum_values, enum_names)
                    && values.len() != names.len()
                {
                    ctx.add_error(
                        "ENUM_NAMES_LENGTH_MISMATCH",
                        format!(
                            "enum and enumNames arrays must have equal length: {} vs {}",
                            values.len(),
                            names.len()
                        ),
                        Some(format!("{}.enumNames", field_path)),
                    );
                }

                // Validate format if present (schema.json:2244-2251)
                if let Some(fmt) = format {
                    let valid_formats = ["email", "uri", "date", "date-time"];
                    if !valid_formats.contains(&fmt.as_str()) {
                        ctx.add_warning(
                            "UNKNOWN_STRING_FORMAT",
                            format!(
                                "Unknown format '{}', expected one of: {:?}",
                                fmt, valid_formats
                            ),
                            Some(format!("{}.format", field_path)),
                        );
                    }
                }
            }
            PrimitiveSchemaDefinition::Number { .. }
            | PrimitiveSchemaDefinition::Integer { .. } => {
                // Number/Integer validation could go here
            }
            PrimitiveSchemaDefinition::Boolean { .. } => {
                // Boolean validation could go here
            }
        }
    }

    /// Validate string value against format constraints
    ///
    /// Validates email, uri, date, and date-time formats per MCP 2025-06-18 spec.
    pub fn validate_string_format(value: &str, format: &str) -> std::result::Result<(), String> {
        match format {
            "email" => {
                // RFC 5322 basic validation
                if !value.contains('@') || !value.contains('.') {
                    return Err(format!("Invalid email format: {}", value));
                }
                let parts: Vec<&str> = value.split('@').collect();
                if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                    return Err(format!("Invalid email format: {}", value));
                }
            }
            "uri" => {
                // Basic URI validation - must have a scheme
                if !value.contains("://") && !value.starts_with('/') {
                    return Err(format!("Invalid URI format: {}", value));
                }
            }
            "date" => {
                // ISO 8601 date format: YYYY-MM-DD
                let parts: Vec<&str> = value.split('-').collect();
                if parts.len() != 3 {
                    return Err("Date must be in ISO 8601 format (YYYY-MM-DD)".to_string());
                }
                if parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
                    return Err("Date must be in ISO 8601 format (YYYY-MM-DD)".to_string());
                }
                // Basic numeric check
                for part in parts {
                    if !part.chars().all(|c| c.is_ascii_digit()) {
                        return Err("Date components must be numeric".to_string());
                    }
                }
            }
            "date-time" => {
                // ISO 8601 datetime format: YYYY-MM-DDTHH:MM:SS[.sss][Z|±HH:MM]
                if !value.contains('T') {
                    return Err(
                        "DateTime must contain 'T' separator (ISO 8601 format)".to_string()
                    );
                }
                let parts: Vec<&str> = value.split('T').collect();
                if parts.len() != 2 {
                    return Err("DateTime must be in ISO 8601 format".to_string());
                }
                // Validate date part
                Self::validate_string_format(parts[0], "date")?;
                // Time part should have colons
                if !parts[1].contains(':') {
                    return Err("Time component must contain ':'".to_string());
                }
            }
            _ => {
                // Unknown formats don't fail validation (forward compatibility)
            }
        }
        Ok(())
    }

    // Private validation methods

    fn validate_jsonrpc_request(&self, request: &JsonRpcRequest, ctx: &mut ValidationContext) {
        // Validate JSON-RPC version (implicitly "2.0" via JsonRpcVersion type)
        // This is handled by type system during deserialization

        // Validate method name - check length first, then format
        if request.method.is_empty() {
            ctx.add_error(
                "EMPTY_METHOD_NAME",
                "Method name cannot be empty".to_string(),
                Some("method".to_string()),
            );
        } else if request.method.len() > self.rules.max_string_length {
            ctx.add_error(
                "METHOD_NAME_TOO_LONG",
                format!(
                    "Method name exceeds maximum length of {}",
                    self.rules.max_string_length
                ),
                Some("method".to_string()),
            );
        } else if !utils::is_valid_method_name(&request.method) {
            ctx.add_error(
                "INVALID_METHOD_NAME",
                format!("Invalid method name format: '{}'", request.method),
                Some("method".to_string()),
            );
        }

        // Validate parameters if present
        if let Some(ref params) = request.params {
            self.validate_parameters(params, ctx);
        }

        // Request ID is always present for requests (enforced by type system)
        // Validate ID format if needed
        self.validate_request_id(&request.id, ctx);
    }

    fn validate_jsonrpc_response(&self, response: &JsonRpcResponse, ctx: &mut ValidationContext) {
        // Validate JSON-RPC version (implicitly "2.0" via JsonRpcVersion type)
        // This is handled by type system during deserialization

        // Validate response has either result or error (enforced by type system)
        // Our JsonRpcResponsePayload enum ensures mutual exclusion

        // Validate response ID
        self.validate_response_id(&response.id, ctx);

        // Validate error if present
        if let Some(error) = response.error() {
            self.validate_jsonrpc_error(error, ctx);
        }

        // Validate result structure if present
        if let Some(result) = response.result() {
            self.validate_result_value(result, ctx);
        }
    }

    fn validate_jsonrpc_notification(
        &self,
        notification: &JsonRpcNotification,
        ctx: &mut ValidationContext,
    ) {
        // Validate JSON-RPC version (implicitly "2.0" via JsonRpcVersion type)
        // This is handled by type system during deserialization

        // Validate method name - check length first, then format
        if notification.method.is_empty() {
            ctx.add_error(
                "EMPTY_METHOD_NAME",
                "Method name cannot be empty".to_string(),
                Some("method".to_string()),
            );
        } else if notification.method.len() > self.rules.max_string_length {
            ctx.add_error(
                "METHOD_NAME_TOO_LONG",
                format!(
                    "Method name exceeds maximum length of {}",
                    self.rules.max_string_length
                ),
                Some("method".to_string()),
            );
        } else if !utils::is_valid_method_name(&notification.method) {
            ctx.add_error(
                "INVALID_METHOD_NAME",
                format!("Invalid method name format: '{}'", notification.method),
                Some("method".to_string()),
            );
        }

        // Validate parameters if present
        if let Some(ref params) = notification.params {
            self.validate_parameters(params, ctx);
        }

        // Notifications do NOT have an ID field (enforced by type system)
    }

    fn validate_jsonrpc_error(
        &self,
        error: &crate::jsonrpc::JsonRpcError,
        ctx: &mut ValidationContext,
    ) {
        // Error codes should be in the valid range
        if error.code >= 0 {
            ctx.add_warning(
                "POSITIVE_ERROR_CODE",
                "Error codes should be negative according to JSON-RPC spec".to_string(),
                Some("error.code".to_string()),
            );
        }

        if error.message.is_empty() {
            ctx.add_error(
                "EMPTY_ERROR_MESSAGE",
                "Error message cannot be empty".to_string(),
                Some("error.message".to_string()),
            );
        }
    }

    fn validate_method_name(&self, method: &str, ctx: &mut ValidationContext) {
        if method.is_empty() {
            ctx.add_error(
                "EMPTY_METHOD_NAME",
                "Method name cannot be empty".to_string(),
                Some("method".to_string()),
            );
            return;
        }

        if !self.rules.method_name_regex().is_match(method) {
            ctx.add_error(
                "INVALID_METHOD_NAME",
                format!("Invalid method name format: {method}"),
                Some("method".to_string()),
            );
        }
    }

    fn validate_method_params(&self, method: &str, params: &Value, ctx: &mut ValidationContext) {
        ctx.push_path("params".to_string());

        match method {
            "initialize" => self.validate_value_structure(params, "initialize", ctx),
            "tools/list" => {
                // Should be empty object or null
                if !params.is_null() && !params.as_object().is_some_and(|obj| obj.is_empty()) {
                    ctx.add_warning(
                        "UNEXPECTED_PARAMS",
                        "tools/list should not have parameters".to_string(),
                        None,
                    );
                }
            }
            "tools/call" => self.validate_value_structure(params, "call_tool", ctx),
            _ => {
                // Unknown method - validate basic structure
                self.validate_value_structure(params, "generic", ctx);
            }
        }

        ctx.pop_path();
    }

    fn validate_tool_input(&self, input: &ToolInputSchema, ctx: &mut ValidationContext) {
        ctx.push_path("inputSchema".to_string());

        // Validate schema type
        if input.schema_type != "object" {
            ctx.add_warning(
                "NON_OBJECT_SCHEMA",
                "Tool input schema should typically be 'object'".to_string(),
                Some("type".to_string()),
            );
        }

        ctx.pop_path();
    }

    fn validate_value_structure(
        &self,
        value: &Value,
        _expected_type: &str,
        ctx: &mut ValidationContext,
    ) {
        // Prevent infinite recursion
        if ctx.depth > self.rules.max_object_depth {
            ctx.add_error(
                "MAX_DEPTH_EXCEEDED",
                format!(
                    "Maximum object depth ({}) exceeded",
                    self.rules.max_object_depth
                ),
                None,
            );
            return;
        }

        match value {
            Value::Object(obj) => {
                ctx.depth += 1;
                for (key, val) in obj {
                    ctx.push_path(key.clone());
                    self.validate_value_structure(val, "unknown", ctx);
                    ctx.pop_path();
                }
                ctx.depth -= 1;
            }
            Value::Array(arr) => {
                if arr.len() > self.rules.max_array_length {
                    ctx.add_error(
                        "ARRAY_TOO_LONG",
                        format!(
                            "Array exceeds maximum length of {}",
                            self.rules.max_array_length
                        ),
                        None,
                    );
                }

                for (index, val) in arr.iter().enumerate() {
                    ctx.push_path(index.to_string());
                    self.validate_value_structure(val, "unknown", ctx);
                    ctx.pop_path();
                }
            }
            Value::String(s) => {
                if s.len() > self.rules.max_string_length {
                    ctx.add_error(
                        "STRING_TOO_LONG",
                        format!(
                            "String exceeds maximum length of {}",
                            self.rules.max_string_length
                        ),
                        None,
                    );
                }
            }
            _ => {} // Other types are fine
        }
    }

    fn validate_parameters(&self, params: &Value, ctx: &mut ValidationContext) {
        // Validate parameter structure depth and content
        self.validate_value_structure(params, "params", ctx);

        // Additional parameter-specific validation
        match params {
            Value::Array(arr) => {
                // Validate array parameters length
                if arr.len() > self.rules.max_array_length {
                    ctx.add_error(
                        "PARAMS_ARRAY_TOO_LONG",
                        format!(
                            "Parameter array exceeds maximum length of {}",
                            self.rules.max_array_length
                        ),
                        Some("params".to_string()),
                    );
                }
            }
            _ => {
                // Other parameter types are acceptable
            }
        }
    }

    fn validate_request_id(&self, _id: &crate::types::RequestId, _ctx: &mut ValidationContext) {
        // Request ID validation
        // ID is always present for requests (enforced by type system)
        // Additional ID format validation could be added here if needed
    }

    fn validate_response_id(&self, id: &crate::jsonrpc::ResponseId, _ctx: &mut ValidationContext) {
        // Validate response ID semantics
        if id.is_null() {
            // Null ID is only valid for parse errors
            // This should be checked at a higher level when the error type is known
        }
        // Additional response ID validation could be added here
    }

    fn validate_result_value(&self, result: &Value, ctx: &mut ValidationContext) {
        // Validate result structure depth and content
        self.validate_value_structure(result, "result", ctx);

        // Additional result validation based on method type could be added here
        // For now, we just validate general structure
    }
}

impl Default for ProtocolValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationContext {
    fn new() -> Self {
        Self {
            path: Vec::new(),
            depth: 0,
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn push_path(&mut self, segment: String) {
        self.path.push(segment);
    }

    fn pop_path(&mut self) {
        self.path.pop();
    }

    fn current_path(&self) -> Option<String> {
        if self.path.is_empty() {
            None
        } else {
            Some(self.path.join("."))
        }
    }

    fn add_error(&mut self, code: &str, message: String, field_path: Option<String>) {
        let path = field_path.or_else(|| self.current_path());
        self.errors.push(ValidationError {
            code: code.to_string(),
            message,
            field_path: path,
        });
    }

    fn add_warning(&mut self, code: &str, message: String, field_path: Option<String>) {
        let path = field_path.or_else(|| self.current_path());
        self.warnings.push(ValidationWarning {
            code: code.to_string(),
            message,
            field_path: path,
        });
    }

    fn into_result(self) -> ValidationResult {
        if !self.errors.is_empty() {
            ValidationResult::Invalid(self.errors)
        } else if !self.warnings.is_empty() {
            ValidationResult::ValidWithWarnings(self.warnings)
        } else {
            ValidationResult::Valid
        }
    }
}

impl ValidationResult {
    /// Check if validation passed (with or without warnings)
    pub fn is_valid(&self) -> bool {
        !matches!(self, ValidationResult::Invalid(_))
    }

    /// Check if validation failed
    pub fn is_invalid(&self) -> bool {
        matches!(self, ValidationResult::Invalid(_))
    }

    /// Check if validation has warnings
    pub fn has_warnings(&self) -> bool {
        matches!(self, ValidationResult::ValidWithWarnings(_))
    }

    /// Get warnings (if any)
    pub fn warnings(&self) -> &[ValidationWarning] {
        match self {
            ValidationResult::ValidWithWarnings(warnings) => warnings,
            _ => &[],
        }
    }

    /// Get errors (if any)
    pub fn errors(&self) -> &[ValidationError] {
        match self {
            ValidationResult::Invalid(errors) => errors,
            _ => &[],
        }
    }
}

/// Utility functions for validation
pub mod utils {
    use super::*;

    /// Create a validation error
    pub fn error(code: &str, message: &str) -> ValidationError {
        ValidationError {
            code: code.to_string(),
            message: message.to_string(),
            field_path: None,
        }
    }

    /// Create a validation warning
    pub fn warning(code: &str, message: &str) -> ValidationWarning {
        ValidationWarning {
            code: code.to_string(),
            message: message.to_string(),
            field_path: None,
        }
    }

    /// Check if a string is a valid URI
    pub fn is_valid_uri(uri: &str) -> bool {
        ValidationRules::default().uri_regex().is_match(uri)
    }

    /// Check if a string is a valid method name
    pub fn is_valid_method_name(method: &str) -> bool {
        ValidationRules::default()
            .method_name_regex()
            .is_match(method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::JsonRpcVersion;
    // use serde_json::json;

    #[test]
    fn test_tool_validation() {
        let validator = ProtocolValidator::new();

        let tool = Tool {
            name: "test_tool".to_string(),
            title: Some("Test Tool".to_string()),
            description: Some("A test tool".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
                additional_properties: None,
            },
            output_schema: None,
            annotations: None,
            meta: None,
        };

        let result = validator.validate_tool(&tool);
        assert!(result.is_valid());

        // Test empty name
        let invalid_tool = Tool {
            name: String::new(),
            title: None,
            description: None,
            input_schema: tool.input_schema.clone(),
            output_schema: None,
            annotations: None,
            meta: None,
        };

        let result = validator.validate_tool(&invalid_tool);
        assert!(result.is_invalid());
    }

    #[test]
    fn test_request_validation() {
        let validator = ProtocolValidator::new();

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "tools/list".to_string(),
            params: None,
            id: RequestId::String("test-id".to_string()),
        };

        let result = validator.validate_request(&request);
        assert!(result.is_valid());

        // Test invalid method name
        let invalid_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: String::new(),
            params: None,
            id: RequestId::String("test-id".to_string()),
        };

        let result = validator.validate_request(&invalid_request);
        assert!(result.is_invalid());
    }

    #[test]
    fn test_initialize_validation() {
        let validator = ProtocolValidator::new();

        let request = InitializeRequest {
            protocol_version: "2025-06-18".to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "test-client".to_string(),
                title: Some("Test Client".to_string()),
                version: "1.0.0".to_string(),
            },
            _meta: None,
        };

        let result = validator.validate_initialize_request(&request);
        assert!(result.is_valid());

        // Test unsupported version (should warn, not error)
        let request_with_old_version = InitializeRequest {
            protocol_version: "2023-01-01".to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "test-client".to_string(),
                title: Some("Test Client".to_string()),
                version: "1.0.0".to_string(),
            },
            _meta: None,
        };

        let result = validator.validate_initialize_request(&request_with_old_version);
        assert!(result.is_valid()); // Valid but with warnings
        assert!(result.has_warnings());
    }

    #[test]
    fn test_validation_result() {
        let valid = ValidationResult::Valid;
        assert!(valid.is_valid());
        assert!(!valid.is_invalid());
        assert!(!valid.has_warnings());

        let warnings = vec![utils::warning("TEST", "Test warning")];
        let valid_with_warnings = ValidationResult::ValidWithWarnings(warnings.clone());
        assert!(valid_with_warnings.is_valid());
        assert!(valid_with_warnings.has_warnings());
        assert_eq!(valid_with_warnings.warnings(), &warnings);

        let errors = vec![utils::error("TEST", "Test error")];
        let invalid = ValidationResult::Invalid(errors.clone());
        assert!(!invalid.is_valid());
        assert!(invalid.is_invalid());
        assert_eq!(invalid.errors(), &errors);
    }

    #[test]
    fn test_utils() {
        assert!(utils::is_valid_uri("file://test.txt"));
        assert!(utils::is_valid_uri("https://example.com"));
        assert!(!utils::is_valid_uri("not-a-uri"));

        assert!(utils::is_valid_method_name("tools/list"));
        assert!(utils::is_valid_method_name("initialize"));
        assert!(!utils::is_valid_method_name("invalid-method-name!"));
    }
}
