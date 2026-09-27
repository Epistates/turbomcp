//! # turbomcp-codec
//!
//! The wire codec layer: `bytes` ↔ typed values. The transport hands the codec
//! *complete* messages (line-delimited for stdio, SSE-event-framed for HTTP);
//! streaming/framing is the transport's job, not the codec's.
//!
//! The seam is a single [`Codec`] trait with two impls:
//!
//! - [`SerdeJsonCodec`] — the portable baseline. Always available, including on
//!   `wasm32`.
//! - [`SonicRsCodec`] — SIMD-accelerated JSON (behind the `simd` feature, native
//!   `x86_64`/`aarch64` only). [`DefaultCodec`] resolves to this where supported.
//!
//! All codecs are version-independent: they serialize whatever
//! [`serde::Serialize`] value they are given. The per-version typed modules and
//! the [`turbomcp_core::JsonRpcMessage`] envelope sit above this layer.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;
use turbomcp_core::{InvalidFrame, JsonRpcMessage};

/// Errors produced while encoding to or decoding from the wire.
///
/// A transport decoding a *message* uses [`decode_message`] instead, which
/// says which of the two JSON-RPC errors a bad frame is owed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodecError {
    /// The value could not be serialized to the wire format.
    #[error("encode failed: {0}")]
    Encode(String),
    /// The bytes could not be deserialized into the target type.
    #[error("decode failed: {0}")]
    Decode(String),
}

/// Bytes ↔ typed value, in a single wire format.
///
/// Object-unsafe by design (generic methods); the codec is chosen at builder
/// time as a concrete type and transports are generic over `C: Codec`. This
/// keeps the hot path monomorphized — no `dyn` dispatch per message.
pub trait Codec: Send + Sync + 'static {
    /// Serialize `value` into a freshly allocated [`Bytes`] buffer.
    ///
    /// # Errors
    /// Returns [`CodecError::Encode`] if `value` cannot be represented.
    fn encode<T: Serialize>(&self, value: &T) -> Result<Bytes, CodecError>;

    /// Deserialize a complete message from `bytes`.
    ///
    /// # Errors
    /// Returns [`CodecError::Decode`] on malformed input or type mismatch.
    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CodecError>;
}

/// Decode one complete frame into a message, or classify why it isn't one.
///
/// Bytes that aren't JSON are a Parse error (`-32700`) with no id; JSON that
/// isn't a valid message (a batch, `"params": "x"`, a fractional id) is an
/// Invalid Request (`-32600`) that echoes the id when it can be read. A
/// leading UTF-8 byte-order mark is skipped: some Windows tooling writes
/// one, and it is never part of a JSON text.
///
/// The fast path is one decode. Only a frame that fails it is parsed again,
/// as a plain value, to find out which error it is owed.
///
/// # Errors
/// The [`InvalidFrame`] describing the error response this frame is owed.
pub fn decode_message<C: Codec>(codec: &C, bytes: &[u8]) -> Result<JsonRpcMessage, InvalidFrame> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match codec.decode::<JsonRpcMessage>(bytes) {
        Ok(msg) => Ok(msg),
        Err(first) => match codec.decode::<serde_json::Value>(bytes) {
            Ok(value) => JsonRpcMessage::from_value(value),
            Err(_) => Err(InvalidFrame::unparseable(first)),
        },
    }
}

/// `serde_json`-backed codec. The portable baseline: available on every target,
/// including `wasm32`, with no SIMD requirement.
#[derive(Debug, Clone, Copy, Default)]
pub struct SerdeJsonCodec;

impl Codec for SerdeJsonCodec {
    fn encode<T: Serialize>(&self, value: &T) -> Result<Bytes, CodecError> {
        serde_json::to_vec(value)
            .map(Bytes::from)
            .map_err(|e| CodecError::Encode(e.to_string()))
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CodecError> {
        serde_json::from_slice(bytes).map_err(|e| CodecError::Decode(e.to_string()))
    }
}

/// SIMD-accelerated JSON codec (`sonic-rs`). Native `x86_64`/`aarch64` only;
/// gated behind the `simd` feature.
#[cfg(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64")))]
#[derive(Debug, Clone, Copy, Default)]
pub struct SonicRsCodec;

#[cfg(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64")))]
impl Codec for SonicRsCodec {
    fn encode<T: Serialize>(&self, value: &T) -> Result<Bytes, CodecError> {
        sonic_rs::to_vec(value)
            .map(Bytes::from)
            .map_err(|e| CodecError::Encode(e.to_string()))
    }

