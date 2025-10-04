//! Validated domain types
//!
//! This module provides newtype wrappers around string types to add
//! compile-time type safety and runtime validation for domain-specific
//! values like URIs, MIME types, and Base64 strings.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A validated URI string
///
/// URIs must follow the format: `scheme:path` where scheme contains only
/// alphanumeric characters, plus, period, or hyphen.
///
/// # Examples
///
/// ```
/// use turbomcp_protocol::types::domain::Uri;
///
/// // Valid URIs
/// let uri1 = Uri::new("file:///path/to/file.txt").unwrap();
/// let uri2 = Uri::new("https://example.com").unwrap();
/// let uri3 = Uri::new("resource://my-resource").unwrap();
///
/// // Invalid URI (no scheme)
/// assert!(Uri::new("not-a-uri").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Uri(String);

impl Uri {
    /// Create a new URI with validation
    ///
    /// # Errors
    ///
    /// Returns an error if the URI format is invalid (missing scheme separator ':')
    pub fn new<S: Into<String>>(uri: S) -> Result<Self, UriError> {
        let uri_string = uri.into();

        // Validate URI format: must have scheme:path structure
        if !uri_string.contains(':') {
            return Err(UriError::MissingScheme(uri_string));
        }

        // Extract scheme and validate it contains only valid characters
        if let Some(scheme_end) = uri_string.find(':') {
            let scheme = &uri_string[..scheme_end];

            if scheme.is_empty() {
                return Err(UriError::EmptyScheme(uri_string));
            }

            // Scheme must start with letter and contain only alphanumeric, +, ., -
            if !scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            {
                return Err(UriError::InvalidScheme(uri_string));
            }

            if !scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
            {
                return Err(UriError::InvalidScheme(uri_string));
            }
        }

        Ok(Self(uri_string))
    }

    /// Create a URI without validation (use with caution)
    ///
    /// This should only be used when you're certain the URI is valid,
    /// such as when deserializing from a trusted source.
    #[must_use]
    pub fn new_unchecked<S: Into<String>>(uri: S) -> Self {
        Self(uri.into())
    }

    /// Get the URI as a string slice
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Get the scheme portion of the URI
    ///
    /// # Examples
    ///
    /// ```
    /// use turbomcp_protocol::types::domain::Uri;
    ///
    /// let uri = Uri::new("https://example.com").unwrap();
    /// assert_eq!(uri.scheme(), Some("https"));
    /// ```
    #[must_use]
    pub fn scheme(&self) -> Option<&str> {
        self.0.split(':').next()
    }

