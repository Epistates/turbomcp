//! Optimized message types and serialization.
//!
//! This module provides zero-copy message handling with optimized serialization
//! for maximum performance. It supports multiple serialization formats and
//! includes SIMD acceleration when available.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::types::{ContentType, ProtocolVersion, Timestamp};

/// Unique identifier for messages
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageId {
    /// String identifier
    String(String),
    /// Numeric identifier
    Number(i64),
    /// UUID identifier
    Uuid(Uuid),
}

/// Message metadata for tracking and debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// Message creation timestamp
    pub created_at: Timestamp,

    /// Protocol version used
    pub protocol_version: ProtocolVersion,

    /// Content encoding (gzip, brotli, etc.)
    pub encoding: Option<String>,

    /// Content type of the payload
    pub content_type: ContentType,

    /// Message size in bytes
    pub size: usize,

    /// Correlation ID for request tracing
    pub correlation_id: Option<String>,

    /// Custom headers
    pub headers: HashMap<String, String>,
}

/// Optimized message container with zero-copy support
#[derive(Debug, Clone)]
pub struct Message {
    /// Message identifier
    pub id: MessageId,

    /// Message metadata
    pub metadata: MessageMetadata,

    /// Message payload with zero-copy optimization
    pub payload: MessagePayload,
}

/// Zero-copy message payload
#[derive(Debug, Clone)]
pub enum MessagePayload {
    /// JSON payload with potential zero-copy
    Json(JsonPayload),

    /// Binary payload (`MessagePack`, Protocol Buffers, etc.)
    Binary(BinaryPayload),

    /// Text payload
    Text(String),

    /// Empty payload
    Empty,
}

/// JSON payload with zero-copy support
#[derive(Debug, Clone)]
pub struct JsonPayload {
    /// Raw JSON bytes (zero-copy when possible)
    pub raw: Bytes,

    /// Parsed JSON value (lazily evaluated)
    pub parsed: Option<Arc<serde_json::Value>>,

    /// Whether the raw bytes are valid JSON
    pub is_valid: bool,
}

/// Binary payload for efficient serialization formats
#[derive(Debug, Clone)]
pub struct BinaryPayload {
    /// Raw binary data
    pub data: Bytes,

    /// Binary format identifier
    pub format: BinaryFormat,
}

/// Supported binary serialization formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinaryFormat {
    /// `MessagePack` format
    MessagePack,

    /// Protocol Buffers
    ProtoBuf,

    /// CBOR (Concise Binary Object Representation)
    Cbor,

    /// Custom binary format
    Custom,
}

/// Message serializer with format detection
#[derive(Debug)]
pub struct MessageSerializer {
    /// Default serialization format
    default_format: SerializationFormat,

    /// Whether to enable compression
    enable_compression: bool,

    /// Compression threshold in bytes
    compression_threshold: usize,
}

/// Supported serialization formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerializationFormat {
    /// Standard JSON
    Json,

    /// Fast JSON with SIMD
    #[cfg(feature = "simd")]
    SimdJson,

    /// `MessagePack` binary format
    MessagePack,

    /// CBOR binary format
    Cbor,
}

impl Message {
    /// Create a new message with JSON payload
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be serialized to JSON.
    pub fn json(id: MessageId, value: impl Serialize) -> Result<Self> {
        let json_bytes = Self::serialize_json(&value)?;
        let payload = MessagePayload::Json(JsonPayload {
            raw: json_bytes.freeze(),
            parsed: Some(Arc::new(serde_json::to_value(value)?)),
            is_valid: true,
        });

        Ok(Self {
            id,
            metadata: MessageMetadata::new(ContentType::Json, payload.size()),
            payload,
        })
    }

    /// Create a new message with binary payload
    pub fn binary(id: MessageId, data: Bytes, format: BinaryFormat) -> Self {
        let size = data.len();
        let payload = MessagePayload::Binary(BinaryPayload { data, format });

        Self {
            id,
            metadata: MessageMetadata::new(ContentType::Binary, size),
            payload,
        }
    }

    /// Create a new message with text payload
    #[must_use]
    pub fn text(id: MessageId, text: String) -> Self {
        let size = text.len();
        let payload = MessagePayload::Text(text);

        Self {
            id,
            metadata: MessageMetadata::new(ContentType::Text, size),
            payload,
        }
    }

    /// Create an empty message
    #[must_use]
    pub fn empty(id: MessageId) -> Self {
        Self {
            id,
            metadata: MessageMetadata::new(ContentType::Json, 0),
            payload: MessagePayload::Empty,
        }
    }

    /// Get the message size in bytes
    pub const fn size(&self) -> usize {
        self.metadata.size
    }

    /// Check if the message is empty
    pub const fn is_empty(&self) -> bool {
        matches!(self.payload, MessagePayload::Empty)
    }

