//! Parameter validation and schema generation system
//!
//! This module provides comprehensive validation capabilities for `TurboMCP` servers,
//! including automatic parameter validation, JSON schema generation, and custom
//! validation rules.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{McpError, McpResult};

#[cfg(feature = "schema-generation")]
use schemars::{JsonSchema, schema_for};

/// Validation error details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Field path (e.g., "user.email")
    pub field: String,
    /// Error message
    pub message: String,
    /// Expected value or type
    pub expected: Option<String>,
    /// Actual value received
    pub actual: Option<Value>,
    /// Validation rule that failed
    pub rule: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Validation failed for '{}': {}",
            self.field, self.message
        )
    }
}

/// Collection of validation errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationErrors {
    /// List of individual validation errors
    pub errors: Vec<ValidationError>,
}

impl ValidationErrors {
    /// Create a new empty validation errors collection
    #[must_use]
    pub const fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Add a validation error
    pub fn add_error(&mut self, error: ValidationError) {
        self.errors.push(error);
    }

    /// Check if there are any validation errors
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get the number of validation errors
    #[must_use]
    pub fn len(&self) -> usize {
        self.errors.len()
    }
}

impl Default for ValidationErrors {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.errors.is_empty() {
            write!(f, "No validation errors")
        } else {
            write!(f, "Validation errors:")?;
            for error in &self.errors {
                write!(f, "\n  - {error}")?;
            }
            Ok(())
        }
    }
}

/// Validation rule trait
pub trait ValidationRule: Send + Sync {
    /// Validate a value
    fn validate(&self, field: &str, value: &Value) -> Result<(), Box<ValidationError>>;

    /// Get the rule name
    fn name(&self) -> &'static str;
}

/// Required field validation rule
#[derive(Debug, Clone)]
pub struct RequiredRule;

impl ValidationRule for RequiredRule {
    fn validate(&self, field: &str, value: &Value) -> Result<(), Box<ValidationError>> {
        match value {
            Value::Null => Err(Box::new(ValidationError {
                field: field.to_string(),
                message: "Field is required".to_string(),
                expected: Some("non-null value".to_string()),
                actual: Some(value.clone()),
                rule: self.name().to_string(),
            })),
            _ => Ok(()),
        }
    }

    fn name(&self) -> &'static str {
        "required"
    }
}

/// String length validation rule
#[derive(Debug, Clone)]
pub struct StringLengthRule {
    /// Minimum length (inclusive)
    pub min: Option<usize>,
    /// Maximum length (inclusive)
    pub max: Option<usize>,
}

impl StringLengthRule {
    /// Create a new string length rule
    #[must_use]
    pub const fn new(min: Option<usize>, max: Option<usize>) -> Self {
        Self { min, max }
    }

    /// Create a minimum length rule
    #[must_use]
    pub const fn min(length: usize) -> Self {
        Self::new(Some(length), None)
    }

    /// Create a maximum length rule
    #[must_use]
    pub const fn max(length: usize) -> Self {
        Self::new(None, Some(length))
    }

    /// Create a range length rule
    #[must_use]
    pub const fn range(min: usize, max: usize) -> Self {
        Self::new(Some(min), Some(max))
    }
}