    /// Convert into the inner String
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for Uri {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<Uri> for String {
    fn from(uri: Uri) -> Self {
        uri.0
    }
}

/// MIME type string
///
/// Represents a media type in the format `type/subtype` with optional parameters.
///
/// # Examples
///
/// ```
/// use turbomcp_protocol::types::domain::MimeType;
///
/// let mime = MimeType::new("text/plain").unwrap();
/// assert_eq!(mime.type_part(), Some("text"));
/// assert_eq!(mime.subtype(), Some("plain"));
///
/// let mime_with_params = MimeType::new("text/html; charset=utf-8").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MimeType(String);

impl MimeType {
    /// Create a new MIME type with validation
    ///
    /// # Errors
    ///
    /// Returns an error if the MIME type format is invalid
    pub fn new<S: Into<String>>(mime: S) -> Result<Self, MimeTypeError> {
        let mime_string = mime.into();

        // Basic validation: must contain '/' separator
        if !mime_string.contains('/') {
            return Err(MimeTypeError::InvalidFormat(mime_string));
        }

        // Extract type and subtype (before any parameters)
        let main_part = mime_string.split(';').next().unwrap_or(&mime_string);
        let parts: Vec<&str> = main_part.split('/').collect();

        if parts.len() != 2 {
            return Err(MimeTypeError::InvalidFormat(mime_string));
        }

        let type_part = parts[0].trim();
        let subtype = parts[1].trim();

        if type_part.is_empty() {
            return Err(MimeTypeError::EmptyType(mime_string));
        }

        if subtype.is_empty() {
            return Err(MimeTypeError::EmptySubtype(mime_string));
        }

        Ok(Self(mime_string))
    }

    /// Create a MIME type without validation
    #[must_use]
    pub fn new_unchecked<S: Into<String>>(mime: S) -> Self {
        Self(mime.into())
    }

    /// Get the MIME type as a string slice
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Get the type part (before '/')
    #[must_use]
    pub fn type_part(&self) -> Option<&str> {
        self.0
            .split('/')
            .next()
            .map(|s| s.split(';').next().unwrap_or(s).trim())
    }

    /// Get the subtype part (after '/', before parameters)
    #[must_use]
    pub fn subtype(&self) -> Option<&str> {
        self.0
            .split('/')
            .nth(1)
            .map(|s| s.split(';').next().unwrap_or(s).trim())
    }

    /// Convert into the inner String
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for MimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for MimeType {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<MimeType> for String {
    fn from(mime: MimeType) -> Self {
        mime.0
    }
}

/// Base64-encoded string
///
/// Represents a Base64-encoded binary data string.
///
/// # Examples
///
/// ```
/// use turbomcp_protocol::types::domain::Base64String;
///
/// let b64 = Base64String::new("SGVsbG8gV29ybGQh").unwrap();
/// assert_eq!(b64.as_str(), "SGVsbG8gV29ybGQh");
///
/// // Invalid Base64 (contains invalid characters)
/// assert!(Base64String::new("not valid!@#").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Base64String(String);

impl Base64String {
    /// Create a new Base64 string with validation
    ///
    /// # Errors
    ///
    /// Returns an error if the string contains invalid Base64 characters
    pub fn new<S: Into<String>>(data: S) -> Result<Self, Base64Error> {
        let data_string = data.into();

        // Validate Base64 characters: A-Z, a-z, 0-9, +, /, =
        if !data_string
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        {
            return Err(Base64Error::InvalidCharacters(data_string));
        }

        // Check padding is only at the end
        if let Some(first_pad) = data_string.find('=')
            && !data_string[first_pad..].chars().all(|c| c == '=')
        {
            return Err(Base64Error::InvalidPadding(data_string));
        }

        Ok(Self(data_string))
    }

    /// Create a Base64 string without validation
    #[must_use]
    pub fn new_unchecked<S: Into<String>>(data: S) -> Self {
        Self(data.into())
    }

    /// Get the Base64 string as a string slice
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Convert into the inner String
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for Base64String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for Base64String {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<Base64String> for String {
    fn from(b64: Base64String) -> Self {
        b64.0
    }
}

/// Error type for URI validation
#[derive(Debug, Clone, thiserror::Error)]
pub enum UriError {
    /// URI is missing a scheme separator (':')
    #[error("URI missing scheme separator: {0}")]
    MissingScheme(String),

    /// URI has an empty scheme
    #[error("URI has empty scheme: {0}")]
    EmptyScheme(String),

    /// URI has an invalid scheme format
    #[error(
        "URI has invalid scheme (must start with letter and contain only alphanumeric, +, ., -): {0}"
    )]
    InvalidScheme(String),
}

/// Error type for MIME type validation
#[derive(Debug, Clone, thiserror::Error)]
pub enum MimeTypeError {
    /// Invalid MIME type format
    #[error("Invalid MIME type format (must be type/subtype): {0}")]
    InvalidFormat(String),

    /// Empty type part
    #[error("MIME type has empty type part: {0}")]
    EmptyType(String),

    /// Empty subtype part
    #[error("MIME type has empty subtype part: {0}")]
    EmptySubtype(String),
}

/// Error type for Base64 validation
#[derive(Debug, Clone, thiserror::Error)]
pub enum Base64Error {
    /// String contains invalid Base64 characters
    #[error("Base64 string contains invalid characters: {0}")]
    InvalidCharacters(String),

    /// Invalid padding
    #[error("Base64 string has invalid padding (= must only appear at end): {0}")]
    InvalidPadding(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_validation() {
        // Valid URIs
        assert!(Uri::new("file:///path/to/file").is_ok());
        assert!(Uri::new("https://example.com").is_ok());
        assert!(Uri::new("resource://test").is_ok());
        assert!(Uri::new("custom+scheme://data").is_ok());

        // Invalid URIs
        assert!(Uri::new("not-a-uri").is_err());
        assert!(Uri::new(":no-scheme").is_err());
        assert!(Uri::new("123://invalid-start").is_err());
    }

    #[test]
    fn test_uri_scheme_extraction() {
        let uri = Uri::new("https://example.com/path").unwrap();
        assert_eq!(uri.scheme(), Some("https"));

        let file_uri = Uri::new("file:///local/path").unwrap();
        assert_eq!(file_uri.scheme(), Some("file"));
    }

    #[test]
    fn test_mime_type_validation() {
        // Valid MIME types
        assert!(MimeType::new("text/plain").is_ok());
        assert!(MimeType::new("application/json").is_ok());
        assert!(MimeType::new("text/html; charset=utf-8").is_ok());
        assert!(MimeType::new("image/png").is_ok());

        // Invalid MIME types
        assert!(MimeType::new("invalid").is_err());
        assert!(MimeType::new("/no-type").is_err());
        assert!(MimeType::new("no-subtype/").is_err());
    }

    #[test]
    fn test_mime_type_parts() {
        let mime = MimeType::new("text/html; charset=utf-8").unwrap();
        assert_eq!(mime.type_part(), Some("text"));
        assert_eq!(mime.subtype(), Some("html"));
    }

    #[test]
    fn test_base64_validation() {
        // Valid Base64
        assert!(Base64String::new("SGVsbG8gV29ybGQh").is_ok());
        assert!(Base64String::new("YWJjMTIz").is_ok());
        assert!(Base64String::new("dGVzdA==").is_ok());
        assert!(Base64String::new("").is_ok()); // Empty is valid

        // Invalid Base64
        assert!(Base64String::new("invalid!@#").is_err());
        assert!(Base64String::new("test=data").is_err()); // Padding in middle
    }

    #[test]
    fn test_domain_type_conversions() {
        let uri = Uri::new("https://example.com").unwrap();
        assert_eq!(uri.as_str(), "https://example.com");
        assert_eq!(uri.to_string(), "https://example.com");

        let mime = MimeType::new("text/plain").unwrap();
        assert_eq!(mime.as_str(), "text/plain");

        let b64 = Base64String::new("dGVzdA==").unwrap();
        assert_eq!(b64.as_str(), "dGVzdA==");
    }
}