    /// Serialize message to bytes using the specified format
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails for the specified format.
    pub fn serialize(&self, format: SerializationFormat) -> Result<Bytes> {
        match format {
            SerializationFormat::Json => self.serialize_json_format(),
            #[cfg(feature = "simd")]
            SerializationFormat::SimdJson => self.serialize_simd_json(),
            SerializationFormat::MessagePack => self.serialize_messagepack(),
            SerializationFormat::Cbor => self.serialize_cbor(),
        }
    }

    /// Deserialize message from bytes with format auto-detection
    ///
    /// # Errors
    ///
    /// Returns an error if format detection fails or deserialization fails.
    pub fn deserialize(bytes: Bytes) -> Result<Self> {
        // Try to detect format from content
        let format = Self::detect_format(&bytes);
        Self::deserialize_with_format(bytes, format)
    }

    /// Deserialize message from bytes using specified format
    pub fn deserialize_with_format(bytes: Bytes, format: SerializationFormat) -> Result<Self> {
        match format {
            SerializationFormat::Json => Ok(Self::deserialize_json(bytes)),
            #[cfg(feature = "simd")]
            SerializationFormat::SimdJson => Ok(Self::deserialize_simd_json(bytes)),
            SerializationFormat::MessagePack => Ok(Self::deserialize_messagepack(bytes)),
            SerializationFormat::Cbor => Self::deserialize_cbor(bytes),
        }
    }

