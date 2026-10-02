//! The OpenTelemetry MCP semantic conventions (`docs/gen-ai/mcp.md` in
//! `open-telemetry/semantic-conventions-genai`, development status), and what
//! both [`TraceContextLayer`](crate::TraceContextLayer) and
//! [`MetricsLayer`](crate::MetricsLayer) read off a request and its outcome.

use std::borrow::Cow;
use std::sync::OnceLock;

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use turbomcp_core::{JsonRpcMessage, ProtocolVersion, codes, meta};
use turbomcp_protocol::methods::request;

pub(crate) const MCP_METHOD_NAME: &str = "mcp.method.name";
pub(crate) const MCP_PROTOCOL_VERSION: &str = "mcp.protocol.version";
pub(crate) const MCP_SESSION_ID: &str = "mcp.session.id";
pub(crate) const MCP_RESOURCE_URI: &str = "mcp.resource.uri";
pub(crate) const JSONRPC_REQUEST_ID: &str = "jsonrpc.request.id";
pub(crate) const GEN_AI_TOOL_NAME: &str = "gen_ai.tool.name";
pub(crate) const GEN_AI_PROMPT_NAME: &str = "gen_ai.prompt.name";
pub(crate) const GEN_AI_OPERATION_NAME: &str = "gen_ai.operation.name";
pub(crate) const ERROR_TYPE: &str = "error.type";
pub(crate) const RPC_RESPONSE_STATUS_CODE: &str = "rpc.response.status_code";
pub(crate) const NETWORK_TRANSPORT: &str = "network.transport";
pub(crate) const NETWORK_PROTOCOL_NAME: &str = "network.protocol.name";
pub(crate) const NETWORK_PROTOCOL_VERSION: &str = "network.protocol.version";
pub(crate) const CLIENT_ADDRESS: &str = "client.address";
pub(crate) const CLIENT_PORT: &str = "client.port";
#[cfg(feature = "client")]
pub(crate) const SERVER_ADDRESS: &str = "server.address";
#[cfg(feature = "client")]
pub(crate) const SERVER_PORT: &str = "server.port";

/// The low-cardinality network attributes, for metric labels and spans
/// alike: `network.transport` and, where there is one, the protocol.
pub(crate) fn network_labels(
    facts: &turbomcp_service::NetworkFacts,
) -> Vec<opentelemetry::KeyValue> {
    use opentelemetry::KeyValue;
    let mut labels = vec![KeyValue::new(NETWORK_TRANSPORT, facts.transport)];
    if let Some(name) = facts.protocol_name {
        labels.push(KeyValue::new(NETWORK_PROTOCOL_NAME, name));
    }
    if let Some(version) = facts.protocol_version {
        labels.push(KeyValue::new(NETWORK_PROTOCOL_VERSION, version));
    }
    labels
}

/// The label for a method outside the known set, as the HTTP conventions
/// bucket unknown methods: one series per made-up name would let any caller
/// blow up a backend's cardinality with cheap `-32601`s.
const OTHER_METHOD: &str = "_OTHER";

/// `mcp.method.name` for `method`: its own name when it is a spec method or
/// one in `extra`, `_OTHER` otherwise.
pub(crate) fn method_label(method: &str, extra: &[String]) -> Cow<'static, str> {
    if let Some(known) = request::ALL.iter().find(|m| **m == method) {
        return Cow::Borrowed(known);
    }
    if let Some(known) = extra.iter().find(|m| *m == method) {
        return Cow::Owned(known.clone());
    }
    Cow::Borrowed(OTHER_METHOD)
}

/// `mcp.protocol.version`: a supported revision by name, `other` for any
/// other declared version (never the raw string, which the client chose), or
/// `None` when the request declares none (`initialize` carries it in the
/// body instead).
pub(crate) fn protocol_version(msg: &JsonRpcMessage) -> Option<&'static str> {
    let JsonRpcMessage::Request(r) = msg else {
        return None;
    };
    let declared = r
        .params
        .as_ref()?
        .get("_meta")?
        .get(meta::keys::PROTOCOL_VERSION)?
        .as_str()?;
    Some(version_label(&ProtocolVersion::from_wire(declared)))
}

/// `mcp.protocol.version` for `version`: its name when supported, else
/// `other`.
pub(crate) fn version_label(version: &ProtocolVersion) -> &'static str {
    ProtocolVersion::SUPPORTED
        .iter()
        .find(|v| *v == version)
        .map_or("other", ProtocolVersion::as_str)
}

/// What a request is about: the convention's span-name target and the
/// attributes that come with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Tool(String),
    Prompt(String),
    Resource(String),
}

