//! # TurboMCP Wire Format Codec
//!
//! This crate provides wire format encoding/decoding abstractions for MCP messages.
//! It enables pluggable serialization formats while maintaining MCP protocol compliance.
//!
//! ## Design Philosophy
//!
//! - **Wire format**: JSON-RPC 2.0 (MCP protocol standard)
//! - **Extensible**: Support for alternative formats (MessagePack, etc.)
//! - **Zero-copy ready**: Integration with rkyv for internal message passing
//! - **`no_std` compatible**: Works in embedded and WASM environments
//!
//! ## Usage
//!
//! ```rust
//! use turbomcp_wire::{Codec, JsonCodec};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct MyMessage {
//!     id: u32,
//!     method: String,
//! }
//!
//! let codec = JsonCodec::new();
//! let msg = MyMessage { id: 1, method: "test".into() };
//!
//! // Encode to bytes
//! let bytes = codec.encode(&msg).unwrap();
//!
//! // Decode from bytes
//! let decoded: MyMessage = codec.decode(&bytes).unwrap();
//! ```
//!
//! ## Features
//!
//! - `std` - Standard library support (default)
//! - `json` - JSON codec (default)
//! - `simd` - SIMD-accelerated JSON (sonic-rs, simd-json)
//! - `msgpack` - MessagePack binary format

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use serde::{Serialize, de::DeserializeOwned};

// Re-export core types for convenience
pub use turbomcp_core::error::McpError;

/// Wire format codec error
#[derive(Debug, Clone)]
pub struct CodecError {
    /// Error message
    pub message: String,
    /// Optional source location
    pub source: Option<String>,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "codec error: {}", self.message)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for CodecError {}

impl CodecError {
    /// Create a new codec error
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }

    /// Create a codec error with source information
    pub fn with_source(message: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    /// Create an encoding error
    pub fn encode(message: impl Into<String>) -> Self {
        Self::new(alloc::format!("encode: {}", message.into()))
    }

    /// Create a decoding error
    pub fn decode(message: impl Into<String>) -> Self {
        Self::new(alloc::format!("decode: {}", message.into()))
    }
}

impl From<CodecError> for McpError {
    fn from(err: CodecError) -> Self {
        McpError::parse_error(err.message)
    }
}

/// Result type for codec operations
pub type CodecResult<T> = Result<T, CodecError>;

/// Wire format codec trait
///
/// This trait abstracts over different serialization formats, allowing
/// pluggable encoding/decoding while maintaining type safety.
///
/// # Implementors
///
/// - [`JsonCodec`] - Standard JSON encoding (default)
/// - `SimdJsonCodec` - SIMD-accelerated JSON (requires `simd` feature)
/// - `MsgPackCodec` - MessagePack binary format (requires `msgpack` feature)
pub trait Codec: Send + Sync {
    /// Encode a value to bytes
    fn encode<T: Serialize>(&self, value: &T) -> CodecResult<Vec<u8>>;

    /// Decode bytes to a value
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> CodecResult<T>;

    /// Get the content type for this codec (e.g., "application/json")
    fn content_type(&self) -> &'static str;

    /// Check if this codec supports streaming
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Get codec name for debugging
    fn name(&self) -> &'static str;
}

/// JSON codec using serde_json
///
/// This is the default codec for MCP protocol compliance.
/// It produces human-readable JSON suitable for debugging and logging.
#[derive(Debug, Clone, Default)]
pub struct JsonCodec {
    /// Pretty print output (default: false)
    pub pretty: bool,
}

impl JsonCodec {
    /// Create a new JSON codec
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a JSON codec with pretty printing enabled
    pub fn pretty() -> Self {
        Self { pretty: true }
    }
}

impl Codec for JsonCodec {
    fn encode<T: Serialize>(&self, value: &T) -> CodecResult<Vec<u8>> {
        if self.pretty {
            serde_json::to_vec_pretty(value)
        } else {
            serde_json::to_vec(value)
        }
        .map_err(|e| CodecError::encode(e.to_string()))
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> CodecResult<T> {
        serde_json::from_slice(bytes).map_err(|e| CodecError::decode(e.to_string()))
    }

    fn content_type(&self) -> &'static str {
        "application/json"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "json"
    }
}

/// SIMD-accelerated JSON codec using sonic-rs
///
/// This codec uses SIMD instructions for faster JSON parsing.
/// Falls back to standard serde_json on unsupported platforms.
#[cfg(feature = "simd")]
#[cfg_attr(docsrs, doc(cfg(feature = "simd")))]
#[derive(Debug, Clone, Default)]
pub struct SimdJsonCodec;