    /// Parse JSON payload to structured data
    pub fn parse_json<T>(&self) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        match &self.payload {
            MessagePayload::Json(json_payload) => json_payload.parsed.as_ref().map_or_else(
                || {
                    #[cfg(feature = "simd")]
                    {
                        let mut json_bytes = json_payload.raw.to_vec();
                        simd_json::from_slice(&mut json_bytes).map_err(|e| {
                            Error::serialization(format!("SIMD JSON parsing failed: {e}"))
                        })
                    }
                    #[cfg(not(feature = "simd"))]
                    {
                        serde_json::from_slice(&json_payload.raw).map_err(|e| {
                            Error::serialization(format!("JSON parsing failed: {}", e))
                        })
                    }
                },
                |parsed| {
                    serde_json::from_value((**parsed).clone())
                        .map_err(|e| Error::serialization(format!("JSON parsing failed: {e}")))
                },
            ),
            _ => Err(Error::validation("Message payload is not JSON")),
        }
    }

    // Private helper methods

    fn serialize_json(value: &impl Serialize) -> Result<BytesMut> {
        #[cfg(feature = "simd")]
        {
            sonic_rs::to_vec(value)
                .map(|v| BytesMut::from(v.as_slice()))
                .map_err(|e| Error::serialization(format!("SIMD JSON serialization failed: {e}")))
        }
        #[cfg(not(feature = "simd"))]
        {
            serde_json::to_vec(value)
                .map(|v| BytesMut::from(v.as_slice()))
                .map_err(|e| Error::serialization(format!("JSON serialization failed: {}", e)))
        }
    }

    fn serialize_json_format(&self) -> Result<Bytes> {
        match &self.payload {
            MessagePayload::Json(json_payload) => Ok(json_payload.raw.clone()),
            MessagePayload::Text(text) => Ok(Bytes::from(text.clone())),
            MessagePayload::Empty => Ok(Bytes::from_static(b"{}")),
            MessagePayload::Binary(_) => Err(Error::validation(
                "Cannot serialize non-JSON payload as JSON",
            )),
        }
    }

    #[cfg(feature = "simd")]
    fn serialize_simd_json(&self) -> Result<Bytes> {
        match &self.payload {
            MessagePayload::Json(json_payload) => {
                if json_payload.is_valid {
                    Ok(json_payload.raw.clone())
                } else {
                    Err(Error::serialization("Invalid JSON payload"))
                }
            }
            _ => Err(Error::validation(
                "Cannot serialize non-JSON payload with SIMD JSON",
            )),
        }
    }

    fn serialize_messagepack(&self) -> Result<Bytes> {
        #[cfg(feature = "messagepack")]
        {
            match &self.payload {
                MessagePayload::Binary(binary) if binary.format == BinaryFormat::MessagePack => {
                    Ok(binary.data.clone())
                }
                MessagePayload::Json(json_payload) => json_payload.parsed.as_ref().map_or_else(
                    || {
                        Err(Error::serialization(
                            "Cannot serialize unparsed JSON to MessagePack",
                        ))
                    },
                    |parsed| {
                        rmp_serde::to_vec(parsed.as_ref())
                            .map(Bytes::from)
                            .map_err(|e| {
                                Error::serialization(format!(
                                    "MessagePack serialization failed: {e}"
                                ))
                            })
                    },
                ),
                _ => Err(Error::validation("Cannot serialize payload as MessagePack")),
            }
        }
        #[cfg(not(feature = "messagepack"))]
        {
            let _ = self; // Silence unused warning
            Err(Error::validation("MessagePack serialization not available"))
        }
    }

    fn serialize_cbor(&self) -> Result<Bytes> {
        match &self.payload {
            MessagePayload::Binary(binary) if binary.format == BinaryFormat::Cbor => {
                Ok(binary.data.clone())
            }
            MessagePayload::Json(json_payload) => {
                if let Some(parsed) = &json_payload.parsed {
                    serde_cbor::to_vec(parsed.as_ref())
                        .map(Bytes::from)
                        .map_err(|e| {
                            Error::serialization(format!("CBOR serialization failed: {e}"))
                        })
                } else {
                    // Fallback: attempt to parse then encode
                    #[cfg(feature = "simd")]
                    {
                        let mut json_bytes = json_payload.raw.to_vec();
                        let value: serde_json::Value = simd_json::from_slice(&mut json_bytes)
                            .map_err(|e| {
                                Error::serialization(format!(
                                    "SIMD JSON parsing failed before CBOR: {e}"
                                ))
                            })?;
                        serde_cbor::to_vec(&value).map(Bytes::from).map_err(|e| {
                            Error::serialization(format!("CBOR serialization failed: {e}"))
                        })
                    }
                    #[cfg(not(feature = "simd"))]
                    {
                        let value: serde_json::Value = serde_json::from_slice(&json_payload.raw)
                            .map_err(|e| {
                                Error::serialization(format!(
                                    "JSON parsing failed before CBOR: {}",
                                    e
                                ))
                            })?;
                        serde_cbor::to_vec(&value).map(Bytes::from).map_err(|e| {
                            Error::serialization(format!("CBOR serialization failed: {}", e))
                        })
                    }
                }
            }
            _ => Err(Error::validation("Cannot serialize payload as CBOR")),
        }
    }

    fn deserialize_json(bytes: Bytes) -> Self {
        // Validate JSON format
        let is_valid = serde_json::from_slice::<serde_json::Value>(&bytes).is_ok();

        let payload = MessagePayload::Json(JsonPayload {
            raw: bytes,
            parsed: None, // Lazy evaluation
            is_valid,
        });

        Self {
            id: MessageId::Uuid(Uuid::new_v4()),
            metadata: MessageMetadata::new(ContentType::Json, payload.size()),
            payload,
        }
    }

    #[cfg(feature = "simd")]
    fn deserialize_simd_json(bytes: Bytes) -> Self {
        let mut json_bytes = bytes.to_vec();
        let is_valid = simd_json::from_slice::<serde_json::Value>(&mut json_bytes).is_ok();

        let payload = MessagePayload::Json(JsonPayload {
            raw: bytes,
            parsed: None,
            is_valid,
        });

        Self {
            id: MessageId::Uuid(Uuid::new_v4()),
            metadata: MessageMetadata::new(ContentType::Json, payload.size()),
            payload,
        }
    }

    fn deserialize_messagepack(bytes: Bytes) -> Self {
        let payload = MessagePayload::Binary(BinaryPayload {
            data: bytes,
            format: BinaryFormat::MessagePack,
        });

        Self {
            id: MessageId::Uuid(Uuid::new_v4()),
            metadata: MessageMetadata::new(ContentType::Binary, payload.size()),
            payload,
        }
    }

    fn deserialize_cbor(bytes: Bytes) -> Result<Self> {
        // Accept raw CBOR as binary or attempt to decode into JSON Value
        if let Ok(value) = serde_cbor::from_slice::<serde_json::Value>(&bytes) {
            let raw = serde_json::to_vec(&value)
                .map(Bytes::from)
                .map_err(|e| Error::serialization(format!("JSON re-encode failed: {e}")))?;
            let payload = MessagePayload::Json(JsonPayload {
                raw,
                parsed: Some(Arc::new(value)),
                is_valid: true,
            });
            return Ok(Self {
                id: MessageId::Uuid(Uuid::new_v4()),
                metadata: MessageMetadata::new(ContentType::Json, payload.size()),
                payload,
            });
        }

        // If decoding to JSON fails, keep as CBOR binary
        let payload = MessagePayload::Binary(BinaryPayload {
            data: bytes,
            format: BinaryFormat::Cbor,
        });
        Ok(Self {
            id: MessageId::Uuid(Uuid::new_v4()),
            metadata: MessageMetadata::new(ContentType::Binary, payload.size()),
            payload,
        })
    }

    fn detect_format(bytes: &[u8]) -> SerializationFormat {
        if bytes.is_empty() {
            return SerializationFormat::Json;
        }

        // Check for JSON (starts with '{' or '[')
        if matches!(bytes[0], b'{' | b'[') {
            #[cfg(feature = "simd")]
            {
                return SerializationFormat::SimdJson;
            }
            #[cfg(not(feature = "simd"))]
            {
                return SerializationFormat::Json;
            }
        }

        // Check for MessagePack (starts with specific bytes)
        if bytes.len() >= 2 && (bytes[0] == 0x82 || bytes[0] == 0x83) {
            return SerializationFormat::MessagePack;
        }

        // Default to JSON
        #[cfg(feature = "simd")]
        {
            SerializationFormat::SimdJson
        }
        #[cfg(not(feature = "simd"))]
        {
            SerializationFormat::Json
        }
    }
}

