//! JSON-RPC 2.0 envelope types — the cross-version stable wire frame.
//!
//! These are the *neutral* envelope shapes (stable since MCP 1.0): request,
//! response, notification, error, and id. Per-version semantic types
//! (`CallToolRequest` etc.) live in `turbomcp-protocol`, not here.
//!
//! **No `Batch` variant.** JSON-RPC batches were added in MCP `2025-03-26` and
//! removed in `2025-06-18`; neither supported version includes them. A received
//! batch is a parse error (`-32700`) at the codec layer (PLAN.md §13.1).

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
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JsonRpcResponse {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Correlation id (matches the originating request).
    pub id: RequestId,
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
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }

    /// Build an error response.
    pub fn error(id: impl Into<RequestId>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: jsonrpc_version(),
            id: id.into(),
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
/// The protocol seam is `Service<JsonRpcMessage, Response = Option<JsonRpcMessage>>`
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
