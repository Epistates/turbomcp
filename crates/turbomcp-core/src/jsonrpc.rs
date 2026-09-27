//! JSON-RPC 2.0 envelope types — the cross-version stable wire frame.
//!
//! These are the *neutral* envelope shapes (stable since MCP 1.0): request,
//! response, notification, error, and id. Per-version semantic types
//! (`CallToolRequest` etc.) live in `turbomcp-protocol`, not here.
//!
//! **No `Batch` variant.** JSON-RPC batches were added in MCP `2025-03-26` and
//! removed in `2025-06-18`; no supported revision includes them. A received
//! batch is well-formed JSON that is not a valid message: an
//! [`InvalidFrame`] answered with Invalid Request (`-32600`).

use alloc::string::String;
use serde_json::Value;

/// A JSON-RPC request/response correlation id: a string or an integer.
///
/// MCP forbids fractional and null ids; this models the two legal shapes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// Integer id.
    Number(i64),
    /// String id.
    String(String),
}

impl From<i64> for RequestId {
    fn from(n: i64) -> Self {
        Self::Number(n)
    }
}
impl From<String> for RequestId {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}
impl From<&str> for RequestId {
    fn from(s: &str) -> Self {
        Self::String(s.into())
    }
}

const JSONRPC_VERSION: &str = "2.0";

fn jsonrpc_version() -> String {
    JSONRPC_VERSION.into()
}

fn is_jsonrpc_version(s: &str) -> bool {
    s == JSONRPC_VERSION
}

/// A JSON-RPC request: has an `id` and a `method`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JsonRpcRequest {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Correlation id (required for requests).
    pub id: RequestId,
    /// Method name (e.g. `"tools/call"`).
    pub method: String,
    /// Method parameters, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// Build a request with the canonical `jsonrpc` field set.
    pub fn new(id: impl Into<RequestId>, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: jsonrpc_version(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC notification: a `method` with no `id` (no response expected).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JsonRpcNotification {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Notification method (e.g. `"notifications/cancelled"`).
    pub method: String,
    /// Notification parameters, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    /// Build a notification with the canonical `jsonrpc` field set.
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: jsonrpc_version(),
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC error object.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JsonRpcError {
    /// JSON-RPC error code.
    pub code: i32,
    /// Human-readable message.
    pub message: String,
    /// Optional structured error data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// A JSON-RPC response: an `id` plus exactly one of `result` / `error`.
///
/// The "exactly one" invariant is enforced by the [`JsonRpcResponse::success`]
/// and [`JsonRpcResponse::error`] constructors.
///
/// The `id` is optional for one case only: an error answering a frame whose
/// id couldn't be read ("except in error cases where the ID could not be read
/// due a malformed request"; the schema's `JSONRPCErrorResponse` has
/// `id?: RequestId`). A success always has one, and decoding enforces that.
/// `"id": null`, which JSON-RPC 2.0 uses for the same case, decodes to `None`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JsonRpcResponse {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Correlation id (matches the originating request); `None` only on an
    /// error answering a frame whose id was unreadable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
    /// Success payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Build a success response.
    pub fn success(id: impl Into<RequestId>, result: Value) -> Self {
        Self {
            jsonrpc: jsonrpc_version(),
            id: Some(id.into()),
            result: Some(result),
            error: None,
        }
    }

    /// Build an error response.
    pub fn error(id: impl Into<RequestId>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: jsonrpc_version(),
            id: Some(id.into()),
            result: None,
            error: Some(error),
        }
    }

    /// Build an error response for a frame whose id couldn't be read (it
    /// wasn't JSON, or its `id` wasn't a string or integer). The `id` is
    /// omitted, as the schema's `id?: RequestId` allows.
    pub fn error_without_id(error: JsonRpcError) -> Self {
        Self {
            jsonrpc: jsonrpc_version(),
            id: None,
            result: None,
            error: Some(error),
        }
    }

    /// Whether this response carries an error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// A single JSON-RPC frame: request, response, or notification.
///
/// The protocol seam is `Service<McpRequest, Response = Option<JsonRpcMessage>>`
/// (notifications produce `None`).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    /// A request (has `id` + `method`).
    Request(JsonRpcRequest),
    /// A notification (has `method`, no `id`).
    Notification(JsonRpcNotification),
    /// A response (has `id`, no `method`).
    Response(JsonRpcResponse),
}

impl<'de> serde::Deserialize<'de> for JsonRpcMessage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let value = Value::deserialize(deserializer)?;
        let map = value
            .as_object()
            .ok_or_else(|| D::Error::custom("JSON-RPC frame must be an object"))?;
        if map.get("jsonrpc").and_then(Value::as_str).is_none() {
            return Err(D::Error::custom("JSON-RPC version is required"));
        }
        if map.contains_key("method") {
            if map.contains_key("result") || map.contains_key("error") {
                return Err(D::Error::custom("request cannot contain response fields"));
            }
            if map
                .get("params")
                .is_some_and(|p| !p.is_object() && !p.is_array())
            {
                return Err(D::Error::custom("params must be an object or array"));
            }
            if map.contains_key("id") {
                serde_json::from_value(value)
                    .map(Self::Request)
                    .map_err(D::Error::custom)
            } else {
                serde_json::from_value(value)
                    .map(Self::Notification)
                    .map_err(D::Error::custom)
            }
        } else {
            if map.contains_key("result") == map.contains_key("error") {
                return Err(D::Error::custom(
                    "response requires exactly one of result or error",
                ));
            }
            if map.contains_key("result") && map.get("id").is_none_or(Value::is_null) {
                return Err(D::Error::custom("a result response requires an id"));
            }
            let result = map.get("result").cloned();
            let has_error = map.contains_key("error");
            let mut response: JsonRpcResponse =
                serde_json::from_value(value).map_err(D::Error::custom)?;
            if has_error && response.error.is_none() {
                return Err(D::Error::custom("error must be an error object"));
            }
            response.result = result; // Preserve a legitimate JSON null result.
            Ok(Self::Response(response))
        }
    }
}