#[cfg(feature = "simd")]
impl SimdJsonCodec {
    /// Create a new SIMD JSON codec
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "simd")]
impl Codec for SimdJsonCodec {
    fn encode<T: Serialize>(&self, value: &T) -> CodecResult<Vec<u8>> {
        sonic_rs::to_vec(value).map_err(|e| CodecError::encode(e.to_string()))
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> CodecResult<T> {
        sonic_rs::from_slice(bytes).map_err(|e| CodecError::decode(e.to_string()))
    }

    fn content_type(&self) -> &'static str {
        "application/json"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "simd-json"
    }
}

/// MessagePack binary codec
///
/// This codec produces compact binary output, suitable for
/// high-throughput scenarios where bandwidth is limited.
///
/// **Note**: MessagePack is not MCP-compliant for external communication
/// but can be used for internal message passing.
#[cfg(feature = "msgpack")]
#[cfg_attr(docsrs, doc(cfg(feature = "msgpack")))]
#[derive(Debug, Clone, Default)]
pub struct MsgPackCodec;

#[cfg(feature = "msgpack")]
impl MsgPackCodec {
    /// Create a new MessagePack codec
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "msgpack")]
impl Codec for MsgPackCodec {
    fn encode<T: Serialize>(&self, value: &T) -> CodecResult<Vec<u8>> {
        // Use named serialization to support skip_serializing_if on optional fields
        rmp_serde::to_vec_named(value).map_err(|e| CodecError::encode(e.to_string()))
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> CodecResult<T> {
        rmp_serde::from_slice(bytes).map_err(|e| CodecError::decode(e.to_string()))
    }

    fn content_type(&self) -> &'static str {
        "application/msgpack"
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn name(&self) -> &'static str {
        "msgpack"
    }
}

/// Streaming JSON decoder for Server-Sent Events (SSE)
///
/// This decoder handles newline-delimited JSON streams commonly
/// used in HTTP/SSE transports.
#[derive(Debug)]
pub struct StreamingJsonDecoder {
    buffer: Vec<u8>,
}

impl Default for StreamingJsonDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingJsonDecoder {
    /// Create a new streaming decoder
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Create with pre-allocated buffer capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Feed data into the decoder
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Try to decode the next complete message
    ///
    /// Returns `Some(T)` if a complete message is available,
    /// `None` if more data is needed.
    pub fn try_decode<T: DeserializeOwned>(&mut self) -> CodecResult<Option<T>> {
        // Look for newline delimiter
        if let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let line = &self.buffer[..pos];

            // Skip empty lines
            if line.is_empty() || line.iter().all(|b| b.is_ascii_whitespace()) {
                self.buffer.drain(..=pos);
                return Ok(None);
            }

            // Try to decode
            let result = serde_json::from_slice(line);

            // Remove processed data (including newline)
            self.buffer.drain(..=pos);

            match result {
                Ok(value) => Ok(Some(value)),
                Err(e) => Err(CodecError::decode(e.to_string())),
            }
        } else {
            Ok(None)
        }
    }

    /// Clear the internal buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get current buffer length
    pub fn len(&self) -> usize {
        self.buffer.len()
    }
}

/// Enum wrapper for all codec types
///
/// This provides a unified type for codec selection without requiring
/// dyn trait objects (which aren't compatible with generic methods).
#[derive(Debug, Clone)]
pub enum AnyCodec {
    /// Standard JSON codec
    Json(JsonCodec),
    /// SIMD-accelerated JSON codec
    #[cfg(feature = "simd")]
    #[cfg_attr(docsrs, doc(cfg(feature = "simd")))]
    SimdJson(SimdJsonCodec),
    /// MessagePack binary codec
    #[cfg(feature = "msgpack")]
    #[cfg_attr(docsrs, doc(cfg(feature = "msgpack")))]
    MsgPack(MsgPackCodec),
}