impl ValidationRule for StringLengthRule {
    fn validate(&self, field: &str, value: &Value) -> Result<(), Box<ValidationError>> {
        if let Value::String(s) = value {
            let len = s.len();

            if let Some(min) = self.min
                && len < min
            {
                return Err(Box::new(ValidationError {
                    field: field.to_string(),
                    message: format!("String must be at least {min} characters long"),
                    expected: Some(format!("length >= {min}")),
                    actual: Some(Value::Number(len.into())),
                    rule: self.name().to_string(),
                }));
            }

            if let Some(max) = self.max
                && len > max
            {
                return Err(Box::new(ValidationError {
                    field: field.to_string(),
                    message: format!("String must be at most {max} characters long"),
                    expected: Some(format!("length <= {max}")),
                    actual: Some(Value::Number(len.into())),
                    rule: self.name().to_string(),
                }));
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "string_length"
    }
}

/// Numeric range validation rule
#[derive(Debug, Clone)]
pub struct NumericRangeRule {
    /// Minimum value (inclusive)
    pub min: Option<f64>,
    /// Maximum value (inclusive)
    pub max: Option<f64>,
}

impl NumericRangeRule {
    /// Create a new numeric range rule
    #[must_use]
    pub const fn new(min: Option<f64>, max: Option<f64>) -> Self {
        Self { min, max }
    }

    /// Create a minimum value rule
    #[must_use]
    pub const fn min(value: f64) -> Self {
        Self::new(Some(value), None)
    }

    /// Create a maximum value rule
    #[must_use]
    pub const fn max(value: f64) -> Self {
        Self::new(None, Some(value))
    }

    /// Create a range rule
    #[must_use]
    pub const fn range(min: f64, max: f64) -> Self {
        Self::new(Some(min), Some(max))
    }
}

impl ValidationRule for NumericRangeRule {
    fn validate(&self, field: &str, value: &Value) -> Result<(), Box<ValidationError>> {
        if let Some(num) = value.as_f64() {
            if let Some(min) = self.min
                && num < min
            {
                return Err(Box::new(ValidationError {
                    field: field.to_string(),
                    message: format!("Value must be at least {min}"),
                    expected: Some(format!(">= {min}")),
                    actual: Some(value.clone()),
                    rule: self.name().to_string(),
                }));
            }

            if let Some(max) = self.max
                && num > max
            {
                return Err(Box::new(ValidationError {
                    field: field.to_string(),
                    message: format!("Value must be at most {max}"),
                    expected: Some(format!("<= {max}")),
                    actual: Some(value.clone()),
                    rule: self.name().to_string(),
                }));
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "numeric_range"
    }
}

/// Email validation rule
#[derive(Debug, Clone)]
pub struct EmailRule;

impl ValidationRule for EmailRule {
    fn validate(&self, field: &str, value: &Value) -> Result<(), Box<ValidationError>> {
        if let Value::String(s) = value {
            // Basic email validation (in production, use a proper regex or validation library)
            if !s.contains('@') || !s.contains('.') {
                return Err(Box::new(ValidationError {
                    field: field.to_string(),
                    message: "Invalid email format".to_string(),
                    expected: Some("valid email address".to_string()),
                    actual: Some(value.clone()),
                    rule: self.name().to_string(),
                }));
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "email"
    }
}

/// Pattern matching validation rule
#[derive(Debug, Clone)]
pub struct PatternRule {
    /// Regular expression pattern
    pub pattern: String,
    /// Human-readable description of the pattern
    pub description: Option<String>,
}

impl PatternRule {
    /// Create a new pattern rule
    pub fn new<S: Into<String>>(pattern: S) -> Self {
        Self {
            pattern: pattern.into(),
            description: None,
        }
    }

    /// Create a pattern rule with description
    pub fn with_description<S: Into<String>, D: Into<String>>(pattern: S, description: D) -> Self {
        Self {
            pattern: pattern.into(),
            description: Some(description.into()),
        }
    }
}

impl ValidationRule for PatternRule {
    fn validate(&self, field: &str, value: &Value) -> Result<(), Box<ValidationError>> {
        if let Value::String(s) = value {
            #[cfg(feature = "uri-templates")]
            {
                use regex::Regex;
                let regex = Regex::new(&self.pattern).map_err(|_| {
                    Box::new(ValidationError {
                        field: field.to_string(),
                        message: "Invalid regex pattern".to_string(),
                        expected: None,
                        actual: None,
                        rule: self.name().to_string(),
                    })
                })?;

                if !regex.is_match(s) {
                    return Err(Box::new(ValidationError {
                        field: field.to_string(),
                        message: format!(
                            "Value does not match required pattern{}",
                            self.description
                                .as_ref()
                                .map(|d| format!(": {d}"))
                                .unwrap_or_default()
                        ),
                        expected: Some(self.pattern.clone()),
                        actual: Some(value.clone()),
                        rule: self.name().to_string(),
                    }));
                }
            }

            #[cfg(not(feature = "uri-templates"))]
            {
                // Basic pattern matching without regex - just check for simple patterns
                match self.pattern.as_str() {
                    r"^\d+$" => {
                        if !s.chars().all(|c| c.is_ascii_digit()) {
                            return Err(Box::new(ValidationError {
                                field: field.to_string(),
                                message: "Value must contain only digits".to_string(),
                                expected: Some("digits only".to_string()),
                                actual: Some(value.clone()),
                                rule: self.name().to_string(),
                            }));
                        }
                    }
                    _ => {
                        // For other patterns, just accept them (regex feature not enabled)
                    }
                }
            }
        }

        Ok(())
    }

    fn name(&self) -> &'static str {
        "pattern"
    }
}

/// Field validator that can have multiple rules
pub struct FieldValidator {
    /// Field name
    pub field: String,
    /// Validation rules
    pub rules: Vec<Box<dyn ValidationRule>>,
}

impl std::fmt::Debug for FieldValidator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldValidator")
            .field("field", &self.field)
            .field("rules", &format!("{} rules", self.rules.len()))
            .finish()
    }
}

impl FieldValidator {
    /// Create a new field validator
    pub fn new<S: Into<String>>(field: S) -> Self {
        Self {
            field: field.into(),
            rules: Vec::new(),
        }
    }

    /// Add a validation rule
    #[must_use]
    pub fn add_rule(mut self, rule: Box<dyn ValidationRule>) -> Self {
        self.rules.push(rule);
        self
    }

    /// Add required rule
    #[must_use]
    pub fn required(self) -> Self {
        self.add_rule(Box::new(RequiredRule))
    }

    /// Add string length rule
    #[must_use]
    pub fn string_length(self, min: Option<usize>, max: Option<usize>) -> Self {
        self.add_rule(Box::new(StringLengthRule::new(min, max)))
    }

    /// Add numeric range rule
    #[must_use]
    pub fn numeric_range(self, min: Option<f64>, max: Option<f64>) -> Self {
        self.add_rule(Box::new(NumericRangeRule::new(min, max)))
    }

    /// Add email rule
    #[must_use]
    pub fn email(self) -> Self {
        self.add_rule(Box::new(EmailRule))
    }

    /// Add pattern rule
    pub fn pattern<S: Into<String>>(self, pattern: S) -> Self {
        self.add_rule(Box::new(PatternRule::new(pattern)))
    }

    /// Validate a value against all rules
    pub fn validate(&self, value: &Value) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        for rule in &self.rules {
            if let Err(error) = rule.validate(&self.field, value) {
                errors.add_error(*error);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Schema-aware validator
pub struct SchemaValidator {
    /// Field validators
    pub validators: HashMap<String, FieldValidator>,
}

impl std::fmt::Debug for SchemaValidator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SchemaValidator")
            .field(
                "validators",
                &format!("{} validators", self.validators.len()),
            )
            .finish()
    }
}

impl SchemaValidator {
    /// Create a new schema validator
    #[must_use]
    pub fn new() -> Self {
        Self {
            validators: HashMap::new(),
        }
    }

    /// Add a field validator
    #[must_use]
    pub fn add_field(mut self, validator: FieldValidator) -> Self {
        self.validators.insert(validator.field.clone(), validator);
        self
    }

    /// Validate a JSON object
    pub fn validate(&self, value: &Value) -> Result<(), ValidationErrors> {
        let mut all_errors = ValidationErrors::new();

        if let Value::Object(obj) = value {
            for (field_name, validator) in &self.validators {
                let field_value = obj.get(field_name).unwrap_or(&Value::Null);

                if let Err(mut errors) = validator.validate(field_value) {
                    all_errors.errors.append(&mut errors.errors);
                }
            }
        } else {
            all_errors.add_error(ValidationError {
                field: "root".to_string(),
                message: "Expected object".to_string(),
                expected: Some("object".to_string()),
                actual: Some(value.clone()),
                rule: "type".to_string(),
            });
        }

        if all_errors.is_empty() {
            Ok(())
        } else {
            Err(all_errors)
        }
    }
}

impl Default for SchemaValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Parameter extraction and validation
pub fn extract_and_validate<T>(
    params: &Map<String, Value>,
    validator: Option<&SchemaValidator>,
) -> McpResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    let value = Value::Object(params.clone());

    // Validate if validator is provided
    if let Some(validator) = validator {
        validator
            .validate(&value)
            .map_err(|errors| McpError::Tool(format!("Parameter validation failed: {errors}")))?;
    }

    // Deserialize
    serde_json::from_value(value)
        .map_err(|e| McpError::Tool(format!("Parameter deserialization failed: {e}")))
}

/// Generate JSON schema for a type
#[cfg(feature = "schema-generation")]
#[must_use]
pub fn generate_parameter_schema<T: JsonSchema>() -> Value {
    let schema = schema_for!(T);
    serde_json::to_value(schema).unwrap_or_else(|_| serde_json::json!({}))
}

/// Fallback schema generation
#[cfg(not(feature = "schema-generation"))]
pub fn generate_parameter_schema<T>() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    })
}

