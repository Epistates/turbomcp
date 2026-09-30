//! Request-metadata header validation (transports spec §Server Validation).

use axum::http::HeaderMap;
use axum::response::Response;
use turbomcp_core::{JsonRpcMessage, ProtocolVersion, RequestId, meta};
use turbomcp_service::mcp_headers;

use super::reject::header_mismatch_rejection;
use crate::headers;

/// The id this message owes a response to — `None` for anything but a request.
/// Captured before dispatch so a panicking handler can still be answered (see
/// [`catch_handler_panic`]).
pub(super) fn request_id(msg: &JsonRpcMessage) -> Option<RequestId> {
    match msg {
        JsonRpcMessage::Request(r) => Some(r.id.clone()),
        _ => None,
    }
}

/// The protocol version a message's own `_meta` declares, if any — the
/// stateless draft envelope. Must be read **before** the dual-stack routing
/// stamps a session's negotiated version into version-less legacy bodies.
pub(super) fn declared_version(msg: &JsonRpcMessage) -> Option<String> {
    let params = match msg {
        JsonRpcMessage::Request(r) => r.params.as_ref(),
        JsonRpcMessage::Notification(n) => n.params.as_ref(),
        JsonRpcMessage::Response(_) => None,
    }?;
    params
        .get("_meta")?
        .get(meta::keys::PROTOCOL_VERSION)?
        .as_str()
        .map(str::to_owned)
}

/// Validate the transport's request-metadata headers against the body
/// (transports spec §Request Metadata / §Server Validation). Applies to
/// messages whose body `_meta` declares a protocol version — the stateless
/// draft envelope; the legacy `2025-11-25` session flow keeps its
/// negotiated-version tolerance. Any failure is `400` + a
/// `HeaderMismatch` JSON-RPC error (`-32020`).
///
/// Headers are pure **mirrors** — the body stays authoritative and values are
/// never sourced *from* headers (the earlier fill-absent `Mcp-Param-*` merge
/// is gone; it let a header inject an argument the body omitted).
pub(super) fn validate_request_headers(
    msg: &JsonRpcMessage,
    headers: &HeaderMap,
) -> Option<Response> {
    let declared = declared_version(msg);
    // Every rejection below echoes this, so the client can correlate it.
    let id = request_id(msg);
    let id = id.as_ref();
    let header_version = headers
        .get(&headers::PROTOCOL_VERSION)
        .and_then(|v| v.to_str().ok());

    // The mirror invariant: when both the header and the body name a version,
    // they MUST agree — whatever the versions.
    if let (Some(h), Some(d)) = (header_version, declared.as_deref())
        && h != d
    {
        return Some(header_mismatch_rejection(
            id,
            &format!("MCP-Protocol-Version header ({h}) does not match the request body ({d})"),
        ));
    }

    // The remaining rules are the draft transport's: they apply when the body
    // carries the draft's stateless envelope. A body declaring a *legacy*
    // version without a header keeps `2025-11-25`'s softer absence rule (the
    // session flow governs); a *header*-only draft version on an
    // envelope-less body (e.g. a notification, whose params carry no
    // envelope) requires nothing further.
    let declared_draft = declared
        .as_deref()
        .is_some_and(|d| ProtocolVersion::from_wire(d) == ProtocolVersion::V2026_07_28);
    if !declared_draft {
        return None;
    }
    if header_version.is_none() {
        // The draft requires the version header on every POST; this server
        // supports no pre-2025-06-18 clients, so absence is a rejection.
        return Some(header_mismatch_rejection(
            id,
            "missing required MCP-Protocol-Version header",
        ));
    }
    let JsonRpcMessage::Request(req) = msg else {
        return None;
    };

    // `Mcp-Method` is required on every request POST and mirrors `method`.
    match headers
        .get(&headers::MCP_METHOD)
        .and_then(|v| v.to_str().ok())
    {
        None => {
            return Some(header_mismatch_rejection(
                id,
                "missing required Mcp-Method header",
            ));
        }
        Some(m) if m != req.method => {
            return Some(header_mismatch_rejection(
                id,
                &format!(
                    "Mcp-Method header ({m}) does not match the request body ({})",
                    req.method
                ),
            ));
        }
        Some(_) => {}
    }

    // `Mcp-Name` is required for `tools/call`/`resources/read`/`prompts/get`
    // and mirrors `params.name`/`params.uri` (Base64 sentinel decoded). On the
    // Tasks extension's methods it mirrors `params.taskId`; the core spec
    // doesn't require it there, so it is checked only when sent.
    if let Some(field) = mcp_headers::routing_name_field(&req.method) {
        let required = mcp_headers::name_field_for(&req.method).is_some();
        let body_value = req
            .params
            .as_ref()
            .and_then(|p| p.get(field))
            .and_then(serde_json::Value::as_str);
        let sent = headers.get(&headers::MCP_NAME);
        if sent.is_none() && !required {
            return None;
        }
        let Some(raw) = sent.and_then(|v| v.to_str().ok()) else {
            return Some(header_mismatch_rejection(
                id,
                "missing required Mcp-Name header",
            ));
        };
        let Some(decoded) = mcp_headers::decode_value(raw) else {
            return Some(header_mismatch_rejection(
                id,
                "malformed Base64 sentinel in Mcp-Name header",
            ));
        };
        if body_value != Some(decoded.as_str()) {
            return Some(header_mismatch_rejection(
                id,
                &format!("Mcp-Name header does not match the request body's `{field}`"),
            ));
        }
    }

    // `Mcp-Param-*` values are checked by the dispatcher, which knows the
    // tool's schema and so which argument each header actually mirrors (see
    // the observed-header hand-off in the endpoint).

    None
}

