//! Wire codec integration for protocol message serialization.
//!
//! This module bridges the [`turbomcp_wire`] codec abstraction with the
//! protocol layer's message types, providing a unified serialization interface.
//!
//! ## Features
//!
//! Enable different wire formats via feature flags:
//!
//! - `wire` - Base wire codec support (JSON by default)
//! - `wire-simd` - SIMD-accelerated JSON (sonic-rs)
//! - `wire-msgpack` - MessagePack binary format
//!
//! ## Usage
//!
//! ```rust
//! use turbomcp_protocol::codec::{ProtocolCodec, CodecType};
//! use turbomcp_protocol::jsonrpc::JsonRpcRequest;
//! use turbomcp_protocol::types::RequestId;
//!
//! // Create codec with default JSON format
//! let codec = ProtocolCodec::new();
//!
//! // Or specify codec type
//! let simd_codec = ProtocolCodec::with_type(CodecType::SimdJson);
//!
//! // Encode/decode messages
//! let request = JsonRpcRequest::without_params("ping".into(), RequestId::Number(1));
//! let bytes = codec.encode(&request)?;
//! let decoded: JsonRpcRequest = codec.decode(&bytes)?;
//! # Ok::<(), turbomcp_protocol::McpError>(())
//! ```

use crate::{McpError, Result};
use serde::{de::DeserializeOwned, Serialize};

// Re-export wire codec types
pub use turbomcp_wire::{
    AnyCodec, Codec, CodecError, CodecResult, JsonCodec, StreamingJsonDecoder,
};

#[cfg(feature = "wire-simd")]
pub use turbomcp_wire::SimdJsonCodec;

#[cfg(feature = "wire-msgpack")]
pub use turbomcp_wire::MsgPackCodec;

/// Protocol codec type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodecType {
    /// Standard JSON codec (default, MCP-compliant)
    #[default]
    Json,
    /// SIMD-accelerated JSON (requires `wire-simd` feature)
    SimdJson,
    /// MessagePack binary format (requires `wire-msgpack` feature)
    MessagePack,
}

impl CodecType {
    /// Check if this codec type is available with current features
    #[must_use]
    pub fn is_available(&self) -> bool {
        match self {
            CodecType::Json => true,
            #[cfg(feature = "wire-simd")]
            CodecType::SimdJson => true,
            #[cfg(not(feature = "wire-simd"))]
            CodecType::SimdJson => false,
            #[cfg(feature = "wire-msgpack")]
            CodecType::MessagePack => true,
            #[cfg(not(feature = "wire-msgpack"))]
            CodecType::MessagePack => false,
        }
    }

    /// Get the content type string for this codec
    #[must_use]
    pub const fn content_type(&self) -> &'static str {
        match self {
            CodecType::Json | CodecType::SimdJson => "application/json",
            CodecType::MessagePack => "application/msgpack",
        }
    }
}

/// Protocol-level codec wrapper
///
/// Provides serialization/deserialization for MCP protocol messages
/// using the underlying wire codec abstraction.
#[derive(Debug, Clone)]
pub struct ProtocolCodec {
    inner: AnyCodec,
    codec_type: CodecType,
}

impl Default for ProtocolCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolCodec {
    /// Create a new protocol codec with default JSON format
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: AnyCodec::Json(JsonCodec::new()),
            codec_type: CodecType::Json,
        }
    }

    /// Create a protocol codec with specified type
    ///
    /// Falls back to JSON if the requested type is not available.
    #[must_use]
    pub fn with_type(codec_type: CodecType) -> Self {
        let (inner, actual_type) = match codec_type {
            CodecType::Json => (AnyCodec::Json(JsonCodec::new()), CodecType::Json),
            #[cfg(feature = "wire-simd")]
            CodecType::SimdJson => (
                AnyCodec::SimdJson(SimdJsonCodec::new()),
                CodecType::SimdJson,
            ),
            #[cfg(not(feature = "wire-simd"))]
            CodecType::SimdJson => {
                tracing::warn!("SIMD JSON codec not available, falling back to standard JSON");
                (AnyCodec::Json(JsonCodec::new()), CodecType::Json)
            }
            #[cfg(feature = "wire-msgpack")]
            CodecType::MessagePack => (
                AnyCodec::MsgPack(MsgPackCodec::new()),
                CodecType::MessagePack,
            ),
            #[cfg(not(feature = "wire-msgpack"))]
            CodecType::MessagePack => {
                tracing::warn!("MessagePack codec not available, falling back to standard JSON");
                (AnyCodec::Json(JsonCodec::new()), CodecType::Json)
            }
        };

        Self {
            inner,
            codec_type: actual_type,
        }
    }

    /// Create a JSON codec with pretty printing
    #[must_use]
    pub fn json_pretty() -> Self {
        Self {
            inner: AnyCodec::Json(JsonCodec::pretty()),
            codec_type: CodecType::Json,
        }
    }

    /// Get the codec type
    #[must_use]
    pub fn codec_type(&self) -> CodecType {
        self.codec_type
    }

    /// Get the content type for HTTP headers
    #[must_use]
    pub fn content_type(&self) -> &'static str {
        self.inner.content_type()
    }

    /// Get the codec name for debugging
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.inner.name()
    }

    /// Encode a value to bytes
    pub fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>> {
        self.inner.encode(value).map_err(|e| McpError::parse_error(e.message))
    }

    /// Decode bytes to a value
    pub fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T> {
        self.inner.decode(bytes).map_err(|e| McpError::parse_error(e.message))
    }

    /// Encode a value to a string (JSON only)
    ///
    /// Returns an error if the codec is not JSON-based.
    pub fn encode_string<T: Serialize>(&self, value: &T) -> Result<String> {
        if matches!(self.codec_type, CodecType::MessagePack) {
            return Err(McpError::invalid_request(
                "Cannot encode MessagePack to string",
            ));
        }
        let bytes = self.encode(value)?;
        String::from_utf8(bytes)
            .map_err(|e| McpError::parse_error(format!("Invalid UTF-8: {e}")))
    }
}