/// Validation builder for creating validators fluently
pub struct ValidationBuilder {
    validators: HashMap<String, FieldValidator>,
}

impl std::fmt::Debug for ValidationBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidationBuilder")
            .field(
                "validators",
                &format!("{} validators", self.validators.len()),
            )
            .finish()
    }
}

impl ValidationBuilder {
    /// Create a new validation builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            validators: HashMap::new(),
        }
    }

    /// Add field validation
    pub fn field<S: Into<String>>(self, name: S) -> FieldValidatorBuilder {
        let field_name = name.into();
        FieldValidatorBuilder {
            field_name: field_name.clone(),
            validator: FieldValidator::new(field_name),
            builder: self,
        }
    }

    /// Build the schema validator
    #[must_use]
    pub fn build(self) -> SchemaValidator {
        SchemaValidator {
            validators: self.validators,
        }
    }
}

impl Default for ValidationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Field validator builder for fluent API
pub struct FieldValidatorBuilder {
    field_name: String,
    validator: FieldValidator,
    builder: ValidationBuilder,
}

impl FieldValidatorBuilder {
    /// Mark field as required
    #[must_use]
    pub fn required(mut self) -> Self {
        self.validator = self.validator.required();
        self
    }

    /// Add string length validation
    #[must_use]
    pub fn string_length(mut self, min: Option<usize>, max: Option<usize>) -> Self {
        self.validator = self.validator.string_length(min, max);
        self
    }

