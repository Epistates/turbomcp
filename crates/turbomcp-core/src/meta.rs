//! `_meta` well-known keys and propagation policy (PLAN.md §13.2).
//!
//! The framework consumes a fixed set of keys; everything else is preserved in
//! [`crate::RequestContext::propagated_meta`] and echoed back on responses.
//! Extensions add keys under their reverse-DNS namespace.

use crate::{JsonRpcMessage, ProtocolVersion, TraceContext};
use alloc::string::{String, ToString};
use serde_json::{Map, Value};

/// Well-known `_meta` keys recognized by the framework.
pub mod keys {
    /// Per-request protocol version (draft stateless model). Verified present
    /// in `schema/draft/schema.ts:83`.
    pub const PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
    /// Progress correlation token.
    pub const PROGRESS_TOKEN: &str = "progressToken";
    /// W3C Trace Context — traceparent (SEP-414; re-verify number).
    pub const TRACEPARENT: &str = "traceparent";
    /// W3C Trace Context — tracestate.
    pub const TRACESTATE: &str = "tracestate";
    /// W3C Baggage.
    pub const BAGGAGE: &str = "baggage";
    /// Subscription stream correlation id (draft `subscriptions/listen`).
    pub const SUBSCRIPTION_ID: &str = "io.modelcontextprotocol/subscriptionId";
    /// Per-request client implementation info (draft stateless model).
    pub const CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
    /// Server implementation info, carried in a *result's* `_meta` (the
    /// `2026-07-28` stateless model, where there is no `initialize` result to
    /// put it in). The 2026-07-28 RC had briefly promoted it back to a
    /// first-class `DiscoverResult.serverInfo`; the frozen spec reverted that,
    /// so identity travels here on every result a server chooses to sign.
    pub const SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
    /// Per-request client capabilities (draft stateless model). Gates which
    /// MRTR input requests a server may send (SEP-2322 MUST).
    pub const CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
    /// Per-request log-level opt-in (draft; replaces `logging/setLevel`).
    /// Absent ⇒ the server MUST NOT send `notifications/message` for the
    /// request.
    pub const LOG_LEVEL: &str = "io.modelcontextprotocol/logLevel";

    /// Tags categorizing a *component* (a tool, resource, resource template, or
    /// prompt) — an array of strings in that component's own `_meta`, not a
    /// request key like the rest of this module. Written by
    /// `#[tool(tags(…))]` and read back by catalog policy.
    ///
    /// Namespaced deliberately. The spec's `_meta` rules make a prefix optional,
    /// but unprefixed names are where the schema reserves purpose-specific
    /// metadata, so a bare `tags` would be squatting on a name MCP may define.
    /// `io.turbomcp` is a legal prefix: the reservation covers prefixes whose
    /// second label is `modelcontextprotocol` or `mcp`.
    pub const TAGS: &str = "io.turbomcp/tags";

    /// The OAuth scopes a *component* requires, as an array of strings in its
    /// own `_meta` — what `#[tool(scopes(…))]` declares.
    ///
    /// On the wire because that is the only place a catalog policy can read it
    /// from: the requirement has to be visible at `tools/list` time to filter
    /// the list, and a component contributed by a mounted or proxied server
    /// carries its own. It discloses nothing new — a caller learns the same
    /// requirement by calling and reading the refusal.
    pub const SCOPES: &str = "io.turbomcp/scopes";
}

/// The first required `_meta` field a `2026-07-28` request is missing, or
/// `None` if the envelope is complete.
///
/// SEP-2575 made the stateless model's `RequestMetaObject` carry the facts the
/// `initialize` handshake used to establish, and the schema marks both
/// `protocolVersion` and `clientCapabilities` **required** (`clientInfo` is
/// only a SHOULD). A request missing either is malformed — invalid params —
/// not a version-negotiation failure.
///
/// One definition, because two layers act on it: the dispatcher rejects the
/// request, and the HTTP transport additionally answers `400` rather than the
/// usual `200`-plus-error-body. Applies only to the stateless wire — earlier
/// revisions establish these at `initialize` and carry no request envelope.
#[must_use]
pub fn missing_request_envelope_field(params: Option<&Value>) -> Option<&'static str> {
    let meta = params
        .and_then(|p| p.get("_meta"))
        .and_then(Value::as_object);
    let Some(meta) = meta else {
        // No `_meta` at all: report the version, the field a client is most
        // likely to have forgotten and the one that identifies the wire.
        return Some(keys::PROTOCOL_VERSION);
    };
    if !meta.contains_key(keys::PROTOCOL_VERSION) {
        return Some(keys::PROTOCOL_VERSION);
    }
    if !meta.contains_key(keys::CLIENT_CAPABILITIES) {
        return Some(keys::CLIENT_CAPABILITIES);
    }
    None
}

/// Whether a `_meta` key is consumed by the framework (and therefore should not
/// be blindly propagated to responses without the framework's involvement).
#[must_use]
pub fn is_framework_key(key: &str) -> bool {
    matches!(
        key,
        keys::PROTOCOL_VERSION
            | keys::PROGRESS_TOKEN
            | keys::TRACEPARENT
            | keys::TRACESTATE
            | keys::BAGGAGE
            | keys::SUBSCRIPTION_ID
            | keys::CLIENT_INFO
            | keys::CLIENT_CAPABILITIES
            | keys::LOG_LEVEL
    )
}

/// Extract the per-request protocol version from a `_meta` map (draft model).
///
/// Returns `None` if the key is absent or not a string. Unrecognized version
/// strings parse to [`ProtocolVersion::Unknown`] rather than `None`.
#[must_use]
pub fn extract_protocol_version(meta: &Map<String, Value>) -> Option<ProtocolVersion> {
    meta.get(keys::PROTOCOL_VERSION)
        .and_then(Value::as_str)
        .map(ProtocolVersion::from_wire)
}

