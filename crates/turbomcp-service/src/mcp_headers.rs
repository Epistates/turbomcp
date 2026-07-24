//! Streamable HTTP request-metadata headers (transports spec §Request Metadata).
//!
//! The draft Streamable HTTP transport mirrors selected JSON-RPC body fields
//! into HTTP headers so intermediaries can route and inspect requests without
//! parsing the body: `MCP-Protocol-Version` (every POST), `Mcp-Method` (every
//! request), `Mcp-Name` (`tools/call` / `resources/read` / `prompts/get`), and
//! `Mcp-Param-{name}` (tool arguments annotated `x-mcp-header`). Headers are
//! pure **mirrors**: the body is authoritative, clients derive header values
//! from it, and servers validate equality — a mismatch is `400` + a
//! `HeaderMismatch` JSON-RPC error (`-32001`). Servers never source values
//! *from* headers.
//!
//! This module is the shared client/server half: header names, the Base64
//! sentinel value codec, and the body-value rendering rules. The client
//! transport encodes with it; the HTTP server transport decodes and compares
//! with it.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

/// `MCP-Protocol-Version` — required on every POST; must equal the request
/// body's `_meta` protocol version on the draft.
pub const PROTOCOL_VERSION: &str = "MCP-Protocol-Version";
/// `Mcp-Method` — required on every draft request POST; mirrors `method`.
pub const MCP_METHOD: &str = "Mcp-Method";
/// `Mcp-Name` — required for `tools/call`/`resources/read`/`prompts/get`;
/// mirrors `params.name` / `params.uri`.
pub const MCP_NAME: &str = "Mcp-Name";
/// The `Mcp-Param-{name}` prefix for `x-mcp-header`-annotated tool arguments.
pub const MCP_PARAM_PREFIX: &str = "Mcp-Param-";

/// The methods whose requests must carry an `Mcp-Name` header, with the body
/// field it mirrors (`params.name` or `params.uri`).
#[must_use]
pub fn name_field_for(method: &str) -> Option<&'static str> {
    match method {
        "tools/call" | "prompts/get" => Some("name"),
        "resources/read" => Some("uri"),
        _ => None,
    }
}

/// Whether `value` can ride in an HTTP header as-is: visible ASCII, interior
/// spaces/tabs allowed, no leading/trailing whitespace, and not shaped like
/// the Base64 sentinel (which must itself be encoded to stay unambiguous).
#[must_use]
pub fn is_header_safe(value: &str) -> bool {
    if value.starts_with(' ')
        || value.starts_with('\t')
        || value.ends_with(' ')
        || value.ends_with('\t')
    {
        return false;
    }
    if value.starts_with("=?base64?") && value.ends_with("?=") {
        return false; // sentinel-shaped literal: encode to disambiguate
    }
    value
        .bytes()
        .all(|b| (0x21..=0x7e).contains(&b) || b == b' ' || b == b'\t')
}

/// Encode a value for an `Mcp-Name` / `Mcp-Param-*` header: as-is when
/// header-safe, else the Base64 sentinel `=?base64?{b64(utf8)}?=`.
#[must_use]
pub fn encode_value(value: &str) -> String {
    if is_header_safe(value) {
        value.to_owned()
    } else {
        format!("=?base64?{}?=", BASE64.encode(value.as_bytes()))
    }
}

/// Decode a header value that may use the Base64 sentinel. Returns `None` for
/// a malformed sentinel (invalid Base64 or non-UTF-8 payload) — servers treat
/// that as a validation failure.
#[must_use]
pub fn decode_value(value: &str) -> Option<String> {
    let Some(payload) = value
        .strip_prefix("=?base64?")
        .and_then(|rest| rest.strip_suffix("?="))
    else {
        return Some(value.to_owned());
    };
    let bytes = BASE64.decode(payload).ok()?;
    String::from_utf8(bytes).ok()
}

/// Render a JSON body value as its header string form (transports spec §Value
/// Encoding type conversion): strings as-is, integers as decimal (only within
/// the JavaScript safe range), booleans lowercase. `None` for any other shape
/// (`x-mcp-header` only applies to primitive `string`/`integer`/`boolean`
/// parameters — `number` is not permitted).
#[must_use]
pub fn render_argument(value: &serde_json::Value) -> Option<String> {
    const JS_SAFE: i64 = (1 << 53) - 1;
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => {
            let i = n.as_i64()?;
            (-JS_SAFE..=JS_SAFE).contains(&i).then(|| i.to_string())
        }
        serde_json::Value::Bool(b) => Some(if *b { "true" } else { "false" }.to_owned()),
        _ => None,
    }
}

/// Whether `name` is a valid `x-mcp-header` value: non-empty RFC 9110
/// field-name token characters only (`tchar`).
#[must_use]
pub fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The spec's §Value Encoding examples table, verbatim.
    #[test]
    fn encoding_examples_from_the_spec() {
        assert_eq!(encode_value("us-west1"), "us-west1");
        assert_eq!(
            encode_value("Hello, 世界"),
            "=?base64?SGVsbG8sIOS4lueVjA==?="
        );
        assert_eq!(encode_value(" padded "), "=?base64?IHBhZGRlZCA=?=");
        assert_eq!(encode_value("line1\nline2"), "=?base64?bGluZTEKbGluZTI=?=");
        assert_eq!(
            encode_value("=?base64?literal?="),
            "=?base64?PT9iYXNlNjQ/bGl0ZXJhbD89?="
        );
    }

    #[test]
    fn decode_round_trips_and_rejects_malformed() {
        for original in ["us-west1", "Hello, 世界", " padded ", "=?base64?literal?="] {
            assert_eq!(
                decode_value(&encode_value(original)).as_deref(),
                Some(original)
            );
        }
        assert_eq!(decode_value("plain"), Some("plain".to_owned()));
        assert_eq!(decode_value("=?base64?!!!not-base64!!!?="), None);
    }

    #[test]
    fn render_argument_type_conversion() {
        assert_eq!(render_argument(&json!("s")).as_deref(), Some("s"));
        assert_eq!(render_argument(&json!(42)).as_deref(), Some("42"));
        assert_eq!(render_argument(&json!(-7)).as_deref(), Some("-7"));
        assert_eq!(render_argument(&json!(true)).as_deref(), Some("true"));
        assert_eq!(render_argument(&json!(false)).as_deref(), Some("false"));
        // `number` is not permitted; nor are composites; nor unsafe integers.
        assert_eq!(render_argument(&json!(1.5)), None);
        assert_eq!(render_argument(&json!({"a": 1})), None);
        assert_eq!(render_argument(&json!(9_007_199_254_740_993_i64)), None);
    }

    #[test]
    fn header_name_token_rules() {
        assert!(is_valid_header_name("Region"));
        assert!(is_valid_header_name("x-1_2.3"));
        assert!(!is_valid_header_name(""));
        assert!(!is_valid_header_name("has space"));
        assert!(!is_valid_header_name("crlf\r\n"));
        assert!(!is_valid_header_name("colon:"));
    }
}
