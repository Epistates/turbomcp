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
//! - [`BaseMetadata`] - Common name/title structure
//! - [`Implementation`] - Implementation information
//! - [`Annotations`] - Common annotation structure
//! - [`Role`] - Message role enum (User/Assistant)
//! - [`JsonRpcError`] - JSON-RPC error structure
//! - [`Timestamp`] - UTC timestamp wrapper

use crate::MessageId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt};

/// Timestamp wrapper for consistent time handling
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Timestamp(pub DateTime<Utc>);

impl Timestamp {
    /// Create a new timestamp with current time
    #[must_use]
    pub fn now() -> Self {
        Self(Utc::now())
    }

    /// Create a timestamp from a DateTime
    #[must_use]
    pub const fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt)
    }

    /// Get the inner DateTime
    #[must_use]
    pub const fn datetime(&self) -> DateTime<Utc> {
        self.0
    }

    /// Get duration since this timestamp
    #[must_use]
    pub fn elapsed(&self) -> chrono::Duration {
        Utc::now() - self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.to_rfc3339())
    }
}

impl From<DateTime<Utc>> for Timestamp {
    fn from(dt: DateTime<Utc>) -> Self {
        Self(dt)
    }
}

/// Protocol version string
pub type ProtocolVersion = String;

/// JSON-RPC request identifier
pub type RequestId = MessageId;

/// URI string (legacy type alias)
///
/// **Note**: For new code, consider using the validated [`crate::types::domain::Uri`] type
/// which provides compile-time type safety and runtime validation.
/// This type alias is kept for backward compatibility.
pub type Uri = String;

/// MIME type (legacy type alias)
///
/// **Note**: For new code, consider using the validated [`crate::types::domain::MimeType`] type
/// which provides compile-time type safety and runtime validation.
/// This type alias is kept for backward compatibility.
pub type MimeType = String;

/// Base64 encoded data (legacy type alias)
///
/// **Note**: For new code, consider using the validated [`crate::types::domain::Base64String`] type
/// which provides compile-time type safety and runtime validation.
/// This type alias is kept for backward compatibility.
pub type Base64String = String;

/// Cursor for pagination
pub type Cursor = String;

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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