/// Partition a `_meta` map into (framework-consumed, propagated) halves.
///
/// The propagated half is what the framework preserves on the request context
/// and echoes to response `_meta` unless a handler overrides it.
#[must_use]
pub fn partition(meta: Map<String, Value>) -> (Map<String, Value>, Map<String, Value>) {
    let mut consumed = Map::new();
    let mut propagated = Map::new();
    for (k, v) in meta {
        if is_framework_key(&k) {
            consumed.insert(k, v);
        } else {
            propagated.insert(k, v);
        }
    }
    (consumed, propagated)
}

/// Insert `key: value` into a request's or notification's `params._meta`,
/// creating `params` and `_meta` as needed. Responses are left untouched, as
/// are (already-invalid) non-object `params`.
///
/// The session adapter uses it to stamp a legacy session's negotiated
/// protocol version onto version-less messages; the client, to stamp its own
/// request envelope.
pub fn set_request_meta(msg: &mut JsonRpcMessage, key: &str, value: Value) {
    let params = match msg {
        JsonRpcMessage::Request(r) => &mut r.params,
        JsonRpcMessage::Notification(n) => &mut n.params,
        JsonRpcMessage::Response(_) => return,
    };
    let params = params.get_or_insert_with(|| Value::Object(Map::new()));
    let Some(obj) = params.as_object_mut() else {
        return;
    };
    let meta = obj
        .entry("_meta")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(meta) = meta.as_object_mut() {
        meta.insert(key.to_string(), value);
    }
}

/// Extract the W3C Trace Context (`traceparent`/`tracestate`/`baggage`) from a
/// `_meta` map, if a `traceparent` is present (SEP-414 propagation over `_meta`).
///
/// `traceparent` is required for a trace context to exist; `tracestate` and
/// `baggage` are optional vendor/state additions.
#[must_use]
pub fn extract_trace_context(meta: &Map<String, Value>) -> Option<TraceContext> {
    let traceparent = meta.get(keys::TRACEPARENT).and_then(Value::as_str)?;
    Some(TraceContext {
        traceparent: traceparent.to_string(),
        tracestate: meta
            .get(keys::TRACESTATE)
            .and_then(Value::as_str)
            .map(ToString::to_string),
        baggage: meta
            .get(keys::BAGGAGE)
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_draft_version() {
        let mut meta = Map::new();
        meta.insert(keys::PROTOCOL_VERSION.into(), json!("2026-07-28"));
        assert_eq!(
            extract_protocol_version(&meta),
            Some(ProtocolVersion::V2026_07_28)
        );
    }

    #[test]
    fn partition_preserves_user_keys_only() {
        let mut meta = Map::new();
        meta.insert(keys::TRACEPARENT.into(), json!("00-abc-def-01"));
        meta.insert("com.acme/tenant".into(), json!("t-42"));
        let (consumed, propagated) = partition(meta);
        assert!(consumed.contains_key(keys::TRACEPARENT));
        assert!(propagated.contains_key("com.acme/tenant"));
        assert_eq!(propagated.len(), 1);
    }

    #[test]
    fn every_wellknown_key_is_framework_consumed() {
        // Dropping any of these from `is_framework_key` would leak it into
        // `propagated_meta` (and echo it back on responses) — pin the full set.
        let framework = [
            keys::PROTOCOL_VERSION,
            keys::PROGRESS_TOKEN,
            keys::TRACEPARENT,
            keys::TRACESTATE,
            keys::BAGGAGE,
            keys::SUBSCRIPTION_ID,
            keys::CLIENT_INFO,
            keys::CLIENT_CAPABILITIES,
            keys::LOG_LEVEL,
        ];
        let mut meta = Map::new();
        for k in framework {
            meta.insert(k.into(), json!("v"));
        }
        meta.insert("com.acme/tenant".into(), json!("t-1"));
        let (consumed, propagated) = partition(meta);
        for k in framework {
            assert!(consumed.contains_key(k), "{k} must be framework-consumed");
        }
        assert_eq!(propagated.len(), 1);
        assert!(propagated.contains_key("com.acme/tenant"));
    }

    #[test]
    fn set_request_meta_creates_params_and_meta() {
        use crate::JsonRpcRequest;
        let mut msg: JsonRpcMessage = JsonRpcRequest::new(1, "tools/list", None).into();
        set_request_meta(&mut msg, "com.acme/tenant", json!("t-1"));
        set_request_meta(&mut msg, keys::PROTOCOL_VERSION, json!("2025-11-25"));
        let JsonRpcMessage::Request(r) = &msg else {
            unreachable!()
        };
        let meta = &r.params.as_ref().unwrap()["_meta"];
        assert_eq!(meta["com.acme/tenant"], "t-1");
        assert_eq!(meta[keys::PROTOCOL_VERSION], "2025-11-25");
    }

    #[test]
    fn extract_trace_context_requires_traceparent() {
        let mut meta = Map::new();
        meta.insert(keys::TRACESTATE.into(), json!("vendor=x"));
        assert!(extract_trace_context(&meta).is_none());
        meta.insert(keys::TRACEPARENT.into(), json!("00-abc-def-01"));
        let tc = extract_trace_context(&meta).unwrap();
        assert_eq!(tc.traceparent, "00-abc-def-01");
        assert_eq!(tc.tracestate.as_deref(), Some("vendor=x"));
        assert!(tc.baggage.is_none());
    }
}
