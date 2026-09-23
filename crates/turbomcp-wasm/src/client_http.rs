//! What the browser and WASI clients share about speaking Streamable HTTP.
//!
//! Both clients POSTed JSON-RPC as if the transport were plain request/reply
//! JSON: no `Accept`, no `Mcp-Session-Id`, no `MCP-Protocol-Version`, and a
//! body that had to be one JSON object. A Streamable HTTP server — including
//! TurboMCP's own — is entitled to answer a request with an SSE stream,
//! requires the session id it issued on every later request, and answers a
//! stale or unsupported version with `400`. These are the pieces both
//! transports now use to get that right.

use serde_json::Value;
use turbomcp_transport_streamable::SseParser;
use turbomcp_types::ProtocolVersion;

/// `Accept` for every POST: the transport spec requires a client to accept
/// both forms of answer.
pub(crate) const ACCEPT: &str = "application/json, text/event-stream";

/// Header carrying the session id a server issued at `initialize`.
pub(crate) const SESSION_ID_HEADER: &str = "Mcp-Session-Id";

/// Header carrying the negotiated protocol version after `initialize`.
pub(crate) const PROTOCOL_VERSION_HEADER: &str = "MCP-Protocol-Version";

/// Find the JSON-RPC response to request `id` in a POST's reply body.
///
/// A JSON body is the response itself. An SSE body may carry notifications
/// and server requests ahead of the response; those are skipped and the
/// first message whose `id` is ours is returned.
pub(crate) fn response_message(
    content_type: Option<&str>,
    body: &str,
    id: u64,
) -> Result<Value, String> {
    let is_sse = content_type.is_some_and(|value| {
        value
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("text/event-stream")
    });

    if !is_sse {
        return serde_json::from_str(body).map_err(|e| format!("Invalid JSON response: {e}"));
    }

    let mut parser = SseParser::new();
    // A final event without its blank-line terminator still counts.
    let mut events = parser.feed(body.as_bytes());
    events.extend(parser.feed(b"\n\n"));
    events
        .into_iter()
        .filter_map(|event| serde_json::from_str::<Value>(&event.data).ok())
        .find(|message| {
            message.get("method").is_none() && message.get("id").and_then(Value::as_u64) == Some(id)
        })
        .ok_or_else(|| format!("SSE stream ended without a response to request {id}"))
}

/// Check the version a server answered `initialize` with.
///
/// The server may answer with a version other than the one requested; the
/// lifecycle spec leaves it to the client to disconnect if it cannot speak
/// that version, rather than carry on with a protocol it does not implement.
pub(crate) fn check_negotiated_version(version: &str) -> Result<(), String> {
    if ProtocolVersion::STABLE
        .iter()
        .any(|supported| supported.as_str() == version)
    {
        Ok(())
    } else {
        Err(format!(
            "Server negotiated unsupported protocol version '{version}'"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_bodies_are_the_response() {
        let message = response_message(
            Some("application/json"),
            r#"{"jsonrpc":"2.0","id":3,"result":{}}"#,
            3,
        )
        .unwrap();
        assert_eq!(message["id"], 3);
    }

    #[test]
    fn sse_bodies_yield_the_response_past_other_messages() {
        let body = concat!(
            ": connected\n\n",
            "id: 1\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{}}\n\n",
            "id: 2\ndata: {\"jsonrpc\":\"2.0\",\"id\":99,\"method\":\"sampling/createMessage\"}\n\n",
            "id: 3\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}"
        );
        let message = response_message(Some("text/event-stream; charset=utf-8"), body, 7).unwrap();
        assert_eq!(message["result"]["ok"], true);

        assert!(response_message(Some("text/event-stream"), body, 8).is_err());
    }

    #[test]
    fn only_supported_versions_are_accepted() {
        assert!(check_negotiated_version("2025-06-18").is_ok());
        assert!(check_negotiated_version("2025-11-25").is_ok());
        assert!(check_negotiated_version("2024-11-05").is_err());
    }
}