/// Whether the message's `params._meta` already states a protocol version (a
/// modern stateless request does; a legacy post-initialize request doesn't).
pub(super) fn message_has_version(msg: &JsonRpcMessage) -> bool {
    let params = match msg {
        JsonRpcMessage::Request(r) => r.params.as_ref(),
        JsonRpcMessage::Notification(n) => n.params.as_ref(),
        JsonRpcMessage::Response(_) => None,
    };
    params
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(meta::keys::PROTOCOL_VERSION))
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, HeaderValue};
    use serde_json::json;
    use turbomcp_core::JsonRpcRequest;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    /// A draft-enveloped `tools/call` body and its compliant header set.
    fn draft_call(region: &str) -> JsonRpcMessage {
        JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({
                "name": "locate",
                "arguments": { "region": region, "n": 3, "ok": true },
                "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" },
            })),
        ))
    }

    fn draft_call_headers<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut all = vec![
            ("MCP-Protocol-Version", "2026-07-28"),
            ("Mcp-Method", "tools/call"),
            ("Mcp-Name", "locate"),
        ];
        all.extend_from_slice(extra);
        all
    }

    /// On `tasks/*`, `Mcp-Name` is the task id: optional for the server, but
    /// when a client sends one it has to be right, or a load balancer routed
    /// the poll by a different task than the body names.
    #[test]
    fn a_task_poll_mcp_name_is_checked_when_present() {
        let poll = JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tasks/get",
            Some(json!({
                "taskId": "t-1",
                "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" },
            })),
        ));
        let base = [
            ("MCP-Protocol-Version", "2026-07-28"),
            ("Mcp-Method", "tasks/get"),
        ];
        assert!(validate_request_headers(&poll, &headers(&base)).is_none());
        let mut named = base.to_vec();
        named.push(("Mcp-Name", "t-1"));
        assert!(validate_request_headers(&poll, &headers(&named)).is_none());
        let mut wrong = base.to_vec();
        wrong.push(("Mcp-Name", "t-2"));
        assert!(validate_request_headers(&poll, &headers(&wrong)).is_some());
    }

    #[test]
    fn validation_passes_a_compliant_draft_request() {
        let msg = draft_call("us-west");
        let ok = validate_request_headers(
            &msg,
            &headers(&draft_call_headers(&[
                ("Mcp-Param-region", "us-west"),
                ("Mcp-Param-n", "3"),
                ("Mcp-Param-ok", "true"),
            ])),
        );
        assert!(ok.is_none());
    }

    #[test]
    fn validation_skips_bodies_without_a_declared_version() {
        // Legacy traffic (no `_meta` version envelope) keeps the session
        // flow's tolerance — no headers required here.
        let msg = JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "locate", "arguments": {} })),
        ));
        assert!(validate_request_headers(&msg, &headers(&[])).is_none());
    }

    #[test]
    fn validation_requires_and_matches_the_standard_headers() {
        let msg = draft_call("us-west");
        // Missing version header.
        assert!(validate_request_headers(&msg, &headers(&[])).is_some());
        // Version header not matching the body.
        assert!(
            validate_request_headers(
                &msg,
                &headers(&[
                    ("MCP-Protocol-Version", "2025-11-25"),
                    ("Mcp-Method", "tools/call"),
                    ("Mcp-Name", "locate"),
                ])
            )
            .is_some()
        );
        // Missing Mcp-Method.
        assert!(
            validate_request_headers(
                &msg,
                &headers(&[
                    ("MCP-Protocol-Version", "2026-07-28"),
                    ("Mcp-Name", "locate")
                ])
            )
            .is_some()
        );
        // Mcp-Name not matching `params.name`.
        assert!(
            validate_request_headers(
                &msg,
                &headers(&[
                    ("MCP-Protocol-Version", "2026-07-28"),
                    ("Mcp-Method", "tools/call"),
                    ("Mcp-Name", "other_tool"),
                ])
            )
            .is_some()
        );
    }
}