impl Target {
    pub(crate) fn of(msg: &JsonRpcMessage) -> Option<Self> {
        let JsonRpcMessage::Request(r) = msg else {
            return None;
        };
        Self::from_parts(&r.method, r.params.as_ref().and_then(Value::as_object))
    }

    /// The target of a request for `method` with `params`.
    pub(crate) fn from_parts(
        method: &str,
        params: Option<&serde_json::Map<String, Value>>,
    ) -> Option<Self> {
        let field = |key: &str| {
            params
                .and_then(|p| p.get(key))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        match method {
            request::TOOLS_CALL => field("name").map(Self::Tool),
            request::PROMPTS_GET => field("name").map(Self::Prompt),
            request::RESOURCES_READ
            | request::RESOURCES_SUBSCRIBE
            | request::RESOURCES_UNSUBSCRIBE => field("uri").map(Self::Resource),
            _ => None,
        }
    }

    /// The span-name suffix: "target SHOULD match `{gen_ai.tool.name}` or
    /// `{gen_ai.prompt.name}` when applicable" (a URI is not a low-cardinality
    /// target).
    pub(crate) fn span_suffix(&self) -> Option<&str> {
        match self {
            Self::Tool(name) | Self::Prompt(name) => Some(name),
            Self::Resource(_) => None,
        }
    }
}

/// How a request ended, as the conventions classify it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// `error.type`, when the operation failed.
    pub error_type: Option<Cow<'static, str>>,
    /// `rpc.response.status_code`, when the response carried an error code
    /// (whether or not it counts as a failure).
    pub status_code: Option<i32>,
    /// The span status description for a failure.
    pub message: Option<String>,
    /// Whether the request named something the server doesn't have
    /// (`-32601`/`-32602`): its target came from the client and isn't one of
    /// the server's own, so it stays off metric labels.
    pub unknown_target: bool,
}

/// "The following error codes ... SHOULD NOT be considered errors": the
/// caller's mistakes, not the server's.
fn counts_as_failure(code: i32) -> bool {
    !matches!(
        code,
        codes::PARSE_ERROR
            | codes::INVALID_REQUEST
            | codes::METHOD_NOT_FOUND
            | codes::INVALID_PARAMS
            | codes::LEGACY_RESOURCE_NOT_FOUND
    )
}

impl Outcome {
    /// Classify a finished request.
    pub(crate) fn of<E>(result: &Result<Option<JsonRpcMessage>, E>) -> Self {
        match result {
            Err(_) => Self::other("_OTHER"),
            Ok(Some(JsonRpcMessage::Response(r))) => match &r.error {
                Some(error) => Self::rpc_error(error.code, &error.message),
                None => r.result.as_ref().map(Self::result).unwrap_or_default(),
            },
            Ok(_) => Self::default(),
        }
    }

    /// A JSON-RPC error response with `code`.
    pub(crate) fn rpc_error(code: i32, message: &str) -> Self {
        let failed = counts_as_failure(code);
        Self {
            error_type: failed.then(|| Cow::Owned(code.to_string())),
            status_code: Some(code),
            message: failed.then(|| message.to_owned()),
            unknown_target: matches!(code, codes::METHOD_NOT_FOUND | codes::INVALID_PARAMS),
        }
    }

    /// A successful result: "When `CallToolResult` returns `isError: true`,
    /// set `error.type` to `tool_error`."
    pub(crate) fn result(value: &Value) -> Self {
        let tool_error = value
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if tool_error {
            Self {
                error_type: Some(Cow::Borrowed("tool_error")),
                message: Some("tool_error".to_owned()),
                ..Self::default()
            }
        } else {
            Self::default()
        }
    }

    /// A failure with no JSON-RPC error to name it.
    pub(crate) fn other(error_type: &'static str) -> Self {
        Self {
            error_type: Some(Cow::Borrowed(error_type)),
            ..Self::default()
        }
    }

    /// A request abandoned mid-flight (client disconnect, a timeout layer).
    pub(crate) fn cancelled() -> Self {
        Self::other("cancelled")
    }
}

/// The key identities and session ids are hashed under before they reach
/// telemetry (HMAC-SHA256).
///
/// An unkeyed hash of an email or employee id is reversible with a dictionary
/// in microseconds by anyone who can read the trace backend, and under GDPR
/// it is still personal data. With a key, the hash correlates requests
/// without identifying anyone to someone who doesn't hold the key.
///
/// [`RedactionKey::per_process`] (the default) is random per process: values
/// correlate within one process and not across restarts or replicas. Share a
/// [`RedactionKey::new`] across a fleet to correlate across it, and keep it
/// secret like any key. It is wiped from memory when dropped, and neither
/// `Copy` nor comparable, so it doesn't spread into copies or leak through
/// timing.
#[derive(Clone)]
pub struct RedactionKey([u8; 32]);