impl JsonRpcMessage {
    /// Decode a message from an already-parsed JSON value, or say why it isn't
    /// one in a form that can be answered: the salvaged id and an Invalid
    /// Request (`-32600`) error.
    ///
    /// # Errors
    /// An [`InvalidFrame`] when `value` is JSON but not a valid message.
    pub fn from_value(value: Value) -> Result<Self, InvalidFrame> {
        use serde::Deserialize;
        Self::deserialize(&value).map_err(|e| InvalidFrame::invalid(&value, &e))
    }

    /// Validate the `jsonrpc` version field, if present.
    #[must_use]
    pub fn has_valid_version(&self) -> bool {
        let v = match self {
            Self::Request(r) => &r.jsonrpc,
            Self::Notification(n) => &n.jsonrpc,
            Self::Response(r) => &r.jsonrpc,
        };
        is_jsonrpc_version(v)
    }

    /// The method name, for requests and notifications.
    #[must_use]
    pub fn method(&self) -> Option<&str> {
        match self {
            Self::Request(r) => Some(&r.method),
            Self::Notification(n) => Some(&n.method),
            Self::Response(_) => None,
        }
    }
}

/// A frame that arrived whole but isn't a JSON-RPC message this SDK can act
/// on: unparseable bytes, or JSON with the wrong shape.
///
/// One bad frame is the peer's bug, not a broken stream. Newline and message
/// framing both resynchronize at the next frame, so the driver answers this
/// one (when it deserves an answer) and keeps reading, instead of dropping
/// the connection and every request in flight on it.
#[derive(Clone, Debug, PartialEq)]
pub struct InvalidFrame {
    /// The frame's `id`, when it was readable as a string or integer.
    pub id: Option<RequestId>,
    /// Whether the frame looked like a response (`result` or `error`, no
    /// `method`). Nothing answers a response, however broken: doing so is
    /// how two peers end up trading error frames forever.
    pub is_response: bool,
    /// The JSON-RPC error code it is owed: Parse error (`-32700`) or
    /// Invalid Request (`-32600`).
    pub code: i32,
    /// What was wrong.
    pub message: String,
}

impl InvalidFrame {
    /// Bytes that aren't JSON at all: Parse error (`-32700`), no id.
    #[must_use]
    pub fn unparseable(detail: impl core::fmt::Display) -> Self {
        Self {
            id: None,
            is_response: false,
            code: crate::codes::PARSE_ERROR,
            message: alloc::format!("Parse error: {detail}"),
        }
    }