impl AnyCodec {
    /// Create a codec by name
    ///
    /// Supported names:
    /// - `"json"` - Standard JSON codec
    /// - `"simd"` or `"simd-json"` - SIMD-accelerated JSON (requires `simd` feature)
    /// - `"msgpack"` - MessagePack binary (requires `msgpack` feature)
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "json" => Some(Self::Json(JsonCodec::new())),
            #[cfg(feature = "simd")]
            "simd" | "simd-json" => Some(Self::SimdJson(SimdJsonCodec::new())),
            #[cfg(feature = "msgpack")]
            "msgpack" => Some(Self::MsgPack(MsgPackCodec::new())),
            _ => None,
        }
    }

    /// List available codec names
    pub fn available_names() -> &'static [&'static str] {
        &[
            "json",
            #[cfg(feature = "simd")]
            "simd-json",
            #[cfg(feature = "msgpack")]
            "msgpack",
        ]
    }

    /// Encode a value to bytes
    pub fn encode<T: Serialize>(&self, value: &T) -> CodecResult<Vec<u8>> {
        match self {
            Self::Json(c) => c.encode(value),
            #[cfg(feature = "simd")]
            Self::SimdJson(c) => c.encode(value),
            #[cfg(feature = "msgpack")]
            Self::MsgPack(c) => c.encode(value),
        }
    }

    /// Decode bytes to a value
    pub fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> CodecResult<T> {
        match self {
            Self::Json(c) => c.decode(bytes),
            #[cfg(feature = "simd")]
            Self::SimdJson(c) => c.decode(bytes),
            #[cfg(feature = "msgpack")]
            Self::MsgPack(c) => c.decode(bytes),
        }
    }

    /// Get the content type
    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Json(c) => c.content_type(),
            #[cfg(feature = "simd")]
            Self::SimdJson(c) => c.content_type(),
            #[cfg(feature = "msgpack")]
            Self::MsgPack(c) => c.content_type(),
        }
    }

    /// Get codec name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Json(c) => c.name(),
            #[cfg(feature = "simd")]
            Self::SimdJson(c) => c.name(),
            #[cfg(feature = "msgpack")]
            Self::MsgPack(c) => c.name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestMessage {
        id: u32,
        method: String,
        params: Option<serde_json::Value>,
    }

    #[test]
    fn test_json_codec_roundtrip() {
        let codec = JsonCodec::new();
        let msg = TestMessage {
            id: 42,
            method: "test/method".into(),
            params: Some(serde_json::json!({"key": "value"})),
        };

        let encoded = codec.encode(&msg).unwrap();
        let decoded: TestMessage = codec.decode(&encoded).unwrap();

        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_json_codec_pretty() {
        let codec = JsonCodec::pretty();
        let msg = TestMessage {
            id: 1,
            method: "test".into(),
            params: None,
        };

        let encoded = codec.encode(&msg).unwrap();
        let output = String::from_utf8(encoded).unwrap();

        // Pretty output should contain newlines
        assert!(output.contains('\n'));
    }

    #[test]
    fn test_codec_content_type() {
        let json = JsonCodec::new();
        assert_eq!(json.content_type(), "application/json");
        assert_eq!(json.name(), "json");
    }

    #[test]
    fn test_streaming_decoder() {
        let mut decoder = StreamingJsonDecoder::new();

        // Feed partial data
        decoder.feed(br#"{"id":1,"method":"a","params":null}"#);
        assert!(decoder.try_decode::<TestMessage>().unwrap().is_none());

        // Feed newline to complete
        decoder.feed(b"\n");
        let msg: TestMessage = decoder.try_decode().unwrap().unwrap();
        assert_eq!(msg.id, 1);
        assert_eq!(msg.method, "a");
    }

    #[test]
    fn test_streaming_decoder_multiple() {
        let mut decoder = StreamingJsonDecoder::new();

        // Feed multiple messages at once
        decoder.feed(
            br#"{"id":1,"method":"a","params":null}
{"id":2,"method":"b","params":null}
"#,
        );

        let msg1: TestMessage = decoder.try_decode().unwrap().unwrap();
        assert_eq!(msg1.id, 1);

        let msg2: TestMessage = decoder.try_decode().unwrap().unwrap();
        assert_eq!(msg2.id, 2);

        // No more messages
        assert!(decoder.try_decode::<TestMessage>().unwrap().is_none());
    }

    #[test]
    fn test_any_codec() {
        let codec = AnyCodec::from_name("json").unwrap();
        assert_eq!(codec.name(), "json");

        assert!(AnyCodec::from_name("unknown").is_none());
        assert!(AnyCodec::available_names().contains(&"json"));
    }

    #[test]
    fn test_codec_error() {
        let codec = JsonCodec::new();
        let result: CodecResult<TestMessage> = codec.decode(b"invalid json");
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.message.contains("decode"));
    }

    #[cfg(feature = "simd")]
    #[test]
    fn test_simd_codec_roundtrip() {
        let codec = SimdJsonCodec::new();
        let msg = TestMessage {
            id: 99,
            method: "simd/test".into(),
            params: Some(serde_json::json!([1, 2, 3])),
        };

        let encoded = codec.encode(&msg).unwrap();
        let decoded: TestMessage = codec.decode(&encoded).unwrap();

        assert_eq!(msg, decoded);
    }

    #[cfg(feature = "msgpack")]
    #[test]
    fn test_msgpack_codec_roundtrip() {
        let codec = MsgPackCodec::new();
        let msg = TestMessage {
            id: 77,
            method: "msgpack/test".into(),
            params: None,
        };

        let encoded = codec.encode(&msg).unwrap();
        let decoded: TestMessage = codec.decode(&encoded).unwrap();

        assert_eq!(msg, decoded);
        assert_eq!(codec.content_type(), "application/msgpack");
    }
}