    fn decode<T: DeserializeOwned>(&self, bytes: &[u8]) -> Result<T, CodecError> {
        sonic_rs::from_slice(bytes).map_err(|e| CodecError::Decode(e.to_string()))
    }
}

/// The codec selected by default for the current build: [`SonicRsCodec`] where
/// SIMD is available, otherwise [`SerdeJsonCodec`]. Both encode byte-compatible
/// JSON, so this choice is transparent to peers.
#[cfg(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64")))]
pub type DefaultCodec = SonicRsCodec;

/// The codec selected by default for the current build. See the SIMD variant
/// for the rationale; this is the portable fallback.
#[cfg(not(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64"))))]
pub type DefaultCodec = SerdeJsonCodec;

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::{JsonRpcMessage, JsonRpcRequest};

    fn sample() -> JsonRpcMessage {
        JsonRpcRequest::new(1, "tools/list", Some(serde_json::json!({"cursor": "abc"}))).into()
    }

    fn roundtrip<C: Codec>(codec: C) {
        let msg = sample();
        let bytes = codec.encode(&msg).expect("encode");
        let back: JsonRpcMessage = codec.decode(&bytes).expect("decode");
        assert_eq!(msg, back);
    }

    #[test]
    fn serde_json_roundtrips() {
        roundtrip(SerdeJsonCodec);
    }

    #[cfg(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64")))]
    #[test]
    fn sonic_rs_roundtrips() {
        roundtrip(SonicRsCodec);
    }

    #[cfg(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64")))]
    #[test]
    fn codecs_are_byte_compatible() {
        let msg = sample();
        let a = SerdeJsonCodec.encode(&msg).unwrap();
        // Cross-decode: serde_json output parses under sonic-rs.
        let via_sonic: JsonRpcMessage = SonicRsCodec.decode(&a).unwrap();
        assert_eq!(msg, via_sonic);
    }

    #[test]
    fn decode_error_is_reported() {
        let err = SerdeJsonCodec
            .decode::<JsonRpcMessage>(b"{not json")
            .unwrap_err();
        assert!(matches!(err, CodecError::Decode(_)));
    }