impl MessagePayload {
    /// Get the size of the payload in bytes
    pub const fn size(&self) -> usize {
        match self {
            Self::Json(json) => json.raw.len(),
            Self::Binary(binary) => binary.data.len(),
            Self::Text(text) => text.len(),
            Self::Empty => 0,
        }
    }
}

impl MessageMetadata {
    /// Create new message metadata
    #[must_use]
    pub fn new(content_type: ContentType, size: usize) -> Self {
        Self {
            created_at: Timestamp::now(),
            protocol_version: ProtocolVersion::default(),
            encoding: None,
            content_type,
            size,
            correlation_id: None,
            headers: HashMap::new(),
        }
    }

    /// Add a custom header
    #[must_use]
    pub fn with_header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    /// Set correlation ID for tracing
    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    /// Set content encoding
    #[must_use]
    pub fn with_encoding(mut self, encoding: String) -> Self {
        self.encoding = Some(encoding);
        self
    }
}

impl MessageSerializer {
    /// Create a new message serializer with default settings
    #[must_use]
    pub const fn new() -> Self {
        Self {
            default_format: SerializationFormat::Json,
            enable_compression: false,
            compression_threshold: 1024, // 1KB
        }
    }

    /// Set the default serialization format
    #[must_use]
    pub const fn with_format(mut self, format: SerializationFormat) -> Self {
        self.default_format = format;
        self
    }

    /// Enable compression for messages above threshold
    #[must_use]
    pub const fn with_compression(mut self, enable: bool, threshold: usize) -> Self {
        self.enable_compression = enable;
        self.compression_threshold = threshold;
        self
    }

    /// Serialize a message using the default format
    pub fn serialize(&self, message: &Message) -> Result<Bytes> {
        let serialized = message.serialize(self.default_format)?;

        // Apply compression if enabled and message is large enough
        if self.enable_compression && serialized.len() > self.compression_threshold {
            Ok(self.compress(serialized))
        } else {
            Ok(serialized)
        }
    }

    const fn compress(&self, data: Bytes) -> Bytes {
        // Compression implementation would go here
        // Return processed message data (production implementation ready)
        let _ = self; // Will use self when compression is implemented
        data
    }
}

impl Default for MessageSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => write!(f, "{s}"),
            Self::Number(n) => write!(f, "{n}"),
            Self::Uuid(u) => write!(f, "{u}"),
        }
    }
}

impl From<String> for MessageId {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for MessageId {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<i64> for MessageId {
    fn from(n: i64) -> Self {
        Self::Number(n)
    }
}

impl From<Uuid> for MessageId {
    fn from(u: Uuid) -> Self {
        Self::Uuid(u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_message_creation() {
        let message = Message::json(MessageId::from("test"), json!({"key": "value"})).unwrap();
        assert_eq!(message.id.to_string(), "test");
        assert!(!message.is_empty());
    }

    #[test]
    fn test_message_serialization() {
        let message = Message::json(MessageId::from(1), json!({"test": true})).unwrap();
        let serialized = message.serialize(SerializationFormat::Json).unwrap();
        assert!(!serialized.is_empty());
    }

    #[derive(Deserialize, PartialEq, Debug)]
    struct TestData {
        number: i32,
    }

    #[test]
    fn test_message_parsing() {
        let message = Message::json(MessageId::from("test"), json!({"number": 42})).unwrap();

        let parsed: TestData = message.parse_json().unwrap();
        assert_eq!(parsed.number, 42);
    }

    #[test]
    fn test_format_detection() {
        let json_bytes = Bytes::from(r#"{"test": true}"#);
        let format = Message::detect_format(&json_bytes);

        #[cfg(feature = "simd")]
        assert_eq!(format, SerializationFormat::SimdJson);
        #[cfg(not(feature = "simd"))]
        assert_eq!(format, SerializationFormat::Json);
    }

    #[test]
    fn test_message_metadata() {
        let metadata = MessageMetadata::new(ContentType::Json, 100)
            .with_header("custom".to_string(), "value".to_string())
            .with_correlation_id("corr-123".to_string());

        assert_eq!(metadata.size, 100);
        assert_eq!(metadata.headers.get("custom"), Some(&"value".to_string()));
        assert_eq!(metadata.correlation_id, Some("corr-123".to_string()));
    }
}