    /// JSON that isn't a valid message: Invalid Request (`-32600`), echoing
    /// the id when one can be read.
    #[must_use]
    pub fn invalid(value: &Value, detail: impl core::fmt::Display) -> Self {
        let map = value.as_object();
        let field = |key: &str| map.and_then(|m| m.get(key));
        let id = field("id").and_then(|id| match id {
            Value::String(s) => Some(RequestId::String(s.clone())),
            Value::Number(n) => n.as_i64().map(RequestId::Number),
            _ => None,
        });
        let is_response =
            field("method").is_none() && (field("result").is_some() || field("error").is_some());
        Self {
            id,
            is_response,
            code: crate::codes::INVALID_REQUEST,
            message: alloc::format!("Invalid Request: {detail}"),
        }
    }

    /// A frame longer than the transport accepts: Invalid Request, no id
    /// (nothing of it was parsed).
    #[must_use]
    pub fn too_large(max_bytes: usize) -> Self {
        Self {
            id: None,
            is_response: false,
            code: crate::codes::INVALID_REQUEST,
            message: alloc::format!("Invalid Request: frame exceeds {max_bytes} bytes"),
        }
    }

    /// The error response this frame is owed, or `None` for a broken
    /// response (which is never answered).
    #[must_use]
    pub fn response(&self) -> Option<JsonRpcResponse> {
        if self.is_response {
            return None;
        }
        Some(match &self.id {
            Some(id) => JsonRpcResponse::error(id.clone(), self.error()),
            None => JsonRpcResponse::error_without_id(self.error()),
        })
    }

    /// The error object this frame is owed.
    #[must_use]
    pub fn error(&self) -> JsonRpcError {
        JsonRpcError {
            code: self.code,
            message: self.message.clone(),
            data: None,
        }
    }
}

impl core::fmt::Display for InvalidFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message)
    }
}

impl core::error::Error for InvalidFrame {}

impl From<JsonRpcRequest> for JsonRpcMessage {
    fn from(r: JsonRpcRequest) -> Self {
        Self::Request(r)
    }
}
impl From<JsonRpcNotification> for JsonRpcMessage {
    fn from(n: JsonRpcNotification) -> Self {
        Self::Notification(n)
    }
}
impl From<JsonRpcResponse> for JsonRpcMessage {
    fn from(r: JsonRpcResponse) -> Self {
        Self::Response(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use serde_json::json;

    #[test]
    fn untagged_discriminates_request_notification_response() {
        let req = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string();
        let notif = json!({"jsonrpc":"2.0","method":"notifications/cancelled"}).to_string();
        let resp = json!({"jsonrpc":"2.0","id":1,"result":{}}).to_string();
        let err = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}}).to_string();