impl Drop for RedactionKey {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

impl core::fmt::Debug for RedactionKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("RedactionKey(..)")
    }
}

impl RedactionKey {
    /// A key you supply, to correlate redacted values across processes.
    #[must_use]
    pub fn new(key: [u8; 32]) -> Self {
        Self(key)
    }

    /// The process's own random key (the same one for every caller in the
    /// process).
    ///
    /// # Panics
    /// If the OS random source is unavailable.
    #[must_use]
    pub fn per_process() -> Self {
        static KEY: OnceLock<RedactionKey> = OnceLock::new();
        KEY.get_or_init(|| {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).expect("the OS random source is unavailable");
            let key = Self(bytes);
            zeroize::Zeroize::zeroize(&mut bytes);
            key
        })
        .clone()
    }

    /// `{prefix}:` and the first 64 bits of `HMAC-SHA256(key, value)`, in hex.
    #[must_use]
    pub fn redact(&self, prefix: &str, value: &str) -> String {
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(&self.0).expect("HMAC accepts any key length");
        mac.update(value.as_bytes());
        let digest = mac.finalize().into_bytes();
        let mut out = String::with_capacity(prefix.len() + 17);
        out.push_str(prefix);
        out.push(':');
        for byte in &digest[..8] {
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }
}

impl Default for RedactionKey {
    fn default() -> Self {
        Self::per_process()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_core::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};

    fn error(code: i32) -> Result<Option<JsonRpcMessage>, ()> {
        Ok(Some(
            JsonRpcResponse::error(
                1,
                JsonRpcError {
                    code,
                    message: "m".into(),
                    data: None,
                },
            )
            .into(),
        ))
    }

    #[test]
    fn caller_mistakes_are_not_failures_but_keep_their_code() {
        for code in [
            codes::PARSE_ERROR,
            codes::INVALID_REQUEST,
            codes::METHOD_NOT_FOUND,
            codes::INVALID_PARAMS,
            codes::LEGACY_RESOURCE_NOT_FOUND,
        ] {
            let o = Outcome::of(&error(code));
            assert_eq!(o.error_type, None, "{code}");
            assert_eq!(o.status_code, Some(code));
        }
        let o = Outcome::of(&error(codes::INTERNAL_ERROR));
        assert_eq!(o.error_type.as_deref(), Some("-32603"));
        assert_eq!(o.message.as_deref(), Some("m"));
    }

    #[test]
    fn a_tool_error_result_is_a_tool_error() {
        let r: Result<_, ()> = Ok(Some(
            JsonRpcResponse::success(1, json!({ "content": [], "isError": true })).into(),
        ));
        assert_eq!(Outcome::of(&r).error_type.as_deref(), Some("tool_error"));
        let ok: Result<_, ()> = Ok(Some(
            JsonRpcResponse::success(1, json!({ "content": [] })).into(),
        ));
        assert_eq!(Outcome::of(&ok), Outcome::default());
    }

    #[test]
    fn targets_name_tools_and_prompts_but_not_uris() {
        let call: JsonRpcMessage =
            JsonRpcRequest::new(1, "tools/call", Some(json!({ "name": "add" }))).into();
        assert_eq!(Target::of(&call), Some(Target::Tool("add".into())));
        assert_eq!(Target::of(&call).unwrap().span_suffix(), Some("add"));
        let read: JsonRpcMessage =
            JsonRpcRequest::new(1, "resources/read", Some(json!({ "uri": "file://x" }))).into();
        assert_eq!(Target::of(&read).unwrap().span_suffix(), None);
    }

    /// The redacted form doesn't reveal the value, depends on the key, and is
    /// stable under one key.
    #[test]
    fn redaction_is_keyed() {
        let a = RedactionKey::new([1; 32]);
        let b = RedactionKey::new([2; 32]);
        let alice = a.redact("sub", "alice@example.com");
        assert!(alice.starts_with("sub:") && alice.len() == 4 + 16);
        assert!(!alice.contains("alice"));
        assert_eq!(alice, a.redact("sub", "alice@example.com"));
        assert_ne!(alice, b.redact("sub", "alice@example.com"));
        assert_eq!(
            RedactionKey::per_process().redact("sub", "alice"),
            RedactionKey::per_process().redact("sub", "alice"),
        );
    }
}