    #[test]
    fn decode_message_says_which_error_a_bad_frame_is_owed() {
        use turbomcp_core::{RequestId, codes};
        let codec = DefaultCodec::default();

        let garbage = decode_message(&codec, b"Server starting...").unwrap_err();
        assert_eq!(garbage.code, codes::PARSE_ERROR);
        assert!(garbage.id.is_none());

        let bad_params = decode_message(
            &codec,
            br#"{"jsonrpc":"2.0","id":7,"method":"x","params":"s"}"#,
        )
        .unwrap_err();
        assert_eq!(bad_params.code, codes::INVALID_REQUEST);
        assert_eq!(bad_params.id, Some(RequestId::Number(7)));

        let batch =
            decode_message(&codec, br#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#).unwrap_err();
        assert_eq!(batch.code, codes::INVALID_REQUEST);

        let with_bom = decode_message(
            &codec,
            b"\xEF\xBB\xBF{\"jsonrpc\":\"2.0\",\"method\":\"x\"}",
        );
        assert!(matches!(with_bom, Ok(JsonRpcMessage::Notification(_))));
    }

    #[test]
    fn batch_array_is_decode_error() {
        // No `Batch` variant by design (PLAN.md §13.1): a received JSON-RPC
        // batch is a decode error here (`decode_message` answers it `-32600`).
        let batch = br#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#;
        assert!(matches!(
            SerdeJsonCodec.decode::<JsonRpcMessage>(batch),
            Err(CodecError::Decode(_))
        ));
        // The empty batch is likewise invalid.
        assert!(matches!(
            SerdeJsonCodec.decode::<JsonRpcMessage>(b"[]"),
            Err(CodecError::Decode(_))
        ));
        #[cfg(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            assert!(matches!(
                SonicRsCodec.decode::<JsonRpcMessage>(batch),
                Err(CodecError::Decode(_))
            ));
            assert!(matches!(
                SonicRsCodec.decode::<JsonRpcMessage>(b"[]"),
                Err(CodecError::Decode(_))
            ));
        }
    }

    struct FailingSerialize;
    impl Serialize for FailingSerialize {
        fn serialize<S: serde::Serializer>(&self, _s: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("deliberate"))
        }
    }

    #[test]
    fn encode_error_is_reported() {
        let err = SerdeJsonCodec.encode(&FailingSerialize).unwrap_err();
        assert!(matches!(err, CodecError::Encode(_)));
        assert!(err.to_string().starts_with("encode failed:"));
        #[cfg(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64")))]
        assert!(matches!(
            SonicRsCodec.encode(&FailingSerialize),
            Err(CodecError::Encode(_))
        ));
    }

    #[test]
    fn default_codec_round_trips() {
        // Whatever DefaultCodec resolves to on this build, it round-trips.
        roundtrip(DefaultCodec::default());
    }

    /// Payloads that historically shake out JSON-backend divergence: non-BMP
    /// unicode, escape-heavy strings, integer extremes, high-precision floats,
    /// deep nesting, a large string, and null/absent params.
    fn edge_messages() -> Vec<JsonRpcMessage> {
        let deep = (0..64).fold(
            serde_json::json!("bottom"),
            |inner, _| serde_json::json!({ "d": inner }),
        );
        vec![
            JsonRpcRequest::new(
                i64::MAX,
                "tools/call",
                Some(serde_json::json!({
                    "name": "emoji-🦀-tool",
                    "arguments": {
                        "text": "line1\nline2\ttab \"quoted\" back\\slash \u{0007} ✓ 𝔘𝔫𝔦𝔠𝔬𝔡𝔢 🇺🇳",
                        "zero_width": "a\u{200b}b\u{feff}c",
                    }
                })),
            )
            .into(),
            JsonRpcRequest::new(
                i64::MIN,
                "resources/read",
                Some(serde_json::json!({
                    "ints": [0, -1, i64::MAX, i64::MIN, u32::MAX],
                    "floats": [0.1, -2.5e-308, 1.7976931348623157e308, 42.0],
                })),
            )
            .into(),
            JsonRpcRequest::new("string-id-🔑", "ping", Some(deep)).into(),
            JsonRpcRequest::new(
                7,
                "prompts/get",
                Some(serde_json::json!({ "blob": "x".repeat(64 * 1024) })),
            )
            .into(),
            JsonRpcRequest::new(8, "tools/list", None).into(),
        ]
    }

    #[test]
    fn edge_payloads_roundtrip_under_serde_json() {
        for msg in edge_messages() {
            let bytes = SerdeJsonCodec.encode(&msg).expect("encode");
            let back: JsonRpcMessage = SerdeJsonCodec.decode(&bytes).expect("decode");
            assert_eq!(msg, back);
        }
    }

    /// The `simd` feature swaps `DefaultCodec` to sonic-rs, so byte-level
    /// interchangeability with the serde_json baseline is load-bearing: both
    /// directions must parse the other's output for every edge payload.
    #[cfg(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64")))]
    #[test]
    fn edge_payloads_are_interchangeable_across_codecs() {
        for msg in edge_messages() {
            let via_serde = SerdeJsonCodec.encode(&msg).expect("serde encode");
            let via_sonic = SonicRsCodec.encode(&msg).expect("sonic encode");
            let sonic_reads_serde: JsonRpcMessage = SonicRsCodec
                .decode(&via_serde)
                .expect("sonic decodes serde");
            let serde_reads_sonic: JsonRpcMessage = SerdeJsonCodec
                .decode(&via_sonic)
                .expect("serde decodes sonic");
            assert_eq!(msg, sonic_reads_serde);
            assert_eq!(msg, serde_reads_sonic);
        }
    }

    #[test]
    fn empty_and_truncated_input_are_decode_errors_not_panics() {
        for input in [&b""[..], &b"{"[..], &br#"{"jsonrpc":"2.0","id":1,"met"#[..]] {
            assert!(matches!(
                SerdeJsonCodec.decode::<JsonRpcMessage>(input),
                Err(CodecError::Decode(_))
            ));
            #[cfg(all(feature = "simd", any(target_arch = "x86_64", target_arch = "aarch64")))]
            assert!(matches!(
                SonicRsCodec.decode::<JsonRpcMessage>(input),
                Err(CodecError::Decode(_))
            ));
        }
    }
}
