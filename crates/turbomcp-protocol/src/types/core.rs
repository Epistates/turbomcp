//! Core protocol types and utilities
//!
//! This module contains the fundamental types used throughout the MCP protocol
//! implementation. These types are shared across multiple protocol features
//! and provide the foundational building blocks for the protocol.
//!
//! # Core Types
//!
//! - [`ProtocolVersion`] - Protocol version identifier
//! - [`RequestId`] - JSON-RPC request identifier
//! - [`ProgressToken`] - Progress tracking token (string or integer)
//! - [`BaseMetadata`] - Common name/title structure
//! - [`Implementation`] - Implementation information
//! - [`Annotations`] - Common annotation structure
//! - [`Role`] - Message role enum (User/Assistant)
//! - [`JsonRpcError`] - JSON-RPC error structure

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use turbomcp_core::MessageId;

/// Protocol version string
pub type ProtocolVersion = String;

/// JSON-RPC request identifier
pub type RequestId = MessageId;

/// URI string
pub type Uri = String;

/// MIME type
pub type MimeType = String;

/// Base64 encoded data
pub type Base64String = String;

/// Cursor for pagination
pub type Cursor = String;

/// Progress token for tracking long-running operations
///
/// Per MCP 2025-06-18 specification, progress tokens can be either strings or integers
/// to provide flexibility in how clients and servers track long-running operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProgressToken {
    /// String-based progress token
    String(String),
    /// Integer-based progress token
    Integer(i64),
}

impl ProgressToken {
    /// Create a new string-based progress token
    pub fn from_string<S: Into<String>>(s: S) -> Self {
        Self::String(s.into())
    }

    /// Create a new integer-based progress token
    pub fn from_integer(i: i64) -> Self {
        Self::Integer(i)
    }

    /// Check if this is a string token
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    /// Check if this is an integer token
    pub fn is_integer(&self) -> bool {
        matches!(self, Self::Integer(_))
    }

    /// Get the string value if this is a string token
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            Self::Integer(_) => None,
        }
    }

    /// Get the integer value if this is an integer token
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::String(_) => None,
            Self::Integer(i) => Some(*i),
        }
    }

    /// Convert to a string representation for display/logging
    pub fn to_display_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Integer(i) => i.to_string(),
        }
    }
}

impl std::fmt::Display for ProgressToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => write!(f, "{}", s),
            Self::Integer(i) => write!(f, "{}", i),
        }
    }
}

impl From<String> for ProgressToken {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for ProgressToken {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<i64> for ProgressToken {
    fn from(i: i64) -> Self {
        Self::Integer(i)
    }
}

impl From<i32> for ProgressToken {
    fn from(i: i32) -> Self {
        Self::Integer(i64::from(i))
    }
}

impl From<u32> for ProgressToken {
    fn from(i: u32) -> Self {
        Self::Integer(i64::from(i))
    }
}

/// Standard JSON-RPC error codes per specification
pub mod error_codes {
    /// Parse error - Invalid JSON was received by the server
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid Request - The JSON sent is not a valid Request object
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found - The method does not exist / is not available
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params - Invalid method parameter(s)
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error - Internal JSON-RPC error
    pub const INTERNAL_ERROR: i32 = -32603;
}

/// JSON-RPC error structure per MCP 2025-06-18 specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcError {
    /// The error type that occurred
    pub code: i32,
    /// A short description of the error (should be limited to a concise single sentence)
    pub message: String,
    /// Additional information about the error (detailed error information, nested errors, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// Create a new JSON-RPC error
    pub fn new(code: i32, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// Create a new JSON-RPC error with additional data
    pub fn with_data(code: i32, message: String, data: serde_json::Value) -> Self {
        Self {
            code,
            message,
            data: Some(data),
        }
    }

    /// Create a parse error
    pub fn parse_error() -> Self {
        Self::new(error_codes::PARSE_ERROR, "Parse error".to_string())
    }

    /// Create an invalid request error
    pub fn invalid_request() -> Self {
        Self::new(error_codes::INVALID_REQUEST, "Invalid Request".to_string())
    }

    /// Create a method not found error
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {method}"),
        )
    }

    /// Create an invalid params error
    pub fn invalid_params(details: &str) -> Self {
        Self::new(
            error_codes::INVALID_PARAMS,
            format!("Invalid params: {details}"),
        )
    }

    /// Create an internal error
    pub fn internal_error(details: &str) -> Self {
        Self::new(
            error_codes::INTERNAL_ERROR,
            format!("Internal error: {details}"),
        )
    }
}

/// Base interface for metadata with name (identifier) and title (display name) properties.
/// Per MCP specification 2025-06-18, this is the foundation for Tool, Resource, and Prompt metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseMetadata {
    /// Intended for programmatic or logical use, but used as a display name in past specs or fallback (if title isn't present).
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    ///
    /// If not provided, the name should be used for display (except for Tool,
    /// where `annotations.title` should be given precedence over using `name`, if present).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Implementation information for MCP clients and servers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    /// Implementation name
    pub name: String,
    /// Implementation display title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Implementation version
    pub version: String,
}

/// General annotations that can be attached to various MCP objects
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Annotations {
    /// Audience-specific hints or information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<String>>,
    /// Priority level for ordering or importance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// The moment the resource was last modified, as an ISO 8601 formatted string
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "lastModified")]
    pub last_modified: Option<String>,
    /// Additional custom annotations
    #[serde(flatten)]
    pub custom: HashMap<String, serde_json::Value>,
}

/// Role in conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// User role
    User,
    /// Assistant role
    Assistant,
}

/// Base result type for MCP protocol responses
///
/// Per MCP 2025-06-18 specification, all result types should support
/// optional metadata in the `_meta` field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Result {
    /// Optional metadata per MCP 2025-06-18 specification
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<serde_json::Value>,
}

impl Result {
    /// Create a new result with no metadata
    pub fn new() -> Self {
        Self { _meta: None }
    }

    /// Create a result with metadata
    pub fn with_meta(meta: serde_json::Value) -> Self {
        Self { _meta: Some(meta) }
    }

    /// Add metadata to this result
    pub fn set_meta(&mut self, meta: serde_json::Value) {
        self._meta = Some(meta);
    }
}

impl Default for Result {
    fn default() -> Self {
        Self::new()
    }
}

/// A response that indicates success but carries no data
///
/// Per MCP 2025-06-18 specification, this is simply a Result with no additional fields.
/// This is used for operations where the success of the operation itself
/// is the only meaningful response, such as ping responses.
pub type EmptyResult = Result;

/// Hints to use for model selection
///
/// Keys not declared here are currently left unspecified by the spec and are up
/// to the client to decide how to interpret.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelHint {
    /// Optional model name hint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