    /// Add email validation
    #[must_use]
    pub fn email(mut self) -> Self {
        self.validator = self.validator.email();
        self
    }

    /// Add pattern validation
    pub fn pattern<S: Into<String>>(mut self, pattern: S) -> Self {
        self.validator = self.validator.pattern(pattern);
        self
    }

    /// Add numeric range validation
    #[must_use]
    pub fn numeric_range(mut self, min: Option<f64>, max: Option<f64>) -> Self {
        self.validator = self.validator.numeric_range(min, max);
        self
    }

    /// Finish field validation and return to builder
    #[must_use]
    pub fn and(mut self) -> ValidationBuilder {
        self.builder
            .validators
            .insert(self.field_name, self.validator);
        self.builder
    }

    /// Build the final validator
    #[must_use]
    pub fn build(mut self) -> SchemaValidator {
        self.builder
            .validators
            .insert(self.field_name, self.validator);
        self.builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_required_validation() {
        let validator = FieldValidator::new("name").required();

        // Should fail for null
        assert!(validator.validate(&Value::Null).is_err());

        // Should pass for string
        assert!(validator.validate(&json!("test")).is_ok());
    }

    #[test]
    fn test_string_length_validation() {
        let validator = FieldValidator::new("name").string_length(Some(3), Some(10));

        // Should fail for too short
        assert!(validator.validate(&json!("hi")).is_err());

        // Should fail for too long
        assert!(validator.validate(&json!("this is way too long")).is_err());

        // Should pass for correct length
        assert!(validator.validate(&json!("hello")).is_ok());
    }

    #[test]
    fn test_email_validation() {
        let validator = FieldValidator::new("email").email();

        // Should fail for invalid email
        assert!(validator.validate(&json!("invalid")).is_err());

        // Should pass for valid email
        assert!(validator.validate(&json!("user@example.com")).is_ok());
    }

    #[test]
    fn test_schema_validator() {
        let validator = ValidationBuilder::new()
            .field("name")
            .required()
            .string_length(Some(2), Some(50))
            .and()
            .field("email")
            .required()
            .email()
            .and()
            .field("age")
            .numeric_range(Some(0.0), Some(120.0))
            .and()
            .build();

        // Valid object
        let valid_data = json!({
            "name": "John Doe",
            "email": "john@example.com",
            "age": 30
        });
        assert!(validator.validate(&valid_data).is_ok());

        // Invalid object
        let invalid_data = json!({
            "name": "A", // too short
            "email": "invalid", // invalid email
            "age": 150 // too old
        });
        assert!(validator.validate(&invalid_data).is_err());
    }

    #[test]
    fn test_fluent_validation_builder() {
        let validator = ValidationBuilder::new()
            .field("username")
            .required()
            .string_length(Some(3), Some(20))
            .pattern(r"^[a-zA-Z0-9_]+$")
            .and()
            .field("password")
            .required()
            .string_length(Some(8), None)
            .build();

        let valid_data = json!({
            "username": "john_doe123",
            "password": "securepassword"
        });
        assert!(validator.validate(&valid_data).is_ok());
    }

    // Property-based tests for  coverage
    mod proptest_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Test that range validation correctly accepts values within bounds
            #[test]
            fn test_range_validation_within_bounds(
                value in 0.0..150.0
            ) {
                let validator = FieldValidator::new("age")
                    .numeric_range(Some(0.0), Some(150.0));

                let result = validator.validate(&json!(value));
                prop_assert!(result.is_ok());
            }

            /// Test that range validation correctly rejects values outside bounds
            #[test]
            fn test_range_validation_out_of_bounds_high(
                value in 151.0..1000.0
            ) {
                let validator = FieldValidator::new("age")
                    .numeric_range(Some(0.0), Some(150.0));

                let result = validator.validate(&json!(value));
                prop_assert!(result.is_err());
            }

            /// Test that string length validation accepts valid lengths
            #[test]
            fn test_string_length_validation(
                s in "[a-zA-Z]{5,20}"
            ) {
                let validator = FieldValidator::new("username")
                    .string_length(Some(5), Some(20));

                let result = validator.validate(&json!(s));
                prop_assert!(result.is_ok());
            }

            /// Test that email validation correctly identifies valid patterns
            #[test]
            fn test_email_pattern_validation(
                local in "[a-z]{3,10}",
                domain in "[a-z]{3,10}"
            ) {
                let email = format!("{}@{}.com", local, domain);
                let validator = FieldValidator::new("email").email();

                let result = validator.validate(&json!(email));
                prop_assert!(result.is_ok());
            }

            /// Test that pattern validation works for various strings
            #[test]
            fn test_pattern_validation_alphanumeric(
                s in "[a-zA-Z0-9]{5,15}"
            ) {
                let validator = FieldValidator::new("code")
                    .pattern(r"^[a-zA-Z0-9]+$");

                let result = validator.validate(&json!(s));
                prop_assert!(result.is_ok());
            }
        }
    }
}