/// Protocol message encoder for streaming transports
///
/// Wraps messages with newline delimiters for SSE/streaming use.
#[derive(Debug)]
pub struct StreamingEncoder {
    codec: ProtocolCodec,
}

impl StreamingEncoder {
    /// Create a new streaming encoder
    #[must_use]
    pub fn new(codec: ProtocolCodec) -> Self {
        Self { codec }
    }

    /// Encode a message with newline delimiter
    pub fn encode<T: Serialize>(&self, value: &T) -> Result<Vec<u8>> {
        let mut bytes = self.codec.encode(value)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::JsonRpcRequest;
    use crate::types::RequestId;

    fn test_request(method: &str) -> JsonRpcRequest {
        JsonRpcRequest::without_params(method.to_string(), RequestId::Number(1))
    }

    #[test]
    fn test_protocol_codec_json() {
        let codec = ProtocolCodec::new();
        assert_eq!(codec.codec_type(), CodecType::Json);
        assert_eq!(codec.content_type(), "application/json");

        let request = test_request("test/ping");
        let bytes = codec.encode(&request).unwrap();
        let decoded: JsonRpcRequest = codec.decode(&bytes).unwrap();

        assert_eq!(decoded.method, "test/ping");
    }

    #[test]
    fn test_protocol_codec_pretty() {
        let codec = ProtocolCodec::json_pretty();
        let request = test_request("test");
        let output = codec.encode_string(&request).unwrap();

        // Pretty output should contain newlines
        assert!(output.contains('\n'));
    }

    #[test]
    fn test_codec_type_availability() {
        assert!(CodecType::Json.is_available());

        #[cfg(feature = "wire-simd")]
        assert!(CodecType::SimdJson.is_available());

        #[cfg(not(feature = "wire-simd"))]
        assert!(!CodecType::SimdJson.is_available());
    }

    #[test]
    fn test_streaming_encoder() {
        let codec = ProtocolCodec::new();
        let encoder = StreamingEncoder::new(codec);

        let request = test_request("test");
        let bytes = encoder.encode(&request).unwrap();

        // Should end with newline
        assert!(bytes.ends_with(b"\n"));
    }

    #[test]
    fn test_streaming_decoder_integration() {
        let mut decoder = StreamingJsonDecoder::new();

        let request = test_request("ping");
        let codec = ProtocolCodec::new();
        let mut bytes = codec.encode(&request).unwrap();
        bytes.push(b'\n');

        decoder.feed(&bytes);
        let decoded: JsonRpcRequest = decoder.try_decode().unwrap().unwrap();
        assert_eq!(decoded.method, "ping");
    }

    #[cfg(feature = "wire-simd")]
    #[test]
    fn test_simd_codec() {
        let codec = ProtocolCodec::with_type(CodecType::SimdJson);
        assert_eq!(codec.codec_type(), CodecType::SimdJson);

        let request = test_request("simd/test");
        let bytes = codec.encode(&request).unwrap();
        let decoded: JsonRpcRequest = codec.decode(&bytes).unwrap();
        assert_eq!(decoded.method, "simd/test");
    }

    #[cfg(feature = "wire-msgpack")]
    #[test]
    fn test_msgpack_codec() {
        let codec = ProtocolCodec::with_type(CodecType::MessagePack);
        assert_eq!(codec.codec_type(), CodecType::MessagePack);
        assert_eq!(codec.content_type(), "application/msgpack");

        let request = test_request("msgpack/test");
        let bytes = codec.encode(&request).unwrap();
        let decoded: JsonRpcRequest = codec.decode(&bytes).unwrap();
        assert_eq!(decoded.method, "msgpack/test");

        // encode_string should fail for msgpack
        assert!(codec.encode_string(&request).is_err());
    }
}