        assert!(matches!(
            serde_json::from_str::<JsonRpcMessage>(&req).unwrap(),
            JsonRpcMessage::Request(_)
        ));
        assert!(matches!(
            serde_json::from_str::<JsonRpcMessage>(&notif).unwrap(),
            JsonRpcMessage::Notification(_)
        ));
        let r: JsonRpcMessage = serde_json::from_str(&resp).unwrap();
        assert!(matches!(r, JsonRpcMessage::Response(ref x) if !x.is_error()));
        let e: JsonRpcMessage = serde_json::from_str(&err).unwrap();
        assert!(matches!(e, JsonRpcMessage::Response(ref x) if x.is_error()));
    }

    #[test]
    fn request_id_accepts_string_and_number() {
        let n: RequestId = serde_json::from_str("7").unwrap();
        assert_eq!(n, RequestId::Number(7));
        let s: RequestId = serde_json::from_str("\"abc\"").unwrap();
        assert_eq!(s, RequestId::String("abc".into()));
    }

    #[test]
    fn request_id_rejects_null_and_fractional() {
        // MCP forbids null and fractional ids (see the RequestId doc).
        assert!(serde_json::from_str::<RequestId>("null").is_err());
        assert!(serde_json::from_str::<RequestId>("1.5").is_err());
    }

    #[test]
    fn invalid_id_is_rejected_without_notification_fallback() {
        for id in [json!(null), json!(1.5), json!({})] {
            assert!(
                serde_json::from_value::<JsonRpcMessage>(
                    json!({"jsonrpc":"2.0","id":id,"method":"ping"})
                )
                .is_err()
            );
        }
    }

    #[test]
    fn version_field_validation() {
        let m: JsonRpcMessage = JsonRpcRequest::new(1, "ping", None).into();
        assert!(m.has_valid_version());
        // A wrong version string parses (tolerant reader) but is detectable.
        let raw = json!({"jsonrpc":"1.0","id":1,"method":"ping"}).to_string();
        let bad: JsonRpcMessage = serde_json::from_str(&raw).unwrap();
        assert!(!bad.has_valid_version());
        // A missing version is not silently upgraded.
        let raw = json!({"id":1,"method":"ping"}).to_string();
        assert!(serde_json::from_str::<JsonRpcMessage>(&raw).is_err());
    }

    #[test]
    fn malformed_envelopes_never_fall_back_to_another_message_kind() {
        for raw in [
            json!([]),
            json!(null),
            json!({"jsonrpc":"2.0","method":"ping","params":null}),
            json!({"jsonrpc":"2.0","method":"ping","params":true}),
            json!({"jsonrpc":"2.0","method":"ping","result":{}}),
            json!({"jsonrpc":"2.0","id":1}),
            json!({"jsonrpc":"2.0","id":1,"error":null}),
            json!({"jsonrpc":"2.0","id":1,"result":null,"error":{"code":-1,"message":"bad"}}),
        ] {
            assert!(
                serde_json::from_value::<JsonRpcMessage>(raw.clone()).is_err(),
                "accepted {raw}"
            );
        }
        for result in [json!(null), json!(false), json!([]), json!({"nested":null})] {
            let raw = json!({"jsonrpc":"2.0","id":1,"result":result});
            let decoded: JsonRpcMessage = serde_json::from_value(raw.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), raw);
        }
    }

    /// "Error responses MUST include the same ID as the request they
    /// correspond to (except in error cases where the ID could not be read
    /// due a malformed request)": the schema's `id?` and JSON-RPC's `null`
    /// both decode, and ours goes out with the id absent.
    #[test]
    fn an_error_response_may_omit_its_id() {
        for raw in [
            json!({"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"}}),
            json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
        ] {
            let JsonRpcMessage::Response(r) = serde_json::from_value(raw).unwrap() else {
                panic!("an id-less error is a response");
            };
            assert!(r.id.is_none() && r.is_error());
        }
        let out = JsonRpcResponse::error_without_id(JsonRpcError {
            code: -32700,
            message: "Parse error".into(),
            data: None,
        });
        assert!(serde_json::to_value(out).unwrap().get("id").is_none());
        // A success always correlates to something.
        for raw in [
            json!({"jsonrpc":"2.0","result":{}}),
            json!({"jsonrpc":"2.0","id":null,"result":{}}),
        ] {
            assert!(serde_json::from_value::<JsonRpcMessage>(raw).is_err());
        }
    }

    #[test]
    fn an_invalid_frame_keeps_what_it_can() {
        let bad = JsonRpcMessage::from_value(
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":"x"}),
        )
        .unwrap_err();
        assert_eq!(bad.id, Some(RequestId::Number(7)));
        assert_eq!(bad.code, crate::codes::INVALID_REQUEST);
        assert_eq!(bad.response().unwrap().id, Some(RequestId::Number(7)));

        // A batch is JSON, just not a message.
        let batch = JsonRpcMessage::from_value(json!([{"jsonrpc":"2.0","id":1,"method":"ping"}]))
            .unwrap_err();
        assert!(batch.id.is_none() && !batch.is_response);

        // A fractional id can't be echoed.
        let frac = JsonRpcMessage::from_value(json!({"jsonrpc":"2.0","id":1.5,"method":"ping"}))
            .unwrap_err();
        assert!(frac.id.is_none() && frac.response().unwrap().id.is_none());

        // A broken response is never answered.
        let resp =
            JsonRpcMessage::from_value(json!({"jsonrpc":"2.0","id":3,"error":null})).unwrap_err();
        assert!(resp.is_response && resp.response().is_none());
        assert_eq!(resp.id, Some(RequestId::Number(3)));

        assert_eq!(
            InvalidFrame::unparseable("eof").code,
            crate::codes::PARSE_ERROR
        );
    }

    #[test]
    fn response_constructors_enforce_one_of() {
        let ok = JsonRpcResponse::success(1, json!({"v":1}));
        assert!(!ok.is_error() && ok.result.is_some() && ok.error.is_none());
        let bad = JsonRpcResponse::error(
            1,
            JsonRpcError {
                code: -32603,
                message: "x".into(),
                data: None,
            },
        );
        assert!(bad.is_error() && bad.result.is_none());
    }
}
